# mdview: wrap long table cells

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** A Markdown table never stretches the document sideways; cell text wraps inside its column, with the table fitting the content column.

**Architecture:** `egui_commonmark` renders tables with `egui::Grid`, which always sizes columns to their content, so no cell can wrap. We take tables out of the library's hands: `src/doc.rs` recognises top-level GFM tables and replaces each with one `<mdtable id="…">` HTML line plus a parsed `TableSpec`; `src/app.rs`'s existing HTML hook recognises that placeholder and draws the table itself, one fixed-width column per header cell, each cell rendered as its own little Markdown view inside `ui.set_max_width(cell_width)` so text wraps and inline formatting still works.

**Tech Stack:** existing `pulldown`-free code path (we parse the pipe syntax ourselves), `egui::Grid`, `CommonMarkViewer` per cell with a second `CommonMarkCache`.

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (table rendering is where the design deferred to the library; this amends it).

## Global Constraints

- Only top-level tables are converted: a table starts when a line containing `|` is followed by a delimiter line whose cells match `:?-{1,}:?`. Anything inside fenced code, blockquotes, or lists is left untouched.
- Cell text keeps its inline Markdown (bold, code, links); escaped pipes `\|` stay literal and do not split.
- Column count = header count; rows with fewer cells are padded with empty cells, extra cells are appended as extra columns only if a row is longer than the header (so nothing is dropped).
- Cell width = `(column_width - item_spacing.x * (ncols - 1)) / ncols`, minimum 80 px; wrapping must be enabled (`ui.set_max_width`).
- The placeholder parser must reject `<mdtable id="">` and must not disturb the `<mermaid …>` path.
- `RENDER_VERSION` is untouched (no diagram change).
- No new dependencies; warning-free; themes, zoom, fonts, pan, overscroll untouched.

## Review Focus

- A table with one very long cell no longer widens the document; text wraps to multiple lines, row heights grow.
- Header row stays visually distinct; inline code/bold inside cells still render.
- Tables with 1, 2, 5 columns and unequal row lengths render sanely; escaped pipes and empty cells survive.
- Nothing is dropped or duplicated when a table sits directly after a heading or before a mermaid fence.
- A malformed table (delimiter row mismatched, lone pipe line) falls back to normal Markdown, not a blank gap.

---

### Task 1: Extract tables in `doc.rs`

**Files:**
- Modify: `src/doc.rs`
- Test: inline tests in `src/doc.rs`

**Interfaces:**
- Produces: `pub struct TableSpec { pub header: Vec<String>, pub rows: Vec<Vec<String>> }`, `Prepared.tables: HashMap<String, TableSpec>`, `pub fn table_id(spec: &TableSpec) -> String` (blake3 over a canonical join, 16 hex), `pub fn parse_table_block(lines: &[&str]) -> Option<TableSpec>`.
- Placeholder form: `<mdtable id="…">` on its own line, followed by one blank line (same trick as `<mermaid>`).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn converts_a_long_table_to_a_placeholder() {
    let md = "| a | b |\n|---|---|\n| satu | dua |\n";
    let p = prepare(md, Path::new("/d"));
    assert_eq!(p.tables.len(), 1);
    let id = p.tables.keys().next().unwrap();
    assert!(p.markdown.contains(&format!("<mdtable id=\"{id}\">")));
    assert!(!p.markdown.contains("| a |"));
}

#[test]
fn keeps_escaped_pipes_and_uneven_rows() {
    let spec = parse_table_block(&["| a \\| b | c |", "|---|---|", "| 1 |"]).unwrap();
    assert_eq!(spec.header, vec!["a | b".to_string(), "c".to_string()]);
    assert_eq!(spec.rows, vec![vec!["1".to_string(), "".to_string()]]);
}

#[test]
fn leaves_tables_inside_code_alone() {
    let md = "```\n| a | b |\n|---|---|\n```\n";
    let p = prepare(md, Path::new("/d"));
    assert!(p.tables.is_empty());
}
```

- [ ] **Step 2: `cargo test` → RED.**

- [ ] **Step 3: Implement** the scanner in the same single pass that already handles fences: while not in a fence, collect consecutive lines that contain `|` (at least 2), check the second line is a delimiter row with the same cell count, then build a `TableSpec`, insert it, and emit `<mdtable id="…">\n\n`. Escaped `\|` handling in the splitter. If the delimiter row does not match, emit the lines verbatim.

- [ ] **Step 4: `cargo test` green; `cargo build` warning-free. Commit.**

---

### Task 2: Render wrapped tables in `app.rs`

**Files:**
- Modify: `src/app.rs`
- Test: inline tests in `src/app.rs`

**Interfaces:**
- Consumes: `doc::TableSpec`, `Prepared.tables`, `parse_table_placeholder`.
- Produces: `fn parse_table_placeholder(html: &str) -> Option<&str>`, `fn column_widths(available: f32, spacing: f32, ncols: usize) -> f32`.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn parses_table_placeholder_ids() {
    assert_eq!(super::parse_table_placeholder(r#"<mdtable id="abc">"#), Some("abc"));
    assert_eq!(super::parse_table_placeholder(r#"<mdtable id="">"#), None);
    assert_eq!(super::parse_table_placeholder(r#"<mermaid id="x">"#), None);
}

#[test]
fn column_width_leaves_room_for_spacing() {
    assert_eq!(super::column_widths(900.0, 8.0, 3), 294.0);
    assert_eq!(super::column_widths(100.0, 8.0, 10), 80.0); // floor
}
```

- [ ] **Step 2: `cargo test` → RED.**

- [ ] **Step 3: Implement** the placeholder branch in the existing html hook (before the mermaid branch returns `None`, check the table first), drawing with `egui::Grid::new(id).striped(true)`, and each cell inside:

```rust
ui.allocate_ui_with_layout(
    egui::vec2(w, 0.0),
    egui::Layout::top_down(egui::Align::Min),
    |ui| {
        ui.set_max_width(w);
        egui_commonmark::CommonMarkViewer::new().show(ui, &mut cell_cache, cell_md);
    },
);
```

with a separate `CommonMarkCache` owned by the diagram/UI state (never the outer viewer's cache — that would re-borrow it). Header row drawn first, then `ui.end_row()` per row.

- [ ] **Step 4: `cargo test` green, build warning-free.**

- [ ] **Step 5: Visual check** — `/tmp/table.md` (long 2-column table) plus a 5-column table; capture the window and confirm: table fits the column, long cells wrap to 2+ lines, header distinguishable, inline `code` in a cell still renders. Report captures and what you measured.

- [ ] **Step 6: Commit.**
