# mdview: instant zoom (raster buckets in background)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Pressing `+`/`-` repaints immediately; diagrams never re-rasterize on the UI thread while zooming, and stay sharp at every zoom step.

**Architecture:** Stop handing diagrams to `egui_extras`' SVG loader (it rasterizes synchronously at the requested size, so every zoom step blocks a frame). Rasterize the cached SVG ourselves with `resvg` into a small set of **resolution buckets** (1×, 2×, 3× of `intrinsic_size × pixels_per_point`), each on a background thread, and keep them as egui textures. Zoom picks the smallest bucket that is at least as large as the on-screen size and downscales it — instant, and never upscaled more than 1% (sharp). Buckets are requested lazily: 2× only once zoom passes 1.1, 3× only past 2.1, so an unzoomed view pays for one raster.

**Tech Stack:** `resvg` 0.48 (direct dependency again), `egui::ColorImage`/`TextureHandle`, existing thread/channel plumbing. `egui_extras` keeps `image`/`file` for Markdown images.

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (amends the rasterization decision only; SVG caches, keys, themes stay).

## Global Constraints

- Cache artifacts unchanged: `<id>-<theme>-<RENDER_VERSION>.svg`, `.err`, Chromium pipeline untouched. No `RENDER_VERSION` bump.
- Mermaid rendering, themes, failure map, reload, reanchor, pan keys unchanged.
- Zoom clamp `0.5..=3.0`; `a` still resets zoom and pan.
- Rasterization must run off the UI thread; the UI thread may only decode a ready `ColorImage` into a texture.
- Fonts: `usvg::Options::default()` must load system fonts (`fontdb_mut().load_system_fonts()`), otherwise `<text>` labels vanish.
- Bucket scale = `bucket as f32 * pixels_per_point`; bucket in `{1,2,3}`.
- No new dependencies beyond re-adding `resvg = "0.48"`; build warning-free; no sudo.

## Review Focus

- Zoom from 1.0 to 3.0 and back: no frame blocks longer than a raster thread can hide, images never go blurry beyond one step, no blank flash.
- Labels still render (fonts loaded) at every bucket.
- `pixels_per_point` changes (moving the window to another DPI) do not serve stale low-res rasters forever.
- Memory: buckets are freed on reload and when the doc changes; no unbounded map growth across reloads.
- A diagram whose SVG fails to parse falls back to the existing red error box; no panic, no worker stuck.

---

### Task 1: Bucket picker + background rasterizer

**Files:**
- Modify: `Cargo.toml` (add `resvg = "0.48"`), `src/app.rs`
- Test: inline tests in `src/app.rs`

**Interfaces:**
- Produces: `fn bucket_for(zoom: f32) -> u8` (smallest of 1..=3 covering `zoom`), `fn bucket_scale(bucket: u8, pixels_per_point: f32) -> f32`, `enum Raster { Ready(egui::TextureHandle), Pending, Failed(String) }`, and a `RasterCache` in `DiagramUi` keyed by `(id, theme, bucket, ppp_rounded_2dp)`.
- Consumes: `Renderer::cached` (SVG path), `parse_svg_size`, `theme`.

- [ ] **Step 1: Failing tests** in `src/app.rs`:

```rust
#[test]
fn buckets_cover_the_zoom() {
    assert_eq!(super::bucket_for(0.5), 1);
    assert_eq!(super::bucket_for(1.0), 1);
    assert_eq!(super::bucket_for(1.4), 2);
    assert_eq!(super::bucket_for(2.6), 3);
    assert_eq!(super::bucket_for(3.0), 3);
}

#[test]
fn bucket_scale_multiplies_by_dpi() {
    assert_eq!(super::bucket_scale(2, 1.0), 2.0);
    assert_eq!(super::bucket_scale(1, 1.25), 1.25);
}
```

- [ ] **Step 2: `cargo test` → RED.**

- [ ] **Step 3: Implement the rasterizer**

In `src/app.rs`, on a cache miss for `(id, theme, bucket)`:
- read the SVG file, run `usvg::Tree::from_data(bytes, &options)` (options built once in a `LazyLock` with system fonts loaded), compute `scale = bucket_scale(bucket, ctx.pixels_per_point())`, allocate `tiny_skia::Pixmap` of `ceil(w*scale) × ceil(h*scale)`, `resvg::render(&tree, Transform::from_scale(scale, scale), &mut pixmap)`;
- convert `pixmap.data()` (premultiplied RGBA) to `egui::ColorImage` — use `ColorImage::from_rgba_premultiplied` if present in egui 0.36, else un-premultiply;
- send the `ColorImage` plus key to the existing result channel, insert into the raster cache on `poll`, and `ctx.load_texture` there (UI thread only creates textures);
- bound concurrent rasters with the existing `active_workers < 4` accounting or a dedicated counter; never queue the same key twice.

`draw_svg` then picks `bucket = bucket_for(self.zoom)`, draws the best ready bucket with `fit_to_exact_size(intrinsic × zoom)`, and requests the next bucket up when `zoom` needs it. If the exact bucket is pending but a smaller one is ready, draw the smaller one immediately (mild blur until the better one lands) instead of a placeholder.

- [ ] **Step 4: `cargo test` green; `cargo build` warning-free; run the ignored real-diagram tests and report SVG sizes.**

- [ ] **Step 5: Commit.**

---

### Task 2: Verify zoom latency and sharpness

**Files:**
- Modify: `src/app.rs` only if the measurement finds a defect.

- [ ] **Step 1: Measure** — with a cached diagram open, script 10 `+` presses 100 ms apart, capture the window after each; report max inter-capture file-size change and whether any capture shows a blank/placeholder frame:

```bash
cd /home/developer/personal/mdview
DISPLAY=:0 ./target/debug/mdview /tmp/mdview-zoom.md & sleep 5
W=$(DISPLAY=:0 xdotool search --onlyvisible --class mdview | head -1)
DISPLAY=:0 xdotool windowactivate --sync $W
for i in $(seq 10); do DISPLAY=:0 xdotool key plus; sleep 0.1; DISPLAY=:0 import -window $W /tmp/zoomstep-$i.png; done
```

- [ ] **Step 2: Inspect** the first, fifth and tenth captures by eye (read them as images): text sharp at every step, no blank diagram, no torn frame.

- [ ] **Step 3: Latency check** — time a single `+` press with `xdotool key --clearmodifiers plus` in a loop and assert no single frame exceeds ~250 ms (`ffmpeg`/`import` wall time is dominated by capture; instead report the app's own behaviour: no placeholder appears in the captures).

- [ ] **Step 4: Commit** any fix, then report measurements.
