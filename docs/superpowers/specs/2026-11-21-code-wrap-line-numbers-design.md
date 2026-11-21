# Code blocks: soft wrap + line numbers

Date: 2026-11-21
Status: approved

## Intent

Fenced code blocks currently render verbatim, never wrap, and widen past the
reading column so `h`/`l` can pan. Reading code on a narrow window means
horizontal panning. Wrap long lines to the column and add a line-number gutter
so wrapped code stays readable and referenceable.

## Goals

- Soft-wrap every code line to the reading column.
- Right-aligned line-number gutter, numbered per **source** line.
- Wrapped continuation rows keep an **empty gutter** (chosen option 1).
- Search hits, selection painting, and hit-testing keep working per visual row.
- Copy (toolbar icon and `y` selection) still yields the raw source text, no
  numbers, original newlines.

## Non-goals

- Syntax highlighting.
- Re-numbering continuation rows, folding, or per-line copy buttons.
- Any wrapping change outside fenced code blocks.

## Design

- Gutter width = digits of the largest source line number × monospace cell
  advance + one cell of gap; padded like the block (`row_h * 0.5`).
- Content width = `column - gutter`; lines longer than that are split at any
  character boundary (long tokens break), leading indentation preserved.
- Block height = total visual rows × `row_h` (+ padding). Block width = the
  reading column; code no longer forces horizontal pan.
- Numbers are painted in the weak text colour, right-aligned in the gutter,
  one per source line on its first visual row.
- Search/selection char maps store one span per **visual row** with its source
  char offset and row rect, so hit-testing and highlight painting stay exact.
- Highlights are clipped per visual row; a match spanning a wrap or a newline
  resumes on the next row.
- `search.record` keeps the raw code text, so `selected_text` and the copy
  button are unchanged.
- Line numbers are decoration: never part of the text model, selection, search
  text, or clipboard.

## Constraints

- Rust only, existing egui/eframe stack, no new dependencies.
- All existing gates stay clean: `cargo test`, clippy `-D warnings`,
  `cargo fmt --check`, `cargo build --locked`.
- Character-offset maths must stay char-based (unicode-safe), matching the rest
  of the viewer.

## Trade-offs

- Wrapping at any character can split tokens; acceptable for prose-width code
  without syntax highlighting.
- The empty gutter on continuation rows costs a little width but keeps numbers
  honest and matching `less -N`/VS Code.
- Per-visual-row char spans add a little bookkeeping in `code.rs`, but keep
  selection and search correct instead of special-casing wrapped rows.

## Left to the builder

- Helper shapes (e.g. a `VisualRow` struct) and exact gutter spacing.
- Test placement and how to assert wrap offsets deterministically.
