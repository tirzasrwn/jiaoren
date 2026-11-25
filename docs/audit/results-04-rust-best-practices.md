# Rust best-practices audit — 2026-11-26

Scope: whole crate (~19k lines), reviewed against the Apollo Rust Best
Practices handbook (chapters 1–9) by four reviewers working in disjoint areas.

| Area | Files |
|---|---|
| Parser / justify | `src/doc.rs`, `src/justify.rs` |
| App core | `src/app/{mod,geometry,keys,searchui,toc,selection}.rs` |
| Renderers / search | `src/app/{code,diagram,image,table,raster,heading,list,paragraph,quote,widgets,placeholder}.rs`, `src/search.rs` |
| Infra / CLI | `src/{main,prefs,theme,download,fonts,mermaid}.rs`, `tests/cli.rs`, `Cargo.toml` |

Raw findings: `/tmp/opencode/audit-{a,b,c,d}.md` (session artifacts, not
committed).

## Fixed in this pass

- **Stack overflow on nesting** — `parse_inline` now caps emphasis recursion at
  32 levels and renders deeper markers literally.
- **Quadratic image scan / code rewrite** — `rewrite_images` skips inline code
  spans, so `` `![x](a.png)` `` stays verbatim.
- **Setext underline accepted at 4-space indent**, silently turning indented
  code into a heading and dropping the line.
- **BOM strip copied the whole document**; now drains in place.
- **Autorepeat fired destructive actions** — a held `y`, `r`, `]`, `t`, `g` or
  Tab repeated the action. Only scroll keys repeat now.
- **Scroll units were fixed 40 px** — `j`/`k` move one body line and `h`/`l`
  one monospace column at every zoom, via tested pure helpers.
- **Table copy button/links could be unclickable** — controls are sorted by top
  before the binary-search lookup; regression test with out-of-order controls.
- **Typing in `/` copied the whole document** — `find_matches` takes borrowed
  blocks and reuses one char buffer instead of owning `String`s.
- **Quote blocks cloned their whole spec and re-joined text every frame**;
  now borrows and uses the content-hashed text cache.
- **Invalid UTF-8 SVG became an empty buffer**, hiding the real parse error;
  the original bytes are passed through.
- Removed a duplicated `request_repaint`, dead `occurrences` counter, and a
  stale doc comment that described a pass number that did not exist.
- Added `AGENTS.md` with the owner's rules (no GitHub CI, SemVer bumps).

## Deferred (found, not changed)

Ordered by value; each needs its own change and review.

1. **Unbounded GPU texture caches** (`image.rs`, `diagram.rs`): textures are
   never evicted for the life of a document; a 32 Mpx raster is 128 MB. Needs
   an LRU byte budget with live-key protection.
2. **O(n²) table detection and quote-table scan** (`doc.rs`): a long pipe run
   without a delimiter row rescans the run per line. Needs the header-pair
   check plus one forward row walk.
3. **Per-frame word layout** (`justify.rs`): `build_words`/`fragment_plan`
   clone fragment text and build a `LayoutJob` for every visible block every
   frame. Needs an `Rc<Vec<Word>>` cache keyed by run identity, column, face.
4. **Selection/search highlight relayout** (`justify.rs`): substrings are laid
   out twice per fragment per frame; use galley cursor positions or a per-char
   x table.
5. **TOC is O(n²)** (`toc.rs`): `visible`/`hidden`/`has_children` walk
   ancestors per row and each row allocates a formatted string plus a shape
   pass. Needs one frame pass producing `Vec<Row>` and a fixed monospace
   advance.
6. **Table rows are not culled**; every cell records/parses/lays out even when
   off screen, and `selection_range` recounts chars per cell. Needs clip-rect
   row skipping and a cached char count.
7. **Image key stats the file and scans five caches per frame**
   (`image.rs`). Needs a throttled mtime probe and eviction only on key change.
8. **Code painter allocates per row** (`code.rs`): line table rebuilt, row
   `String` per row, prefix galleys per mark. Needs row byte ranges and
   `pos_from_cursor`.
9. **`line_highlights` allocates one `Vec` per source line, twice per frame**
   (`code.rs`); only non-empty lines are needed.
10. **Overlay state machine duplicated** (`diagram.rs` vs `image.rs`, ~150
    lines each). One `overlay` module would collapse close keys, zoom, pan,
    clamp and paint.
11. **`SearchUi` keeps seven index-parallel vectors**; one `Block` struct
    would remove the desync risk and hold the char count.
12. **`tail_within` measures every suffix per frame** (`mod.rs`); binary
    search or per-char advances.
13. **Selection drag teardown duplicated** with divergent behaviour
    (`mod.rs`, `selection.rs`); one `end_drag`/`abort_drag` API.
14. Smaller items: `Cow` in `clean_text`/`expand_links`, dead URL return in
    `reference_or_inline`, per-line `Vec<char>` in `is_thematic_break`,
    `split_row` unconditional `replace`, misplaced doc comments, grab-bag
    tests, `home_scroll` x-half now dead, duplicated `gg` window rule, pure
    helpers without tests (`column_widths`, `cell_width`, `link_end`,
    `step(dir=0)`, `fold`).
