# mdview: Catppuccin Latte (light) / Macchiato (dark)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** The viewer chrome and the mermaid diagrams use Catppuccin — Latte for the light theme, Macchiato for the dark theme — while keeping the existing contrast/readability behaviour.

**Architecture:** A new `src/theme.rs` holds both palettes as constants plus two pure builders: `visuals(Theme) -> egui::Visuals` for the window, and `mermaid_variables(Theme) -> &'static str` for the diagram theme variables. `src/app.rs` uses the first wherever it currently calls `Visuals::light()/dark()`, and `src/mermaid.rs` embeds the second in the render page's `mermaid.initialize(...)`.

**Tech Stack:** egui 0.36 `Visuals`/`Widgets`/`Selection`, existing mermaid HTML pipeline. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (this plan changes only the theme values it leaves open).

## Global Constraints

Exact palette values (from catppuccin/palette `palette.json`):

| key | Latte | Macchiato |
|---|---|---|
| rosewater | `#dc8a78` | `#f4dbd6` |
| pink | `#ea76cb` | `#f5bde6` |
| mauve | `#8839ef` | `#c6a0f6` |
| red | `#d20f39` | `#ed8796` |
| maroon | `#e64553` | `#ee99a0` |
| peach | `#fe640b` | `#f5a97f` |
| yellow | `#df8e1d` | `#eed49f` |
| green | `#40a02b` | `#a6da95` |
| teal | `#179299` | `#8bd5ca` |
| sky | `#04a5e5` | `#91d7e3` |
| blue | `#1e66f5` | `#8aadf4` |
| lavender | `#7287fd` | `#b7bdf8` |
| text | `#4c4f69` | `#cad3f5` |
| subtext1 | `#5c5f77` | `#b8c0e0` |
| subtext0 | `#6c6f85` | `#a5adcb` |
| overlay2 | `#7c7f93` | `#939ab7` |
| overlay1 | `#8c8fa1` | `#8087a2` |
| overlay0 | `#9ca0b0` | `#6e738d` |
| surface2 | `#acb0be` | `#5b6078` |
| surface1 | `#bcc0cc` | `#494d64` |
| surface0 | `#ccd0da` | `#363a4f` |
| base | `#eff1f5` | `#24273a` |
| mantle | `#e6e9ef` | `#1e2030` |
| crust | `#dce0e8` | `#181926` |

- Mapping for egui: `panel_fill`/`window_fill` = base, `faint_bg_color` = mantle, `extreme_bg_color` + `code_bg_color` = crust (light plane for code blocks), noninteractive fg = text, widget bg_fill = surface0, weak_bg_fill = surface0, hovered bg = surface1, active bg = surface2, `bg_stroke` = overlay0, `hyperlink_color` = blue, `selection.bg_fill` = lavender at ~35% alpha, `warn_fg_color` = yellow, `error_fg_color` = red, `text_edit_bg_color` = mantle. `dark_mode = false` for Latte, `true` for Macchiato.
- Text must stay legible: body text = `text` on `base` in both palettes; never use overlay colors for body text.
- No new dependencies, no sudo, build warning-free.
- Broken diagrams, banners, error boxes keep their behaviour; colors come from the palette.

## Review Focus

- Body text contrast in both themes (text-on-base), code block background distinct from the page, links visible.
- Diagram labels, edges and cluster fills legible after the theme-variable change, in both themes.
- Latte must genuinely be light (`dark_mode=false` affects egui defaults) and Macchiato dark.
- No hardcoded `Color32::from_rgb` outside `src/theme.rs`.
- Cached SVGs from the old theme are not reused after the change.

---

### Task 1: Palette module and window visuals

**Files:**
- Create: `src/theme.rs`
- Modify: `src/main.rs` (`mod theme;`), `src/app.rs` (`apply_theme`), `src/mermaid.rs` (only if `Theme` must be imported)
- Test: inline `#[cfg(test)]` in `src/theme.rs`

**Interfaces:**
- Consumes: `mermaid::Theme`.
- Produces: `pub struct Rgb(pub u8, pub u8, pub u8)`, `pub const LATTE: Palette`, `pub const MACCHIATO: Palette`, `pub struct Palette { pub base, mantle, crust, text, subtext1, subtext0, overlay2, overlay1, overlay0, surface2, surface1, surface0, blue, lavender, yellow, red, green, peach, mauve, teal: Rgb }`, `pub fn palette(theme: Theme) -> &'static Palette`, `pub fn visuals(theme: Theme) -> egui::Visuals`.

- [ ] **Step 1: Failing tests** in `src/theme.rs`

```rust
#[test]
fn palettes_carry_the_documented_base_and_text() {
    assert_eq!(palette(Theme::Light).base.0, 0xef);
    assert_eq!(palette(Theme::Light).base.to_hex(), "#eff1f5");
    assert_eq!(palette(Theme::Dark).base.to_hex(), "#24273a");
    assert_eq!(palette(Theme::Dark).text.to_hex(), "#cad3f5");
}

#[test]
fn visuals_are_light_for_latte_and_dark_for_macchiato() {
    let l = visuals(Theme::Light);
    assert!(!l.dark_mode);
    assert_eq!(l.panel_fill, palette(Theme::Light).base.into());
    let d = visuals(Theme::Dark);
    assert!(d.dark_mode);
    assert_eq!(d.panel_fill, palette(Theme::Dark).base.into());
}
```

- [ ] **Step 2: `cargo test` → RED.**

- [ ] **Step 3: Implement** `Rgb::to_hex`, `From<Rgb> for egui::Color32`, both palette constants with the table above, `palette()`, and `visuals()` in `src/theme.rs` using `egui::Visuals::light()/dark()` as the base and overriding the fields named in Global Constraints. Replace the theme-application body in `src/app.rs` with `ctx.set_visuals(theme::visuals(self.theme))` (keep `ctx.set_theme` consistent with `dark_mode`). Add `mod theme;` to `main.rs`.

- [ ] **Step 4: `cargo test` green, `cargo build` warning-free. Screenshot both themes:**

```bash
cd /home/developer/personal/mdview
DISPLAY=:0 ./target/debug/mdview /tmp/mdview-zoom.md & sleep 4
W=$(DISPLAY=:0 xdotool search --onlyvisible --class mdview | head -1)
DISPLAY=:0 import -window $W /tmp/theme-light.png
DISPLAY=:0 xdotool key t; sleep 2
DISPLAY=:0 import -window $W /tmp/theme-dark.png
```

Expected: `/tmp/theme-light.png` background is Latte base (`#eff1f5`), `/tmp/theme-dark.png` background is Macchiato base (`#24273a`); sample the corner pixel of each with `magick`/`ffmpeg` or `python3 -c` and report the values.

- [ ] **Step 5: Commit.**

---

### Task 2: Catppuccin mermaid theme variables

**Files:**
- Modify: `src/theme.rs` (add `mermaid_variables`), `src/mermaid.rs` (HTML init + `RENDER_VERSION`)
- Test: inline tests in `src/theme.rs` / `src/mermaid.rs`

**Interfaces:**
- Produces: `pub fn mermaid_variables(theme: Theme) -> &'static str` — a JS object literal fragment, e.g. `r#"primaryColor:"#ccd0da",primaryTextColor:"#4c4f69",…"#`.
- `RENDER_VERSION` → `"4-mermaid11.12.0-htmlLabelsOff-2x-strict-svg-catppuccin"`.

- [ ] **Step 1: Failing test** in `src/theme.rs`: both outputs contain their palette's base hex and text hex, and the two outputs differ.

- [ ] **Step 2: Implement** the fragment with `theme:'base'` in `initialize`, variables: `background` (base), `primaryColor` (surface0), `primaryTextColor` (text), `primaryBorderColor` (overlay1), `lineColor` (overlay2), `secondaryColor` (mantle), `tertiaryColor` (crust), `textColor`, `mainBkg` (surface0), `nodeBorder` (overlay1), `clusterBkg` (mantle), `clusterBorder` (surface1), `titleColor` (text), `edgeLabelBackground` (base), `actorBkg` (surface0), `actorBorder` (overlay1), `actorTextColor` (text), `signalColor` (text), `signalTextColor` (text), `labelBoxBkgColor` (surface0), `labelBoxBorderColor` (overlay1), `noteBkgColor` (yellow), `noteTextColor` (base), `noteBorderColor` (yellow).

- [ ] **Step 3: `cargo test` green; `cargo test renders_real_diagram -- --ignored` green. Then the visual check from Task 1 repeated with both themes, plus one document containing a flowchart and a sequence diagram; check PNG corner colors and that diagram text is not blank.**

- [ ] **Step 4: Commit.**
