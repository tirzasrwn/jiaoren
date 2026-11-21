//! Local image rendering for the `<mdimg>` html hook: decode a file on worker
//! threads with bounded memory, cache the decoded source per (id, mtime),
//! downscale it to the display size before uploading a texture, and reuse the
//! diagram geometry for the inline size and the click-to-zoom overlay.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use image::imageops::FilterType;
use image::RgbaImage;

use crate::doc::ImageSpec;

use super::geometry::{
    clamp_pan, diagram_display_size, popup_fit_size, popup_zoom_step, zoom_anchored,
};
use super::table::table_pad;
use super::{catch_worker, lock};

/// Texture cache key: the extracted image id plus its file modification time
/// (nanoseconds since the epoch, `None` when the file is missing). A changed
/// file therefore gets a new key and reloads.
type ImageKey = (String, Option<u128>);

/// Hard limits for decoding untrusted image files: at most 12k pixels per side
/// and 256 MiB of decoder allocations. A crafted huge PNG fails to a red error
/// box instead of OOMing the app.
const MAX_IMAGE_SIDE: u32 = 12_000;
const MAX_IMAGE_ALLOC: u64 = 256 * 1024 * 1024;
/// Reject the file before reading it. Compressed bombs are bounded here.
const MAX_IMAGE_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Bound the RGBA buffer handed to the texture uploader; `image::Limits` can
/// still let a decoded grayscale/RGB image expand past this on `to_rgba8`.
const MAX_IMAGE_RGBA_BYTES: u64 = 256 * 1024 * 1024;

/// How much larger than the cached texture the required texture may get before
/// it is resized again from the decoded source. The first popup zoom step is
/// exactly +25%, so use `>=` to keep that step sharp.
const TEXTURE_GROWTH: f32 = 1.25;

/// An open click-to-zoom overlay: which image, how far zoomed in, and how far
/// panned (image top-left relative to the view, redrawn every frame).
pub(crate) struct PopupState {
    pub(crate) id: String,
    pub(crate) zoom: f32,
    pan: egui::Vec2,
}

/// An uploaded texture plus the source size used for layout and the pixel size
/// it was downscaled to.
#[derive(Clone)]
struct TextureEntry {
    handle: egui::TextureHandle,
    tex_size: [u32; 2],
    intrinsic: (f32, f32),
}

/// A decoded source and (when the display is smaller) its downscaled pixels.
/// The texture is created on the UI thread; the source stays for later zoom.
struct Decoded {
    source: Arc<RgbaImage>,
    resized: Option<RgbaImage>,
    target: [u32; 2],
}

/// One worker request: decode `path` and downscale it for the layout in effect
/// when it was queued.
struct Job {
    key: ImageKey,
    path: PathBuf,
    column: f32,
    zoom: f32,
    ppp: f32,
}

type WorkerMsg = (ImageKey, Result<Decoded, String>);

/// What the caller can draw this frame.
enum LoadState {
    Ready(TextureEntry),
    Pending,
    Failed(String),
}

/// Shared, interior-mutable state backing the `<mdimg>` render path.
pub(crate) struct ImageUi {
    images: HashMap<String, ImageSpec>,
    /// Decoded RGBA sources keyed by `(id, mtime)`, kept so popup zoom can
    /// re-resize without re-reading the file.
    sources: HashMap<ImageKey, Arc<RgbaImage>>,
    /// Uploaded textures keyed the same way, downscaled to the display size.
    textures: HashMap<ImageKey, TextureEntry>,
    /// Decode failures, cached so the same bad file is not retried every frame.
    failures: HashMap<ImageKey, String>,
    /// Keys with a decode job queued or running.
    pending: HashSet<ImageKey>,
    queue: Arc<Mutex<VecDeque<Job>>>,
    result_tx: mpsc::Sender<WorkerMsg>,
    result_rx: mpsc::Receiver<WorkerMsg>,
    active_workers: usize,
    /// Reading column in effect this frame, set from the same `col` the viewer
    /// is given via `ui.set_width(col)`.
    column: f32,
    /// Document zoom, so images scale with text exactly like diagrams.
    zoom: f32,
    /// The zoom overlay, when an image was clicked open.
    pub(crate) popup: Option<PopupState>,
    /// Set when `popup` was opened this frame, so the opening click is not also
    /// read as a click on the dimmed background that would close it again.
    popup_fresh: bool,
}

impl ImageUi {
    pub(crate) fn new(images: HashMap<String, ImageSpec>) -> Self {
        let (tx, rx) = mpsc::channel();
        ImageUi {
            images,
            sources: HashMap::new(),
            textures: HashMap::new(),
            failures: HashMap::new(),
            pending: HashSet::new(),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            result_tx: tx,
            result_rx: rx,
            active_workers: 0,
            column: 0.0,
            zoom: 1.0,
            popup: None,
            popup_fresh: false,
        }
    }

    /// Record the document's reading column and zoom once per frame, before the
    /// viewer runs.
    pub(crate) fn begin_frame(&mut self, column: f32, zoom: f32) {
        self.column = column;
        self.zoom = zoom;
    }

    /// Drop all per-document decode state on reload. Running workers keep their
    /// slot until they report; stale results are discarded in `poll`.
    pub(crate) fn reset(&mut self, images: HashMap<String, ImageSpec>) {
        self.images = images;
        self.sources.clear();
        self.textures.clear();
        self.failures.clear();
        lock(&self.queue).clear();
        self.pending.clear();
        if self
            .popup
            .as_ref()
            .is_some_and(|p| !self.images.contains_key(&p.id))
        {
            self.popup = None;
        }
        self.popup_fresh = false;
    }

    pub(crate) fn close_popup(&mut self) {
        self.popup = None;
        self.popup_fresh = false;
    }

    /// Drain finished decodes, upload their textures and spawn queued work.
    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        let mut got = false;
        while let Ok((key, result)) = self.result_rx.try_recv() {
            got = true;
            self.active_workers = self.active_workers.saturating_sub(1);
            self.pending.remove(&key);
            if !self.key_is_current(&key) {
                continue;
            }
            match result {
                Ok(decoded) => {
                    let (iw, ih) = decoded.source.dimensions();
                    let image = color_image(decoded.resized.as_ref().unwrap_or(&decoded.source));
                    let handle = ctx.load_texture(
                        texture_name(&key, decoded.target),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    let entry = TextureEntry {
                        handle,
                        tex_size: decoded.target,
                        intrinsic: (iw as f32, ih as f32),
                    };
                    trace(format_args!(
                        "texture {} {}x{} (source {iw}x{ih})",
                        key.0, decoded.target[0], decoded.target[1]
                    ));
                    self.sources.insert(key.clone(), decoded.source);
                    self.textures.insert(key, entry);
                }
                Err(msg) => {
                    self.failures.insert(key, msg);
                }
            }
        }
        if !self.pending.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if got {
            self.maybe_spawn();
            ctx.request_repaint();
        }
    }

    /// Start up to four workers, diagram-style.
    fn maybe_spawn(&mut self) {
        while self.active_workers < 4 {
            let job = lock(&self.queue).pop_front();
            let Some(Job {
                key,
                path,
                column,
                zoom,
                ppp,
            }) = job
            else {
                break;
            };
            self.active_workers += 1;
            let tx = self.result_tx.clone();
            thread::spawn(move || {
                let started = std::time::Instant::now();
                trace(format_args!("decode start {}", path.display()));
                let res = catch_worker("decode", || decode_and_resize(&path, column, zoom, ppp))
                    .and_then(std::convert::identity);
                if let Err(msg) = &res {
                    trace(format_args!("decode failed {}: {msg}", path.display()));
                }
                trace(format_args!(
                    "decode done {} in {:?}",
                    path.display(),
                    started.elapsed()
                ));
                let _ = tx.send((key, res));
            });
        }
    }

    /// Draw an image inline, with the same vertical breathing room as a diagram
    /// or table above and below. The popup draws its image directly and skips
    /// this pad.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str) {
        let pad = table_pad(ui.text_style_height(&egui::TextStyle::Body));
        // The html hook runs in a wrapping left-to-right layout, where a bare
        // `add_space` advances horizontally; the vertical wrapper turns the
        // padding into real space above and below the image.
        ui.vertical(|ui| {
            ui.add_space(pad);
            self.show_unpadded(ui, id);
            ui.add_space(pad);
        });
    }

    fn show_unpadded(&mut self, ui: &mut egui::Ui, id: &str) {
        let spec = match self.images.get(id) {
            Some(s) => s.clone(),
            None => return, // unknown id: draw nothing, never panic
        };
        let key = key_for(id, &spec);
        let intrinsic = self.intrinsic_for(&key);
        let display = intrinsic.map(|size| diagram_display_size(size, self.column, self.zoom));
        match self.load_state(ui.ctx(), &key, &spec, display) {
            LoadState::Ready(entry) => {
                let (dw, dh) = display.unwrap_or_else(|| self.placeholder_size());
                let response = ui
                    .allocate_ui_with_layout(
                        egui::vec2(self.column, dh),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Image::from_texture(&entry.handle)
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
            }
            LoadState::Pending => {
                let (dw, dh) = display.unwrap_or_else(|| self.placeholder_size());
                self.draw_loading(ui, (dw, dh));
            }
            LoadState::Failed(msg) => self.draw_error(ui, &spec, &msg),
        }
    }

    /// Draw the zoom overlay for the clicked image. Mirrors the diagram popup
    /// (the geometry helpers are shared, the drawing is local): `Esc`, `q` and
    /// `Ctrl+C` close, wheel/`+`/`-` zoom, `h`/`l`/`j`/`k` and mouse drag pan,
    /// and a click on the dimmed background closes. Returns `Some(ctrl_c)` when
    /// the overlay closed this frame, so the app can keep the closing press from
    /// also quitting.
    pub(crate) fn draw_popup(&mut self, ctx: &egui::Context, column: f32) -> Option<bool> {
        let fresh = std::mem::take(&mut self.popup_fresh);
        let popup = self.popup.as_ref()?;
        let (id, mut zoom, mut pan) = (popup.id.clone(), popup.zoom, popup.pan);
        let spec = match self.images.get(&id) {
            Some(s) => s.clone(),
            None => {
                self.popup = None;
                return Some(false);
            }
        };
        let (esc, quit_key, ctrl_c, plus, minus, wheel_up, wheel_down, h, l, j, k) =
            ctx.input(|i| {
                // Read wheel notches, not `smooth_scroll_delta`: the smoothed
                // value spans many frames and would multiply one notch into
                // many steps.
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
            // Keep the same press from reaching the document next frame.
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                i.consume_key(egui::Modifiers::NONE, egui::Key::Q);
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::C);
                i.consume_key(egui::Modifiers::CTRL, egui::Key::C);
                // winit turns Ctrl+C into `Copy` and never emits a `Key::C`.
                i.events.retain(|e| !matches!(e, egui::Event::Copy));
            });
            return Some(ctrl_c);
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

        let key = key_for(&id, &spec);
        let (iw, ih) = self.intrinsic_for(&key).unwrap_or((640.0, 320.0));
        let screen = ctx.content_rect();
        let margin = 24.0;
        let avail =
            (screen.size() - egui::vec2(2.0 * margin, 2.0 * margin)).max(egui::vec2(64.0, 64.0));
        let pad = 10.0;
        // Fixed frame: the panel is always the window minus the margin, not the
        // image size, so the image can be dragged half a frame past every edge.
        let view = (avail - egui::vec2(2.0 * pad, 2.0 * pad)).max(egui::vec2(1.0, 1.0));
        let (base_w, base_h) = popup_fit_size((iw, ih), self.zoom, column, view);
        let (dw, dh) = (base_w * zoom, base_h * zoom);
        let state = self.load_state(ctx, &key, &spec, Some((dw, dh)));

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
        let dimmed = egui::Area::new(egui::Id::new("jiaoren-image-popup"))
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
                    egui::Id::new("jiaoren-image-popup-drag"),
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
                match &state {
                    LoadState::Ready(entry) => {
                        clipped.image(
                            entry.handle.id(),
                            image_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    LoadState::Failed(msg) => {
                        let galley = clipped.layout(
                            format!("image error: {}: {msg}", spec.src),
                            egui::FontId::proportional(14.0),
                            egui::Color32::RED,
                            dw.max(1.0),
                        );
                        clipped.galley(image_rect.min, galley, egui::Color32::RED);
                    }
                    LoadState::Pending => draw_loading_rect(&clipped, image_rect),
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
            return Some(false);
        }
        None
    }

    /// The current intrinsic size, from the uploaded texture or decoded source.
    fn intrinsic_for(&self, key: &ImageKey) -> Option<(f32, f32)> {
        self.textures
            .get(key)
            .map(|entry| entry.intrinsic)
            .or_else(|| {
                self.sources
                    .get(key)
                    .map(|source| (source.width() as f32, source.height() as f32))
            })
    }

    /// Load or reuse the decode for `key`, then return a texture covering
    /// `display` (already in points). Enqueues a worker on a cache miss and
    /// returns `Pending` instead of blocking the layout pass.
    fn load_state(
        &mut self,
        ctx: &egui::Context,
        key: &ImageKey,
        spec: &ImageSpec,
        display: Option<(f32, f32)>,
    ) -> LoadState {
        // A reload or a changed mtime makes every other version of this image
        // dead: drop its texture and decoded source before adding the new one.
        self.evict_other_versions(key);

        if let Some(msg) = self.failures.get(key) {
            return LoadState::Failed(msg.clone());
        }
        if let Some(entry) = self.textures.get(key).cloned() {
            if let Some(source) = self.sources.get(key).cloned() {
                let needed = texture_target(
                    [source.width(), source.height()],
                    display.unwrap_or(entry.intrinsic),
                    ctx.pixels_per_point(),
                );
                if needs_resize(entry.tex_size, needed) {
                    let entry = upload_texture(ctx, key, &source, needed);
                    self.textures.insert(key.clone(), entry.clone());
                    return LoadState::Ready(entry);
                }
            }
            return LoadState::Ready(entry);
        }
        if let Some(source) = self.sources.get(key).cloned() {
            let target = texture_target(
                [source.width(), source.height()],
                display.unwrap_or((source.width() as f32, source.height() as f32)),
                ctx.pixels_per_point(),
            );
            let entry = upload_texture(ctx, key, &source, target);
            self.textures.insert(key.clone(), entry.clone());
            return LoadState::Ready(entry);
        }
        if !self.pending.contains(key) {
            self.pending.insert(key.clone());
            lock(&self.queue).push_back(Job {
                key: key.clone(),
                path: PathBuf::from(&spec.src),
                column: self.column,
                zoom: self.zoom,
                ppp: ctx.pixels_per_point(),
            });
            self.maybe_spawn();
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        LoadState::Pending
    }

    /// Keep only `keep` for this image id: older mtimes and stale failures go.
    fn evict_other_versions(&mut self, keep: &ImageKey) {
        self.sources.retain(|key, _| key.0 != keep.0 || key == keep);
        self.textures
            .retain(|key, _| key.0 != keep.0 || key == keep);
        self.failures
            .retain(|key, _| key.0 != keep.0 || key == keep);
    }

    /// True while `key` is still the on-disk version of an image in this
    /// document. Results from workers started before a reload are dropped.
    fn key_is_current(&self, key: &ImageKey) -> bool {
        self.images
            .get(&key.0)
            .is_some_and(|spec| key_for(&key.0, spec) == *key)
    }

    fn placeholder_size(&self) -> (f32, f32) {
        diagram_display_size((640.0, 320.0), self.column, self.zoom)
    }

    fn draw_loading(&self, ui: &mut egui::Ui, (dw, dh): (f32, f32)) {
        let (rect, _) = ui
            .allocate_ui_with_layout(
                egui::vec2(self.column, dh),
                egui::Layout::top_down(egui::Align::Center),
                |ui| ui.allocate_exact_size(egui::vec2(dw, dh), egui::Sense::hover()),
            )
            .inner;
        draw_loading_rect(ui.painter(), rect);
    }

    fn draw_error(&self, ui: &mut egui::Ui, spec: &ImageSpec, msg: &str) {
        // The path appears once, in the label; the decode error carries only
        // the reason.
        ui.colored_label(
            egui::Color32::RED,
            format!("image error: {}: {msg}", spec.src),
        );
    }
}

/// Cache key for `id` at `mtime`. Pure: the `mtime` change is what forces a
/// reload, and a missing file (`None`) is its own key.
fn image_cache_key(id: &str, mtime: Option<SystemTime>) -> ImageKey {
    let stamp = mtime
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos());
    (id.to_string(), stamp)
}

fn key_for(id: &str, spec: &ImageSpec) -> ImageKey {
    image_cache_key(id, file_mtime(Path::new(&spec.src)))
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Physical texture size for a source drawn at `display` points. Never upscales
/// past the source and always keeps at least one pixel.
fn texture_target(src: [u32; 2], display: (f32, f32), ppp: f32) -> [u32; 2] {
    let (sw, sh) = (src[0].max(1), src[1].max(1));
    if !display.0.is_finite()
        || !display.1.is_finite()
        || !ppp.is_finite()
        || display.0 <= 0.0
        || display.1 <= 0.0
        || ppp <= 0.0
    {
        return [1, 1];
    }
    let req_w = (display.0 * ppp).ceil().max(1.0);
    let req_h = (display.1 * ppp).ceil().max(1.0);
    let scale = (req_w / sw as f32).min(req_h / sh as f32).clamp(0.0, 1.0);
    let tw = ((sw as f32 * scale).round() as u32).clamp(1, sw);
    let th = ((sh as f32 * scale).round() as u32).clamp(1, sh);
    [tw, th]
}

/// Whether the required texture is enough larger than the cache to justify a
/// re-resize (the image grows when the document or popup zooms in).
fn needs_resize(cached: [u32; 2], needed: [u32; 2]) -> bool {
    needed[0] as f32 >= cached[0].max(1) as f32 * TEXTURE_GROWTH
        || needed[1] as f32 >= cached[1].max(1) as f32 * TEXTURE_GROWTH
}

fn texture_name(key: &ImageKey, target: [u32; 2]) -> String {
    let stamp = key
        .1
        .map_or_else(|| "missing".to_string(), |n| n.to_string());
    format!(
        "jiaoren-image-{}-{stamp}-{}x{}",
        key.0, target[0], target[1]
    )
}

fn color_image(rgba: &RgbaImage) -> egui::ColorImage {
    let (w, h) = rgba.dimensions();
    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw())
}

/// Resize `source` to `target` and upload it. Runs on the UI thread only for a
/// zoom-driven resize of an already decoded source; the initial decode and
/// downscale run on a worker.
fn upload_texture(
    ctx: &egui::Context,
    key: &ImageKey,
    source: &RgbaImage,
    target: [u32; 2],
) -> TextureEntry {
    let started = std::time::Instant::now();
    let (sw, sh) = source.dimensions();
    let resized = (target != [sw, sh])
        .then(|| image::imageops::resize(source, target[0], target[1], FilterType::Triangle));
    let pixels = resized.as_ref().unwrap_or(source);
    let handle = ctx.load_texture(
        texture_name(key, target),
        color_image(pixels),
        egui::TextureOptions::LINEAR,
    );
    trace(format_args!(
        "texture {} {}x{} (re-resize in {:?})",
        key.0,
        target[0],
        target[1],
        started.elapsed()
    ));
    TextureEntry {
        handle,
        tex_size: target,
        intrinsic: (sw as f32, sh as f32),
    }
}

/// Trace decode/upload timings when `JIAOREN_IMAGE_TRACE` is set. Kept tiny so
/// verification can report worker latency and texture sizes without logging in
/// normal runs.
fn trace(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("JIAOREN_IMAGE_TRACE").is_some() {
        eprintln!("image: {args}");
    }
}

fn draw_loading_rect(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(40));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "loading image…",
        egui::FontId::proportional(16.0),
        egui::Color32::from_gray(180),
    );
}

/// Decode and resize on a worker thread: read, bound, decode, then downscale to
/// the display size in physical pixels.
fn decode_and_resize(path: &Path, column: f32, zoom: f32, ppp: f32) -> Result<Decoded, String> {
    let source = Arc::new(decode_source(path)?);
    let (w, h) = source.dimensions();
    let display = diagram_display_size((w as f32, h as f32), column, zoom);
    let target = texture_target([w, h], display, ppp);
    let resized = (target != [w, h]).then(|| {
        image::imageops::resize(source.as_ref(), target[0], target[1], FilterType::Triangle)
    });
    Ok(Decoded {
        source,
        resized,
        target,
    })
}

/// Bytes an RGBA buffer of `w × h` needs. Pure so the post-decode cap is
/// unit-testable without allocating the buffer.
fn rgba_bytes(w: u32, h: u32) -> u64 {
    u64::from(w) * u64::from(h) * 4
}

/// Read and decode an image file under strict size limits. Errors name the
/// reason only; the caller prints the path once.
fn decode_source(path: &Path) -> Result<RgbaImage, String> {
    let len = std::fs::metadata(path)
        .map_err(|e| format!("stat: {e}"))?
        .len();
    if len > MAX_IMAGE_FILE_BYTES {
        return Err(format!(
            "file is {len} bytes (limit {MAX_IMAGE_FILE_BYTES} bytes)"
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("format: {e}"))?;
    reader.limits(image_limits());
    let decoded = reader.decode().map_err(|e| format!("decode: {e}"))?;
    let bytes = rgba_bytes(decoded.width(), decoded.height());
    if bytes > MAX_IMAGE_RGBA_BYTES {
        return Err(format!(
            "image {}x{} needs {bytes} RGBA bytes (limit {MAX_IMAGE_RGBA_BYTES} bytes)",
            decoded.width(),
            decoded.height()
        ));
    }
    Ok(decoded.to_rgba8())
}

/// Decoder limits for untrusted files.
fn image_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC);
    limits
}

#[cfg(test)]
mod tests {
    mod image {
        use std::time::{Duration, UNIX_EPOCH};

        use super::super::*;
        use crate::app::geometry::diagram_display_size;

        #[test]
        fn display_size_reuses_the_diagram_rule() {
            // A 64x64 image stays intrinsic and centred (the caller centres it).
            assert_eq!(diagram_display_size((64.0, 64.0), 900.0, 1.0), (64.0, 64.0));
            // A 1600x300 image is capped to the column, keeping the ratio.
            let (w, h) = diagram_display_size((1600.0, 300.0), 900.0, 1.0);
            assert_eq!(w, 900.0);
            assert!((h - 900.0 * 300.0 / 1600.0).abs() < 0.01);
            // Document zoom multiplies a small image until the cap applies.
            assert_eq!(
                diagram_display_size((64.0, 64.0), 900.0, 2.0),
                (128.0, 128.0)
            );
            assert_eq!(diagram_display_size((1600.0, 300.0), 900.0, 2.0).0, 900.0);
        }

        #[test]
        fn texture_target_downscales_to_display_pixels() {
            // 4000x3000 photo in a 900-point column at 1x: ~900x675 texture,
            // not the full 12 Mpx source.
            let (dw, dh) = diagram_display_size((4000.0, 3000.0), 900.0, 1.0);
            assert_eq!(texture_target([4000, 3000], (dw, dh), 1.0), [900, 675]);
            // DPI counts: the same display at 2x gets twice the pixels.
            assert_eq!(texture_target([4000, 3000], (dw, dh), 2.0), [1800, 1350]);
            // A small source is never upscaled.
            assert_eq!(texture_target([64, 64], (64.0, 64.0), 1.0), [64, 64]);
            // Degenerate input keeps at least one pixel.
            assert_eq!(texture_target([64, 64], (0.0, 0.0), 1.0), [1, 1]);
        }

        #[test]
        fn needs_resize_at_the_first_zoom_step() {
            assert!(needs_resize([864, 648], [1080, 810]));
            assert!(!needs_resize([864, 648], [1000, 750]));
            assert!(!needs_resize([864, 648], [800, 600]));
        }

        #[test]
        fn decode_limits_bound_size_and_alloc() {
            let limits = image_limits();
            assert_eq!(limits.max_image_width, Some(12_000));
            assert_eq!(limits.max_image_height, Some(12_000));
            assert_eq!(limits.max_alloc, Some(256 * 1024 * 1024));
        }

        #[test]
        fn rgba_cap_rejects_a_12000_square_image() {
            assert_eq!(rgba_bytes(8192, 8192), MAX_IMAGE_RGBA_BYTES);
            assert!(rgba_bytes(12_000, 12_000) > MAX_IMAGE_RGBA_BYTES);
            assert!(rgba_bytes(12_000, 5_000) < MAX_IMAGE_RGBA_BYTES);
        }

        #[test]
        fn missing_file_is_an_error_not_a_panic() {
            let err = decode_source(Path::new("/nope/definitely-missing.png")).unwrap_err();
            assert!(err.contains("stat:"));
        }

        #[test]
        fn undecodable_file_is_an_error() {
            let path =
                std::env::temp_dir().join(format!("jiaoren-bad-image-{}", std::process::id()));
            std::fs::write(&path, b"not an image").unwrap();
            assert!(decode_source(&path).is_err());
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn cache_key_changes_with_mtime() {
            let id = "abc";
            let missing = image_cache_key(id, None);
            let t1 = UNIX_EPOCH + Duration::from_secs(10);
            let t2 = UNIX_EPOCH + Duration::from_secs(11);
            assert_ne!(missing, image_cache_key(id, Some(t1)));
            assert_ne!(image_cache_key(id, Some(t1)), image_cache_key(id, Some(t2)));
            assert_eq!(image_cache_key(id, Some(t1)), image_cache_key(id, Some(t1)));
            // The id still separates two images with the same mtime.
            assert_ne!(
                image_cache_key(id, Some(t1)),
                image_cache_key("xyz", Some(t1))
            );
        }

        #[test]
        fn first_load_is_pending_then_the_worker_uploads() {
            let dir =
                std::env::temp_dir().join(format!("jiaoren-image-worker-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("tiny.png");
            image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 10, 10, 255]))
                .save(&path)
                .unwrap();
            let spec = ImageSpec {
                src: path.to_string_lossy().into_owned(),
                alt: String::new(),
            };
            let images = HashMap::from([("abc".to_string(), spec.clone())]);
            let mut ui = ImageUi::new(images);
            ui.begin_frame(900.0, 1.0);
            let ctx = egui::Context::default();
            let key = key_for("abc", &spec);
            // Layout never blocks on decode: the first frame only queues work.
            assert!(matches!(
                ui.load_state(&ctx, &key, &spec, Some((64.0, 64.0))),
                LoadState::Pending
            ));
            assert!(ui.pending.contains(&key));
            for _ in 0..300 {
                ui.poll(&ctx);
                if ui.textures.contains_key(&key) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(ui.sources.contains_key(&key));
            let entry = ui.textures.get(&key).unwrap();
            assert_eq!(entry.intrinsic, (8.0, 8.0));
            assert_eq!(entry.tex_size, [8, 8]);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn reload_drops_textures_and_sources() {
            let images = HashMap::from([(
                "abc".to_string(),
                ImageSpec {
                    src: "/d/a.png".to_string(),
                    alt: String::new(),
                },
            )]);
            let mut ui = ImageUi::new(images.clone());
            let ctx = egui::Context::default();
            let key = image_cache_key("abc", None);
            let source = RgbaImage::new(4, 4);
            ui.sources.insert(key.clone(), Arc::new(source));
            ui.textures.insert(
                key.clone(),
                TextureEntry {
                    handle: ctx.load_texture(
                        "test-image",
                        egui::ColorImage::filled([4, 4], egui::Color32::RED),
                        egui::TextureOptions::LINEAR,
                    ),
                    tex_size: [4, 4],
                    intrinsic: (4.0, 4.0),
                },
            );
            ui.failures.insert(key.clone(), "boom".to_string());
            ui.pending.insert(key);
            ui.reset(images);
            assert!(ui.sources.is_empty());
            assert!(ui.textures.is_empty());
            assert!(ui.failures.is_empty());
            assert!(ui.pending.is_empty());
        }

        #[test]
        fn newer_mtime_evicts_the_old_texture_and_source() {
            let old = image_cache_key("abc", None);
            let new = image_cache_key("abc", Some(UNIX_EPOCH + Duration::from_secs(2)));
            let mut ui = ImageUi::new(HashMap::new());
            ui.sources
                .insert(old.clone(), Arc::new(RgbaImage::new(2, 2)));
            ui.failures.insert(old.clone(), "old".to_string());
            ui.evict_other_versions(&new);
            assert!(ui.sources.is_empty());
            assert!(ui.failures.is_empty());
        }
    }
}
