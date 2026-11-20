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
use crate::mermaid::{file_url, find_chromium, parse_svg_size, Renderer, Theme};

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

/// Apply the app theme to egui so the window chrome and the mermaid renders
/// agree. `set_theme` selects the style; `set_visuals` guarantees the exact
/// light/dark palette even when the OS preference is the other one.
fn apply_theme(ctx: &egui::Context, theme: Theme) {
    let (egui_theme, visuals) = match theme {
        Theme::Light => (egui::Theme::Light, egui::Visuals::light()),
        Theme::Dark => (egui::Theme::Dark, egui::Visuals::dark()),
    };
    ctx.set_theme(egui_theme);
    ctx.set_visuals(visuals);
}

/// Keep a placeholder's top edge steady across a height change. Only content
/// whose placeholder started above the viewport is re-anchored; anything at or
/// below the fold is left alone. Pure so it is unit-testable.
fn reanchor(scroll_y: f32, viewport_top: f32, placeholder_top: f32, delta: f32) -> f32 {
    if placeholder_top < viewport_top {
        (scroll_y + delta).max(0.0)
    } else {
        scroll_y.max(0.0)
    }
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

/// Last drawn placeholder for a diagram, in screen coordinates.
#[derive(Clone, Copy)]
struct Placeholder {
    top: f32,
    height: f32,
    viewport_top: f32,
}

/// Shared, interior-mutable state backing the markdown html hook and the worker pool.
pub struct DiagramUi {
    renderer: Arc<Renderer>,
    sources: HashMap<String, String>,
    theme: Theme,
    zoom: f32,
    /// Intrinsic SVG size per diagram id (keyed `id-theme`); `None` when the
    /// file has no parsable size, and absent until first read succeeds.
    sizes: HashMap<String, Option<(f32, f32)>>,
    inflight: HashSet<String>,
    failures: HashMap<(String, &'static str), String>,
    queue: Arc<Mutex<VecDeque<Job>>>,
    result_tx: mpsc::Sender<DiagramResult>,
    result_rx: mpsc::Receiver<DiagramResult>,
    active_workers: usize,
    placeholders: HashMap<String, Placeholder>,
    /// Mirror of the app's scroll offset, for the pure re-anchor arithmetic.
    scroll_y: f32,
    /// Scroll offset change to apply after this frame's layout.
    scroll_adjust: f32,
}

impl DiagramUi {
    fn new(renderer: Arc<Renderer>, sources: HashMap<String, String>, theme: Theme) -> Self {
        let (tx, rx) = mpsc::channel();
        DiagramUi {
            renderer,
            sources,
            theme,
            zoom: 1.0,
            sizes: HashMap::new(),
            inflight: HashSet::new(),
            failures: HashMap::new(),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            result_tx: tx,
            result_rx: rx,
            active_workers: 0,
            placeholders: HashMap::new(),
            scroll_y: 0.0,
            scroll_adjust: 0.0,
        }
    }

    /// Drop pending work and per-document state on reload. Running workers keep
    /// their `active_workers` slots until they report back in `poll`.
    fn reset_for_reload(&mut self, sources: HashMap<String, String>) {
        self.sources = sources;
        self.sizes.clear();
        self.inflight.clear();
        self.failures.clear();
        self.queue.lock().unwrap().clear();
        self.placeholders.clear();
    }

    pub fn show(&mut self, ui: &mut egui::Ui, id: &str) {
        if let Some(msg) = self.failures.get(&(id.to_string(), self.theme.as_str())) {
            let msg = msg.clone();
            self.draw_error(ui, id, &msg);
            return;
        }
        match self.renderer.cached(id, self.theme) {
            Some(Ok(svg)) => self.draw_svg(ui, id, &svg, self.theme),
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
                if let Some(Ok(svg)) = self.renderer.cached(id, other) {
                    self.draw_svg(ui, id, &svg, other);
                } else {
                    self.draw_placeholder(ui, id);
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

    /// Intrinsic SVG size for `id` in `theme`, reading the file once. A missing
    /// file is not cached so a just-rendered diagram is picked up next frame.
    fn intrinsic(&mut self, id: &str, theme: Theme, svg: &Path) -> Option<(f32, f32)> {
        let key = format!("{id}-{}", theme.as_str());
        if let Some(size) = self.sizes.get(&key) {
            return *size;
        }
        match std::fs::read_to_string(svg) {
            Ok(text) => {
                let size = parse_svg_size(&text);
                self.sizes.insert(key, size);
                size
            }
            Err(_) => None,
        }
    }

    fn draw_svg(&mut self, ui: &mut egui::Ui, id: &str, svg: &Path, theme: Theme) {
        let (w, h) = self.intrinsic(id, theme, svg).unwrap_or((640.0, 320.0));
        let (w, h) = (w * self.zoom, h * self.zoom);
        // The svg loader rasterizes at the requested size and caches per
        // SizeHint, so zoom changes stay sharp rather than scaling a bitmap.
        let uri = format!("file://{}", file_url(svg));
        let response = ui.add(egui::Image::from_uri(uri).fit_to_exact_size(egui::vec2(w, h)));
        if let Some(ph) = self.placeholders.remove(id) {
            let delta = response.rect.height() - ph.height;
            if delta != 0.0 {
                let new_scroll = reanchor(self.scroll_y, ph.viewport_top, ph.top, delta);
                self.scroll_adjust += new_scroll - self.scroll_y;
                ui.ctx().request_repaint();
            }
        }
    }

    fn draw_error(&self, ui: &mut egui::Ui, id: &str, msg: &str) {
        ui.colored_label(egui::Color32::RED, format!("diagram error: {msg}"));
        if let Some(src) = self.sources.get(id) {
            ui.code(src);
        }
    }

    fn draw_placeholder(&mut self, ui: &mut egui::Ui, id: &str) {
        // Use the intrinsic size when a cached SVG (either theme) reveals it, so
        // the placeholder occupies the same space the diagram will.
        let other = match self.theme {
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Light,
        };
        let path = self.renderer.svg_path(id, self.theme);
        let other_path = self.renderer.svg_path(id, other);
        let (w, h) = self
            .intrinsic(id, self.theme, &path)
            .or_else(|| self.intrinsic(id, other, &other_path))
            .unwrap_or((640.0, 320.0));
        let (w, h) = (w * self.zoom, h * self.zoom);
        let viewport_top = ui.clip_rect().top();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        self.placeholders.insert(
            id.to_string(),
            Placeholder {
                top: rect.top(),
                height: rect.height(),
                viewport_top,
            },
        );
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
    ScrollCols(f32),
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
        Key::H => Some(Action::ScrollCols(-1.0)),
        Key::L => Some(Action::ScrollCols(1.0)),
        Key::ArrowLeft if modifiers.shift => Some(Action::ScrollCols(-1.0)),
        Key::ArrowRight if modifiers.shift => Some(Action::ScrollCols(1.0)),
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
                // A `g` with no recent predecessor (including one after the
                // window elapsed) arms a fresh `gg`.
                _ => {
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
    scroll_x: f32,
    applied_theme: Option<Theme>,
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
    pub fn new(path: PathBuf, dark: bool, ctx: &egui::Context) -> Self {
        let theme = if dark { Theme::Dark } else { Theme::Light };
        apply_theme(ctx, theme);
        let mut renderer = Renderer::new();
        renderer.chromium = find_chromium();
        // First-run download happens here, once, on the UI thread, before any
        // render worker can spawn: concurrent workers must never race on the
        // shared temp file.
        if renderer.chromium.is_some() {
            let _ = renderer.ensure_mermaid_js();
        }
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
        let watch_ctx = ctx.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                // Wake the UI only for real changes, not our own read events.
                if !matches!(ev.kind, notify::EventKind::Access(_)) {
                    let _ = tx.send(ev);
                    watch_ctx.request_repaint();
                }
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
            scroll_x: 0.0,
            applied_theme: Some(theme),
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
        self.diagrams.borrow_mut().reset_for_reload(sources);
    }

    /// Drive live reload: drain watcher events (or poll mtime when the watcher
    /// is unavailable), debounce 100 ms, then reload once.
    fn watch_tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if let Some(rx) = &self.watch_rx {
            // The watcher callback requests a repaint, so an idle app does not
            // need a timer; drain whatever arrived after that wake-up.
            while let Ok(ev) = rx.try_recv() {
                if matches!(ev.kind, notify::EventKind::Access(_)) {
                    continue;
                }
                if ev.paths.iter().any(|p| same_file(p, &self.path)) {
                    self.pending_reload = Some(now);
                }
            }
        } else if now.duration_since(self.last_poll) >= Duration::from_millis(500) {
            // No watcher: fall back to polling mtime.
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
            ctx.request_repaint_after(Duration::from_millis(500));
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
        // Keep egui's window chrome and the mermaid renders on one theme.
        if self.applied_theme != Some(self.theme) {
            apply_theme(ui.ctx(), self.theme);
            self.applied_theme = Some(self.theme);
        }
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
                Action::ScrollCols(f) => self.scroll_x = (self.scroll_x + f * 40.0).max(0.0),
                Action::Top => self.scroll_y = 0.0,
                Action::Bottom => self.scroll_y = f32::MAX,
                Action::ZoomIn => self.zoom = (self.zoom * 1.1).min(3.0),
                Action::ZoomOut => self.zoom = (self.zoom / 1.1).max(0.5),
                Action::Fit => {
                    self.zoom = 1.0;
                    self.scroll_x = 0.0;
                }
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
            d.scroll_y = self.scroll_y;
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
            let scroll_x = self.scroll_x;
            let output = egui::ScrollArea::both()
                .auto_shrink([false, false])
                .vertical_scroll_offset(scroll_y)
                .horizontal_scroll_offset(scroll_x)
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
            let max_x = (output.content_size.x - output.inner_rect.width()).max(0.0);
            let adjust = std::mem::take(&mut self.diagrams.borrow_mut().scroll_adjust);
            self.scroll_y = (output.state.offset.y + adjust).clamp(0.0, max_y);
            self.scroll_x = output.state.offset.x.clamp(0.0, max_x);
            if adjust != 0.0 {
                // Re-lay out next frame at the corrected offset.
                ui.ctx().request_repaint();
            }
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

    #[test]
    fn reload_drops_queued_jobs() {
        let d = std::env::temp_dir().join(format!("mdview-reload-{}", std::process::id()));
        let r = Arc::new(Renderer {
            chromium: None,
            mermaid_js: d.join("mermaid.min.js"),
            cache_dir: d,
        });
        let mut ui = DiagramUi::new(r, sources(), Theme::Light);
        assert!(enqueue_diagram(
            &ui.failures,
            &mut ui.inflight,
            &ui.sources,
            &ui.queue,
            "abc",
            Theme::Light
        ));
        assert_eq!(ui.queue.lock().unwrap().len(), 1);
        ui.reset_for_reload(HashMap::new());
        assert!(ui.queue.lock().unwrap().is_empty());
        assert!(ui.inflight.is_empty());
        assert!(ui.sources.is_empty());
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::reanchor;

    #[test]
    fn placeholder_above_viewport_is_reanchored() {
        assert_eq!(reanchor(500.0, 100.0, 50.0, 200.0), 700.0);
    }

    #[test]
    fn placeholder_below_viewport_is_not_reanchored() {
        assert_eq!(reanchor(500.0, 100.0, 400.0, 200.0), 500.0);
    }

    #[test]
    fn partially_visible_placeholder_is_reanchored() {
        // Top above the fold, bottom still visible.
        assert_eq!(reanchor(500.0, 100.0, 50.0, -120.0), 380.0);
    }

    #[test]
    fn zero_and_negative_delta_are_clamped() {
        assert_eq!(reanchor(500.0, 100.0, 50.0, 0.0), 500.0);
        assert_eq!(reanchor(50.0, 100.0, 20.0, -100.0), 0.0);
    }
}

#[cfg(test)]
mod theme_tests {
    use super::{apply_theme, Theme};

    #[test]
    fn applying_theme_updates_active_visuals() {
        let ctx = egui::Context::default();
        apply_theme(&ctx, Theme::Light);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert!(!ctx.global_style().visuals.dark_mode);
        apply_theme(&ctx, Theme::Dark);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        assert!(ctx.global_style().visuals.dark_mode);
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
        assert_eq!(apply_key(&mut s, Key::G, Modifiers::SHIFT, t0 + std::time::Duration::from_millis(2300)), Some(Action::Bottom));
    }

    #[test]
    fn shifted_g_jumps_to_bottom_and_clears_pending_g() {
        let mut s = KeyState::default();
        let t0 = std::time::Instant::now();
        // A lone `g` arms the window...
        assert_eq!(apply_key(&mut s, Key::G, Modifiers::NONE, t0), None);
        // ...and Shift+G is bottom immediately, dropping the pending `g`.
        assert_eq!(apply_key(&mut s, Key::G, Modifiers::SHIFT, t0), Some(Action::Bottom));
        assert!(s.last_g.is_none());
        // The cleared state means the next `g` re-arms rather than going top.
        assert_eq!(apply_key(&mut s, Key::G, Modifiers::NONE, t0), None);
    }

    #[test]
    fn h_and_l_pan_horizontally() {
        let mut s = KeyState::default();
        let now = std::time::Instant::now();
        assert!(matches!(apply_key(&mut s, Key::H, Modifiers::NONE, now), Some(Action::ScrollCols(v)) if v < 0.0));
        assert!(matches!(apply_key(&mut s, Key::L, Modifiers::NONE, now), Some(Action::ScrollCols(v)) if v > 0.0));
    }

    #[test]
    fn shifted_arrows_pan_horizontally() {
        let mut s = KeyState::default();
        let now = std::time::Instant::now();
        assert!(matches!(apply_key(&mut s, Key::ArrowLeft, Modifiers::SHIFT, now), Some(Action::ScrollCols(v)) if v < 0.0));
        assert!(matches!(apply_key(&mut s, Key::ArrowRight, Modifiers::SHIFT, now), Some(Action::ScrollCols(v)) if v > 0.0));
        // Unshifted arrows keep their vertical meaning.
        assert!(matches!(apply_key(&mut s, Key::ArrowLeft, Modifiers::NONE, now), None));
    }

    #[test]
    fn zoom_keys_map_to_zoom_actions() {
        let mut s = KeyState::default();
        let now = std::time::Instant::now();
        assert_eq!(apply_key(&mut s, Key::Plus, Modifiers::NONE, now), Some(Action::ZoomIn));
        assert_eq!(apply_key(&mut s, Key::Equals, Modifiers::NONE, now), Some(Action::ZoomIn));
        assert_eq!(apply_key(&mut s, Key::Minus, Modifiers::NONE, now), Some(Action::ZoomOut));
        assert_eq!(apply_key(&mut s, Key::A, Modifiers::NONE, now), Some(Action::Fit));
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
