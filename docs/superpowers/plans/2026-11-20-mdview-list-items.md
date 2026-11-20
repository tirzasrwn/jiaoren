# mdview: justified list items with hanging indent

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Bullet, numbered and task list items wrap like body paragraphs: justified to the column, continuation lines aligned under the item's text (hanging indent), long tokens broken at separators, nesting preserved.

**Architecture:** Like tables and paragraphs, one block becomes ours: `src/doc.rs` recognises a top-level list block and replaces it with `<mdlist id="…">` plus a `ListSpec` of items (`depth`, `kind`, raw inline Markdown). `src/app/list.rs` renders each item with a marker (`•`/`◦`/`1.`/`☐`/`☑`) and the text indented by marker width, calling `justify::draw` with the remaining width so wrapping, justification and token splitting come for free. Blocks whose items contain images or raw HTML stay on the library path.

**Tech Stack:** existing `justify` layout; no new deps.

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (lists were left to the library; this amends list rendering).

## Global Constraints

- Recognised markers: `-`, `*`, `+` (bullets), `1.`/`1)` (ordered), and task suffixes `[ ]` / `[x]` / `[X]`. Indent depth = leading spaces / 2, capped at 5.
- Only top-level blocks: a list starting at column 0 outside fences, tables and blockquotes. A block ends at the first non-list, non-indented blank-separated line.
- Continuation lines (indented more than the marker, or plain lines that are not a new item) join the current item's text with a single space; they never become separate paragraphs.
- Whole-block fallback to `CommonMarkViewer` if any item contains `![` or `<`; no mixed rendering inside one block.
- Marker geometry: indent step = width of two spaces in the body font; marker gap = one space; text starts after the marker; continuation lines start at the same x as the item text.
- Items are justified exactly like paragraphs (`justify::draw`), including the existing rule that lines touching a split token are not stretched.
- Item vertical spacing = half `item_spacing.y` inside a block, full block spacing before/after the block.
- Placeholder `<mdlist id="…">` + blank line; `RENDER_VERSION` unchanged; no new deps; warning-free; clippy/fmt gates green.

## Review Focus

- A long bullet with a long path wraps inside the column, justified, and the second line starts under the text (not under the bullet).
- Nested bullets (2 and 4 spaces) indent consistently and do not inherit the parent's hanging indent.
- Ordered lists number correctly (start value from the first item, increments per item at the same depth), task boxes render checked/unchecked.
- A list item containing an image or `<br>` leaves the whole block on the library path (no half-rendered block).
- Lists inside blockquotes and fenced code are untouched; a `-` inside a paragraph or after a table is not mistaken for a list.
- No regression: paragraphs immediately after a list still render justified; scrolling/reanchor unaffected.

---

### Task 1: Extract list blocks in `doc.rs`

**Files:** `src/doc.rs`, `src/app/mod.rs` (struct literal site), `src/app/list.rs` (new, spec only), `main.rs` if needed.

**Interfaces:**
- `pub struct ListSpec { pub items: Vec<ListItem> }`, `pub struct ListItem { pub depth: u8, pub kind: ListKind, pub text: String }`, `pub enum ListKind { Bullet, Ordered(u64), Task(bool) }`.
- `Prepared.lists: HashMap<String, ListSpec>`, `pub fn list_id(spec: &ListSpec) -> String`.
- Placeholder `<mdlist id="…">` + blank line.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn extracts_bullets_with_depth_and_numbers() {
    let md = "- satu\n- dua\n  - anak\n    1. cucu\n";
    let p = prepare(md, Path::new("/d"));
    let spec = p.lists.values().next().unwrap();
    assert_eq!(spec.items.len(), 4);
    assert_eq!((spec.items[0].depth, &spec.items[0].kind), (0, &ListKind::Bullet));
    assert_eq!(spec.items[2].depth, 1);
    assert!(matches!(spec.items[3].kind, ListKind::Ordered(1)));
    assert!(p.markdown.contains("<mdlist id="));
}

#[test]
fn joins_continuation_lines() {
    let md = "- item yang panjang\n  lanjutan baris pertama\n  dan kedua\n";
    let spec = prepare(md, Path::new("/d")).lists.into_values().next().unwrap();
    assert_eq!(spec.items.len(), 1);
    assert_eq!(spec.items[0].text, "item yang panjang lanjutan baris pertama dan kedua");
}

#[test]
fn task_lists_and_ordered_start() {
    let md = "3. tiga\n4. empat\n- [x] selesai\n- [ ] belum\n";
    let p = prepare(md, Path::new("/d"));
    let specs: Vec<_> = p.lists.into_values().collect();
    let all: Vec<_> = specs.iter().flat_map(|s| &s.items).collect();
    assert!(matches!(all[0].kind, ListKind::Ordered(3)));
    assert!(matches!(all[1].kind, ListKind::Ordered(4)));
    assert_eq!(all[2].kind, ListKind::Task(true));
    assert_eq!(all[3].kind, ListKind::Task(false));
}

#[test]
fn image_items_fall_back_to_library() {
    let md = "- biasa\n- dengan ![gambar](a.png)\n";
    let p = prepare(md, Path::new("/d"));
    assert!(p.lists.is_empty());
    assert!(p.markdown.contains("- dengan"));
}

#[test]
fn blockquote_lists_are_not_extracted() {
    assert!(prepare("> - satu\n", Path::new("/d")).lists.is_empty());
}
```

- [ ] **Step 2: RED.** Step 3: implement detection before paragraph extraction (lists start at column 0), item join, ordered numbering per depth, task parsing, whole-block fallback on `![`/`<`. **Step 4:** tests green, clippy/fmt/build clean, commit.

---

### Task 2: Render lists in `app/list.rs`

**Files:** `src/app/list.rs` (new), `src/app/mod.rs` (state + hook branch), `src/justify.rs` (indent-aware entry point), `src/app/placeholder.rs` (add `List`).

**Interfaces:**
- `pub fn draw_indented(ui: &mut egui::Ui, runs: &[Run], column: f32, indent: f32, marker: Marker)` in `justify.rs`, with `pub enum Marker { Bullet, Hollow, Number(u64), Task(bool) }`.
- `app::list::ListUi { specs, begin_frame, reset, show(ui, id) }`.

- [ ] **Step 1: Failing tests** for `Marker::label()` (`Bullet → "•"`, `Number(3) → "3."`, `Task(true) → "☑"`) and for `list_indent(depth, space_width)` (`(0, 8.0) → 0.0`, `(2, 8.0) → 32.0`).
- [ ] **Step 2: RED.** Step 3: implement `draw_indented` (reserve `indent + marker_w + space + text_w` height; paint marker at `left+indent`; paint text starting at `left+indent+marker_w+space`, justify to remaining width; continuation lines align to text x). **Step 4:** hook branch + `ListUi` wiring, item gap half `item_spacing.y`, block padding from Task "table spacing" convention. **Step 5:** tests, clippy, fmt, build.
- [ ] **Step 6: Visual check** with `/tmp/list.md` (long bullets, nested, ordered, task, long path). Capture; confirm justified wrapping inside the column, hanging indent, markers, no clipping. Report measurements (right edge per item, continuation x).
- [ ] **Step 7:** Update README known limitations if a list edge case remains (e.g. multi-paragraph items), commit.
