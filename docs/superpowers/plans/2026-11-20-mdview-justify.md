# mdview: justified paragraphs

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Body paragraphs are flush on both edges: words on every line except a paragraph's last are spread so the text exactly fills the reading column.

**Architecture:** `egui`/epaint has no justification (`TextAlign` is left/center/right only), so paragraphs leave the library's hands the same way tables did. `src/doc.rs` replaces each top-level paragraph with `<mdpara id="…">` and stores its raw Markdown; `src/app.rs` hands that Markdown to a new `src/justify.rs`, which parses the inline styles it supports (bold, italic, code, strikethrough, links), wraps words to the column using the actual egui font metrics, and paints each line word-by-word with the leftover space divided between the gaps. Paragraphs containing anything the mini-parser cannot style (images, raw HTML) fall back to `CommonMarkViewer` unchanged.

**Tech Stack:** epaint `Fonts::layout_no_wrap`, `Painter::galley`, existing `CommonMarkCache` for fallbacks. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (paragraph rendering was delegated to the library there; this amends it).

## Global Constraints

- Only top-level paragraphs are converted: a run of consecutive non-blank lines whose first line starts at column 0 and is not a heading (`#`), list item (`-`/`*`/`+`/digit`.`), quote (`>`), table row (`|`), fence, thematic break, or HTML (`<`), and which is not inside a fence. Everything else keeps the library path.
- Placeholder: `<mdpara id="…">` on its own line followed by a blank line; ids are blake3(16 hex) of the paragraph source.
- Inline styles supported: `**bold**`, `*italic*`/`_italic_`, `` `code` ``, `~~strike~~`, `[text](url)`, plus `\` escapes. Nested combinations are applied outermost-first; unbalanced markers are emitted literally.
- Wrapping: greedy by words; a word may exceed the column and then overflows alone rather than being cut.
- Justification: extra space = `column_width - sum(word_widths)`, divided equally among the `n-1` gaps; never below the natural space width; the last line of a paragraph and any line with a single word stays left-aligned.
- Style parity with the library path: body text style and colour from `ui.style()`, code gets the same background treatment as the library (rounded `code_bg_color`), links use `hyperlink_color` + underline and open on click, strike draws a line through.
- Paragraphs that the mini-parser cannot fully represent (image syntax, `<…>`, footnotes) are passed to `CommonMarkViewer` with a dedicated cache; no content is ever dropped.
- No new deps; warning-free; themes, fonts, zoom, tables, diagrams, pan, overscroll untouched.

## Review Focus

- Both edges flush on multi-line paragraphs at zoom 1, 1.5 and 3; last line never stretched; single-word lines never stretched.
- Long file paths / URLs (single long word) do not loop, panic, or produce negative gaps.
- Inline styles: bold, italic, code background, strike, link underline + click, escapes (`\*`), and unbalanced `**` render sensibly.
- Paragraph directly after a heading, table, mermaid fence, or at EOF: no swallowed or duplicated content, blank lines preserved.
- Lists, quotes, headings, tables and fenced code are untouched.
- Fallback path: paragraph with an image renders as before (via the library), no double render.

---

### Task 1: Extract top-level paragraphs in `doc.rs`

**Files:**
- Modify: `src/doc.rs`, `src/app.rs` (struct literal sites only)
- Test: inline tests in `src/doc.rs`

**Interfaces:**
- Produces: `Prepared.paragraphs: HashMap<String, String>`, `pub fn is_paragraph_start(line: &str) -> bool`, `pub fn paragraph_id(source: &str) -> String`.
- Placeholder `<mdpara id="…">` + blank line.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn extracts_a_top_level_paragraph() {
    let md = "# h\n\nHello *world*, this is a paragraph.\nStill the same paragraph.\n\n- list\n";
    let p = prepare(md, Path::new("/d"));
    assert_eq!(p.paragraphs.len(), 1);
    let id = p.paragraphs.keys().next().unwrap();
    assert!(p.markdown.contains(&format!("<mdpara id=\"{id}\">")));
    assert!(p.markdown.contains("- list"));
    assert!(!p.markdown.contains("world\", this is a paragraph"));
}

#[test]
fn skips_lists_quotes_headings_and_fences() {
    let md = "- item one\n\n> quote\n\n## h\n\n```\ncode\n```\n";
    assert!(prepare(md, Path::new("/d")).paragraphs.is_empty());
}

#[test]
fn keeps_blank_line_separation() {
    let md = "first para\n\nsecond para\n";
    let p = prepare(md, Path::new("/d"));
    assert_eq!(p.paragraphs.len(), 2);
    assert!(p.markdown.ends_with("\n"));
}
```

- [ ] **Step 2: `cargo test` → RED.**

- [ ] **Step 3: Implement** in the same pass: when at column 0, outside fences, and `is_paragraph_start`, collect following non-blank lines that also pass `is_paragraph_start` (so a heading/list immediately after ends the paragraph), join with `\n`, emit placeholder + blank line, store source. Keep table/mermaid/image logic intact; tables and paragraphs must not both claim the same lines (paragraph detection comes after table detection).

- [ ] **Step 4: `cargo test` green, build warning-free, commit.**

---

### Task 2: Inline parser, justified layout and painting

**Files:**
- Create: `src/justify.rs`
- Modify: `src/main.rs` (`mod justify;`), `src/app.rs` (hook branch + fallback)
- Test: inline tests in `src/justify.rs` and `src/app.rs`

**Interfaces:**
- `pub struct Run { pub text: String, pub kind: RunKind }`, `pub enum RunKind { Text, Bold, Italic, BoldItalic, Code, Strike, Link(String) }`
- `pub fn parse_inline(md: &str) -> Vec<Run>`
- `pub fn wrap_words(widths: &[f32], space: f32, column: f32) -> (Vec<Vec<usize>>, f32)` — indices of words per line plus the gap count; pure and unit-tested.
- `pub fn draw(ui: &mut egui::Ui, runs: &[Run], column: f32)` — paints the paragraph.
- `fn parse_paragraph_placeholder(html: &str) -> Option<&str>`

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn parses_the_supported_inline_styles() {
    let runs = parse_inline("a **b** c *d* e `f` g ~~h~~ [i](https://x)");
    assert!(runs.iter().any(|r| r.kind == RunKind::Bold && r.text == "b"));
    assert!(runs.iter().any(|r| r.kind == RunKind::Italic && r.text == "d"));
    assert!(runs.iter().any(|r| r.kind == RunKind::Code && r.text == "f"));
    assert!(runs.iter().any(|r| r.kind == RunKind::Strike && r.text == "h"));
    assert!(runs.iter().any(|r| r.kind == RunKind::Link("https://x".into()) && r.text == "i"));
}

#[test]
fn leaves_unbalanced_markers_literal() {
    let runs = parse_inline("a **b");
    assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), "a **b");
}

#[test]
fn wraps_greedily() {
    let widths = vec![30.0, 30.0, 30.0, 30.0];
    let (lines, _) = wrap_words(&widths, 10.0, 100.0);
    // 30 + 10 + 30 = 70 fits; adding the third word needs 110 > 100.
    assert_eq!(lines, vec![vec![0, 1], vec![2, 3]]);
}
```

- [ ] **Step 2: `cargo test` → RED.**

- [ ] **Step 3: Implement** the parser (single left-to-right scan, `\` escapes, code spans win over emphasis, `**` before `*`), the pure wrapper, and `draw`:

```rust
// per word: let galley = ui.fonts(|f| f.layout_no_wrap(word.clone(), font_id, color));
// line width = Σ widths + space*(n-1)
// extra = (column - line_width).max(0.0)
// gap = space + if last_line_or_single_word { 0.0 } else { extra/(n-1) }
// paint with ui.painter().galley(pos, galley, color); advance pos.x by width+gap
```

Code runs get a `painter.rect_filled` behind the word (rounded 2 px, `visuals.code_bg_color`); link runs get an underline stroke and `ui.interact` + `Response::clicked` → `ctx.open_url`; strike runs a line at mid-x-height. Use `ui.style().text_styles[&TextStyle::Body]` and `visuals.text_color()`/`override_text_color` so zoom and theme keep working. Reserve the paragraph's height with `ui.allocate_space(vec2(column, used_height))` so scrolling/layout stays correct.

- [ ] **Step 4: Wire** the `<mdpara>` branch in the html hook (before `<mermaid>`/`<mdtable>`), with fallback: if the paragraph source contains `![` or `<`, render it with a dedicated `CommonMarkCache` (reuse the `TableUi` cell cache) instead.

- [ ] **Step 5: `cargo test` green, build warning-free; commit.**

- [ ] **Step 6: Visual check** — a document with a long paragraph (5+ lines), bold/italic/code/strike/link, a single-word line, a long URL, and paragraphs after heading/table/mermaid. Capture at zoom 1 and after `+`×4; confirm both edges flush except the last line, styles render, links clickable (report underline presence), and nothing else regressed. Report measurements (left/right text edges per line if practical).
