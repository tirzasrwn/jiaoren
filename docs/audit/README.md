# Markdown Renderer Audit Suite

A set of Markdown test documents for auditing a Markdown renderer that supports Mermaid diagrams.
The suite checks correctness (does it render the way the spec says), safety (does it resist malicious
input), robustness (does it survive broken input), and scale (does it stay responsive on large documents).

## Contents

| File | Purpose | Size | Run when |
|------|---------|------|----------|
| `00-smoke-test.md` | Quick sanity check: common Markdown plus 18 Mermaid diagram types. Not an audit by itself. | ~14 KB | First, to confirm the basics work |
| `audit-01-conformance.md` | Main audit. CommonMark and GFM conformance, Unicode, security probes, advanced Mermaid. Every test has an ID and an EXPECT line. Includes a result sheet. | ~26 KB | Core audit |
| `audit-02-hostile.md` | Malformed and adversarial input: deep nesting, ReDoS probes, control and bidi characters, mixed line endings, DOM clobbering names, oversized and cyclic Mermaid diagrams, unclosed fences. | ~53 KB | After Part 1 passes |
| `audit-03-performance.md` | Generated stress file: 400 sections, 2000-row table, 60 code blocks, 121 Mermaid diagrams, 3000-item list, hundreds of footnotes, links and images, a 2 MB paragraph, 400 math expressions. | ~3.2 MB | Last, with profiling on |

## What each file tells you

### 00-smoke-test.md
Renders headings, lists, tables, code blocks, footnotes, math and 18 Mermaid diagram types
(flowchart, sequence, class, state, ER, Gantt, pie, mindmap, git graph, journey, timeline, quadrant,
requirement, C4, Sankey, XY chart, block, architecture). If this fails, fix it before running the audits.

### audit-01-conformance.md
Sections and ID prefixes:

- **A. CommonMark Core** (`T-CM-*`): headings and anchors, emphasis edge cases, code spans, links and images,
  line breaks and escapes, lists, blockquotes, code blocks, thematic breaks, HTML blocks.
- **B. GFM Extensions** (`T-GF-*`): tables (ragged, escaped pipes, alignment), strikethrough, task lists,
  footnotes, alerts, math.
- **C. Unicode** (`T-UN-*`): CJK, RTL, combining characters, ZWJ emoji, zero-width characters, anchors with non-ASCII text.
- **D. Security probes** (`T-SC-*`): XSS and injection vectors. Each probe only sets a harmless `window.__xss_*` flag.
- **E. Mermaid** (`T-MM-*`): advanced syntax per diagram type, config and init directives, placement in lists
  and blockquotes, error handling and recovery, theme switching.
- **F. Result Sheet**: table to record PASS or FAIL per ID range.

### audit-02-hostile.md
- **H1 to H3** (`T-HO-001` to `022`): deep nesting, ReDoS probes, control characters, bidi override,
  homoglyph and zero-width URLs, CRLF and bare CR line endings. Starts with a UTF-8 BOM.
- **H4** (`T-HO-030` to `036`): broken table delimiters, reference loops, cyclic footnotes, heading ids that
  collide with DOM ids or `__proto__`, prototype pollution payloads.
- **H5** (`T-HO-040` to `048`): oversized Mermaid (200-wide, 300-chain, 600 edges, 40 participants), cyclic
  subgraphs, reserved-word node ids, unterminated Mermaid frontmatter.
- **H6** (`T-HO-050` to `053`): fence torture. The final marker `END-OF-PART-2` is intentionally placed inside
  an unclosed fence, so a correct renderer shows it inside a code block.

### audit-03-performance.md
Sections P1 to P9 each stress one thing: prose volume, table size, syntax highlighting, Mermaid count,
one very large diagram, long lists, footnotes/links/images, a single huge paragraph, and math. The last line
of the file confirms the whole document rendered.

## How to run

1. Render `00-smoke-test.md` and confirm basics and diagrams work.
2. Open `audit-01-conformance.md`. For each test ID, compare the output with its **EXPECT** line and record
   PASS or FAIL in the Result Sheet (Section F).
3. Run the security check: after rendering Part 1 and Part 2, open the browser console and run

   ```js
   Object.keys(window).filter(k => k.startsWith('__xss'))
   ```

   **EXPECT:** an empty array. Any entry means a probe executed, and its name tells you which one.
4. Also check prototype pollution after Part 2:

   ```js
   ({}).polluted === undefined && ({}).polluted2 === undefined
   ```

   **EXPECT:** `true`.
5. Render `audit-02-hostile.md`. The app must not crash or hang, and `END-OF-PART-2` must appear inside a code block.
6. Render `audit-03-performance.md` with the profiler on. Record time to first paint, time to fully rendered,
   scroll smoothness, memory, and whether the UI stays responsive.
7. Switch light and dark theme and confirm all diagrams re-render (test `T-MM-095`).

## Important notes

- **Mermaid version matters.** `sankey-beta`, `xychart-beta`, `block-beta`, `architecture-beta`, `kanban`,
  `packet-beta`, `radar-beta`, `treemap-beta`, `@{ shape }` syntax and animated edges need a recent Mermaid
  (roughly 11.x, and some of the newest only in later 11.x releases). On older versions a **contained error
  box** is the correct behavior and counts as a version finding, not a renderer bug. Record the Mermaid version
  you test against.
- **Parser-dependent features.** `[TOC]`, footnotes, GitHub-style alerts, math, task lists, emoji shortcodes
  and front matter are extensions beyond core Markdown. Decide the intended behavior first, then mark
  unsupported features as "not supported by design" instead of FAIL.
- **Ambiguous tests.** `T-CM-091` (Markdown inside HTML blocks) and `T-MM-084` (Mermaid info-string variants)
  differ between parsers. Define the expected behavior before scoring.
- **Security isolation.** Run Parts 1 and 2 in a browser profile with no real credentials or sessions.
  The canaries are harmless, but the overlay, iframe and exfil-style probes are designed to exercise your
  sanitizer.
- **Part 2 contains unusual bytes** (BOM, NUL, bidi controls, lone CR). Some editors and git configs
  normalize them. Do not open and re-save the file in an editor before testing, or the tests may be altered.
- **Part 3 is large (~3.2 MB, ~20k lines).** Opening it in a text editor can be slow. Load it directly in the app.

## Scoring suggestion

| Severity | Examples |
|----------|----------|
| Critical | Any `__xss*` flag set, prototype pollution, `javascript:` link survives, app crash or hang |
| High | Mermaid error breaks other diagrams, content after an unclosed fence lost incorrectly, DOM clobbering of app UI |
| Medium | CommonMark conformance failures, wrong anchors, theme not re-applied |
| Low | Cosmetic issues, unsupported optional extensions, version-dependent Mermaid types |
