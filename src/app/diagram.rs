//! Diagram rendering state: the `<mermaid>` html hook, the render queue and
//! worker pool, and the texture bookkeeping for rasters produced by `raster`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use crate::mermaid::{parse_svg_size, Renderer, Theme};

use super::geometry::{
    bucket_for_scale, bucket_scale, clamp_pan, diagram_display_size, keep_bucket, popup_fit_size,
    popup_zoom_step, raster_key, reanchor, zoom_anchored, RasterKey,
};
use super::lock;
use super::raster::{rasterize, Raster, RasterJob};
use super::table::table_pad;

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

/// Outcome of looking up (and, when needed, queuing) the raster for a drawn
/// diagram size: the best ready texture and the bucket it was drawn from, the
/// error of a failed raster, or nothing ready yet. The bucket is what the
/// caller keeps (plus one below) in [`DiagramUi::rasters`].
enum RasterDraw {
    Ready(egui::TextureHandle),
    Failed(String),
    Pending,
}

/// An open click-to-zoom overlay: which diagram, how far zoomed in, and how far
/// panned (image top-left relative to the view, redrawn every frame).
pub(crate) struct PopupState {
    pub(crate) id: String,
    pub(crate) zoom: f32,
    pan: egui::Vec2,
}

/// Paint the grey "rendering diagram…" box used by the inline placeholder and
/// the popup while its first raster is still queued.
fn draw_placeholder_box(ui: &egui::Ui, rect: egui::Rect) {
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
    /// The zoom overlay, when a diagram was clicked open.
    pub(crate) popup: Option<PopupState>,
    /// Input time when the overlay last closed. Document keys ignore quit input
    /// for a short window after this, so a held `q`/`Ctrl+C` cannot close the
    /// overlay and then immediately quit the app on its next repeat.
    pub(crate) popup_closed_at: Option<f64>,
    /// Whether the overlay closed on `Ctrl+C`. Extends the guard above while
    /// that same Ctrl press is still held (X autorepeat starts after ~660 ms).
    pub(crate) popup_closed_with_ctrl: bool,
    /// Set when `popup` was opened this frame, so the opening click is not also
    /// read as a click on the dimmed background that would close it again.
    popup_fresh: bool,
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
            popup: None,
            popup_closed_at: None,
            popup_closed_with_ctrl: false,
            popup_fresh: false,
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
        // A popup whose diagram is gone with the new document closes.
        if self
            .popup
            .as_ref()
            .is_some_and(|p| !self.sources.contains_key(&p.id))
        {
            self.popup = None;
        }
        self.popup_fresh = false;
    }

    /// Record the document's reading column once per frame, before the viewer
    /// runs. `ui.available_width()` inside the html hook is not that column.
    pub(crate) fn begin_frame(&mut self, column: f32) {
        self.column = column;
    }

    /// Draw a diagram inline, with the same vertical breathing room as a table
    /// above and below. The popup draws its image directly and skips this pad.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str) {
        let pad = table_pad(ui.text_style_height(&egui::TextStyle::Body));
        // The html hook runs in a wrapping left-to-right layout, where a bare
        // `add_space` advances horizontally; the vertical wrapper turns the
        // padding into real space above and below the diagram.
        ui.vertical(|ui| {
            ui.add_space(pad);
            self.show_unpadded(ui, id);
            ui.add_space(pad);
        });
    }

    fn show_unpadded(&mut self, ui: &mut egui::Ui, id: &str) {
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
                    let name = format!("jiaoren-raster-{}-{}-{}-{}", key.0, key.1, key.2, key.3);
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

    /// The texture that covers drawing `id` at `display_width` points: request
    /// the exact bucket lazily, then return the sharpest ready raster at or
    /// below it. A smaller ready raster is still returned (mildly blurred)
    /// while the sharper one renders, so the caller never draws a blank frame.
    fn raster_for(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        svg: &Path,
        theme: Theme,
        intrinsic: (f32, f32),
        display_width: f32,
    ) -> (RasterDraw, u16) {
        let (iw, ih) = intrinsic;
        let ppp = ctx.pixels_per_point();
        // The raster must cover the drawn size at display density, not the
        // SVG's intrinsic CSS size: a small diagram drawn large still gets a
        // raster that matches. `display_width` is in points, so `ppp` appears
        // only here and in `bucket_scale` — never twice.
        let bucket = bucket_for_scale(display_width / iw, iw, ih, ppp);
        let key = raster_key(id, theme, bucket, ppp);
        // Request lazily (only when this size needs it); the `Pending` marker
        // keeps the same key from being queued twice.
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
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let mut failed: Option<String> = None;
        for b in (1..=bucket).rev() {
            match self.rasters.get(&raster_key(id, theme, b, ppp)) {
                Some(Raster::Ready(tex)) => return (RasterDraw::Ready(tex.clone()), b),
                Some(Raster::Failed(msg)) if failed.is_none() => failed = Some(msg.clone()),
                _ => {}
            }
        }
        match failed {
            Some(msg) => (RasterDraw::Failed(msg), bucket),
            None => (RasterDraw::Pending, bucket),
        }
    }

    /// Evict the raster textures this diagram/theme no longer needs: keep every
    /// bucket that is on screen (the inline diagram and the popup can differ)
    /// plus one below each as the best-ready fallback; drop the rest. Passing
    /// all on-screen buckets matters: pruning to just the inline one while the
    /// popup needs a sharper bucket would evict it and re-render every frame.
    /// `Pending` and `Failed` entries are markers, not GPU memory, and dropping
    /// them would re-queue or retry the same work.
    fn prune_rasters(&mut self, id: &str, theme: Theme, buckets: &[u16]) {
        let theme = theme.as_str();
        self.rasters.retain(|key, raster| {
            if key.0 != id || key.1 != theme || !matches!(raster, Raster::Ready(_)) {
                return true;
            }
            buckets.iter().any(|b| keep_bucket(*b, key.2))
        });
    }

    fn draw_svg(&mut self, ui: &mut egui::Ui, id: &str, svg: &Path, theme: Theme) {
        let (iw, ih) = self.intrinsic(id, theme, svg).unwrap_or((640.0, 320.0));
        let (dw, dh) = diagram_display_size((iw, ih), self.column, self.zoom);
        let (draw, bucket) = self.raster_for(ui.ctx(), id, svg, theme, (iw, ih), dw);
        // The popup prunes this diagram's union of buckets; doing it here too
        // would evict the sharper texture the popup is about to draw.
        if self.popup.is_none() {
            self.prune_rasters(id, theme, &[bucket]);
        }
        match draw {
            RasterDraw::Ready(tex) => {
                let response = ui
                    .allocate_ui_with_layout(
                        egui::vec2(self.column, dh),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Image::from_texture(&tex)
                                    .fit_to_exact_size(egui::vec2(dw, dh))
                                    .sense(egui::Sense::click()),
                            )
                        },
                    )
                    .inner;
                if response.clicked() {
                    self.popup = Some(PopupState {
                        id: id.to_string(),
                        zoom: 1.0,
                        pan: egui::Vec2::ZERO,
                    });
                    self.popup_fresh = true;
                    ui.ctx().request_repaint();
                }
                if let Some(ph) = self.placeholders.remove(id) {
                    // While the overlay is up the document must not shift under it.
                    let delta = response.rect.height() - ph.height;
                    if delta != 0.0 && self.popup.is_none() {
                        let new_scroll = reanchor(self.scroll_y, ph.viewport_top, ph.top, delta);
                        self.scroll_adjust += new_scroll - self.scroll_y;
                        ui.ctx().request_repaint();
                    }
                }
            }
            RasterDraw::Failed(msg) => self.draw_error(ui, id, &msg),
            RasterDraw::Pending => self.draw_placeholder(ui, id),
        }
    }

    /// Draw the zoom overlay for the clicked diagram. Its own input handling:
    /// `Esc`, `q` and `Ctrl+C` close, wheel/`+`/`-` zoom, `h`/`l`/`j`/`k` and
    /// mouse drag pan, and a click on the dimmed background closes. The
    /// document behind it does not scroll.
    pub(crate) fn draw_popup(&mut self, ctx: &egui::Context, column: f32) {
        let input_time = ctx.input(|i| i.time);
        let fresh = std::mem::take(&mut self.popup_fresh);
        let (id, mut zoom, mut pan) = match self.popup.as_ref() {
            Some(p) => (p.id.clone(), p.zoom, p.pan),
            None => return,
        };
        let theme = self.theme;
        let other = match theme {
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Light,
        };
        // Use this theme's render, else the other theme's (as the inline path
        // does while a theme switch renders), else show a cached error. Only a
        // diagram with no cached SVG at all closes the overlay.
        let (svg, raster_theme, cached_error) = match self.renderer.cached(&id, theme) {
            Some(Ok(svg)) => (Some(svg), theme, None),
            Some(Err(msg)) => (None, theme, Some(msg)),
            None => match self.renderer.cached(&id, other) {
                Some(Ok(svg)) => (Some(svg), other, None),
                Some(Err(msg)) => (None, other, Some(msg)),
                None => {
                    self.popup = None;
                    return;
                }
            },
        };
        let (esc, quit_key, ctrl_c, plus, minus, wheel_up, wheel_down, h, l, j, k) =
            ctx.input(|i| {
                // Read wheel notches, not `smooth_scroll_delta`: the smoothed value
                // spans many frames and would multiply one notch into many steps.
                let (mut up, mut down) = (false, false);
                for event in &i.events {
                    if let egui::Event::MouseWheel { delta, .. } = event {
                        up |= delta.y > 0.0;
                        down |= delta.y < 0.0;
                    }
                }
                (
                    i.key_pressed(egui::Key::Escape),
                    i.key_pressed(egui::Key::Q) && !i.modifiers.any(),
                    (i.key_pressed(egui::Key::C) && (i.modifiers.command || i.modifiers.ctrl))
                        || i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                    i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals),
                    i.key_pressed(egui::Key::Minus),
                    up,
                    down,
                    i.key_pressed(egui::Key::H),
                    i.key_pressed(egui::Key::L),
                    i.key_pressed(egui::Key::J),
                    i.key_pressed(egui::Key::K),
                )
            });
        if esc || quit_key || ctrl_c {
            self.popup = None;
            self.popup_closed_at = Some(input_time);
            self.popup_closed_with_ctrl = ctrl_c;
            // Keep the same press from reaching the document next frame: a held
            // `q`/`Ctrl+C` would otherwise quit right after closing the overlay.
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                i.consume_key(egui::Modifiers::NONE, egui::Key::Q);
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::C);
                i.consume_key(egui::Modifiers::CTRL, egui::Key::C);
                // winit turns Ctrl+C into `Copy` and never emits a `Key::C`.
                i.events.retain(|e| !matches!(e, egui::Event::Copy));
            });
            return;
        }
        let steps = i32::from(plus || wheel_up) - i32::from(minus || wheel_down);
        let zoom_before = zoom;
        zoom = popup_zoom_step(zoom, steps);
        pan += egui::vec2(
            if l {
                40.0
            } else if h {
                -40.0
            } else {
                0.0
            },
            if j {
                40.0
            } else if k {
                -40.0
            } else {
                0.0
            },
        );

        let (iw, ih) = match &svg {
            Some(svg) => self
                .intrinsic(&id, raster_theme, svg)
                .unwrap_or((640.0, 320.0)),
            None => (640.0, 320.0),
        };
        let screen = ctx.content_rect();
        let margin = 24.0;
        let avail =
            (screen.size() - egui::vec2(2.0 * margin, 2.0 * margin)).max(egui::vec2(64.0, 64.0));
        let pad = 10.0;
        // Fixed frame: the panel is always the window minus the margin, not the
        // image size, so the image can be dragged half a frame past every edge.
        let view = (avail - egui::vec2(2.0 * pad, 2.0 * pad)).max(egui::vec2(1.0, 1.0));
        // Popup zoom 1.0 fits the whole diagram inside `view`; the document
        // column may now be wider than the popup, so it cannot be the only cap.
        let (base_w, base_h) = popup_fit_size((iw, ih), self.zoom, column, view);
        let (dw, dh) = (base_w * zoom, base_h * zoom);
        let (draw, bucket) = match (&svg, &cached_error) {
            (Some(svg), _) => self.raster_for(ctx, &id, svg, raster_theme, (iw, ih), dw),
            (None, Some(msg)) => (RasterDraw::Failed(msg.clone()), 1),
            (None, None) => (RasterDraw::Pending, 1),
        };
        // Keep both the popup's bucket and the inline diagram's (the document
        // is still laid out behind the overlay), each with one below.
        let ppp = ctx.pixels_per_point();
        let inline_w = diagram_display_size((iw, ih), column, self.zoom).0;
        let inline_bucket = bucket_for_scale(inline_w / iw, iw, ih, ppp);
        self.prune_rasters(&id, raster_theme, &[bucket, inline_bucket]);

        let panel = egui::Rect::from_center_size(screen.center(), avail);
        let view_rect = egui::Rect::from_min_size(panel.min + egui::vec2(pad, pad), view);
        let image = egui::vec2(dw, dh);
        // A new popup opens centred. Without this the clamp would push a small
        // image to its edge-at-the-middle bound instead.
        if fresh {
            pan = (view - image) * 0.5;
        }
        // Anchor a zoom step at the pointer: the image point under it must stay
        // put. Outside the viewport, zoom around the centre instead.
        if zoom != zoom_before {
            let anchor = ctx
                .input(|i| i.pointer.hover_pos())
                .filter(|pos| view_rect.contains(*pos))
                .map_or(view * 0.5, |pos| pos - view_rect.min);
            let old_size = egui::vec2(base_w * zoom_before, base_h * zoom_before);
            pan = zoom_anchored(pan, anchor, old_size, image);
        }
        pan = clamp_pan(pan, image, view);
        let dimmed = egui::Area::new(egui::Id::new("jiaoren-diagram-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                let response = ui.allocate_rect(screen, egui::Sense::click());
                let painter = ui.painter().clone();
                painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(160));
                // An explicit rect each frame: an auto-sized `Area` would keep
                // the first frame's panel size and clip a zoomed image.
                painter.rect(
                    panel,
                    0.0,
                    ui.visuals().window_fill,
                    ui.visuals().window_stroke,
                    egui::StrokeKind::Inside,
                );
                let drag = ui.interact(
                    view_rect,
                    egui::Id::new("jiaoren-diagram-popup-drag"),
                    egui::Sense::drag(),
                );
                let dragging = drag.dragged();
                if dragging {
                    pan = clamp_pan(pan + drag.drag_delta(), image, view);
                }
                drag.on_hover_cursor(if dragging {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::Grab
                });
                let clipped = painter.with_clip_rect(view_rect);
                let image_rect = egui::Rect::from_min_size(view_rect.min + pan, image);
                match &draw {
                    RasterDraw::Ready(tex) => {
                        clipped.image(
                            tex.id(),
                            image_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    RasterDraw::Failed(msg) => {
                        let galley = clipped.layout(
                            format!("diagram error: {msg}"),
                            egui::FontId::proportional(14.0),
                            egui::Color32::RED,
                            dw.max(1.0),
                        );
                        clipped.galley(image_rect.min, galley, egui::Color32::RED);
                        if let Some(src) = self.sources.get(&id) {
                            let galley = clipped.layout(
                                src.clone(),
                                egui::FontId::monospace(12.0),
                                egui::Color32::from_gray(180),
                                dw.max(1.0),
                            );
                            clipped.galley(
                                image_rect.min + egui::vec2(0.0, 20.0),
                                galley,
                                egui::Color32::from_gray(180),
                            );
                        }
                    }
                    RasterDraw::Pending => {
                        clipped.rect_filled(image_rect, 0.0, egui::Color32::from_gray(40));
                        clipped.text(
                            image_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "rendering diagram…",
                            egui::FontId::proportional(16.0),
                            egui::Color32::from_gray(180),
                        );
                    }
                }
                response.clicked()
            })
            .inner;

        pan = clamp_pan(pan, image, view);
        self.popup = Some(PopupState { id, zoom, pan });
        // Only a click on the dim background outside the frame closes: clicks
        // on the frame's padding keep it open.
        let inside_panel = ctx
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|pos| panel.contains(pos));
        if dimmed && !fresh && !inside_panel {
            self.popup = None;
            self.popup_closed_at = Some(input_time);
            self.popup_closed_with_ctrl = false;
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
        draw_placeholder_box(ui, rect);
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
        fn failed_raster_is_reported_and_not_requeued() {
            let d = std::env::temp_dir().join(format!("jiaoren-failed-{}", std::process::id()));
            let r = Arc::new(Renderer {
                chromium: None,
                mermaid_js: d.join("mermaid.min.js"),
                cache_dir: d,
            });
            let mut ui = DiagramUi::new(r, sources(), Theme::Light);
            let ctx = egui::Context::default();
            let key = raster_key("abc", Theme::Light, 1, ctx.pixels_per_point());
            ui.rasters
                .insert(key.clone(), Raster::Failed("boom".to_string()));
            let (draw, bucket) = ui.raster_for(
                &ctx,
                "abc",
                Path::new("/nope.svg"),
                Theme::Light,
                (86.0, 166.0),
                86.0,
            );
            assert_eq!(bucket, 1);
            assert!(matches!(draw, RasterDraw::Failed(msg) if msg == "boom"));
            // The recorded failure stays put: no retry of the same bucket.
            assert!(matches!(ui.rasters.get(&key), Some(Raster::Failed(_))));
            assert!(lock(&ui.raster_queue).is_empty());
        }

        #[test]
        fn reload_drops_queued_jobs() {
            let d = std::env::temp_dir().join(format!("jiaoren-reload-{}", std::process::id()));
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
