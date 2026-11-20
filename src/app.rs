use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use crate::doc::{prepare, Prepared};
use crate::mermaid::{find_chromium, Renderer, Theme};

/// A render job handed to a worker thread: (diagram id, mermaid source, theme).
type Job = (String, String, Theme);

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

/// Shared, interior-mutable state backing the markdown html hook and the worker pool.
pub struct DiagramUi {
    renderer: Arc<Renderer>,
    sources: HashMap<String, String>,
    theme: Theme,
    zoom: f32,
    textures: HashMap<String, (egui::TextureHandle, [usize; 2])>,
    inflight: HashSet<String>,
    queue: Arc<Mutex<VecDeque<Job>>>,
    result_tx: mpsc::Sender<(String, Result<PathBuf, String>)>,
    result_rx: mpsc::Receiver<(String, Result<PathBuf, String>)>,
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
            queue: Arc::new(Mutex::new(VecDeque::new())),
            result_tx: tx,
            result_rx: rx,
            active_workers: 0,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, id: &str) {
        let col = ui.available_width();
        match self.renderer.cached(id, self.theme) {
            Some(Ok(png)) => self.draw_png(ui, id, &png, col),
            Some(Err(msg)) => {
                ui.colored_label(egui::Color32::RED, format!("diagram error: {msg}"));
                if let Some(src) = self.sources.get(id) {
                    ui.code(src);
                }
            }
            None => {
                if !self.inflight.contains(id) {
                    if let Some(src) = self.sources.get(id) {
                        self.inflight.insert(id.to_string());
                        self.queue
                            .lock()
                            .unwrap()
                            .push_back((id.to_string(), src.clone(), self.theme));
                    }
                }
                self.maybe_spawn();
                self.draw_placeholder(ui);
            }
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        let mut got = false;
        while let Ok((id, _result)) = self.result_rx.try_recv() {
            got = true;
            self.inflight.remove(&id);
            self.active_workers = self.active_workers.saturating_sub(1);
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
                let _ = tx.send((id, res));
            });
        }
    }

    fn draw_png(&mut self, ui: &mut egui::Ui, id: &str, png: &Path, col: f32) {
        let key = format!("{id}-{}", self.theme.as_str());
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
        if let Some((tex, _)) = self.textures.get(&key) {
            // PNGs are rasterized at 2x; show them at the intended SVG size,
            // scaled by zoom and capped to the content column.
            ui.add(
                egui::Image::new(tex)
                    .fit_to_original_size(0.5 * self.zoom)
                    .max_width(col),
            );
        } else {
            ui.colored_label(egui::Color32::RED, "diagram: failed to load image");
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

pub struct App {
    #[allow(dead_code)] // consumed by live reload (Task 5)
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
        }
    }

    #[allow(dead_code)] // consumed by live reload (Task 5)
    pub fn load(&mut self) {
        match read_doc(&self.path) {
            Ok(text) => {
                self.prepared = prepare(&text, parent_dir(&self.path));
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    #[allow(dead_code)] // consumed by live reload (Task 5)
    pub fn reload(&mut self) {
        self.load();
        self.cache = CommonMarkCache::default();
        let sources = self.prepared.diagrams.clone();
        let mut d = self.diagrams.borrow_mut();
        d.sources = sources;
        d.textures.clear();
        d.inflight.clear();
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.diagrams.borrow_mut().poll(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        {
            let mut d = self.diagrams.borrow_mut();
            d.theme = self.theme;
            d.zoom = self.zoom;
        }

        egui::CentralPanel::default().show(ui, |ui| {
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
            self.scroll_y = output.state.offset.y;
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
}
