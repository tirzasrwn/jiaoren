//! Local image rendering for the `<mdimg>` html hook: decode a file with the
//! `image` crate, cache its texture per (id, file mtime), and reuse the
//! diagram geometry for the inline size and the click-to-zoom overlay.

use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::doc::ImageSpec;

use super::geometry::{
    clamp_pan, diagram_display_size, popup_fit_size, popup_zoom_step, zoom_anchored,
};
use super::table::table_pad;

/// Texture cache key: the extracted image id plus its file modification time
/// (nanoseconds since the epoch, `None` when the file is missing). A changed
/// file therefore gets a new key and reloads.
type ImageKey = (String, Option<u128>);

/// An open click-to-zoom overlay: which image, how far zoomed in, and how far
/// panned (image top-left relative to the view, redrawn every frame).
pub(crate) struct PopupState {
    pub(crate) id: String,
    pub(crate) zoom: f32,
    pan: egui::Vec2,
}

/// A decoded image texture, or the decode error to draw in its place.
enum Loaded {
    Ready {
        texture: egui::TextureHandle,
        size: (f32, f32),
    },
    Failed(String),
}

/// Shared, interior-mutable state backing the `<mdimg>` render path.
pub(crate) struct ImageUi {
    images: HashMap<String, ImageSpec>,
    textures: HashMap<ImageKey, Loaded>,
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
        ImageUi {
            images,
            textures: HashMap::new(),
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

    pub(crate) fn reset(&mut self, images: HashMap<String, ImageSpec>) {
        self.images = images;
        self.textures.clear();
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
        let (texture, size, error) = self.texture_for(ui.ctx(), id, &spec);
        let Some(texture) = texture else {
            self.draw_error(ui, &spec, error.as_deref().unwrap_or("cannot load image"));
            return;
        };
        let (dw, dh) = diagram_display_size(size, self.column, self.zoom);
        let response = ui
            .allocate_ui_with_layout(
                egui::vec2(self.column, dh),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.add(
                        egui::Image::from_texture(&texture)
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

        let (texture, size, error) = self.texture_for(ctx, &id, &spec);
        let (iw, ih) = if texture.is_some() {
            size
        } else {
            (640.0, 320.0)
        };
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
                match &texture {
                    Some(tex) => {
                        clipped.image(
                            tex.id(),
                            image_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    None => {
                        let msg = error.as_deref().unwrap_or("cannot load image");
                        let galley = clipped.layout(
                            format!("image error: {msg}"),
                            egui::FontId::proportional(14.0),
                            egui::Color32::RED,
                            dw.max(1.0),
                        );
                        clipped.galley(image_rect.min, galley, egui::Color32::RED);
                        let galley = clipped.layout(
                            spec.src.clone(),
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

    /// The cached texture for `spec`, decoding and caching it on a key change.
    /// The returned texture is cloned (egui handles are cheap).
    fn texture_for(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        spec: &ImageSpec,
    ) -> (Option<egui::TextureHandle>, (f32, f32), Option<String>) {
        let key = image_cache_key(id, file_mtime(Path::new(&spec.src)));
        if !self.textures.contains_key(&key) {
            // Keep one entry per image: a changed mtime replaces the old
            // texture (and its GPU memory) instead of accumulating.
            self.textures.retain(|(other, _), _| other != id);
            let loaded = match decode_image(Path::new(&spec.src)) {
                Ok((image, size)) => {
                    let stamp = key
                        .1
                        .map_or_else(|| "missing".to_string(), |n| n.to_string());
                    let name = format!("jiaoren-image-{id}-{stamp}");
                    Loaded::Ready {
                        texture: ctx.load_texture(name, image, egui::TextureOptions::LINEAR),
                        size,
                    }
                }
                Err(msg) => Loaded::Failed(msg),
            };
            self.textures.insert(key.clone(), loaded);
        }
        match self.textures.get(&key) {
            Some(Loaded::Ready { texture, size }) => (Some(texture.clone()), *size, None),
            Some(Loaded::Failed(msg)) => (None, (640.0, 320.0), Some(msg.clone())),
            None => (None, (640.0, 320.0), Some("image not loaded".to_string())),
        }
    }

    fn draw_error(&self, ui: &mut egui::Ui, spec: &ImageSpec, msg: &str) {
        ui.colored_label(egui::Color32::RED, format!("image error: {msg}"));
        // The path is the useful part when a file is missing or undecodable.
        ui.code(&spec.src);
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

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Read and decode an image file into an egui texture image and its pixel size.
fn decode_image(path: &Path) -> Result<(egui::ColorImage, (f32, f32)), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let decoded =
        image::load_from_memory(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let rgba = decoded.to_rgba8();
    let (w, h) = rgba.dimensions();
    let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
    Ok((image, (w as f32, h as f32)))
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
        fn missing_file_is_an_error_not_a_panic() {
            let err = decode_image(Path::new("/nope/definitely-missing.png")).unwrap_err();
            assert!(err.contains("/nope/definitely-missing.png"));
        }

        #[test]
        fn undecodable_file_is_an_error() {
            let path =
                std::env::temp_dir().join(format!("jiaoren-bad-image-{}", std::process::id()));
            std::fs::write(&path, b"not an image").unwrap();
            assert!(decode_image(&path).is_err());
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
    }
}
