# mdview

A zathura-like Markdown viewer. `mdview doc.md` opens one window showing the
rendered Markdown — the way `zathura doc.pdf` does for PDFs — with ```mermaid
fences drawn as real diagrams.

Keyboard-first, almost no chrome, live-reloads when the file changes on disk.

## Install

```bash
cargo install --path . --locked
```

Installs `mdview` to `~/.cargo/bin/mdview`. No sudo needed.

## Usage

```bash
mdview [--dark] file.md
```

- `--dark` — start with the dark theme.
- A missing file, a directory, or a non-UTF-8 file prints a readable error to
  stderr and exits with status `2` (no window).

## Keys

| Key | Action |
| --- | --- |
| `j` / `↓` | scroll down one line |
| `k` / `↑` | scroll up one line |
| `d` | scroll down half a page |
| `u` | scroll up half a page |
| `space` | scroll down a page |
| `b` | scroll up a page |
| `gg` | jump to top |
| `G` (shift+g) | jump to bottom |
| `+` / `=` | zoom in |
| `-` | zoom out |
| `a` | reset zoom to 100% |
| `t` | toggle light/dark theme |
| `r` | reload from disk |
| `q` / `esc` | quit |

## Mermaid diagrams

Rendering ```mermaid fences into images requires **Chromium (headless)**, which
mdview runs to let mermaid.js rasterize each diagram.

Chromium is already present on this machine via the playwright cache; mdview
uses the newest `chromium-*` directory under `~/.cache/ms-playwright/`
automatically. To point mdview at a different binary, set:

```bash
export MDVIEW_CHROMIUM=/path/to/chrome
```

If no Chromium is found, the document still opens and a warning banner is
shown; diagrams simply won't render.

## Cache

Rendered diagrams are cached under `~/.cache/mdview/` (keyed by diagram content
and theme), so reopening a document is instant. `mermaid.min.js` is downloaded
there on first run, not bundled. Clear it with:

```bash
rm -rf ~/.cache/mdview
```

The directory and `mermaid.min.js` are recreated on the next run.

## Trust boundary

Markdown and diagram source are untrusted input. Diagrams are rasterized by a
headless browser whose render page runs with Mermaid `securityLevel: "strict"`
and with networking blocked (`--host-resolver-rules=MAP * ~NOTFOUND`), so a
document cannot exfiltrate data or load remote resources during rendering.

## Known limitations

- Wide tables and long code lines wrap instead of getting a horizontal
  scrollbar. Use `+` to zoom in when a wrapped line is hard to read.

## Non-goals

Editing, tabs, a TUI mode, `/` search, PDF/HTML export, math, remote images,
and mermaid interactivity are out of scope for v1. See
`docs/superpowers/specs/2026-11-20-mdview-design.md` for the full intent,
goals, and constraints.
