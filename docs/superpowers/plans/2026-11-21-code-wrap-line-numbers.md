# Code Blocks: Soft Wrap and Line Numbers — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fenced code blocks soft-wrap to the reading column and show a right-aligned line-number gutter, with search, selection, and copy still exact.

**Architecture:** All changes live in `src/app/code.rs`. Three pure helpers (line wrapping, visual-row assembly, mark clipping) are unit-tested without egui; the existing `CodeUi::show` paint loop then walks visual rows instead of source lines, paints a gutter, and records one `CharSpan` per visual row. The recorded search text stays the raw source, so selection/copy are untouched.

**Tech Stack:** Rust, egui/eframe, existing monospace font metrics. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-21-code-wrap-line-numbers-design.md`

## Global Constraints

- Rust only; no new dependencies, no system packages, no sudo.
- Gates after every task: `cargo test`, `cargo clippy --all-targets --all-features --locked -- -D warnings`, `cargo fmt --check`, `cargo build --locked` (warning-free).
- All char offsets are **char-based**, never byte-based.
- Line numbers are decoration only: never in the search text, selection, or clipboard.
- Commit as `tirzasrwn <tirzasrwn@gmail.com>`.

## Review Focus

- A single char wider than the content width (narrow window, CJK/emoji): must still occupy a row, never loop forever.
- Empty and whitespace-only lines: still keep a numbered row with the right source offset.
- Selection/hit-test across a wrapped row: pointer in a continuation row must map to the source chars on that row, not the line start.
- Search hit spanning a wrap boundary or a newline: highlight resumes on the next visual row.
- `y` selection and the copy icon: must produce raw source text (numbers never leak in), with original newlines.

---

### Task 1: Pure line wrapper

**Files:**
- Modify: `src/app/code.rs` (add helper near `code_line_spans`, tests in `mod layout`)

**Interfaces:**
- Produces: `pub(crate) fn wrap_line(line: &str, max_width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<(usize, usize)>` — `(start, len)` char ranges covering `line` in order; empty line → `vec![(0, 0)]`; a single char wider than `max_width` sits alone on its row.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn wrap_line_splits_on_width_in_chars() {
    // Every char is 10 px wide.
    let m = |s: &str| s.chars().count() as f32 * 10.0;
    assert_eq!(wrap_line("abcdef", 1000.0, &m), vec![(0, 6)]);
    assert_eq!(
        wrap_line("abcdef", 30.0, &m),
        vec![(0, 3), (3, 3)]
    );
    assert_eq!(wrap_line("", 30.0, &m), vec![(0, 0)]);
    // Leading spaces are kept and count toward the width.
    assert_eq!(
        wrap_line("  abc", 20.0, &m),
        vec![(0, 2), (2, 2)]
    );
    // One char wider than the row still gets its own row.
    assert_eq!(wrap_line("ab", 5.0, &m), vec![(0, 1), (1, 1)]);
    // Multi-byte chars keep char offsets, not byte offsets.
    assert_eq!(wrap_line("ééé", 20.0, &m), vec![(0, 2), (2, 1)]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test wrap_line_splits_on_width_in_chars`
Expected: FAIL — `wrap_line` not found.

- [ ] **Step 3: Implement `wrap_line` in `src/app/code.rs`**

Greedy: track `start` char index; for each char index `end > start`, if `measure(&line[start..end]) > max_width` then push `(start, end - start)` and set `start = end`. At the end push the tail (possibly `(start, 0)` for an empty line). Measure with `&str` slices taken by `char_indices` so slicing is byte-safe.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test wrap_line_splits_on_width_in_chars`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/code.rs
git commit -m "feat: wrap code lines to a width"
```

---

### Task 2: Visual-row assembly and gutter metrics

**Files:**
- Modify: `src/app/code.rs` (struct + helpers, tests in `mod layout`)

**Interfaces:**
- Produces: `pub(crate) struct Row { pub line: usize, pub start: usize, pub len: usize }` — source line index and char range within that line.
- Produces: `pub(crate) fn layout_rows(text: &str, max_width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<Row>` — one entry per visual row, in paint order.
- Produces: `pub(crate) fn gutter_width(line_count: usize, cell: f32) -> f32` — `digits(line_count.max(1)) as f32 * cell + cell` (number column plus one cell of gap).
- Produces: `pub(crate) fn clip_marks(marks: &[(usize, usize, bool)], row: &Row) -> Vec<(usize, usize, bool)>` — marks local to the row: intersect each `(start, end, current)` with `[row.start, row.start + row.len)` and subtract `row.start`; drop empties.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn rows_split_long_lines_and_keep_line_numbers() {
    let m = |s: &str| s.chars().count() as f32 * 10.0;
    let rows = layout_rows("abcdef\nx", 30.0, &m);
    assert_eq!(
        rows,
        vec![
            Row { line: 0, start: 0, len: 3 },
            Row { line: 0, start: 3, len: 3 },
            Row { line: 1, start: 0, len: 1 },
        ]
    );
    // An empty source line still owns one row.
    let rows = layout_rows("a\n", 100.0, &m);
    assert_eq!(rows, vec![Row { line: 0, start: 0, len: 1 }, Row { line: 1, start: 0, len: 0 }]);
}

#[test]
fn gutter_is_digits_plus_one_cell() {
    assert_eq!(gutter_width(1, 8.0), 16.0);
    assert_eq!(gutter_width(9, 8.0), 16.0);
    assert_eq!(gutter_width(10, 8.0), 24.0);
    assert_eq!(gutter_width(999, 8.0), 32.0);
    assert_eq!(gutter_width(0, 8.0), 16.0);
}

#[test]
fn marks_are_clipped_and_shifted_to_the_row() {
    let row = Row { line: 0, start: 3, len: 3 };
    assert_eq!(
        clip_marks(&[(1, 5, false), (4, 6, true), (8, 9, true)], &row),
        vec![(1, 2, false), (1, 3, true)]
    );
    assert_eq!(clip_marks(&[], &row), Vec::new());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test rows_split_long_lines_and_keep_line_numbers gutter_is_digits_plus_one_cell marks_are_clipped_and_shifted_to_the_row`
Expected: FAIL — types/helpers not found.

- [ ] **Step 3: Implement `Row`, `layout_rows`, `gutter_width`, `clip_marks`**

`layout_rows` reuses `wrap_line` per `text.split('\n')` line, tagging each emitted range with the line index. `gutter_width` uses `ilog10() + 1` on `line_count.max(1)`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib`  (or the three test names)
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/code.rs
git commit -m "feat: lay out wrapped code rows and a number gutter"
```

---

### Task 3: Paint wrapped rows, gutter numbers, and exact char maps

**Files:**
- Modify: `src/app/code.rs` (`CodeUi::show` and helpers; remove now-unused `code_lines`, `widest_line`, `block_height` if nothing else uses them)
- Test: same file, `mod layout` / new `mod paint` as needed

**Interfaces:**
- Consumes: `wrap_line`, `layout_rows`, `gutter_width`, `clip_marks`, `Row` from Tasks 1–2; existing `code_line_spans`, `line_highlights`, `line_width`, `Widgets::copy_button_at`, `SearchUi::{record, highlights, selection_range, set_height, set_rect, set_char_map, add_control}`.
- Produces: unchanged public behaviour of `CodeUi::show` except the block now fits the column and paints numbers.

- [ ] **Step 1: Add a failing test for the char-map row split**

```rust
#[test]
fn wrapped_rows_have_disjoint_char_spans() {
    let m = |s: &str| s.chars().count() as f32 * 10.0;
    let rows = layout_rows("abcdef", 20.0, &m);
    let offsets: Vec<usize> = rows.iter().map(|r| r.start).collect();
    assert_eq!(offsets, vec![0, 2, 4]);
    // Row starts plus lengths tile the line without gaps or overlap.
    assert_eq!(
        rows.iter().map(|r| (r.start, r.len)).collect::<Vec<_>>(),
        vec![(0, 2), (2, 2), (4, 2)]
    );
}

#[test]
fn pointer_on_a_continuation_row_maps_to_that_rows_chars() {
    use crate::app::selection::hit_test;
    use crate::justify::CharSpan;
    let spans = vec![
        CharSpan { start: 0, len: 2, rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(20.0, 10.0)) },
        CharSpan { start: 2, len: 2, rect: egui::Rect::from_min_size(egui::pos2(0.0, 10.0), egui::vec2(20.0, 10.0)) },
    ];
    // Midpoint of the second row: char 3, not char 1.
    assert_eq!(hit_test(&spans, egui::pos2(10.0, 15.0)), Some(3));
}
```

- [ ] **Step 2: Run it to verify it fails (if it does not already pass, fix `layout_rows`)**

Run: `cargo test wrapped_rows_have_disjoint_char_spans`
Expected: PASS once Task 2 is right; adjust the test only if the interface changed.

- [ ] **Step 3: Rewrite the paint loop in `CodeUi::show`**

Decisions pinned:
- `cell = line_width(ui, "M", &mono)`; `gutter = gutter_width(lines.len(), cell)` (source line count); content width `(self.column - gutter).max(cell)`.
- `rows = layout_rows(&spec.text, content_width, &|s| line_width(ui, s, &mono))`.
- `width = self.column.max(gutter + cell)`; `height = rows.len().max(1) as f32 * row_h + pad * 2.0`; keep the `pad = row_h * 0.5` fill and `search.set_height`/`set_rect` calls.
- Row text: `line = code-line slice`; row origin `x = rect.left() + pad + gutter`, `y = rect.top() + pad + row_index as f32 * row_h`.
- Gutter number: paint the 1-based source line number on the row where `row.start == 0` (and only there), right-aligned inside `[rect.left() + pad, rect.left() + pad + gutter - cell*0.5]`, using `ui.visuals().weak_text_color()` and `mono` at `row_h` row height. Continuation rows paint nothing in the gutter.
- Selection and search marks: compute per source line as today (`line_highlights`), then `clip_marks(&marks[line], row)` for each row and paint at the row origin using `line_width` prefixes within the row text.
- Glyphs: one `layout_no_wrap` per row of its slice; record `CharSpan { start: line_offset + row.start, len: row.len, rect: row rect }` where `line_offset` is `code_line_spans()[line].0`.
- Copy icon logic stays, anchored at `rect.left() + self.column` as today.
- Remove `widest_line` and `block_height` if unused; update/remove their tests.

- [ ] **Step 4: Run the whole suite and gates**

Run: `cargo test`
Expected: all pass (existing count plus new tests).
Run: `cargo clippy --all-targets --all-features --locked -- -D warnings`
Expected: clean.
Run: `cargo fmt --check`
Expected: clean.
Run: `cargo build --locked`
Expected: warning-free.

- [ ] **Step 5: Visual check on `DISPLAY=:0`**

Open `examples/demo.md`, scroll to `## Code`. Verify: numbers 1–3 aligned right, the `println!` line intact, `h`/`l` no longer needed, and a long code line (temporarily add one to a scratch file, e.g. 300 chars) wraps with an empty gutter on continuation rows. Select across the wrap and press `y`; `xclip -selection clipboard -o | xxd` must equal the raw source slice (no numbers, newline preserved).

- [ ] **Step 6: Commit**

```bash
git add src/app/code.rs
git commit -m "feat: wrapped code blocks with line numbers"
```

---

### Task 4: Table row hover highlight

**Files:**
- Modify: `src/app/table.rs` (`TableUi`, `show`, row fills; tests in `mod table`)

**Interfaces:**
- Produces: `pub(crate) fn row_fill_hover(dark: bool, row_index: usize) -> egui::Color32` — `surface2` for row 0 (header), `surface1` otherwise, from `crate::theme::palette`.
- Produces: `pub(crate) fn hovered_row(rects: &[egui::Rect], pos: egui::Pos2) -> Option<usize>` — first rect containing `pos`.
- Consumes: existing `row_fill`, `table_fills`, `theme::palette`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn hover_fill_uses_surface2_header_and_surface1_rows() {
    let hex = |c: egui::Color32| crate::theme::Rgb(c.r(), c.g(), c.b()).to_hex();
    assert_eq!(hex(row_fill_hover(false, 0)), "#acb0be");
    assert_eq!(hex(row_fill_hover(false, 1)), "#bcc0cc");
    assert_eq!(hex(row_fill_hover(true, 0)), "#5b6078");
    assert_eq!(hex(row_fill_hover(true, 2)), "#494d64");
}

#[test]
fn hovered_row_is_the_containing_rect_or_none() {
    let rect = |y: f32| egui::Rect::from_min_size(egui::pos2(10.0, y), egui::vec2(100.0, 20.0));
    let rects = vec![rect(0.0), rect(20.0), rect(40.0)];
    assert_eq!(hovered_row(&rects, egui::pos2(50.0, 10.0)), Some(0));
    assert_eq!(hovered_row(&rects, egui::pos2(50.0, 45.0)), Some(2));
    assert_eq!(hovered_row(&rects, egui::pos2(5.0, 10.0)), None);
    assert_eq!(hovered_row(&[], egui::pos2(5.0, 10.0)), None);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test hover_fill_uses_surface2_header_and_surface1_rows hovered_row_is_the_containing_rect_or_none`
Expected: FAIL — helpers not found.

- [ ] **Step 3: Implement the helpers**

`row_fill_hover` takes `crate::theme::palette(if dark { Dark } else { Light })` and returns `.surface2`/`.surface1`. `hovered_row` returns the first `rect.contains(pos)` index.

- [ ] **Step 4: Wire the hover into `TableUi::show`**

- Add `hover: HashMap<String, usize>` to `TableUi`; clear it in `reset`, leave it across frames in `begin_frame`.
- Before building the grid, read `let hovered = self.hover.get(id).copied();`.
- `Grid::with_row_color` returns `row_fill_hover(dark, row)` when `Some(row) == hovered`, else `row_fill(dark, row)`.
- Record each row's rect during the draw: before the row's first cell take `let top = ui.cursor().min.y`; before `end_row()` take the grid's left/right (`ui.min_rect()`) and build `Rect::from_min_max(pos2(left, top), pos2(right, ui.cursor().min.y))`; push header first, then each data row.
- After the grid closes, `let next = ui.ctx().pointer_hover_pos().and_then(|p| hovered_row(&row_rects, p));` then update `self.hover` (insert or remove) and `ui.ctx().request_repaint()` when it changed.

- [ ] **Step 5: Run tests and gates**

Run: `cargo test`
Expected: pass.
Run: `cargo clippy --all-targets --all-features --locked -- -D warnings`, `cargo fmt --check`, `cargo build --locked`
Expected: clean.

- [ ] **Step 6: Visual check on `DISPLAY=:0`**

Hover each row of the demo table: the whole row (all three columns) changes to surface1 (light `#bcc0cc` / dark `#494d64`); the header row uses surface2. Moving off returns to the normal fill. Text stays readable; clicking the copy icon still copies CSV.

- [ ] **Step 7: Commit**

```bash
git add src/app/table.rs
git commit -m "feat: highlight the hovered table row"
```

---

### Task 5: Docs and demo

**Files:**
- Modify: `README.md` (code-block paragraph and the keys/limitations notes that say code does not wrap/pans)
- Modify: `docs/assets/demo.gif` (re-record via the existing `/tmp/record-gif.sh` flow)

**Interfaces:** none.

- [ ] **Step 1: Update the README text**

Replace the "code loses syntax highlighting, does not wrap, pans via `h`/`l`" claim with: code wraps to the reading column, line numbers are shown, continuation rows have an empty gutter, search/selection still work, and copy yields raw text without numbers.

- [ ] **Step 2: Re-record the demo GIF**

Run the existing recording script, assemble the GIF (palette 128, `paletteuse=dither=none`, 11 fps), and copy it over `docs/assets/demo.gif`. Check one frame with the code block: numbers visible, wrapping correct.

- [ ] **Step 3: Gates**

Run all four gates again; expected clean.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/assets/demo.gif
git commit -m "docs: code wrapping and line numbers"
```
