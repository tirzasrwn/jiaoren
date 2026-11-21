# jiaoren

A zathura-like Markdown viewer. `jr doc.md` opens one window showing the
rendered Markdown — the way `zathura doc.pdf` does for PDFs — with ```mermaid
fences drawn as real diagrams.

Keyboard-first, almost no chrome, live-reloads when the file changes on disk.

## Install

```bash
cargo install --path . --locked
```

Installs the binary `jr` to `~/.cargo/bin/jr`. No sudo needed.

## Usage

```bash
jr [--dark] file.md
```

- `--dark` — start with the dark theme.

Colors are [Catppuccin](https://catppuccin.com/palette/): Latte for the light
theme, Macchiato for the dark theme. Window chrome and mermaid diagrams use the
same palette.
- A missing file, a directory, or a non-UTF-8 file prints a readable error to
  stderr and exits with status `2` (no window).

## Typography

jiaoren uses **FiraCode Nerd Font** (family name `FiraCode Nerd Font`) for both
body text and code, and for mermaid diagram labels. It looks for
`FiraCodeNerdFont-Regular.ttf` / `FiraCodeNerdFont-Bold.ttf` in `~/.fonts`,
`$XDG_DATA_HOME/fonts`, `~/.local/share/fonts`, then
`/usr/share/fonts/truetype`. When the font is absent jiaoren starts normally on
egui's built-in fonts.

To go back to the defaults, delete (or comment out) the
`crate::fonts::install(ctx);` call in `src/app.rs::App::new`.

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
| `a` | reset zoom to 100% and pan back to the left |
| `0` | reset zoom to 100% (keeps scroll position and pan) |
| `h` / `l`, `shift+←` / `shift+→` | pan left / right (for zoomed-in content) |
| `t` | toggle light/dark theme |
| `r` | reload from disk |
| `q` / `esc` / `ctrl+c` | quit |

## Mermaid diagrams

Rendering ```mermaid fences into images requires **Chromium (headless)**, which
jiaoren runs to let mermaid.js rasterize each diagram.

Chromium is already present on this machine via the playwright cache; jiaoren
uses the newest `chromium-*` directory under `~/.cache/ms-playwright/`
automatically. To point jiaoren at a different binary, set:

```bash
export JIAOREN_CHROMIUM=/path/to/chrome
```

If no Chromium is found, the document still opens and a warning banner is
shown; diagrams simply won't render.

A diagram is drawn at its intrinsic size (centred when narrower than the text
column) and capped to the column width when wider, with the same vertical
spacing above and below as a table. Click a diagram to open it in a dimmed
overlay: `+`/`-` or the wheel zoom (1×–3×), `h`/`l`/`j`/`k` and mouse drag pan
(both axes, with half a frame of overscroll past every edge), and `Esc`, `q`,
`Ctrl+C` or a click outside closes it. Zoom is anchored at the pointer when it is over the diagram, so the
same part of the image stays under the cursor. `q` and `Ctrl+C` only close the overlay; `Ctrl+C` with no overlay open quits
jiaoren. On Linux the window backend reports `Ctrl+C` as egui's `Event::Copy`
rather than `Key::C`; `Cmd+C` on macOS and `Ctrl+Insert` also map to `Copy`, so
they quit too when no overlay is open.

## Cache

Rendered diagrams are cached as SVG under `~/.cache/jiaoren/` (keyed by diagram
content and theme), so reopening a document is instant and diagrams stay sharp
at any zoom level. `mermaid.min.js` is downloaded there on first run, not
bundled. Clear it with:

```bash
rm -rf ~/.cache/jiaoren
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
- Body paragraphs are justified by a built-in layout: bold/italic/code/strike/
  links are supported, but a paragraph containing an image or raw HTML falls
  back to plain (left-aligned) rendering.
- List items render justified with a hanging indent for `-`/`*`/`+`, `1.`/`1)`
  and `[ ]`/`[x]` markers; a list block containing an image or raw HTML falls
  back to the library whole. An item is one paragraph: continuation lines are
  joined, and a blank line ends the block, so a second paragraph inside an item
  renders as a separate block-level paragraph. `3. [x]` renders as an unnumbered
  task box because the ordered number is dropped when the task suffix wins.
- Long tokens (paths, URLs, identifiers) break at separators (`/`, `-`, `_`,
  `:`, `.`, `,`, `;`, `?`, `&`, `=`, `\`) and, failing those, at a hard
  character budget, in paragraphs and table cells alike. Cells wrap and break
  at separators too, but a token too wide for a cell — including one split
  across styled runs like `src/**db**/x` — is clipped inside that cell instead
  of widening the table. A cell holding an image or raw HTML still renders
  through the library.
- `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;`, `&apos;` and `&nbsp;` are decoded
  in paragraphs and cells; other HTML entities show literally.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test
cargo test -- --ignored        # needs headless chromium
```

`all` and `perf` clippy groups are denied in `Cargo.toml`, so the gate is
enforced by the compiler. `pedantic` is intentionally off (its casts are
noise here); targeted pedantic advice is applied by hand.

## Non-goals

Editing, tabs, a TUI mode, `/` search, PDF/HTML export, math, remote images,
and mermaid interactivity are out of scope for v1. See
`docs/superpowers/specs/2026-11-20-mdview-design.md` for the full intent,
goals, and constraints.
