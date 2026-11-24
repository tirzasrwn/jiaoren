# Audit Results — Part 2 (Hostile and Malformed Input)

**Historical:** recorded against jiaoren 0.1.0 at the commit named below. Kept as-is; re-run the suite on the current version (see README) when re-scoring.

**Environment:** jiaoren `6054524` (`./target/debug/jr`, `jiaoren 0.1.0`), native egui/egui_commonmark
renderer; no document DOM and no document JavaScript. Mermaid `11.12.0` in headless Chromium
`158.0.8127.36`, `securityLevel: strict`, render key
`5-mermaid11.12.0-htmlLabelsOff-2x-strict-svg-catppuccin-firacode`,
`--host-resolver-rules=MAP * ~NOTFOUND`, `--virtual-time-budget=5000`. X11 `:0`, i3 workspace 9,
floating window 1200x900 at 40,40; date 2026-11-22.

**Second fix pass (commit `4f23025`).** Control characters are stripped, Unicode line/paragraph
separators become spaces, and the `-:-` delimiter is rejected. Revised counts: PASS 27 · FAIL 0.
Deferred: nothing from this file except cosmetic Mermaid error styling (fixed too).

**Post-fix re-check (commit `8ecd827`).** The two raster panics (T-HO-040/041) and the fence
suppression (T-HO-003) are fixed; the SVG raster is capped at 16384 px per side, backticks in a
backtick-fence info string are rejected, and links outside `http(s)`/`mailto` are inert with the target
shown. Revised counts: PASS 24 · FAIL 3 · N-A 2 · VERSION 2 · BY-DESIGN 2. Details:
`results-after-fixes.md`.

**Input:** `docs/audit/audit-02-hostile.md`, unmodified (BOM and mixed line endings intact).

Evidence: `docs/audit/evidence-02-hostile/` (PNG captures). Scratch probes were written only under
`/tmp/opencode/`; the audit file, app code and tests were not changed.

---

## 1. Full-document run

| Phase | Wall time | Result |
|---|---:|---|
| Launch → window mapped / first paint | 66 ms | window opens, content painted |
| Six full-page scrolls through H1–H4 | 997 ms (~166 ms/page) | responsive; no dropped input |
| Diagram renders in the full file | 0 svg / 0 err | **no Mermaid fence is extracted** (see finding H-1) |
| End of document | ~1 s after `G` | `END-OF-PART-2` visible inside a code block; app alive |
| Crashes / hangs in the full file | 0 / 0 | no freeze; process stayed responsive |

The full file does **not** crash. That is not a pass for hostile Mermaid: no Mermaid after line 64 is
extracted at all. The same diagrams rendered from a valid prefix crash the app (§4).

---

## 2. Result by test ID

Legend: **PASS** = matches EXPECT; **FAIL** = user-visible deviation; **N-A** = no DOM/JS in a native
renderer; **BY-DESIGN** = documented non-goal; **VERSION** = contained Mermaid-source/version behaviour.

| ID | Verdict | Evidence (one line) |
|---|---|---|
| T-HO-001 | PASS | 60-level quote caps to one quote bar; "deep quote" visible; no hang (`01-top-bom-heading.png`). |
| T-HO-002 | PASS | all 40 list items render; nesting stops at depth 5, items 6–40 share one indent (`02-deep-list-levels.png`). |
| T-HO-003 | PASS (fixed `2495dc0`) | The 200-backtick line is no longer a fence opener, so extraction continues; the huge `![…]` still prints a truncated image error (low, cosmetic). |
| T-HO-004 | PASS | all ReDoS probes return at first paint; 400-column table / long link wrap and clip horizontally, no lag. |
| T-HO-010 | PASS (fixed `4f23025`) | Control characters are stripped from prose. |
| T-HO-011 | PASS | noncharacters show tofu and U+FFFD shows a diamond fallback; inert, no crash (`03-…png`). |
| T-HO-012 | PASS | RLO/isolate do not reorder the line or surrounding UI; reads `abc.exe and isolate` (`03-…png`). |
| T-HO-013 | FAIL | U+2028/U+2029 are not line/paragraph breaks here: shown as tofu `a□b□c` (`03-…png`). |
| T-HO-014 | PASS | Cyrillic homoglyph link renders as `pаypal.com`, indistinguishable from the real name; target is `https://evil.example/` and is not previewed (security finding S-1). |
| T-HO-015 | PASS | zero-width URL link `x` renders and is clickable; target `https://exam​ple.com` is not shown (S-1). |
| T-HO-020 | PASS | CRLF paragraph, list and ```js fence render correctly (`04-line-endings-crlf-cr-mixed.png`). |
| T-HO-021 | PASS | bare CR is treated as a line ending; no stray CR glyphs, text survives. |
| T-HO-022 | PASS | mixed LF/CRLF/CR inside one fence produces three clean code lines (`04-…png`). |
| T-HO-030 | PASS | unclosed HTML/comment/CDATA/PI/DOCTYPE block is dropped inertly; later content survives. |
| T-HO-031 | **FAIL** | `\|--\|` delimiter is rejected (literal text), but `\| :-: \| -:- \|` is silently accepted as a table; invalid delimiter row disappears (`05-broken-tables-refs-footnotes.png`). |
| T-HO-032 | BY-DESIGN | reference links unsupported (known Part 1); `[a] [b] [loop]` render as blue text links, loop does not hang. |
| T-HO-033 | BY-DESIGN | footnotes unsupported; cyclic definitions render as inline definitions, no recursion or hang. |
| T-HO-034 | PASS | four duplicate headings all render; no collision/crash. `[to dup](#duplicate-1)` is a blue link but internal anchors are handed to the OS, so it cannot jump (low). |
| T-HO-035 | N-A | no DOM to clobber; `<a id=root>`/`<form>` shown literally and UI unaffected. `### __proto__` displays as **"proto"** (emphasis eats the underscores) (`07-dom-inert-h5-suppressed.png`). |
| T-HO-036 | N-A | no document JS/prototype; JSON block is inert text and the UI is unaffected. Console checks N/A. |
| T-HO-040 | PASS (fixed `d366135`) | Re-ran the isolated 200-node case: the raster scale is capped, the diagram renders (reduced resolution), and the app is alive after 20 s. Original run panicked at 9.5 s. |
| T-HO-041 | PASS (fixed `d366135`) | Re-ran the isolated 300-node chain: capped raster, diagram renders, app alive after 20 s. Original run panicked at 3.7 s. |
| T-HO-042 | PASS | Mermaid's own `Edge limit exceeded. 500 edges found` is contained; app alive at 1.35 s. Error dumps all 600 source lines (cosmetic). (`12-mermaid-042-edge-limit.png`). |
| T-HO-043 | PASS | cyclic subgraphs rejected by Mermaid (`Setting s2 as parent of s1 would create a cycle`), contained, app alive (`13-mermaid-043-cycle.png`). |
| T-HO-044 | VERSION | reserved-word flow (`end --> start`) is a contained Mermaid parse error; message leaks `--&gt;` and a long token list (`14-mermaid-044-reserved.png`). |
| T-HO-045 | PASS | 40-participant sequence renders 8050×2121 and is scaled to fit; readable only in the click-to-zoom popup (`15-mermaid-045-participants.png`). |
| T-HO-046 | PASS | unterminated frontmatter is a contained Mermaid error, app alive (`16-mermaid-046-frontmatter.png`). |
| T-HO-047 | VERSION | prototype-name node ids hit a Mermaid error (`Cannot set properties of undefined (setting 'order')`); contained; host process unaffected (`17-mermaid-047-prototype.png`). |
| T-HO-048 | PASS | no blank line after fence still renders Tight1→Tight2; "Text directly after fence." and the list follow normally (`20-tight-fence-text-after.png`). |
| T-HO-050 | PASS | closing fence longer than opener, with trailing spaces, closes the ```js block correctly (`08-h6-end-marker.png`). |
| T-HO-051 | PASS | ``` `not a fence` line is a paragraph with inline code, not a code fence (`08-…png`). |
| T-HO-052 | PASS | `-`, `#`, `>` lines inside the text fence stay literal code (`08-…png`). |
| T-HO-053 | PASS | `END-OF-PART-2` is inside the final unclosed code block; the app still renders everything above it (`08-…png`). |

Verdict counts (33 IDs): **original run** PASS 21, FAIL 6, N-A 2, VERSION 2, BY-DESIGN 2 —
**after both fix passes** PASS 27, FAIL 0.

---

## 3. Mermaid-heavy phase timings

Isolated per-case files (exact bytes from the audit, no edits) with a fresh render cache:

| Case | SVG/error time | Outcome | App |
|---|---:|---|---|
| T-HO-040 | 1671 ms | SVG 393×33277 requested | **panic at 9535 ms** |
| T-HO-041 | 1845 ms | SVG 193×50110 requested | **panic at 3719 ms** |
| T-HO-042 | 1350 ms | contained edge-limit error | alive, scroll OK |
| T-HO-045 | 1411 ms | rendered 8050×2121 | alive, scroll OK |
| T-HO-048 | 1302 ms | rendered 304×66 | alive, scroll OK |
| T-HO-042…048 together | 2100 ms for 7 outcomes (2 svg, 5 err) | all contained | alive |

Responsiveness during render: while those seven diagrams were rendering, six full-page scrolls were
accepted in **424 ms** (~71 ms/page) and the app stayed alive (`19-h5safe-scroll-during-render.png`).
The UI thread is not blocked by the Chromium work; the crash comes later, when the oversized raster
is uploaded as a texture.

No hangs. T-HO-040's ~8 s dark placeholder before the panic is the worst stall.

---

## 4. Genuine defects

### Critical

1. **FIXED (`d366135`) — Oversized diagram raster panics the app (T-HO-040, T-HO-041).**
   `bucket_for_scale` caps the *bucket* against an 8192 px / 16 Mpx budget
   (`src/app/geometry.rs:260-283`), but when the intrinsic height times `pixels_per_point` is already
   above the GPU limit it returns bucket 1 and `rasterize` still produces the full image
   (`src/app/raster.rs:45-58`). `DiagramUi::poll` then calls
   `ctx.load_texture` (`src/app/diagram.rs:273`) with 393×33277 or 193×50110; egui panics at
   `egui-0.36.2/src/context.rs:2399` ("maximum texture side is 16384"). This is a reproducible
   process abort on valid Mermaid input, not just the hostile suite.
   Panic logs: `/tmp/opencode/cases/040.log`, `/tmp/opencode/cases/041.log`.

### High

2. **FIXED (`2495dc0`) — A backtick in a fence info string disables all app-level extraction for the rest of the file
   (T-HO-003 / T-HO-051).** `open_fence` accepts any text after the backtick run
   (`src/doc.rs:721-730`) and does not implement CommonMark's rule that a backtick fence's info string
   may not contain backticks. The 200-backtick line at `audit-02-hostile.md:64` therefore opens a fence
   that never closes. The EOF fallback emits the rest as raw Markdown
   (`src/doc.rs:625-630`), so egui_commonmark renders the text, but the app's own extraction — Mermaid,
   tables, lists, headings, standalone images — never runs again. Result: every H5 diagram in the
   audit file appears as a plain code block with a copy button. Minimal proof:
   `09-fence-suppression-probe.png` (BEFORE renders, invalid fence line, AFTER renders as code).
   No text is lost; Mermaid/table/list functionality is lost for 95% of the document. It also masks
   defect 1 in the full-file run.

3. **No URL scheme allowlist; link targets are handed to the OS handler (security, S-1).**
   `parse_link` stores any destination verbatim (`src/justify.rs:316-326`) and a click calls
   `ctx.open_url` (`src/justify.rs:1440-1446`). `javascript:`, `file:`, `data:` and arbitrary schemes
   are not filtered or confirmed. The renderer also gives no hover/status preview of the target, so
   the homoglyph link (`T-HO-014`) and the zero-width-host link (`T-HO-015`) are indistinguishable
   from legitimate links. Not clicked during this audit.

   Images: `is_local_image`/`rewrite_images` keep `http(s)`, `data:` and `file://` off the custom
   image path (`src/doc.rs:981-999`, `1020-1026`); remote HTTP is not fetched (no `http` feature), but
   `egui_extras`' file loader will read `file://` images from disk and the image loader decodes
   `data:` URIs in-process. No image-scheme allowlist either.

### Low / cosmetic

4. **UTF-8 BOM is not stripped** (`src/doc.rs:28-37`). The first line is not recognised as a heading;
   the user sees a literal `# Markdown Renderer Audit: Part 2 …` (`01-top-bom-heading.png`).
5. **Control and separator characters are visible tofu** (T-HO-010, T-HO-013): boxes instead of
   invisible formatting characters; noncharacters and U+FFFD also use fallback boxes/diamonds.
6. **Invalid table delimiter accepted** (T-HO-031): `-:-` is treated as a delimiter, so a malformed
   table silently renders while the nearby `|--|` case is rejected (`05-…png`).
7. **Heading text mutation**: `### __proto__` renders as `proto` because emphasis consumes the
   underscores (T-HO-035).
8. **Mermaid error text leaks internals**: every error is prefixed `id="out">ERR:` and can contain
   `--&gt;`; the full source is printed under the error, up to 600 lines (T-HO-042), which pushes the
   following content far down.
9. **Fitted-wide diagrams are unreadable inline** (T-HO-045, 8050 px wide); usable only via the
   popup zoom.

### Content loss

None observed. The unclosed-final-fence marker is retained, and T-HO-030's malformed HTML is dropped
by design rather than eating later content. The line-64 defect costs features, not text.

---

## 5. `END-OF-PART-2` location

Visible **inside the final `text` code block**, below "This fence is intentionally never closed.",
with the app's copy button on the block. It is the last content on the page; nothing after it is lost
(`08-h6-end-marker.png`). This satisfies T-HO-053.

---

## 6. Evidence index (`docs/audit/evidence-02-hostile/`)

| File | Shows |
|---|---|
| `01-top-bom-heading.png` | BOM prevents H1; H1/list start |
| `02-deep-list-levels.png` | 40-level list, T-HO-003 emphasis and image error |
| `03-controls-bidi-homoglyph.png` | controls, bidi, separators, homoglyph/zero-width links |
| `04-line-endings-crlf-cr-mixed.png` | CRLF, bare CR, mixed fence |
| `05-broken-tables-refs-footnotes.png` | malformed delimiters, reference links, cyclic footnotes |
| `06-duplicate-heading-ids.png` | duplicate headings, `__proto__`/`constructor` headings |
| `07-dom-inert-h5-suppressed.png` | inert HTML payload; H5 shown as code (extraction disabled) |
| `08-h6-end-marker.png` | fence torture and `END-OF-PART-2` inside code |
| `09-fence-suppression-probe.png` | BEFORE diagram renders, AFTER (post invalid fence) does not |
| `10-mermaid-040-stall.png` | 040 dark placeholder before panic |
| `11-mermaid-041-stall.png` | 041 dark placeholder before panic |
| `12-mermaid-042-edge-limit.png` | contained 500-edge error |
| `13-mermaid-043-cycle.png` | contained cyclic-subgraph error |
| `14-mermaid-044-reserved.png` | reserved-word parse error |
| `15-mermaid-045-participants.png` | 40 participants, fit-to-width |
| `16-mermaid-046-frontmatter.png` | unterminated frontmatter error |
| `17-mermaid-047-prototype.png` | prototype-id error |
| `18-mermaid-048-tight-fence.png` | no blank line after fence |
| `19-h5safe-scroll-during-render.png` | scrolling while seven diagrams render |
| `20-tight-fence-text-after.png` | text/list after a Mermaid fence without blank line |
