//! Diagram rendering state: the `<mermaid>` html hook, the render queue and
//! worker pool, and the texture bookkeeping for rasters produced by `raster`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use crate::mermaid::{parse_svg_size, Renderer, Theme};

use super::geometry::{
    bucket_for_scale, bucket_scale, diagram_display_size, raster_key, reanchor, RasterKey,
};
use super::lock;
use super::raster::{rasterize, Raster, RasterJob};

/// A render job handed to a worker thread: (diagram id, mermaid source, theme).
type Job = (String, String, Theme);

/// A finished render delivered back to the UI thread: (id, theme, outcome).
type DiagramResult = (String, Theme, Result<PathBuf, String>);

/// Everything a worker delivers back to the UI thread.
enum WorkerMsg {
    Diagram(DiagramResult),
    Raster(RasterKey, Result<egui::ColorImage, String>),
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
        lock(queue).push_back((id.to_string(), src.clone(), theme));
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
pub(crate) struct DiagramUi {
    renderer: Arc<Renderer>,
    sources: HashMap<String, String>,
    pub(crate) theme: Theme,
    pub(crate) zoom: f32,
    /// Reading column in effect this frame, set from the same `col` the viewer
    /// is given via `ui.set_width(col)`.
    column: f32,
    /// Intrinsic SVG size per diagram id (keyed `id-theme`); `None` when the
    /// file has no parsable size, and absent until first read succeeds.
    sizes: HashMap<String, Option<(f32, f32)>>,
    inflight: HashSet<String>,
    failures: HashMap<(String, &'static str), String>,
    queue: Arc<Mutex<VecDeque<Job>>>,
    result_tx: mpsc::Sender<WorkerMsg>,
    result_rx: mpsc::Receiver<WorkerMsg>,
    active_workers: usize,
    /// Rasterization jobs awaiting a free worker slot.
    raster_queue: Arc<Mutex<VecDeque<RasterJob>>>,
    /// Raster state per [`RasterKey`]; also the "queued/in-flight" marker via
    /// [`Raster::Pending`], so a key is never enqueued twice.
    rasters: HashMap<RasterKey, Raster>,
    placeholders: HashMap<String, Placeholder>,
    /// Mirror of the app's scroll offset, for the pure re-anchor arithmetic.
    pub(crate) scroll_y: f32,
    /// Scroll offset change to apply after this frame's layout.
    pub(crate) scroll_adjust: f32,
}

impl DiagramUi {
    pub(crate) fn new(
        renderer: Arc<Renderer>,
        sources: HashMap<String, String>,
        theme: Theme,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        DiagramUi {
            renderer,
            sources,
            theme,
            zoom: 1.0,
            column: 0.0,
            sizes: HashMap::new(),
            inflight: HashSet::new(),
            failures: HashMap::new(),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            result_tx: tx,
            result_rx: rx,
            active_workers: 0,
            raster_queue: Arc::new(Mutex::new(VecDeque::new())),
            rasters: HashMap::new(),
            placeholders: HashMap::new(),
            scroll_y: 0.0,
            scroll_adjust: 0.0,
        }
    }

    /// Drop pending work and per-document state on reload. Running workers keep
    /// their `active_workers` slots until they report back in `poll`.
    pub(crate) fn reset_for_reload(&mut self, sources: HashMap<String, String>) {
        self.sources = sources;
        self.sizes.clear();
        self.inflight.clear();
        self.failures.clear();
        lock(&self.queue).clear();
        self.placeholders.clear();
        // Drop every raster with the document: dropping the `TextureHandle`s
        // frees the GPU textures.
        self.rasters.clear();
        lock(&self.raster_queue).clear();
    }

    /// Record the document's reading column once per frame, before the viewer
    /// runs. `ui.available_width()` inside the html hook is not that column.
    pub(crate) fn begin_frame(&mut self, column: f32) {
        self.column = column;
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str) {
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
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
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

    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        let mut got = false;
        while let Ok(msg) = self.result_rx.try_recv() {
            got = true;
            self.active_workers = self.active_workers.saturating_sub(1);
            match msg {
                WorkerMsg::Diagram((id, theme, result)) => {
                    self.inflight.remove(&id);
                    if let Err(msg) = result {
                        self.failures.insert((id, theme.as_str()), msg);
                    }
                }
                WorkerMsg::Raster(key, Ok(image)) => {
                    // Only the UI thread ever creates a texture.
                    let name = format!("mdview-raster-{}-{}-{}-{}", key.0, key.1, key.2, key.3);
                    let handle = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
                    self.rasters.insert(key, Raster::Ready(handle));
                }
                WorkerMsg::Raster(key, Err(msg)) => {
                    self.rasters.insert(key, Raster::Failed(msg));
                }
            }
        }
        if !self.inflight.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.rasters.values().any(|r| matches!(r, Raster::Pending)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if got {
            self.maybe_spawn();
            ctx.request_repaint();
        }
    }

    fn maybe_spawn(&mut self) {
        while self.active_workers < 4 {
            // Diagram renders gate new content, so they go first.
            let job = lock(&self.queue).pop_front();
            if let Some((id, source, theme)) = job {
                self.active_workers += 1;
                let renderer = Arc::clone(&self.renderer);
                let tx = self.result_tx.clone();
                thread::spawn(move || {
                    let res = renderer
                        .render(&id, &source, theme)
                        .map_err(|e| e.to_string());
                    let _ = tx.send(WorkerMsg::Diagram((id, theme, res)));
                });
                continue;
            }
            let job = lock(&self.raster_queue).pop_front();
            if let Some(RasterJob {
                key,
                path,
                w,
                h,
                scale,
            }) = job
            {
                self.active_workers += 1;
                let tx = self.result_tx.clone();
                thread::spawn(move || {
                    let res = rasterize(&path, w, h, scale);
                    let _ = tx.send(WorkerMsg::Raster(key, res));
                });
                continue;
            }
            break;
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
        let (iw, ih) = self.intrinsic(id, theme, svg).unwrap_or((640.0, 320.0));
        let ppp = ui.ctx().pixels_per_point();
        let (dw, dh) = diagram_display_size((iw, ih), self.column, self.zoom);
        // The raster must cover the drawn size at display density, not the
        // SVG's intrinsic CSS size: a small diagram is drawn 1:1, a wide one is
        // capped to the column and still gets a raster that matches.
        let bucket = bucket_for_scale(dw * ppp / iw, iw, ih);
        let key = raster_key(id, theme, bucket, ppp);
        // Request the exact bucket lazily (only when this size needs it); the
        // `Pending` marker keeps the same key from being queued twice.
        if !self.rasters.contains_key(&key) {
            let scale = bucket_scale(bucket, ppp);
            self.rasters.insert(key.clone(), Raster::Pending);
            lock(&self.raster_queue).push_back(RasterJob {
                key,
                path: svg.to_path_buf(),
                w: iw,
                h: ih,
                scale,
            });
            self.maybe_spawn();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        // Draw the best ready bucket at or below the needed one: a smaller
        // ready raster is shown (mildly blurred) until the sharper one lands.
        let mut failed: Option<String> = None;
        for b in (1..=bucket).rev() {
            match self.rasters.get(&raster_key(id, theme, b, ppp)) {
                Some(Raster::Ready(tex)) => {
                    let tex = tex.clone();
                    let response = ui
                        .allocate_ui_with_layout(
                            egui::vec2(self.column, dh),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Image::from_texture(&tex)
                                        .fit_to_exact_size(egui::vec2(dw, dh)),
                                )
                            },
                        )
                        .inner;
                    if let Some(ph) = self.placeholders.remove(id) {
                        let delta = response.rect.height() - ph.height;
                        if delta != 0.0 {
                            let new_scroll =
                                reanchor(self.scroll_y, ph.viewport_top, ph.top, delta);
                            self.scroll_adjust += new_scroll - self.scroll_y;
                            ui.ctx().request_repaint();
                        }
                    }
                    return;
                }
                Some(Raster::Failed(msg)) if failed.is_none() => failed = Some(msg.clone()),
                _ => {}
            }
        }
        if let Some(msg) = failed {
            self.draw_error(ui, id, &msg);
        } else {
            self.draw_placeholder(ui, id);
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
        let (w, h) = diagram_display_size((w, h), self.column, self.zoom);
        let viewport_top = ui.clip_rect().top();
        let (rect, _) = ui
            .allocate_ui_with_layout(
                egui::vec2(self.column, h),
                egui::Layout::top_down(egui::Align::Center),
                |ui| ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover()),
            )
            .inner;
        self.placeholders.insert(
            id.to_string(),
            Placeholder {
                top: rect.top(),
                height: rect.height(),
                viewport_top,
            },
        );
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_gray(40));
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "rendering diagram…",
            egui::FontId::proportional(16.0),
            egui::Color32::from_gray(180),
        );
    }
}

#[cfg(test)]
mod tests {
    mod diagram {
        use super::super::*;

        fn sources() -> HashMap<String, String> {
            HashMap::from([("abc".to_string(), "graph TD; A-->B;".to_string())])
        }

        #[test]
        fn lock_recovers_poisoned_mutex_and_keeps_queued_work() {
            let queue = Mutex::new(VecDeque::new());
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = queue.lock().unwrap();
                panic!("poison the queue");
            }));
            assert!(queue.is_poisoned());
            let failures = HashMap::new();
            let mut inflight = HashSet::new();
            assert!(enqueue_diagram(
                &failures,
                &mut inflight,
                &sources(),
                &queue,
                "abc",
                Theme::Light
            ));
            assert_eq!(lock(&queue).len(), 1);
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
            let failures = HashMap::from([(
                ("abc".to_string(), "light"),
                "chromium not found".to_string(),
            )]);
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
            let failures = HashMap::from([(("abc".to_string(), "light"), "boom".to_string())]);
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
}
