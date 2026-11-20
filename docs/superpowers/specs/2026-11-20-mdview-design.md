# mdview — zathura-like Markdown + Mermaid viewer

## Intent

`mdview doc.md` opens a new window showing the rendered Markdown, the way
`zathura doc.pdf` does for PDFs, with ```mermaid fences drawn as real diagrams.
Keyboard-first, almost no chrome, reloads when the file changes.

## Goals

- One command, one window: `mdview <file.md>`, process per window.
- GFM rendering: headings, lists, task lists, tables, code with syntax
  highlighting, blockquotes, footnotes, strikethrough, alerts.
- Each ```mermaid fence appears as an image, rendered by mermaid.js.
- Zathura-style keys: `j/k` scroll, `d/u` half page, `space/b` page,
  `gg`/`G` top/bottom, `+`/`-` zoom, `a` fit width, `t` theme toggle,
  `r` reload, `q` quit.
- Live reload when the open file changes on disk.
- Diagram results cached across runs; re-open is instant.

## Non-goals (v1)

- Editing, tabs, TUI/terminal mode, PDF/HTML export.
- `/` text search. Needs rendered-text access egui_commonmark doesn't expose;
  upgrade path is an outline/heading sidebar built from the pulldown events.
- Math, remote (`http`) images, mermaid interactivity (diagrams are static
  images).
- Local `![](img.png)` images are in scope and resolve relative to the
  document's directory, not the process CWD.

## Constraints

- No sudo on this machine: no GTK/WebKit dev headers, so `wry`/WebKit is out.
- Pure Rust build from crates.io; `egui` 0.36 and `egui_commonmark` 0.25 are
  already in the local cargo cache.
- Mermaid is JavaScript. Chromium exists at
  `~/.cache/ms-playwright/chromium-1255/chrome-linux64/chrome`; it is the
  runtime dependency for diagram rendering.
- Target desktop: X11 `:0`.

## Approaches considered

- **A. egui native window (chosen).** Markdown drawn by egui_commonmark;
  mermaid fences rasterized off-process by headless Chromium, results cached.
  No sudo, no system packages, full key control, real Mermaid output.
- **B. wry + WebKit2GTK.** Best fidelity, least code, mermaid natively, but
  blocked: needs `apt install libwebkit2gtk-4.1-dev libgtk-3-dev` (no sudo).
- **C. HTML + `firefox --new-window`.** ~100 lines, zero native deps, but no
  zathura keys and visible browser chrome. Rejected as a viewer.

## Design

Crates: `eframe` 0.36 (glow), `egui_commonmark` 0.25, `egui_extras` 0.36,
`blake3`, `notify` 8, `anyhow`. No clap: one positional arg plus `--dark`
parsed by hand.

Three modules:

1. `main.rs` — CLI parse, window setup, keys, scroll state, watcher, theme.
2. `doc.rs` — preprocess the Markdown: scan fenced blocks, pull out
   ```mermaid bodies, replace each with a single HTML line
   `<mermaid id="<blake3-hex>">`, and return `(rewritten_md, map<id, source>)`.
   Also rewrites relative `![](path)` URIs to absolute `file://` URIs based on
   the document's directory. Keeps everything else untouched.
3. `mermaid.rs` — cache and rasterizer.

Rendering into the view: `CommonMarkViewer` with `render_html_fn`, which
receives HTML events. Our closure matches `<mermaid id="…">`, then:

- PNG present at `~/.cache/mdview/<id>.png` → `ctx.load_texture`, draw it at
  the last known size, scaled by the current zoom.
- Not present → draw a fixed placeholder box ("rendering diagram…"), send the
  source to a worker thread over `mpsc`, and `ctx.request_repaint()` when the
  worker reports back.

Rasterizer (`mermaid.rs`, worker pool of up to 4 threads): write
`~/.cache/mdview/<id>.html` containing the vendored `mermaid.min.js` and the
diagram source, then run

    <chrome> --headless --no-sandbox --disable-gpu --dump-dom \
             --virtual-time-budget=5000 file://<html>

Mermaid is initialized with `htmlLabels: false` and an explicit `fontFamily`,
because resvg cannot draw `<foreignObject>` labels. Extract the `<svg>…</svg>`
element from stdout, rasterize with `resvg` (2× scale, transparent
background, fontdb with system sans), write PNG to cache. Deterministic size,
no screenshot trimming needed. Chromium resolution order: `$MDVIEW_CHROMIUM`,
`chromium`, `google-chrome`, `chrome` in `PATH`, then the newest
`~/.cache/ms-playwright/chromium-*/chrome-linux64/chrome`.

Cache key = blake3 of (diagram source, theme, mermaid.js version string,
render-config hash), so a mermaid upgrade or config change cannot serve stale
PNGs. Failures cache too: a `<id>.err` marker keyed the same way, so a broken
diagram is not retried until its source changes. A failed diagram draws an
inline error box in its place; other diagrams continue. When Chromium is
missing, the window still opens with text and one banner across the top.

`mermaid.min.js` is downloaded once into `~/.cache/mdview/` via `curl` if
missing (`$MDVIEW_MERMAID_JS` overrides the path). Diagram theme (dark/light)
is part of the cache key, so `t` re-renders rather than mis-coloring.

Keys map to scroll deltas on an `egui::ScrollArea`; `gg` is a two-key sequence
with a 500 ms window. Zoom multiplies the text style size and image scale, and
text re-wraps as it grows. `a` fits diagrams and wide code blocks to the
current column width (zoom back to 100% for images, nothing else changes).

Layout: content wraps to the window width with a readable max column
(`min(window − 64px, 900px)`), centered — no fixed page like zathura, because
Markdown reflows. Wide tables and long unbreakable code lines get a horizontal
scrollbar on their own block instead of shrinking.

First open with uncached diagrams: text paints immediately, each diagram shows
a placeholder sized 640×320 at 1× and is replaced in place when its PNG lands
(2–4 concurrently). Content below shifts once; scroll position is re-anchored
to keep the visible heading steady. On `t`, the old PNG stays visible until the
new-theme render replaces it, so nothing flashes.

Live reload: `notify` watcher on the parent directory, 100 ms debounce,
reload only when the event path equals the open file.

Deployment: `cargo install --path .` → `~/.cargo/bin/mdview`, so
`mdview doc.md` works anywhere.

## Verification

- Unit test in `doc.rs`: a document with prose + two mermaid fences + a normal
  ```rust block rewrites to exactly two `<mermaid id=…>` lines, ids stable
  across runs, and non-mermaid code blocks survive unchanged.
- Manual: open `docs/superpowers/specs/2026-11-20-mdview-design.md`, a doc with
  a flowchart and a sequence diagram; check keys, reload, and that a second
  open renders diagrams from cache instantly.

## Left to the builder

- Exact egui key handling details (focus, `gg` timing).
- Whether zoom resets on reload (sensible default: keep).
- Wording of the placeholder and error text when Chromium is missing.
