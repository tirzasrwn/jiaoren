# Audit follow-up: Critical and High fixes

Code base: `f30e668` (after the original reports at `6054524`).
Environment: same as the reports (jiaoren 0.1.0, Mermaid 11.12.0, native egui
renderer, X11 `:0`).

## What changed

| Finding | Fix | Verification |
|---|---|---|
| Critical: diagram rasters >16384 px panic in `load_texture` | `src/app/raster.rs` caps the render scale so neither side exceeds `MAX_TEXTURE_SIDE` (16384) | Unit test `huge_svg_is_capped_below_the_texture_limit`; a 300-node chain (`/tmp/huge-chain.md`) now renders a capped image instead of panicking; app stays alive |
| Critical: a table inside a blockquote truncated the rest of the document (T-CM-061) | `src/doc.rs` `quote_table_split` extracts a table from any position in a blockquote, keeps the quote text before/after it, and renders the table through the normal table path | Tests `table_inside_a_blockquote_becomes_a_placeholder_and_keeps_later_content` and `mixed_quote_keeps_text_around_the_extracted_table`; screenshots show the table plus the "intro line", "outro line" and "after marker" |
| Critical/High: backtick in a fence info string opened a fence and suppressed the rest (T-HO-003) | `open_fence` rejects backticks in a backtick fence's info string (CommonMark) | Test `backtick_info_string_cannot_contain_a_backtick`; the hostile file now extracts 4 Mermaid SVGs and 5 contained errors instead of 0 |
| Critical: P3 code layout dominated the 3.2 MB document | `wrap_line` accumulates per-char advances instead of re-measuring every growing prefix (O(n²) → O(n) cheap lookups); tabs count four cells | Test extended with `a\tb`; isolated P3 (739 KB, 9,188 lines) with `target/release`: first frame 1.1 s, stable 3.9 s, 432 MB RSS (was 132–159 s first paint, 2.3 GB in debug) |
| High: `xlink:href` SVGs rejected (`unknown namespace prefix`) | `ensure_xlink_namespace` declares `xmlns:xlink` on the root `<svg>` before usvg parses it | Test `undeclared_xlink_namespace_is_added`; the smoke C4Context diagram now rasterizes and displays |
| High: CJK/Arabic tofu | `fonts.rs` loads Noto Sans CJK and Noto Naskh/Sans Arabic from the system font dirs as fallbacks after FiraCode (proportional, monospace and bold families) | Test `fallbacks_cover_cjk_and_arabic`; screenshot of 日本語/中文/한국어 and Arabic text renders shaped glyphs |
| High security: no URL scheme allowlist | `link_is_safe` allows only `http(s)://` and `mailto:`; other links stay inert and every link shows its destination as a hover tooltip | Test `only_web_and_mail_schemes_are_clickable` |

Gates after the fixes: `cargo test` 344 passed / 2 ignored, clippy
`-D warnings` clean, `cargo fmt --check` clean, `cargo build --locked`
warning-free; binary reinstalled.

## Still open

- **Full 3.2 MB document**: with the post-fix release binary, the first stable
  frame is ≈5.7 s and `G` responds in 0.62 s, settling in 4.1 s. The remaining
  slow action is a theme toggle while all 121 Mermaid diagrams need their dark
  rasters (each theme renders and caches separately); it is latency, not a hang.
- Medium/Low conformance findings from Part 1 were not part of this pass:
  reference links, two-space hard break, GFM bare autolinks, ragged tables,
  lazy continuation lines, BOM, control-character tofu, anchors, broken-image
  alt text, Mermaid error prefix/entity leakage, diagram styling details.
