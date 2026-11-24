# Audit follow-up: Critical and High fixes

**Historical:** recorded against jiaoren 0.1.0 at the commit named below. Kept as-is; re-run the suite on the current version (see README) when re-scoring.

Code base: `f30e668` (after the original reports at `6054524`).
Environment: same as the reports (jiaoren 0.1.0, native egui renderer, X11 `:0`),
Mermaid **12.3.0** after the version bump below.

## What changed

| Finding | Fix | Verification |
|---|---|---|
| Critical: diagram rasters >16384 px panic in `load_texture` | `src/app/raster.rs` caps the render scale so neither side exceeds `MAX_TEXTURE_SIDE` (16384) | Unit test `huge_svg_is_capped_below_the_texture_limit`; a 300-node chain (`/tmp/huge-chain.md`) now renders a capped image instead of panicking; app stays alive |
| Critical: a table inside a blockquote truncated the rest of the document (T-CM-061) | `src/doc.rs` `quote_table_split` extracts a table from any position in a blockquote, keeps the quote text before/after it, and renders the table through the normal table path | Tests `table_inside_a_blockquote_becomes_a_placeholder_and_keeps_later_content` and `mixed_quote_keeps_text_around_the_extracted_table`; screenshots show the table plus the "intro line", "outro line" and "after marker" |
| Critical/High: backtick in a fence info string opened a fence and suppressed the rest (T-HO-003) | `open_fence` rejects backticks in a backtick fence's info string (CommonMark) | Test `backtick_info_string_cannot_contain_a_backtick`; the hostile file now extracts 4 Mermaid SVGs and 5 contained errors instead of 0 |
| Critical: P3 code layout dominated the 3.2 MB document | `wrap_line` accumulates per-char advances instead of re-measuring every growing prefix (O(n²) → O(n) cheap lookups); tabs count four cells | Test extended with `a\tb`; isolated P3 (739 KB, 9,188 lines) with `target/release`: first frame 1.1 s, stable 3.9 s, 432 MB RSS (was 132–159 s first paint, 2.3 GB in debug) |
| High: `xlink:href` SVGs rejected (`unknown namespace prefix`) | `ensure_xlink_namespace` declares `xmlns:xlink` on the root `<svg>` before usvg parses it | Test `undeclared_xlink_namespace_is_added`; the smoke C4Context diagram now rasterizes and displays |
| High: CJK/Arabic tofu | `fonts.rs` loads Noto Sans CJK and Noto Naskh/Sans Arabic from the system font dirs as fallbacks after FiraCode (proportional, monospace and bold families) | Test `fallbacks_cover_cjk_and_arabic`; screenshot of 日本語/中文/한국어 and Arabic text renders shaped glyphs |
| Mermaid version bump (11.12.0 → 12.3.0) | `src/mermaid.rs`: `MERMAID_VERSION`, versioned `mermaid.min.js` filename, `RENDER_VERSION` key bumped to `6-mermaid12.3.0…`; `jr --version` prints the Mermaid and Chromium versions | Smoke re-render: 20 light + 20 dark SVGs regenerated, 1 expected invalid-diagram error (fresh 12.3.0 cache). `T-HO-047` (`__proto__` node ids) now renders; the ER quoted-attribute case (`T-MM-040`) still fails inside Mermaid 12.3.0 |
| Medium: reference links/images ignored | `src/doc.rs` collects `[id]: url` definitions, removes them, and resolves full/collapsed/shortcut references and reference images before parsing | Tests `reference_links_resolve_and_definitions_disappear`, `reference_images_resolve` |
| Medium: bare GFM autolinks plain | `expand_links` linkifies `http(s)://`, `www.` and bare emails, leaving code spans and inline links alone | Test `bare_urls_and_emails_are_linkified` |
| Medium: ragged tables kept extra cells | `parse_table_block` truncates rows to the header column count | Test `extra_cells_are_dropped_per_gfm` |
| Medium: lazy list/quote continuations escaped | List parsing continues an item on a non-block unindented line; quote parsing re-prefixes lazy lines | Tests `unindented_line_lazily_continues_the_item`, `unindented_line_stays_inside_a_quote` |
| Low: `#anchor` links did nothing | Headings record GitHub-style slugs; clicks store an anchor request that the app turns into a jump | Test `slugs_match_github_style`, visual check |
| Low: `1)` normalised to `1.` | Ordered kind keeps the source delimiter | Tests `parenthesised_ordered_marker_is_kept`, marker label test |
| Low: control chars/Unicode separators tofu | `clean_text` strips C0/C1/DEL and maps U+2028/U+2029 to spaces for prose (code blocks untouched) | Test `controls_are_stripped_and_separators_become_spaces` |
| Low: broken/remote images lost alt text | Remote/data/absolute images render their alt text; never fetched | Tests `remote_images_become_their_alt_text`, `remote_data_and_absolute_images_show_alt_text` |
| Low: Mermaid error text leaked wrapper/entities | `clean_error` strips `id="out">ERR:` and decodes entities | Test `error_text_loses_the_wrapper_and_entities` |
| Low: Mermaid inside a blockquote was code | A quote containing exactly one Mermaid fence is extracted | Test `mermaid_inside_a_blockquote_is_extracted` |
| Medium: two-space hard break lost | Paragraph lines keep a backslash marker; `parse_inline` emits a `Break`, the layout wraps a forced line | Tests `two_trailing_spaces_become_a_forced_break`, `forced_break_closes_the_line` |
| Medium: nested blockquotes flattened | New `<mdquote>` renderer: depth from repeated `>`, one bar per level, per-line search/selection; complex quote content still falls back to the library | Visual check of the smoke `Blockquotes (nested)` section; quote/lazy tests |
| Medium: fenced code inside a list collapsed | The list parser stops at an indented fence, so the fence becomes a real code block (placed after the list part, not indented) | Test `code_inside_quotes_and_indented_code_is_untouched` updated |
| Low: numbered task items lost their number | `ListKind::Task { done, ordered }` keeps `1.`/`1)` and the marker paints `1. ☑` | Test `ordered_task_item_keeps_its_number` |
| Low: local image in a table cell fell back to the library | A lone local image cell renders with `egui::Image` in our cell renderer; text cells keep strike/bold | Visual check `| ![pic](pic.png) | ~~strike~~ |` |
| High security: no URL scheme allowlist | `link_is_safe` allows only `http(s)://` and `mailto:`; other links stay inert and every link shows its destination as a hover tooltip | Test `only_web_and_mail_schemes_are_clickable` |

Gates after the fixes: `cargo test` 344 passed / 2 ignored, clippy
`-D warnings` clean, `cargo fmt --check` clean, `cargo build --locked`
warning-free; binary reinstalled.

## Still open

- **Full 3.2 MB document**: with the post-fix release binary, the first stable
  frame is ≈5.7 s and `G` responds in 0.62 s, settling in 4.1 s. The remaining
  slow action is a theme toggle while all 121 Mermaid diagrams need their dark
  rasters (each theme renders and caches separately); it is latency, not a hang.
- Still open: colour-emoji flags (the font stack cannot render colour bitmap
  fonts); Mermaid SVG styling choices (autonumber digits, quoted aliases,
  ER/journey labels, `theme:` frontmatter); a fenced code block inside a list
  item renders after the list part instead of being indented under the item;
  paragraphs/quotes nested inside list items still go to the library.
  Updated counts: Part 1 PASS 93 · FAIL 4 · N-A 15 · unsupported 4 · version 1;
  Part 2 PASS 27 · FAIL 0.
