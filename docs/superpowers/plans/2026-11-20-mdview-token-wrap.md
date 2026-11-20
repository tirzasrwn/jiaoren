# mdview: break long tokens, and wrap table cells with our own layout

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** A long unbreakable token (path, URL, identifier) wraps inside the column instead of pushing a table past the window and getting clipped, in body paragraphs and in table cells alike.

**Architecture:** `src/justify.rs` already wraps words and paints them. Give it a token splitter that chops an over-wide word at preferred separators (`/`, `-`, `_`, `:`, `.`, `,`, `;`, `?`, `&`, `=`, `\`) and, failing those, at a hard character budget; use the splitter both in paragraph wrapping and in a new left-aligned `draw_left` entry point. `src/app.rs` renders table cells through `draw_left` (falling back to `CommonMarkViewer` for cells with images or raw HTML), so cells wrap and break like paragraphs do.

**Tech Stack:** existing `justify` module and table renderer. No new deps.

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (table/paragraph wrapping amended).

## Global Constraints

- `split_token(word: &str, max_width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<String>`: split after a separator when the accumulated piece fits, never losing a character; hard-split at the largest char boundary that fits when no separator exists; a single char wider than `max_width` stands alone (no infinite loop).
- Wrapping stays greedy; `draw_left` = same painting as `draw` with every gap at natural space width (no stretching) and rows left-aligned.
- Table cells: `draw_left` for plain/inline-markdown cells; `CommonMarkViewer` fallback when the cell contains `![` or `<`. Cell height must be reserved so grid rows do not overlap.
- Column widths unchanged (`(column - spacing*(n-1))/n`, floor 80) and the grid must never exceed the column: if a cell's minimum chunk still exceeds its width, it overflows inside that cell only, not by widening the grid.
- Headers keep their current styling (library path is fine for headers if easier, but they must wrap too).
- No new deps; warning-free; themes/fonts/zoom/justify/pan untouched.

## Review Focus

- The screenshot case: a 5-column table with paths like `src/db/schema.ts:293-311,1148-1175;` fits the column, no clipping at the window edge, row heights grow with wrapped content.
- No character loss from splitting: the concatenation of pieces equals the original token.
- A pathological 500-char token with no separators still terminates, wraps, and reserves height.
- Body paragraphs with long URLs now wrap instead of clipping (README limitation removed).
- Cells with images/HTML still render via the library fallback, no double draw.
- Selection of break characters keeps semantic chunks (`src/db/`, `schema.ts`, `293-311`) rather than mid-word chops where possible.

---

### Task 1: Token splitting + left-aligned painting in `justify.rs`

**Files:** Modify `src/justify.rs`; Test: inline tests.

**Interfaces:**
- `pub fn split_token(word: &str, max_width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<String>`
- `pub fn draw_left(ui: &mut egui::Ui, runs: &[Run], column: f32)`
- `fn break_priority(c: char) -> u8` (higher = preferred break point).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn splits_prefer_separators_and_lose_nothing() {
    let m = |s: &str| s.chars().count() as f32 * 10.0; // fake measure
    let parts = split_token("src/db/schema.ts:293-311", 100.0, &m);
    assert!(parts.len() > 1);
    assert_eq!(parts.concat(), "src/db/schema.ts:293-311");
    assert!(parts.iter().all(|p| m(p) <= 100.0));
}

#[test]
fn hard_splits_when_no_separator_and_terminates() {
    let m = |s: &str| s.chars().count() as f32 * 10.0;
    let word = "a".repeat(300);
    let parts = split_token(&word, 100.0, &m);
    assert_eq!(parts.concat(), word);
    assert!(parts.iter().all(|p| m(p) <= 100.0));
}

#[test]
fn breaks_after_separator_not_before() {
    let m = |s: &str| s.chars().count() as f32 * 10.0;
    let parts = split_token("alpha/beta/gamma", 60.0, &m);
    assert_eq!(parts, vec!["alpha/", "beta/", "gamma"]);
}
```

- [ ] **Step 2: RED.** Step 3: implement splitter (single pass, char indices, measure only at candidate cut points), integrate into the existing wrapper (`wrap_words` or the drawing code — say which), and add `draw_left` that reuses the paint routine with zero extra gap. **Step 4:** `cargo test`, build, commit.

---

### Task 2: Table cells through the justified layout

**Files:** Modify `src/app.rs`; Test: inline tests where cheap.

**Interfaces:**
- Consumes: `justify::parse_inline`, `justify::draw_left`, existing `TableUi`.
- Produces: `fn cell_needs_library_fallback(md: &str) -> bool` (`contains("![") || contains('<')`).

- [ ] **Step 1: Failing test** for `cell_needs_library_fallback` (image → true, `<br>` → true, `**bold**` → false).
- [ ] **Step 2: RED.** Step 3: render each cell as `draw_left` with the computed width in a `ui.vertical(|ui| { ui.set_width(w); … })`, fallback to the dedicated cache + `CommonMarkViewer` for images/HTML; wrap header cells the same way; reserve height via the painter so grid rows advance correctly (mirror what the paragraph path already does).
- [ ] **Step 4:** `cargo test`, `cargo build`; then the visual check with the user's screenshot case: a 5-column table with `src/db/schema.ts:293-311,1148-1175; skill-repo.ts:45-66;` in a cell and `POST /skills/synonyms/import` in another; capture and confirm the table ends inside the column, no clipped text, rows tall enough. Report measurements.
- [ ] **Step 5:** Update `README.md` known limitations (remove the long-token clip line, note cells break at separators). Commit.
