use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use egui::{Key, Modifiers};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use notify::{RecursiveMode, Watcher};

use crate::doc::{prepare, Prepared};
use crate::mermaid::{find_chromium, Renderer, Theme};

/// A render job handed to a worker thread: (diagram id, mermaid source, theme).
type Job = (String, String, Theme);

/// A finished render delivered back to the UI thread: (id, theme, outcome).
type DiagramResult = (String, Theme, Result<PathBuf, String>);

pub fn read_doc(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not valid UTF-8", path.display()))
}

/// Returns the diagram id when `html` is exactly a `<mermaid id="…">` placeholder.
fn parse_placeholder(html: &str) -> Option<&str> {
    let s = html.trim();
    let rest = s.strip_prefix("<mermaid")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("id=")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let id = &rest[..end];
    if id.is_empty() {
        return None;
    }
    let after = &rest[end + 1..];
    let after = after.trim_end();
    if !after.ends_with('>') {
        return None;
    }
    if !after[..after.len() - 1].trim().is_empty() {
        return None;
    }
    Some(id)
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Whether a filesystem event path refers to the open document.
fn same_file(event: &Path, doc: &Path) -> bool {
    if event == doc {
        return true;
    }
    if event.file_name() != doc.file_name() {
        return false;
    }
    // Compare absolute forms; canonicalization may fail if the file was just
    // deleted, in which case a matching file name is good enough.
    let abs = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    abs(event) == abs(doc)
}

/// Queue a render for `id` in `theme`, unless it has already failed this session
/// (for the same theme) or is in flight. Returns true when a job was enqueued, in
/// which case the caller must request a repaint so the result is picked up without
/// user input.
fn enqueue_diagram(
    failures: &HashMap<(String, &'static str), String>,
    inflight: &mut HashSet<String>,
    sources: &HashMap<String, String>,
    queue: &Mutex<VecDeque<Job>>,
    id: &str,
    theme: Theme,
) -> bool {
    if failures.contains_key(&(id.to_string(), theme.as_str())) || inflight.contains(id) {
        return false;
    }
    if let Some(src) = sources.get(id) {
        inflight.insert(id.to_string());
        queue
            .lock()
            .unwrap()
            .push_back((id.to_string(), src.clone(), theme));
        true
    } else {
        false
    }
}

/// Shared, interior-mutable state backing the markdown html hook and the worker pool.
pub struct DiagramUi {
    renderer: Arc<Renderer>,
    sources: HashMap<String, String>,
    theme: Theme,
    zoom: f32,
    textures: HashMap<String, (egui::TextureHandle, [usize; 2])>,
    inflight: HashSet<String>,
    failures: HashMap<(String, &'static str), String>,
    queue: Arc<Mutex<VecDeque<Job>>>,
    result_tx: mpsc::Sender<DiagramResult>,
    result_rx: mpsc::Receiver<DiagramResult>,
    active_workers: usize,
}

impl DiagramUi {
    fn new(renderer: Arc<Renderer>, sources: HashMap<String, String>, theme: Theme) -> Self {
        let (tx, rx) = mpsc::channel();
        DiagramUi {
            renderer,
            sources,
            theme,
            zoom: 1.0,
            textures: HashMap::new(),
            inflight: HashSet::new(),
            failures: HashMap::new(),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            result_tx: tx,
            result_rx: rx,
            active_workers: 0,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, id: &str) {
        let col = ui.available_width();
        if let Some(msg) = self.failures.get(&(id.to_string(), self.theme.as_str())) {
            let msg = msg.clone();
            self.draw_error(ui, id, &msg);
            return;
        }
        match self.renderer.cached(id, self.theme) {
            Some(Ok(png)) => self.draw_png(ui, id, &png, self.theme, col),
            Some(Err(msg)) => self.draw_error(ui, id, &msg),
            None => {
                let enqueued = enqueue_diagram(
                    &self.failures,
                    &mut self.inflight,
                    &self.sources,
                    &self.queue,
                    id,
                    self.theme,
                );
                self.maybe_spawn();
                if enqueued {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                }
                // On a theme switch keep the old theme's image on screen until
                // the new render arrives; never flash a placeholder.
                let other = match self.theme {
                    Theme::Light => Theme::Dark,
                    Theme::Dark => Theme::Light,
                };
                if let Some(Ok(png)) = self.renderer.cached(id, other) {
                    self.draw_png(ui, id, &png, other, col);
                } else {
                    self.draw_placeholder(ui);
                }
            }
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        let mut got = false;
        while let Ok((id, theme, result)) = self.result_rx.try_recv() {
            got = true;
            self.inflight.remove(&id);
            self.active_workers = self.active_workers.saturating_sub(1);
            if let Err(msg) = result {
                self.failures.insert((id, theme.as_str()), msg);
            }
        }
        if !self.inflight.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if got {
            self.maybe_spawn();
            ctx.request_repaint();
        }
    }

    fn maybe_spawn(&mut self) {
        while self.active_workers < 4 {
            let job = self.queue.lock().unwrap().pop_front();
            let Some((id, source, theme)) = job else {
                break;
            };
            self.active_workers += 1;
            let renderer = Arc::clone(&self.renderer);
            let tx = self.result_tx.clone();
            thread::spawn(move || {
                let res = renderer.render(&id, &source, theme);
                let _ = tx.send((id, theme, res));
            });
        }
    }

    fn draw_png(&mut self, ui: &mut egui::Ui, id: &str, png: &Path, theme: Theme, col: f32) {
        let key = format!("{id}-{}", theme.as_str());
        if !self.textures.contains_key(&key) {
            if let Ok(bytes) = std::fs::read(png) {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let size = [w as usize, h as usize];
                    let tex = ui.ctx().load_texture(
                        format!("diagram-{key}"),
                        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
                        egui::TextureOptions::LINEAR,
                    );
                    self.textures.insert(key.clone(), (tex, size));
                }
            }
        }
        if let Some((tex, size)) = self.textures.get(&key) {
            // PNGs are rasterized at 2x; show at the intended SVG size scaled by
            // zoom, capped to the column when zoom is at or below 1.0.
            let base_w = size[0] as f32 * 0.5;
            let base_h = size[1] as f32 * 0.5;
            let (mut w, mut h) = (base_w * self.zoom, base_h * self.zoom);
            if self.zoom <= 1.0 && w > col {
                let s = col / w;
                (w, h) = (w * s, h * s);
            }
            ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(w, h)));
        } else {
            ui.colored_label(egui::Color32::RED, "diagram: failed to load image");
        }
    }

    fn draw_error(&self, ui: &mut egui::Ui, id: &str, msg: &str) {
        ui.colored_label(egui::Color32::RED, format!("diagram error: {msg}"));
        if let Some(src) = self.sources.get(id) {
            ui.code(src);
        }
    }

    fn draw_placeholder(&self, ui: &mut egui::Ui) {
        let (w, h) = (640.0 * self.zoom, 320.0 * self.zoom);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(40));
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "rendering diagram…",
            egui::FontId::proportional(16.0),
            egui::Color32::from_gray(180),
        );
    }
}

/// Keys pressed on the keyboard, with no egui Context dependency (unit-testable).
#[derive(Default)]
pub struct KeyState {
    pub last_g: Option<Instant>,
}

/// A viewer action produced by a key press.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    ScrollLines(f32),
    ScrollPages(f32),
    Top,
    Bottom,
    ZoomIn,
    ZoomOut,
    Fit,
    ToggleTheme,
    Reload,
    Quit,
}

/// How long a first `g` waits for a second one.
const GG_WINDOW: Duration = Duration::from_millis(500);

/// Map one pressed key to an [`Action`]. Pure: no egui context, no I/O.
pub fn apply_key(
    state: &mut KeyState,
    key: Key,
    modifiers: Modifiers,
    now: Instant,
) -> Option<Action> {
    match key {
        Key::J | Key::ArrowDown => Some(Action::ScrollLines(1.0)),
        Key::K | Key::ArrowUp => Some(Action::ScrollLines(-1.0)),
        Key::D => Some(Action::ScrollPages(0.5)),
        Key::U => Some(Action::ScrollPages(-0.5)),
        Key::Space => Some(Action::ScrollPages(1.0)),
        Key::B => Some(Action::ScrollPages(-1.0)),
        Key::Plus | Key::Equals => Some(Action::ZoomIn),
        Key::Minus => Some(Action::ZoomOut),
        Key::A => Some(Action::Fit),
        Key::T => Some(Action::ToggleTheme),
        Key::R => Some(Action::Reload),
        Key::Q | Key::Escape => Some(Action::Quit),
        Key::G => {
            // Shift+G (bottom) is immediate; a lone `g` starts the gg window.
            if modifiers.shift {
                state.last_g = None;
                return Some(Action::Bottom);
            }
            match state.last_g.take() {
                Some(t) if now.duration_since(t) < GG_WINDOW => Some(Action::Top),
                // A `g` after the window has elapsed: nothing to pair with, so
                // treat it as a standalone jump to the bottom.
                Some(_) => Some(Action::Bottom),
                None => {
                    state.last_g = Some(now);
                    None
                }
            }
        }
        _ => None,
    }
}

pub struct App {
    pub path: PathBuf,
    pub prepared: Prepared,
    pub renderer: Renderer,
    pub theme: Theme,
    pub zoom: f32,
    error: Option<String>,
    cache: CommonMarkCache,
    diagrams: Rc<RefCell<DiagramUi>>,
    html_fn: Box<dyn Fn(&mut egui::Ui, &str)>,
    scroll_y: f32,
    keys: KeyState,
    // Held to keep the filesystem watch alive; never read directly.
    #[allow(dead_code)]
    watcher: Option<notify::RecommendedWatcher>,
    watch_rx: Option<mpsc::Receiver<notify::Event>>,
    pending_reload: Option<Instant>,
    last_mtime: Option<SystemTime>,
    last_poll: Instant,
}

impl App {
    pub fn new(path: PathBuf, dark: bool) -> Self {
        let theme = if dark { Theme::Dark } else { Theme::Light };
        let mut renderer = Renderer::new();
        renderer.chromium = find_chromium();
        let shared = Arc::new(Renderer {
            chromium: renderer.chromium.clone(),
            mermaid_js: renderer.mermaid_js.clone(),
            cache_dir: renderer.cache_dir.clone(),
        });

        let (error, prepared) = match read_doc(&path) {
            Ok(text) => (None, prepare(&text, parent_dir(&path))),
            Err(e) => (
                Some(e),
                Prepared {
                    markdown: String::new(),
                    diagrams: HashMap::new(),
                },
            ),
        };

        let diagrams = Rc::new(RefCell::new(DiagramUi::new(
            shared,
            prepared.diagrams.clone(),
            theme,
        )));
        let html_fn: Box<dyn Fn(&mut egui::Ui, &str)> = {
            let d = Rc::clone(&diagrams);
            Box::new(move |ui: &mut egui::Ui, html: &str| {
                if let Some(id) = parse_placeholder(html) {
                    d.borrow_mut().show(ui, id);
                }
            })
        };

        let dir = parent_dir(&path).to_path_buf();
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                let _ = tx.send(ev);
            }
        })
        .ok();
        let watch_rx = match watcher.as_mut() {
            Some(w) => match w.watch(&dir, RecursiveMode::NonRecursive) {
                Ok(()) => Some(rx),
                Err(_) => {
                    watcher = None;
                    None
                }
            },
            None => None,
        };
        let last_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

        App {
            path,
            prepared,
            renderer,
            theme,
            zoom: 1.0,
            error,
            cache: CommonMarkCache::default(),
            diagrams,
            html_fn,
            scroll_y: 0.0,
            keys: KeyState::default(),
            watcher,
            watch_rx,
            pending_reload: None,
            last_mtime,
            last_poll: Instant::now(),
        }
    }

    pub fn load(&mut self) {
        match read_doc(&self.path) {
            Ok(text) => {
                self.prepared = prepare(&text, parent_dir(&self.path));
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    pub fn reload(&mut self) {
        self.load();
        self.cache = CommonMarkCache::default();
        let sources = self.prepared.diagrams.clone();
        let mut d = self.diagrams.borrow_mut();
        d.sources = sources;
        d.textures.clear();
        d.inflight.clear();
        d.failures.clear();
    }

    /// Drive live reload: drain watcher events (or poll mtime when the watcher
    /// is unavailable), debounce 100 ms, then reload once.
    fn watch_tick(&mut self, ctx: &egui::Context) {
        // Keep the event loop waking so watcher events (which arrive while the
        // UI is idle) get drained even when the user does nothing.
        ctx.request_repaint_after(Duration::from_millis(100));
        let now = Instant::now();
        if let Some(rx) = &self.watch_rx {
            while let Ok(ev) = rx.try_recv() {
                // Ignore read access events (our own reads fire them); react to
                // content/name changes only.
                if matches!(ev.kind, notify::EventKind::Access(_)) {
                    continue;
                }
                if ev.paths.iter().any(|p| same_file(p, &self.path)) {
                    self.pending_reload = Some(now);
                }
            }
        } else if now.duration_since(self.last_poll) >= Duration::from_millis(500) {
            self.last_poll = now;
            if let Ok(meta) = std::fs::metadata(&self.path) {
                if let Ok(m) = meta.modified() {
                    match self.last_mtime {
                        Some(prev) if prev != m => self.pending_reload = Some(now),
                        _ => {}
                    }
                    self.last_mtime = Some(m);
                }
            }
        }
        if let Some(t) = self.pending_reload {
            if now.duration_since(t) >= Duration::from_millis(100) {
                self.pending_reload = None;
                self.reload();
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.diagrams.borrow_mut().poll(ctx);
        self.watch_tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Turn this frame's key presses into viewer actions.
        let now = Instant::now();
        let mut acts: Vec<Action> = Vec::new();
        ui.input(|i| {
            for ev in &i.events {
                if let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } = ev
                {
                    if let Some(a) = apply_key(&mut self.keys, *key, *modifiers, now) {
                        acts.push(a);
                    }
                }
            }
        });
        let page_h = ui.available_height();
        for act in acts {
            match act {
                Action::ScrollLines(f) => self.scroll_y = (self.scroll_y + f * 40.0).max(0.0),
                Action::ScrollPages(f) => self.scroll_y = (self.scroll_y + f * page_h).max(0.0),
                Action::Top => self.scroll_y = 0.0,
                Action::Bottom => self.scroll_y = f32::MAX,
                Action::ZoomIn => self.zoom = (self.zoom * 1.1).min(3.0),
                Action::ZoomOut => self.zoom = (self.zoom / 1.1).max(0.5),
                Action::Fit => self.zoom = 1.0,
                Action::ToggleTheme => {
                    self.theme = match self.theme {
                        Theme::Light => Theme::Dark,
                        Theme::Dark => Theme::Light,
                    }
                }
                Action::Reload => self.reload(),
                Action::Quit => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }

        {
            let mut d = self.diagrams.borrow_mut();
            d.theme = self.theme;
            d.zoom = self.zoom;
        }

        egui::CentralPanel::default().show(ui, |ui| {
            // Scale text with zoom so text and diagrams stay consistent.
            let z = self.zoom;
            let styles = ui.style().text_styles.clone();
            ui.style_mut().text_styles = styles
                .into_iter()
                .map(|(kind, font)| (kind, egui::FontId::new(font.size * z, font.family)))
                .collect();

            if self.renderer.chromium.is_none() {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "warning: chromium not found — mermaid diagrams will not render",
                );
            }
            if let Some(err) = &self.error {
                ui.colored_label(egui::Color32::RED, err);
            }

            let scroll_y = self.scroll_y;
            let output = egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .vertical_scroll_offset(scroll_y)
                .show(ui, |ui| {
                    let available = ui.available_width();
                    let col = (available - 64.0).min(900.0).max(0.0);
                    let indent = ((available - col) * 0.5).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(indent);
                        ui.vertical(|ui| {
                            ui.set_width(col);
                            let html_fn = &self.html_fn;
                            let cache = &mut self.cache;
                            let md = &self.prepared.markdown;
                            CommonMarkViewer::new()
                                .max_image_width(Some(col as usize))
                                .render_html_fn(Some(&**html_fn))
                                .show(ui, cache, md);
                        });
                    });
                });
            let max_y = (output.content_size.y - output.inner_rect.height()).max(0.0);
            self.scroll_y = output.state.offset.y.clamp(0.0, max_y);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_placeholder_ids() {
        assert_eq!(
            super::parse_placeholder(r#"<mermaid id="abc123">"#),
            Some("abc123")
        );
        assert_eq!(super::parse_placeholder("<div>hi</div>"), None);
        assert_eq!(super::parse_placeholder(r#"<mermaid id="">"#), None);
    }

    #[test]
    fn read_doc_reports_missing_dir_and_bad_utf8() {
        assert!(read_doc(Path::new("/nope/missing.md"))
            .unwrap_err()
            .contains("cannot read"));
        assert!(read_doc(Path::new("/tmp")).unwrap_err().contains("cannot read"));
        let bad = std::env::temp_dir().join("mdview-bad-utf8.md");
        std::fs::write(&bad, [0xff, 0xfe]).unwrap();
        assert!(read_doc(&bad).unwrap_err().contains("UTF-8"));
        let _ = std::fs::remove_file(&bad);
    }

    fn sources() -> HashMap<String, String> {
        HashMap::from([("abc".to_string(), "graph TD; A-->B;".to_string())])
    }

    #[test]
    fn enqueue_diagram_queues_once_then_skips_inflight() {
        let failures = HashMap::new();
        let mut inflight = HashSet::new();
        let queue = Mutex::new(VecDeque::new());
        // A successful enqueue signals "must repaint" (returns true).
        assert!(enqueue_diagram(
            &failures,
            &mut inflight,
            &sources(),
            &queue,
            "abc",
            Theme::Light
        ));
        assert!(!enqueue_diagram(
            &failures,
            &mut inflight,
            &sources(),
            &queue,
            "abc",
            Theme::Light
        ));
        assert_eq!(queue.lock().unwrap().len(), 1);
    }

    #[test]
    fn failed_id_is_never_requeued() {
        // A failed render must not be queued again this session, even though the
        // on-disk cache never got a png/err (.err is only written for diagram errors).
        let failures =
            HashMap::from([(("abc".to_string(), "light"), "chromium not found".to_string())]);
        let mut inflight = HashSet::new();
        let queue = Mutex::new(VecDeque::new());
        assert!(!enqueue_diagram(
            &failures,
            &mut inflight,
            &sources(),
            &queue,
            "abc",
            Theme::Light
        ));
        assert!(inflight.is_empty());
        assert!(queue.lock().unwrap().is_empty());
    }

    #[test]
    fn failure_is_theme_scoped() {
        // A Light-mode failure must not suppress the same id under Dark.
        let failures =
            HashMap::from([(("abc".to_string(), "light"), "boom".to_string())]);
        let mut inflight = HashSet::new();
        let queue = Mutex::new(VecDeque::new());
        assert!(!enqueue_diagram(
            &failures,
            &mut inflight,
            &sources(),
            &queue,
            "abc",
            Theme::Light
        ));
        assert!(enqueue_diagram(
            &failures,
            &mut inflight,
            &sources(),
            &queue,
            "abc",
            Theme::Dark
        ));
        assert_eq!(queue.lock().unwrap().len(), 1);
    }

    #[test]
    fn missing_source_is_not_queued() {
        let failures = HashMap::new();
        let mut inflight = HashSet::new();
        let queue = Mutex::new(VecDeque::new());
        assert!(!enqueue_diagram(
            &failures,
            &mut inflight,
            &sources(),
            &queue,
            "unknown",
            Theme::Light
        ));
        assert!(inflight.is_empty());
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn j_and_k_scroll_lines() {
        let mut s = KeyState::default();
        let now = std::time::Instant::now();
        assert!(matches!(apply_key(&mut s, Key::J, Modifiers::NONE, now), Some(Action::ScrollLines(v)) if v > 0.0));
        assert!(matches!(apply_key(&mut s, Key::K, Modifiers::NONE, now), Some(Action::ScrollLines(v)) if v < 0.0));
    }

    #[test]
    fn double_g_jumps_to_top_only() {
        let mut s = KeyState::default();
        let t0 = std::time::Instant::now();
        assert_eq!(apply_key(&mut s, Key::G, Modifiers::NONE, t0), None);
        assert_eq!(apply_key(&mut s, Key::G, Modifiers::NONE, t0 + std::time::Duration::from_millis(200)), Some(Action::Top));
        assert_eq!(apply_key(&mut s, Key::G, Modifiers::NONE, t0 + std::time::Duration::from_millis(700)), None);
        assert_eq!(apply_key(&mut s, Key::G, Modifiers::NONE, t0 + std::time::Duration::from_millis(2300)), Some(Action::Bottom));
    }

    #[test]
    fn t_r_q_map_to_actions() {
        let mut s = KeyState::default();
        let now = std::time::Instant::now();
        assert_eq!(apply_key(&mut s, Key::T, Modifiers::NONE, now), Some(Action::ToggleTheme));
        assert_eq!(apply_key(&mut s, Key::R, Modifiers::NONE, now), Some(Action::Reload));
        assert_eq!(apply_key(&mut s, Key::Q, Modifiers::NONE, now), Some(Action::Quit));
        assert_eq!(apply_key(&mut s, Key::Space, Modifiers::NONE, now), Some(Action::ScrollPages(1.0)));
        assert_eq!(apply_key(&mut s, Key::A, Modifiers::NONE, now), Some(Action::Fit));
    }
}
