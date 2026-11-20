# mdview: content column grows with zoom

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Zooming in widens the reading column (shrinking the left/right margins) instead of magnifying text inside the same 900 px measure, so a big window is fully used when zoomed.

**Architecture:** One pure helper replaces the hardcoded measure: `content_column(available, zoom) = min(available − 64, 900 × zoom)`, floored at 0. Applied where the column width is computed today, and reused by the table renderer (it takes its width from the same column `ui`).

**Tech Stack:** unchanged.

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (line 108 fixed the measure at `min(window − 64, 900)`; this amends it).

## Global Constraints

- `content_column(available, zoom)`: `(available - 64).min(900.0 * zoom)` clamped `>= 0`.
- Zoom at 1.0 keeps today's behaviour exactly (900 cap); zoom 0.5 narrows (450 cap); zoom 2/3 fills the window when it is wide enough.
- Text still wraps to the column; tables keep using the column width; pan/overscroll/reanchor untouched.
- No new deps; warning-free; themes/fonts/mermaid unchanged.

## Review Focus

- Wide window (≥ 1800 px): at zoom 1 the column is 900; at zoom 2 the column is nearly the full width and margins shrink symmetrically.
- Narrow window: no negative/zero width, no horizontal overflow from prose.
- Tables re-wrap with the wider column; no clipping.
- Zoom out to 0.5 keeps a sane, narrower measure.

---

### Task 1: zoom-aware content column

**Files:**
- Modify: `src/app.rs`
- Test: inline tests in `src/app.rs`

**Interfaces:**
- Produces: `fn content_column(available: f32, zoom: f32) -> f32`.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn column_grows_with_zoom() {
    assert_eq!(super::content_column(1800.0, 1.0), 900.0);
    assert_eq!(super::content_column(1800.0, 2.0), 1736.0);
    assert_eq!(super::content_column(1800.0, 3.0), 1736.0);
    assert_eq!(super::content_column(600.0, 1.0), 536.0);
    assert_eq!(super::content_column(1800.0, 0.5), 450.0);
    assert_eq!(super::content_column(40.0, 1.0), 0.0);
}
```

- [ ] **Step 2: `cargo test` → RED.**

- [ ] **Step 3: Implement** the helper and use it for `let col = content_column(available, self.zoom)`; pass the zoom in from the app state where the column is computed (or read it from the shared state if the closure has no direct access — state which you did). Everything else stays.

- [ ] **Step 4: `cargo test` green, `cargo build` warning-free.**

- [ ] **Step 5: Visual check** — a document with a long paragraph and the table from `/tmp/table.md`; capture at zoom 1 and after four `+` presses on a wide window (resize the window first with `xdotool windowsize <win> 1600 900` if the WM allows). Report: column width and left margin in pixels at both zooms (measure text bounding boxes or the table's left/right edges).

- [ ] **Step 6: Commit.**
