# mdview: sharp mermaid at any zoom + horizontal pan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Mermaid diagrams stay sharp when zoomed in (vector, rasterized at display size), and zoomed-in content can be panned left/right instead of being clipped.

**Architecture:** The renderer keeps the SVG that Chromium already produces and writes it to the cache instead of a 2× PNG. The window draws diagrams through `egui_extras`' SVG loader, which rasterizes at the on-screen size, so zoom never scales a bitmap. The content area becomes a two-axis scroll area with keyboard horizontal panning.

**Tech Stack:** existing crate set; `egui_extras` feature changes only (`svg`, `svg_text`, `file`), no new packages.

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (this plan amends its rasterization and scrolling decisions).

## Global Constraints

- No new dependencies beyond enabling `egui_extras` features `svg`, `svg_text`, `file` (all backed by crates already in the lock file: resvg 0.45, fontdb).
- `RENDER_VERSION` bumps to `"3-mermaid11.12.0-htmlLabelsOff-2x-strict-svg"` so old PNG entries are ignored.
- Cache file per diagram: `<id>-<theme>-<RENDER_VERSION>.svg`; `.err` marker unchanged.
- Mermaid render stays `securityLevel:"strict"`, `htmlLabels:false`, `fontFamily:"sans-serif"`, Chromium flags unchanged (including `--host-resolver-rules=MAP * ~NOTFOUND`).
- Zoom clamp stays `0.5..=3.0`; `a` resets zoom to 1.0 and horizontal offset to 0.
- Text still wraps to the column `min(available_width - 64, 900)`; only overflow is panned.
- Never drop content; error states and banners keep their current behaviour.

## Review Focus

- SVG text renders (labels/glyphs) — missing `svg_text`/system fonts shows blank text; the real-diagram check must look at pixels, not just file existence.
- Zoom to 3.0 stays sharp and the full diagram is reachable with horizontal panning.
- Cached open with no Chromium still renders diagrams (SVG is on disk).
- Zoom out below 1.0 keeps aspect ratio and does not clip.
- A diagram whose SVG has no `width`/`height` attributes still gets a sane size.

---

### Task 1: Cache SVG instead of PNG

**Files:**
- Modify: `src/mermaid.rs`, `src/app.rs` (draw path only)
- Test: inline tests in `src/mermaid.rs`

**Interfaces:**
- Produces: `Renderer::svg_path(&self, id: &str, theme: Theme) -> PathBuf`; `Renderer::cached(&self, id, theme) -> Option<Result<PathBuf, String>>` now returns the `.svg` path; `pub fn parse_svg_size(svg: &str) -> Option<(f32, f32)>`.
- Consumes: nothing new.

- [ ] **Step 1: Write failing tests** in `src/mermaid.rs`:

```rust
#[test]
fn svg_path_uses_svg_extension_and_version() {
    let r = test_renderer(Path::new("/tmp/x"));
    let p = r.svg_path("abc", Theme::Light);
    assert!(p.to_string_lossy().ends_with(&format!("abc-light-{RENDER_VERSION}.svg")));
}

#[test]
fn parses_width_and_height() {
    assert_eq!(parse_svg_size(r#"<svg width="120" height="40" viewBox="0 0 120 40">"#), Some((120.0, 40.0)));
    assert_eq!(parse_svg_size(r#"<svg viewBox="0 0 120 40">"#), Some((120.0, 40.0)));
    assert_eq!(parse_svg_size("<svg>"), None);
}
```

- [ ] **Step 2: Run `cargo test` — expect RED (missing items).**

- [ ] **Step 3: Implement**

Rename `png_path`→`svg_path` and the write path: after extracting the `<svg>…</svg>` slice, write it verbatim with `atomic_write` to `svg_path`. Delete the resvg rasterization and the `image` decode in this module (resvg still used by `egui_extras`, which is fine). `parse_svg_size` takes `width`/`height` attributes in px, falling back to `viewBox`'s 3rd/4th numbers, returning `None` if neither exists. Keep `.err` behaviour and the `cached` contract, now returning the SVG path.

- [ ] **Step 4: `cargo test` green; `cargo test renders_real_diagram -- --ignored` green and report the SVG's first line.**

- [ ] **Step 5: Commit.**

---

### Task 2: Draw SVG through the loader, pan horizontally

**Files:**
- Modify: `Cargo.toml` (features), `src/app.rs`
- Test: inline tests in `src/app.rs`

**Interfaces:**
- Consumes: `Renderer::cached` (SVG path), `parse_svg_size`.
- Produces: `pub enum Action` gains `ScrollCols(f32)`; `apply_key` maps `h` → `-1.0`, `l` → `+1.0`, `Shift+←`/`Shift+→` likewise; drawing uses `egui::Image::from_uri(format!("file://{}", path.display()))`.

- [ ] **Step 1: Failing tests** in `src/app.rs` `key_tests`: `h`→`ScrollCols(v) if v < 0.0`, `l`→`Positive`, `Shift+Right`→positive. Run `cargo test` → RED.

- [ ] **Step 2: Enable loader features** in `Cargo.toml`:

```toml
egui_extras = { version = "0.36", features = ["image", "file", "svg", "svg_text"] }
```

- [ ] **Step 3: Draw** — replace the texture path in `DiagramUi::show`/`draw_png`: get intrinsic `(w, h)` from `parse_svg_size` (fallback `(640, 320)`), draw

```rust
let (w, h) = (w * self.zoom, h * self.zoom);
ui.add(egui::Image::from_uri(uri).fit_to_exact_size(egui::vec2(w, h)));
```

Keep the placeholder when uncached, using the same intrinsic size when known. Keep the error box path.

- [ ] **Step 4: Two-axis scrolling** — `ScrollArea::both()` instead of `vertical()`; keep writing back `state.offset.y` and also `state.offset.x` into `self.scroll_x`; continue applying them with `vertical_scroll_offset`/`horizontal_scroll_offset`. Wire `ScrollCols` to `scroll_x ± 40 px` clamped at 0. `a` sets `zoom = 1.0` and `scroll_x = 0.0`.

- [ ] **Step 5: `cargo test` green; `cargo build` warning-free.**

- [ ] **Step 6: Visual check** (criteria below), then commit.

```bash
cd /home/developer/personal/mdview
printf '# zoom\n\n```mermaid\ngraph LR; A[Start]-->B{ok?};\n```\n' > /tmp/mdview-zoom.md
DISPLAY=:0 ./target/debug/mdview /tmp/mdview-zoom.md &   # press + three times, then screenshot the window
DISPLAY=:0 import -window "$(DISPLAY=:0 xdotool search --onlyvisible --class mdview | head -1)" /tmp/mdview-zoom.png
```

Expected in the screenshot: diagram text sharp at 3× (no blur), labels present (not blank), right-hand content reachable after `l` presses.

- [ ] **Step 7: Commit.**
