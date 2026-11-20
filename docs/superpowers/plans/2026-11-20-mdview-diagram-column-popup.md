# mdview: diagrams span the text column, click to open a zoomable popup

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** A mermaid diagram occupies the same column width as a paragraph, its raster is generated sharp for that width, and clicking it opens an overlay where the diagram can be zoomed (1×–3×) and panned, closing with `Esc` or a click outside.

**Architecture:** `DiagramUi` receives the current reading column each frame (like `TableUi::begin_frame(col)`) and draws every diagram at `column × (column * ih/iw)`. Raster buckets stop being tied to zoom: the needed bucket is `ceil(column_px / intrinsic_css_width)`, clamped by a pixel budget, so a small SVG scaled up to the column still gets a sharp raster. The popup reuses the same bucket machinery at `popup_scale × zoom`, inside a full-window overlay with a two-axis scroll area for panning.

**Tech Stack:** existing raster worker, `egui::Image`, `egui::Area`/`Frame` overlay, `ScrollArea::both`. No new deps.

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (image sizing and interactivity amended).

## Global Constraints

- Every diagram's displayed width = current reading column (`content_column(available, zoom)`), height = `column * ih / iw`; aspect ratio preserved; no upscale of the *display* beyond the column.
- Raster bucket = `ceil(column_px / iw)` where `column_px = column * pixels_per_point`; integer buckets allowed up to the pixel budget: raster side ≤ 8192 px and raster area ≤ 16 M px. If the budget clamps the bucket, draw what is available (mild blur) rather than failing.
- Buckets are cached per `(id, theme, bucket, ppp)` as today; requests stay lazy and deduplicated; workers keep the 4-thread cap.
- Popup: opens on click (whole diagram is the click target), covers the window with a dimmed background, draws the diagram centred; zoom steps `+`/`=`/wheel-up and `-`/wheel-down, clamp `1.0..=3.0`, pan via two-axis scroll and `h`/`l`/`j`/`k`; `Esc` or a click outside the image closes it. While open, document keys (`t`, `r`, `q`, `0`, `a`) are ignored except `Esc`.
- No behaviour change elsewhere: theme toggle, failure box, placeholder, reanchor, tables, lists, paragraphs untouched.
- Tests: pure helpers (`bucket_for_scale`, `popup_clamp`, `diagram_display_size`) plus a placeholder for popup zoom steps; clippy/fmt/build gates green; no new deps.

## Review Focus

- Small diagram (intrinsic 87×160) at column 900: raster requested at the right bucket, display fills the column, text stays readable (no obvious blur), page height grows accordingly.
- Wide diagram (> column): displayed at column width, height scaled, still sharp (bucket 1 suffices).
- Click opens the popup at the same size, `+` zooms in 3 steps sharp (new bucket requested), `-` returns, `Esc` closes, clicking outside closes, clicking the image does not close.
- Popup does not steal scroll from the document while closed, and document scrolling is suspended while open.
- Pixel budget: a diagram needing bucket 12 on a huge column clamps instead of allocating an enormous pixmap; no panic.
- Reanchor: a placeholder that becomes a column-wide image adjusts scroll by the real height delta.
- Zoom out to 0.5 and in to 3.0: column changes, diagram follows, buckets follow, no stale low-res forever.

---

### Task 1: Column-fit display and scale-correct buckets

**Files:** `src/app/geometry.rs`, `src/app/diagram.rs`, `src/app/mod.rs` (pass column), tests inline.

**Interfaces:**
- `pub(crate) fn diagram_display_size(intrinsic: (f32, f32), column: f32) -> (f32, f32)` — width = column, height = column * ih/iw (guard `iw <= 0.0` → `(column, column * 0.5)`).
- `pub(crate) fn bucket_for_scale(scale: f32, iw: f32, ih: f32) -> u16` — `ceil(scale).max(1)` clamped by side ≤ 8192 / (iw, ih) and area ≤ 16 M px; returns at least 1.
- `DiagramUi::begin_frame(&mut self, column: f32)`.

- [ ] **Step 1: failing tests** for `diagram_display_size` (900 column, 87×160 → 900×1655; zero width guard) and `bucket_for_scale` (column 900, iw 87, ppp 1 → 11; 2000×1500 at column 900 → 1; absurd 10×10 needing 100 → clamped by budget, still ≥1 and finite).
- [ ] **Step 2: RED.** Step 3: implement helpers; store `column` on `DiagramUi`; replace `bucket_for(self.zoom)` with `bucket_for_scale(column * ppp / iw)`; draw at `diagram_display_size` (no `self.zoom` multiplication); `App` calls `begin_frame(col)` where it already computes the column. Keep reanchor using the drawn height. **Step 4:** gates + tests; visual check `/tmp/justify.md` and a wide flowchart doc; measure displayed width vs column and raster width from the texture size; commit.

---

### Task 2: Click-to-zoom popup

**Files:** `src/app/diagram.rs` (interaction + overlay), `src/app/mod.rs` (overlay draw + key routing), `src/app/geometry.rs` (zoom clamp helper), tests.

**Interfaces:**
- `pub(crate) struct PopupState { pub id: String, pub zoom: f32 }`, `DiagramUi::popup: Option<PopupState>`.
- `pub(crate) fn popup_zoom_step(zoom: f32, delta: i32) -> f32` (clamp 1.0..=3.0, step 0.25).
- `DiagramUi::draw_popup(&mut self, ctx: &egui::Context, column: f32)`.

- [ ] **Step 1: failing tests** for `popup_zoom_step` (1.0 +1 → 1.25; 3.0 +1 → 3.0; 1.0 -1 → 1.0).
- [ ] **Step 2: RED.** Step 3: clicking the drawn image sets `popup = Some(..)`; overlay uses a full-window `Area` (`egui::Order::Foreground`) with a dim background (`Color32::from_black_alpha(160)`), a centred frame, and its own `ScrollArea::both`; image drawn at `diagram_display_size(intrinsic, column) * popup.zoom` with `bucket_for_scale(display_px / iw)`; input: wheel/`+`/`-` change zoom, `Esc` or a click outside the image closes, document keys suppressed while open. **Step 4:** gates; visual check with a click followed by three `+` presses (screenshots), measure sharpness (raster ≥ display px) and that the popup closes on `Esc`; commit.
