# mdview: FiraCode Nerd Font

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** The window chrome, the rendered Markdown, and mermaid diagrams all use FiraCode Nerd Font, with the machine's existing install and a graceful fallback when it is absent.

**Architecture:** A new `src/fonts.rs` locates `FiraCodeNerdFont-Regular.ttf` / `-Bold.ttf` from the user font directories already present on this machine (`~/.fonts`), loads them into `egui::FontDefinitions` as the primary entry for both `Proportional` and `Monospace`, and exposes the same family name for the mermaid `fontFamily` setting. resvg/Chromium already resolve the family from the system font database.

**Tech Stack:** epaint 0.36 `FontDefinitions`/`FontData`, existing mermaid HTML pipeline. No new dependencies, no network.

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (typography was left to the builder there).

## Global Constraints

- Font family name used everywhere: `"FiraCode Nerd Font"` (exact `fc-list` family string).
- Font files: `~/.fonts/FiraCodeNerdFont-Regular.ttf` and `~/.fonts/FiraCodeNerdFont-Bold.ttf` (both exist; verified). Also accept `$XDG_DATA_HOME/fonts/...` and `/usr/share/fonts/truetype/FiraCodeNerdFont-*.ttf` before giving up.
- Missing font must not panic: keep egui defaults and log nothing louder than a `warn` banner-free path (silent fallback is fine).
- FiraCode is monospace; it becomes the primary for **both** `Proportional` and `Monospace`, so prose also renders in it. Document that in README, with the one-line way back.
- `RENDER_VERSION` → `"5-mermaid11.12.0-htmlLabelsOff-2x-strict-svg-catppuccin-firacode"` (the SVG bakes `font-family`).
- Themes, zoom, buckets, pan, failure states unchanged. No new deps, no sudo, warning-free build.

## Review Focus

- Text actually changes on screen (not just font_data inserted): measure glyph shapes or compare screenshots before/after.
- Bold text is real bold (family list includes the Bold face).
- Diagram labels use FiraCode: compare a rendered label's pixels against the same diagram with the default font, or check the SVG contains the new `font-family`.
- Missing-font path: temporarily point the loader at a nonexistent dir and confirm the app still runs.
- No per-frame font registration; installed once at startup.

---

### Task 1: Font loader, wiring, mermaid family

**Files:**
- Create: `src/fonts.rs`
- Modify: `src/main.rs` (`mod fonts;`, install on startup), `src/app.rs` (install in `App::new`/`CreationContext`), `src/mermaid.rs` (font family + `RENDER_VERSION`), `README.md`
- Test: inline tests in `src/fonts.rs` and `src/mermaid.rs`

**Interfaces:**
- Produces: `pub const FAMILY: &str = "FiraCode Nerd Font"`, `pub fn load() -> Option<egui::FontDefinitions>`, `pub fn install(ctx: &egui::Context)`.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn finds_the_regular_face_on_this_machine() {
    let defs = load().expect("FiraCode Nerd Font is installed under ~/.fonts");
    assert!(defs.font_data.keys().any(|k| k.contains("FiraCode")));
    assert_eq!(defs.families[&egui::FontFamily::Proportional].first().unwrap(), "FiraCodeNerdFont-Regular");
    assert_eq!(defs.families[&egui::FontFamily::Monospace].first().unwrap(), "FiraCodeNerdFont-Regular");
}

#[test]
fn missing_files_return_none_instead_of_panicking() {
    assert!(load_from_dir(std::path::Path::new("/nonexistent/fonts")).is_none());
}
```

- [ ] **Step 2: `cargo test` → RED.**

- [ ] **Step 3: Implement** `load_from_dir(dir) -> Option<FontDefinitions>` that reads Regular and Bold from that directory, inserts them (`FontData::from_owned`), and prepends `FiraCodeNerdFont-Regular` (and keeps default fonts as fallbacks) for both families; `load()` tries `~/.fonts`, `$XDG_DATA_HOME/fonts`, `~/.local/share/fonts`, `/usr/share/fonts/truetype` in order; `install(ctx)` calls `ctx.set_fonts(defs)` once. Wire `install` at startup (inside `App::new`, before the first frame). Set mermaid `fontFamily:"FiraCode Nerd Font"` and bump `RENDER_VERSION`.

- [ ] **Step 4: `cargo test`, `cargo build` warning-free; ignored real-diagram tests pass; check the produced SVG contains `FiraCode Nerd Font`.**

- [ ] **Step 5: Visual check** — screenshot the same document before and after (or a document with bold + code + a diagram); report that text shape changed and diagram labels render. Compare widths of a fixed string as evidence.

- [ ] **Step 6: README** — note the font, the family name, and how to revert (drop the `fonts::install` call). Commit.
