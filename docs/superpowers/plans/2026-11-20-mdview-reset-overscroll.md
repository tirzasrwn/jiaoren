# mdview: `0` resets zoom; bottom overscroll of half a screen

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** `0` resets zoom to 100% (leaving the current vertical position alone), and scrolling to the end of a document lets the last line rise to the middle of the window instead of stopping at the bottom edge.

**Architecture:** One new `Action::ResetZoom` in the existing pure key map, and a trailing spacer of half the viewport height appended inside the `ScrollArea`, so `max_y` (computed from `content_size`) grows by that amount.

**Tech Stack:** unchanged; egui 0.36 `ScrollArea`, `apply_key`.

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (key table and scroll behaviour amended).

## Global Constraints

- `0` → `Action::ResetZoom` → `zoom = 1.0`, horizontal offset unchanged, `scroll_y` unchanged. `a` keeps its current meaning (zoom 1.0 + pan to left).
- Overscroll: trailing pad = exactly `viewport_height * 0.5`, applied once, inside the scroll content, after the Markdown.
- Pad must not affect horizontal scrolling, the reanchor logic, or the "content never dropped" rule.
- No new deps; warning-free; themes/zoom buckets/pan/fonts untouched.

## Review Focus

- `0` does not scroll the document, does not move horizontally, and does not change the theme.
- Bottom overscroll: reaching the end shows the last line around mid-window; scrolling back up is immediate (no dead zone).
- Top of the document unchanged (no extra space above the first heading).
- Zoomed-in horizontal panning still clamps at the real content edge.

---

### Task 1: Reset key and bottom overscroll

**Files:**
- Modify: `src/app.rs`
- Test: inline tests in `src/app.rs`

**Interfaces:**
- Produces: `Action::ResetZoom`, `fn bottom_pad(viewport_height: f32) -> f32`.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn zero_resets_zoom_without_scrolling() {
    let mut s = KeyState::default();
    let now = std::time::Instant::now();
    assert_eq!(apply_key(&mut s, Key::Num0, Modifiers::NONE, now), Some(Action::ResetZoom));
}

#[test]
fn bottom_pad_is_half_the_viewport() {
    assert_eq!(super::bottom_pad(800.0), 400.0);
    assert_eq!(super::bottom_pad(0.0), 0.0);
}
```

- [ ] **Step 2: `cargo test` → RED.**

- [ ] **Step 3: Implement** the `Key::Num0` arm and the wiring (`zoom = 1.0`), then append `ui.add_space(bottom_pad(viewport_h))` as the last item inside the scroll area's content closure, where `viewport_h` is `ui.available_height()` measured before the `ScrollArea` is shown. Keep `max_y` clamping unchanged.

- [ ] **Step 4: `cargo test` green, `cargo build` warning-free.**

- [ ] **Step 5: Visual check** — open a long document, press `G` (bottom), capture the window: the last line sits near the middle, not clamped to the bottom edge. Press `0` after zooming: text returns to 100% and the view does not jump. Report captures + what changed.

- [ ] **Step 6: Commit.**
