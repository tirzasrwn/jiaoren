# Audit Results — Part 0 (Smoke) + Part 1 (Conformance)

**Environment:** jiaoren `6054524` (`./target/debug/jr`, `jiaoren 0.1.0`), native egui renderer; Mermaid
`11.12.0` headless Chromium, `securityLevel: strict`,
`--host-resolver-rules=MAP * ~NOTFOUND`, render key
`5-mermaid11.12.0-htmlLabelsOff-2x-strict-svg-catppuccin-firacode`; X11 `:0`, i3 workspace 9;
date 2026-11-22. Window 1200x900 at 40,40.

Evidence: `docs/audit/evidence-00-01/` (PNG captures, 1200x900).

**Mermaid updated to 12.3.0 after this run.** Smoke re-checked: all 18 diagram types render (20
light + 20 dark SVGs, only the intentionally invalid diagram errors). `T-MM-040` still fails inside
Mermaid 12.3.0 (ER quoted attribute); `T-HO-047` in Part 2 now renders.

**Post-fix re-check (commit `8ecd827`).** The critical and high findings below were fixed after this
run: quote tables (T-CM-061), the `xlink` SVGs, CJK/Arabic fallbacks, and URL schemes. Fixed rows are
marked PASS (fixed); the original evidence is kept. Revised Part 1 counts: PASS 77 · FAIL 20 · N-A 15 ·
UNSUPPORTED 4 · VERSION 1. Full list: `results-after-fixes.md`.

**Important method note.** `audit-01-conformance.md` is truncated by the app at `T-CM-061`: everything
after a table inside a blockquote is lost (evidence `audit-conf-08.png`, minimal repro
`audit-probe-qtable.png`). To score sections A8, B, C, D and E, byte-identical extracts of those sections
were rendered as separate files (`A8-A10.md`, `B-gfm.md`, `C-unicode.md`, `D-security.md`,
`E-mermaid.md`). No test text was edited.

---

## 1. Smoke test (`00-smoke-test.md`)

### Basics

| Area | Result | Notes | Evidence |
|---|---|---|---|
| Text formatting | PASS | bold/italic/bold-italic/inline code/links/escapes work; raw HTML (`<kbd>`, `<sup>`, `<sub>`) is literal by design. | `audit-smoke-00.png` |
| Lists | FAIL | Nested bullets under ordered item "PostgreSQL" became a code block. Task-list boxes render. | `audit-smoke-05.png` |
| Tables | PASS | Alignment, inline code, emoji status cells, header fill, copy button. | `audit-smoke-06.png` |
| Code blocks | PASS | Line numbers, wrap, copy button, fences preserved. | `audit-smoke-07.png` |
| Footnotes / references | UNSUPPORTED-BY-DESIGN | `[^1]` literal and definitions inlined as paragraph; reference links not resolved (also T-CM-031). | `audit-smoke-09.png` |
| Math | UNSUPPORTED-BY-DESIGN | `$…$` and `$$…$$` literal; math is a documented non-goal. | `audit-smoke-09.png` |
| HTML section | PASS (by design) | `<details>`/`<summary>` tags dropped, contents and inner Mermaid render; no collapse UI. | `audit-smoke-34.png` |
| `[TOC]` | UNSUPPORTED-BY-DESIGN | Literal `[TOC]`. | `audit-smoke-00.png` |

### 18 Mermaid diagrams in §8

| # | Type | Result | Notes | Evidence |
|---:|---|---|---|---|
| 1 | Flowchart | rendered | subgraphs, styling, classDef OK. | `audit-smoke-10.png` |
| 2 | Sequence | rendered | alt/loop/notes/activation OK; autonumber digits absent. | `audit-smoke-11.png` |
| 3 | Class | rendered | relations/notations OK. | `audit-smoke-12.png` |
| 4 | State | rendered | `attempt < 3` label shows literally as `attempt &lt; 3`. | `audit-smoke-13.png` |
| 5 | ER | rendered | tables/keys OK; relationship labels (`places`, `has`, …) not visible. | `audit-smoke-14.png`, `audit-smoke-15.png` |
| 6 | Gantt | rendered | deps, crit, milestone, today line OK; labels overlap bars. | `audit-smoke-15.png` |
| 7 | Pie | rendered | `showData` percentages + legend. | `audit-smoke-16.png` |
| 8 | Mindmap | rendered | text contrast low on purple nodes. | `audit-smoke-17.png` |
| 9 | Git graph | rendered | tags/branches OK. | `audit-smoke-18.png` |
| 10 | Journey | rendered | section titles ("Setup", "First task", "Review") blank. | `audit-smoke-18.png` |
| 11 | Timeline | rendered | OK. | `audit-smoke-19.png` |
| 12 | Quadrant | rendered | right side clipped at window width; pan/zoom needed. | `audit-smoke-19.png`, `audit-smoke-20.png` |
| 13 | Requirement | rendered | OK. | `audit-smoke-21.png` |
| 14 | C4 Context | **rendered** (fixed `30aba65`) | Was a contained `xlink` namespace error; the rasterizer now declares `xmlns:xlink`. | `audit-smoke-21.png` |
| 15 | Sankey | rendered | labels clipped at right edge. | `audit-smoke-22.png` |
| 16 | XY chart | rendered | bars/line present, very pale. | `audit-smoke-23.png` |
| 17 | Block | rendered | OK. | `audit-smoke-24.png` |
| 18 | Architecture | rendered | icons/groups OK. | `audit-smoke-25.png` |

Smoke verdict: **PASS after fixes** — 18/18 diagram types render (C4 fixed by `30aba65`); list nesting is
wrong. §9 edge cases: init-theme diagram renders but the `forest` theme is ignored (app locks theme);
invalid diagram is contained (`audit-smoke-27.png`); HTML comment is printed literally; broken image shows
only a warning glyph, alt text lost.

---

## 2. Conformance result sheet

Legend: **PASS** = matches EXPECT; **FAIL** = user-visible deviation; **N-A** = not applicable to a native
renderer / unsupported HTML; **UNSUPPORTED-BY-DESIGN** = documented extension non-goal;
**VERSION** = Mermaid/source-version behaviour, contained.

### A. CommonMark Core (57 IDs)

| ID | Verdict | Note | Evidence |
|---|---|---|---|
| T-CM-001 | PASS | ATX 1–6 all render as headings (H4–H6 share one visual size). | |
| T-CM-002 | PASS | 7 hashes = literal paragraph. | |
| T-CM-003 | PASS | `#NoSpace` literal. | |
| T-CM-004 | PASS | Both setext headings render. | |
| T-CM-005 | PASS | Closing hashes stripped ("Closed heading"). | |
| T-CM-006 | PASS | IDs are hash+occurrence, so duplicates are unique (not externally inspectable). | |
| T-CM-007 | PASS | Inline code/emphasis/link + literal `<b>`; no broken layout. | |
| T-CM-008 | FAIL | Anchor link renders but does not navigate; app uses hashed heading IDs and sends `#…` to the OS opener. | `audit-ac-anchor.png` |
| T-CM-010 | PASS | `snake_case_word` stays plain. | |
| T-CM-011 | PASS | Intraword `*` emphasis works. | |
| T-CM-012 | PASS | `foo_bar_baz` stays plain. | |
| T-CM-013 | PASS | `**bold**text`. | |
| T-CM-014 | PASS | Nested emphasis. | |
| T-CM-015 | PASS | Bold+italic. | |
| T-CM-016 | PASS | Nested bold/italic. | |
| T-CM-017 | PASS | Spaced `*` literal. | |
| T-CM-018 | PASS | `2 * 3 * 4` literal. | |
| T-CM-019 | PASS | `__init__.py` → bold `init`. | |
| T-CM-020 | PASS | Overlapping emphasis no crash. | |
| T-CM-021 | PASS | Double-backtick spans and space trimming correct. | `audit-conf-03.png` |
| T-CM-022 | PASS | `<script>` shown as code text. | |
| T-CM-023 | PASS | Escaped pipes inside table cells. | |
| T-CM-030 | PASS | Inline links incl. title/parens/angle brackets. | |
| T-CM-031 | FAIL | Reference links are not resolved; `[full ref][ref-one]`, `[collapsed][]`, `[shortcut]` shown literally. | `audit-conf-04.png` |
| T-CM-032 | PASS | Undefined ref literal. | |
| T-CM-033 | PASS | Angle autolinks (`https`, `mailto`, email). | |
| T-CM-034 | FAIL | GFM bare autolinks are not linkified (`https://…`, `www.…`, `test@…` plain). | `audit-conf-04.png` |
| T-CM-035 | FAIL | Bare URL not linkified, so trailing-punctuation rule cannot apply. | `audit-conf-04.png` |
| T-CM-036 | FAIL | Inline/ref images not rendered; ref image literal; broken image shows only ⚠, alt text lost. Remote images are a documented non-goal. | `audit-conf-04.png` |
| T-CM-037 | FAIL | Link-wrapped badge shows only ⚠; alt text lost. | `audit-conf-04.png` |
| T-CM-038 | PASS | Relative links render clickable; fragment-only anchor does nothing. | |
| T-CM-040 | FAIL | Two-space hard break ignored (paragraph joined). `trim_end()` in doc.rs discards the spaces. | `audit-conf-04.png` |
| T-CM-041 | PASS | Backslash hard break works. | |
| T-CM-042 | PASS | Soft break becomes a space. | |
| T-CM-043 | PASS | All backslash escapes literal. | |
| T-CM-044 | PASS | Documented entity subset decoded; `&unknown;`/`&#0;` literal as documented. | |
| T-CM-050 | PASS | Tight list. | |
| T-CM-051 | PASS | Loose list keeps item spacing. | |
| T-CM-052 | FAIL | Start-at-5 correct, but `1)` delimiter is normalised to `1.`. | `audit-conf-05.png` |
| T-CM-053 | PASS | Bullet-char changes render contiguously; no observable structural difference. | |
| T-CM-054 | FAIL | Fenced code in item 1 collapses inline (`bash echo …`); paragraph/quote/nested list of item 2 are not indented under it. List items are documented as one paragraph. | `audit-conf-06.png` |
| T-CM-055 | FAIL | Lazy continuation "continuation line" becomes a separate paragraph, not part of the item; contradicts README's "continuation lines are joined". | `audit-conf-06.png` |
| T-CM-056 | FAIL | Ordered numbers dropped on task items (`1.`/`2.` → checkbox only); documented for `3. [x]`, generalised here. | `audit-conf-07.png` |
| T-CM-060 | FAIL | Lazy line "line two" falls outside the blockquote. | `audit-conf-07.png` |
| T-CM-061 | PASS (fixed `2495dc0`, `8ecd827`) | A quote table is extracted and rendered, quote text before/after it is kept, and the rest of the document survives. Original run: content loss. | `audit-conf-08.png` |
| T-CM-070 | PASS | 4-space indented code. | |
| T-CM-071 | PASS | Tab-indented code. | |
| T-CM-072 | PASS | Tilde fence; info attributes ignored. | `audit-a8-00.png` |
| T-CM-073 | PASS | Inner triple fence stays literal. | |
| T-CM-074 | PASS | Info-string case/extras; no highlighting applied. | |
| T-CM-075 | PASS | Empty and whitespace-only fences. | |
| T-CM-076 | PASS | Long line wraps inside code; layout intact. | |
| T-CM-080 | PASS | All five forms produce rules. | |
| T-CM-090 | N-A | Raw HTML block dropped entirely (native renderer, by design). | `audit-a8-04.png` |
| T-CM-091 | N-A | Markdown after a blank line renders; the no-blank-line `<div>` block is dropped, so literal asterisks never appear. | `audit-a8-04.png` |
| T-CM-092 | PASS | `<details>` tags dropped but contents + inner Mermaid render; no execution. | |
| T-CM-093 | N-A | Raw HTML table dropped (by design). | `audit-a8-02.png` |

### B. GFM Extensions (11 IDs)

| ID | Verdict | Note | Evidence |
|---|---|---|---|
| T-GF-001 | FAIL | Alignment/links/bold OK, but table routes through fallback because of the inline image; `~~strike~~` in a cell is not struck through; image cell only ⚠. | `audit-b-00.png` |
| T-GF-002 | FAIL | Extra cells are kept (`4`, `5`), not dropped per GFM. | `audit-b-00.png` |
| T-GF-003 | PASS | Table without outer pipes. | |
| T-GF-004 | PASS | Escaped pipe and empty cells. | |
| T-GF-005 | FAIL | CJK and Arabic are tofu boxes; emoji render. | `audit-b-01.png` |
| T-GF-010 | PASS | `~~one~~` struck; `~~~three~~~` literal. | |
| T-GF-011 | UNSUPPORTED-BY-DESIGN | Footnotes literal; definitions printed inline. | `audit-b-01.png` |
| T-GF-012 | PASS | All five GitHub alerts styled (NOTE/TIP/IMPORTANT/WARNING/CAUTION). | `audit-b-02.png` |
| T-GF-020 | UNSUPPORTED-BY-DESIGN | Inline math literal; math is a non-goal. | |
| T-GF-021 | UNSUPPORTED-BY-DESIGN | Block math flattened to literal TeX text. | |
| T-GF-022 | UNSUPPORTED-BY-DESIGN | Invalid math literal; no math error UI expected. | |

### C. Unicode (7 IDs)

| ID | Verdict | Note | Evidence |
|---|---|---|---|
| T-UN-001 | PASS (fixed `6d0df5b`) | Japanese/Chinese/Korean render via the Noto CJK fallback. | `audit-c-00.png` |
| T-UN-002 | PASS (fixed `6d0df5b`) | Arabic renders shaped; Hebrew is covered by the same fallback chain. | `audit-c-00.png` |
| T-UN-003 | PASS | Precomposed/combining accents and zalgo render. | |
| T-UN-004 | FAIL | ZWJ people render; regional-indicator flags show as boxed letter pairs (🇮🇩→"I D"). | `audit-c-00.png` |
| T-UN-005 | PASS | Zero-width characters invisible, text intact. | |
| T-UN-006 | PASS | NBSP/en/em spaces preserved as spacing. | |
| T-UN-007 | PASS for glyphs (fixed `6d0df5b`); anchors still inert | CJK heading renders; `#fragment` navigation remains a low finding. | `audit-c-00.png` |

### D. Security probes (14 IDs)

No document JavaScript runs in a native renderer. Each probe was checked as inert text / non-executable,
not as a browser flag.

| ID | Verdict | Note | Evidence |
|---|---|---|---|
| T-SC-001 | N-A | Script tag dropped; N/A (native). | `audit-d-00.png` |
| T-SC-002 | N-A | `onerror` image dropped. | `audit-d-00.png` |
| T-SC-003 | N-A | SVG shown as inert literal text. | `audit-d-00.png` |
| T-SC-004 | N-A | Raw anchor/onclick shown as inert text. | `audit-d-00.png` |
| T-SC-005 | N-A | Four `javascript:` links render as clickable blue links but clicking is inert (no JS, no flag). href not stripped — note only. | `audit-d-04.png` |
| T-SC-006 | N-A | `data:` link inert; data SVG not loaded (⚠). | `audit-d-00.png` |
| T-SC-007 | N-A | iframe/object/embed/form/meta/base/link/style dropped. | `audit-d-00.png` |
| T-SC-008 | N-A | No red overlay; style block dropped. | `audit-d-00.png` |
| T-SC-009 | N-A | mXSS/noscript payloads shown as inert literal text (entities not decoded). | `audit-d-00.png` |
| T-SC-010 | N-A | Link/image attribute injection inert; image not loaded. | `audit-d-01.png` |
| T-SC-011 | N-A | Autolink with quotes inert. | `audit-d-01.png` |
| T-SC-012 | N-A | Raw HTML anchor dropped, so `rel` is N/A. | `audit-d-01.png` |
| T-SC-013 | PASS (fixed `8965b2c`) | Only `http(s)://` and `mailto:` are clickable; every other scheme stays inert and the target is shown as a hover tooltip. | `audit-d-01.png` |
| T-SC-014 | PASS | All four traversal images refused (⚠); no local file read. | |

### E. Mermaid advanced (28 IDs)

| ID | Verdict | Note | Evidence |
|---|---|---|---|
| T-MM-001 | PASS | Markdown labels, shapes, linkStyle, class/style. | |
| T-MM-002 | PASS | Nested subgraphs, per-subgraph direction, cross-links. | `audit-e-01.png` |
| T-MM-003 | PASS | New shape syntax, edge IDs, animated edges render on 11.12.0. | |
| T-MM-004 | PASS (fixed `30aba65`) | `click` directives emit `xlink:href`; the rasterizer now declares the namespace and the diagram renders. | `audit-e-01.png`, `audit-e-02.png` |
| T-MM-005 | PASS | HTML labels escaped (`<img …>`, `<b>`, `<a>` shown literally); no execution. | |
| T-MM-010 | FAIL | box/rect/par/critical/break/loop/notes/async all render, but autonumber digits are absent. | `audit-e-02.png` |
| T-MM-011 | FAIL | create/destroy and `<br/>` work; quoted alias keeps literal quotes (`"Service A (v2.1)"`). | `audit-e-04.png` |
| T-MM-020 | PASS (fixed `30aba65`) | `link User …` emits `xlink:href`, now rasterized. | `audit-e-04.png` |
| T-MM-030 | PASS | Fork/join bars, choice diamond, concurrent regions, notes. | |
| T-MM-040 | VERSION | Mermaid 11.12.0 rejects `"Table With Spaces"` in an ER attribute block (`Expecting ATTRIBUTE_WORD, got COMMENT`); contained error, page continues. | `audit-e-07.png` |
| T-MM-050 | PASS | Dependency chain, crit, milestone, `vert` marker, weekend exclusion. | |
| T-MM-060 | PASS | kanban, packet-beta, radar-beta, treemap-beta all render on 11.12.0. | |
| T-MM-070 | FAIL | Title + step edges honoured; dark theme ignored (light nodes) because the app locks the diagram theme. | `audit-e-011.png` |
| T-MM-071 | PASS | Custom themeVariables ignored consistently (allowed by EXPECT when the app locks theme). | |
| T-MM-072 | PASS | `securityLevel: loose` ignored; `<img>` escaped; no flag. | |
| T-MM-080 | PASS | Mermaid in a list item renders (block not indented under the item). | |
| T-MM-081 | FAIL | Mermaid inside a blockquote is shown as a literal code block, not a diagram (blockquote goes through the library fallback). | `audit-e-012.png` |
| T-MM-082 | PASS | Fence in a table cell stays literal. | |
| T-MM-083 | PASS | Tilde-fenced Mermaid renders. | |
| T-MM-084 | PASS | Only exact `mermaid` info string renders; `Mermaid`/`mmd` stay code — consistent. | `audit-e-013.png` |
| T-MM-085 | PASS | Back-to-back diagrams both render; no style bleed. | |
| T-MM-086 | PASS | Three identical pies render independently. | |
| T-MM-090 | PASS | Error box only for this diagram; document continues. Error text is malformed (`id="out">ERR`, `&gt;`). | `audit-e-016.png` |
| T-MM-091 | PASS | Unknown type → contained error box. | `audit-e-017.png` |
| T-MM-092 | PASS | Empty diagram → contained "No diagram type detected" error, no crash. | `audit-e-017.png` |
| T-MM-093 | PASS | Huge label wraps in a tall node, no layout explosion. | |
| T-MM-094 | PASS | Diagram after an error renders. | |
| T-MM-095 | PASS | Light/dark toggle re-renders diagrams with the new palette, including after an error box (takes a few seconds; "rendering diagram…" placeholder meanwhile). | `audit-e-022-light2.png` |

---

## 3. Summary

Post-fix table (original run in `results-00-01` history above; fixed rows marked above):

| Verdict | CM | GF | UN | SC | MM | Total |
|---|---:|---:|---:|---:|---:|---:|
| PASS | 42 | 4 | 6 | 2 | 23 | **77** |
| FAIL | 12 | 3 | 1 | 0 | 4 | **20** |
| N-A (native/HTML) | 3 | 0 | 0 | 12 | 0 | **15** |
| UNSUPPORTED-BY-DESIGN | 0 | 4 | 0 | 0 | 0 | **4** |
| VERSION (Mermaid) | 0 | 0 | 0 | 0 | 1 | **1** |
| **Total** | 57 | 11 | 7 | 14 | 28 | **117** |

Smoke: basics 5/7 areas pass; Mermaid **18/18** rendered after the `xlink` fix (original run: 17/18).

## 4. Genuine renderer defects

Ordered by user impact. File/line localization where available.

1. **FIXED (`2495dc0`, `8ecd827`) — Blockquote containing a table truncates the rest of the document** (critical, data loss).
   `T-CM-061` and minimal repro: a `> | A | B |` table renders flattened, then every later block disappears
   (`G`/search cannot find `T-CM-070`; search reports `[0/0]`). Quote tables are deliberately left to the
   library (`src/doc.rs:557`); `egui_commonmark` then consumes/drops the remainder. The whole Part 1 after
   §A7 is unreadable in one document.
2. **FIXED (`30aba65`) — `xlink` SVG namespace rejected by the rasterizer** (high). Any Mermaid diagram that emits
   `xlink:href` fails: C4Context (smoke §8.14), class `link` directive (T-MM-020), `click` directives
   (T-MM-004): `parse svg: … unknown namespace prefix 'xlink'`.
   `src/app/raster.rs:46` calls `resvg::usvg::Tree::from_data`; inject/declare `xmlns:xlink` or rewrite
   `xlink:href` to `href` before parsing.
3. **FIXED (`6d0df5b`) — CJK and Arabic render as tofu** (high). `src/fonts.rs::load()` installs only FiraCode Nerd Font and
   never registers system fallbacks, although Noto CJK/Arabic are installed. Hits tables, body text,
   headings and Mermaid labels (T-GF-005, T-UN-001/002/007).
4. **Reference-style links/images unsupported** (medium). `src/justify.rs::parse_inline` handles only
   inline `[x](url)`; `[x][ref]` and `![x][ref]` stay literal, and the reference definitions print as a
   visible paragraph (T-CM-031/036).
5. **Two-space hard break lost** (medium). Paragraph lines are trimmed with `trim_end()` when rejoined
   (`src/doc.rs` paragraph builder), so the trailing spaces that mean `<br>` are gone (T-CM-040).
6. **GFM bare autolinks not linkified** (medium). `https://…`, `www.…`, bare emails stay plain text
   (T-CM-034/035).
7. **GFM ragged tables keep extra cells** (medium). `src/doc.rs::parse_table_block` says "Extra cells
   beyond the header are kept, never dropped", contrary to GFM (T-GF-002).
8. **Lazy continuation lines split out of their container** (medium). The next column-0 line is treated as
   a new paragraph, so list/blockquote lazy continuation is lost (T-CM-055/060).
9. **Broken/remote images lose alt text** (low). Only a red ⚠ is drawn; `![Alt text …]` text is not shown
   (T-CM-036/037, smoke §10).
10. **Mermaid error boxes leak wrapper junk** (low). Extraction starts at `id="out">ERR:` without skipping
    the prefix, and HTML entities stay encoded (`&gt;`) — `src/mermaid.rs:158` (T-MM-090/091 and smoke §9).
11. **Anchors do not navigate** (low). Heading IDs are blake3 hashes; link clicks call
    `ctx.open_url` (`src/justify.rs:1446`) and a `#fragment` is handed to the OS opener instead of
    scrolling (T-CM-008, T-UN-007).
12. **Mermaid styling omissions** (low, renderer config/library): `htmlLabelsOff` leaves `&lt;`/`&quot;`
    literal in state/flowchart labels; sequence autonumber digits absent; ER relation labels and journey
    section titles not visible; ER labels/large diagrams clip at window width.
13. **Table cell strikethrough missing on the fallback route** (low). A table with an inline image routes
    to `egui_commonmark`, where `~~strike~~` in a cell is not struck through though paragraphs support it
    (T-GF-001).
14. **Mermaid blockquote fallback** (low). A ```mermaid fence inside `>` renders as a code block
    (T-MM-081), because blockquotes are library-routed.
15. **Explicitly locked theme overrides document theme** (low). Mermaid `theme: dark` frontmatter is
    ignored (T-MM-070); consistent ignoring is accepted for `init` themes (T-MM-071).

### By-design / documented (not counted as defects)

- Raw HTML execution/rendering: dropped or shown literally; `<details>`, `<summary>`, anchors, definition
  lists, tables in raw HTML have no browser semantics. Math, remote images, footnotes, `[TOC]`, front
  matter, emoji shortcodes are non-goals/unsupported.
- List items are one paragraph; block content inside items becomes separate blocks; ordered number is
  dropped when a task suffix wins (`README` known limitations) — T-CM-054/056.
- Body paragraphs/tables containing inline images or raw HTML fall back to the library renderer.
- `securityLevel: strict`, no document JavaScript: all `window.__xss_*` probes are N/A by construction.

### Mermaid version findings

- T-MM-040: ER entity name with spaces in an attribute block (`"Table With Spaces"`) is rejected by Mermaid
  11.12.0 itself. Contained error is correct; not an app parser bug.
- Newer types kanban, packet-beta, radar-beta, treemap-beta and `@{ shape }`/animated edges all render on
  11.12.0.
