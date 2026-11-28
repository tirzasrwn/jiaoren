# jiaoren

A zathura-like Markdown viewer. `jr doc.md` opens one window showing the
rendered Markdown — the way `zathura doc.pdf` does for PDFs — with ```mermaid
fences drawn as real diagrams.

Keyboard-first, almost no chrome, live-reloads when the file changes on disk.

![Opening examples/demo.md: justified prose with inline code and links, dragging a phrase and releasing it to copy, hovering a link for its destination tooltip, a vim-style `/` search starting at the current section with `n`/`N` and `↑`/`↓` history, hovering a table row and clicking the table icon to copy CSV, opening the diagram in the zoomable popup and switching its theme with `t`, holding `}` and pressing `{` to step blocks, `ctrl+o`/`ctrl+i` walking the jump list, the heading index opening at the current section with `h`/`l` folding and Enter jumping, a live reload holding that section while text is inserted above it, image captions and a tall image capped to the viewport, the bottom-right reading progress, `]` then `G` then `[` returning to the previous file's position, the `ctrl+g` path status and the `?` overlay, zooming and panning with `+` and `h`/`l`, `yy` copying the document, and the Catppuccin dark and light themes](docs/assets/demo.gif)

## Why "jiaoren"?

**鲛人** (jiāorén) is the merfolk of Chinese mythology — the Chinese mermaid.
Classical sources describe them living in the sea like fish, weaving without
rest, and weeping tears that harden into pearls; the silk they weave,
*jiaoxiao* (鲛绡), stays dry under water. The *Bowuzhi* (博物志, c. 290 CE) and
the *Shuyi ji* (述异记, 6th century) carry the story.

The name fits a viewer whose reason to exist is rendering **mermaid** diagrams,
and it keeps the command short: `jr`. (Some scholars read the earliest 鲛 as
"shark"; the pearl-weeping mermaid image is the later, poetic one.)

## Install

```bash
cargo install --path . --locked
```

Installs the binary `jr` to `~/.cargo/bin/jr`. No sudo needed.

Requirements: the first run downloads mermaid.js, a Chromium zip and fonts, so
it needs `curl` and `unzip` on `PATH` plus network — and a Chromium (the managed
download or one already on the system) to render diagrams. A document larger
than 32 MiB is rejected with a message instead of being opened.

## Usage

```bash
jr [--dark] file.md
jr --clean-cache          # cached diagram renders + mermaid.js
jr --clean-cache --all    # also Chromium and cached fonts (~132 MB)
```

- `--dark` — start with the dark theme.
- `--clean-cache` — delete cached diagram renders and the downloaded
  `mermaid.min.js`, then exit. The managed Chromium and downloaded fonts are
  kept (a re-download is expensive); add `--all` to remove those too. Startup
  prunes renders from older Mermaid/Chromium versions and font versions other
  than the pinned one, and is safe while other `jr` instances are running.

Colors are [Catppuccin](https://catppuccin.com/palette/): Latte for the light
theme, Macchiato for the dark theme. Window chrome and mermaid diagrams use the
same palette.
- A missing file, a directory, or a non-UTF-8 file prints a readable error to
  stderr and exits with status `2` (no window).
- Clicking an `http(s)`/`mailto` link copies its URL and shows a `link copied`
  chip; `#anchor` links jump to the heading instead.

## Typography

jiaoren uses **FiraCode Nerd Font** (family name `FiraCode Nerd Font`) for both
body text and code. On first run it downloads `FiraCodeNerdFont-Regular.ttf` and
`FiraCodeNerdFont-Bold.ttf` into `~/.cache/jiaoren/fonts/3.4.0/` and reports the
percentage in the bottom-left status line; the window opens immediately. A
failed or offline download falls back to a system copy.

Load order: `$JIAOREN_FONT_DIR` (explicit directory, no download), the managed
cache, the system fonts (`~/.fonts`, `$XDG_DATA_HOME/fonts`,
`~/.local/share/fonts`, `/usr/share/fonts/truetype`), then egui's built-in
fonts. `jr --version` prints the font version and which source is in use.
Mermaid diagram labels still need `FiraCode Nerd Font` installed system-wide.

## Keys

| Key | Action |
| --- | --- |
| `j` / `↓` | scroll down one line |
| `k` / `↑` | scroll up one line |
| `d` | scroll down half a page |
| `u` | scroll up half a page |
| `space` | scroll down a page |
| `b` | scroll up a page |
| `gg` | jump to the start (top-left) |
| `G` (shift+g) | jump to the end |
| `+` / `=` | zoom in |
| `-` | zoom out |
| `a` | reset zoom to 100% and pan back to the left |
| `0` | reset zoom to 100%; horizontal position is kept, the scroll clamp settles it |
| `h` / `l`, `shift+←` / `shift+→` | pan left / right (for zoomed-in content) |
| `t` | toggle light/dark theme |
| `r` | reload from disk |
| `]` / `[` | next / previous Markdown file in the same folder (wraps) |
| `}` / `{` | jump to the next / previous block start (paragraph, heading, list, table, code, image) |
| `ctrl+o` / `ctrl+i` | jump back / forward through index, anchor, search and `}`/`{` jumps |
| `o` | open the link under the pointer in the browser |
| `yy` | copy the whole document as Markdown |
| drag | select text across blocks; releasing the drag copies it |
| `y` | copy the text selection; with no selection the first `y` arms `yy` |
| `Tab` | heading index (depth 3), opening at the section being read: `j`/`k` move, `h`/`l` fold, `gg`/`G` first/last, `Enter`/`Space` jump, `t` theme |
| `/` | search from the current section: type a pattern, `Enter` confirms; `↑`/`↓` walk committed queries |
| `n` / `N` | next / previous match, wrapping |
| `ctrl+g` | show the open file's path in the status line |
| `?` | keyboard-shortcut overlay; `esc`, `q`, `?`, `ctrl+c` or a click closes it |
| `esc` / `ctrl+c` | copy the selection, else cancel the prompt, else clear the search, else quit |
| `q` | quit |

Hovering a link shows its destination in a tooltip. `o` opens the hovered
`http(s)`/`mailto` link with `$BROWSER`, else `xdg-open`,
`gio open` or `sensible-browser`; other schemes stay inert. `]` and `[` step
through the `.md`/`.markdown` files in the open document's folder, wrapping at
both ends, and land at the top of the next file.

Zoom and theme are saved to `$XDG_CONFIG_HOME/jiaoren/state.toml` (else
`~/.config/jiaoren/state.toml`) whenever they change, so the next launch starts
with the same view; `--dark` still wins over a saved light theme. Every
document's scroll position is saved on quit and when stepping with `]`/`[`, so
switching back to a file returns to where you left it (the 32 most recent files
are kept). A live reload (`r`, or an external save) keeps the committed search
pattern and the heading index's folds, and holds the heading at the top of the
viewport in place when text above it changes.

`jr --keys` prints the same list to stdout, so the shortcuts are reachable
without opening a document, and the in-viewer `?` overlay reads the same text.

`/` opens a plain vim-style command line at the bottom-left on the same solid
chip as the status line: matches update live as you type, Backspace edits,
Ctrl+W deletes a word, Esc/Ctrl+C restores the previous search, and
Enter commits. Matching starts at the block under the viewport and wraps, so a
search moves forward from where you are instead of jumping to the top. Enter
remembers the query; `↑`/`↓` walk the last 20 committed queries, returning to
the draft when you go past the newest. Search indexes the text `justify` paints: soft-wrapped source
lines join with one space, emphasis markup is dropped, link labels and code text
are kept, and code blocks are literal. Library-routed blocks are not searchable — blockquotes, and
paragraphs or table cells containing inline images or raw HTML. Inline-image
syntax in list items and headings matches its literal `!alt`. Spaces inside a match are part of the user's query, so the painted
highlight covers the gap between words too. Patterns are
smartcase — any uppercase character makes the search case-sensitive. Every hit
is highlighted in place, with a stronger fill for the current one; `n`/`N` wrap
around and scroll the current hit's block into view.

Mouse: drag anywhere to pan the document in both axes (the wheel still
scrolls, `gg` returns to the start). Dragging across text selects it; a plain
click clears the selection. Releasing a drag copies it; Ctrl+C and `y` also
copy the selection, including while the drag button is still held, and the copy
clears the highlight so the
next Ctrl+C moves on to clearing the search or quitting. A plain click on a
diagram or local image opens the zoom popup; dragging from it pans the document
instead.

While text is selected — dragging or after release — a code block stops painting
its hover row, so the selection is the only highlight on screen. Each visual line
is one continuous chip from its first selected glyph to its last, so the spaces
between words and the stretched gaps of a justified line are covered, and no tint
spills into the margins or the vertical gaps between blocks. Holding a drag in the
top or bottom strip of the window scrolls the document, and the wheel scrolls it
too while the button is still held — the selection follows the text that moves
under the pointer, and `y` or Ctrl+C then copies all of it.

Hovering a table row tints the whole row (all columns), so long rows are easy
to trace. Data rows use the theme's `surface1`; the header uses `surface2` in
the dark theme and `surface1` in the light theme, which keeps the text at AA
contrast.

Every table also carries a small copy icon at its top-right (tooltip: “Copy
table as CSV”); clicking it copies the table as CSV (header row first, Markdown
stripped from cells, fields quoted when they contain commas, quotes or line
breaks). Code blocks have the same icon over their top-right corner (tooltip
“Copy code”) and copy the block verbatim, with the trailing newline stripped.
Copy buttons show a 1.5-second bottom-left status line and never steal focus.
When the document is scrolled, a separate chip in the bottom-right corner shows
reading progress (`42%`); it stays hidden at the top.

Code blocks soft-wrap to the reading column and show a right-aligned line
number per source line. A wrapped line continues on the next row with an empty
gutter. Numbers are decoration: search, selection and both copy paths use the
raw source text, so the clipboard never contains them.

`Ctrl+C` shares the cancel-then-quit ladder with `esc`, but copies first when
text is selected: prompt, then copy the selection, then clear the search, then
quit. One press per Ctrl hold is acted on, so holding it cannot skip from
clearing the search to quitting.

Resizing the window keeps the reading column centred: scrolling down does not
stop it, and only a sideways pan of your own keeps its position. `a` is still
there to reset the zoom and pan back when you want to.

## Mermaid diagrams

Rendering ```mermaid fences into images requires **Chromium (headless)**, which
jiaoren runs to let mermaid.js rasterize each diagram.

jiaoren manages its own copy, so no Playwright, Node or sudo is needed. On
startup it looks for a browser in this order:

1. `JIAOREN_CHROMIUM` (an explicit binary path; ignored with a warning unless
   it is an executable file)
2. `chrome-headless-shell`, `chromium`, `google-chrome` or `chrome` on `PATH`
3. the managed install at
   `~/.cache/jiaoren/chromium/<version>/chrome-headless-shell-linux64/chrome-headless-shell`
   (under `$XDG_CACHE_HOME/jiaoren/...` when that variable is set)

When none is found, the first run downloads Chrome for Testing's
`chrome-headless-shell` (pinned version `158.0.8127.36`, about 132 MB, so the
first run needs network plus `curl` and `unzip` on `PATH`) and caches it under
`~/.cache/jiaoren/chromium/<version>/`. Later runs reuse it; delete that
directory to force a fresh download. Concurrent first runs are serialized by a
lock file, so only one instance downloads and the other reuses its install.

To point jiaoren at a different binary instead, set:

```bash
export JIAOREN_CHROMIUM=/path/to/chrome
```

jiaoren pins a SHA-256 for the Chrome for Testing zip, `mermaid.min.js`, and
both managed font faces. Each download is hashed before it is renamed or
unpacked; a mismatch deletes the temp file and reports the artifact and both
digests. This is not a signature and does not vouch for the upstream host, but
it rejects a body that differs from the pinned release. Downloads also have byte
caps and abort if the transfer stalls. The Chromium download is validated in a
temp directory and only then renamed into place, so a killed install leaves no
half-written binary.

If Chromium cannot be found or installed, the document still opens and a
warning banner names the failing step; diagrams simply won't render.

A diagram is drawn at its intrinsic size (centred when narrower than the text
column), capped to the column width when wider and to 75% of the window height
when taller, keeping its aspect ratio, with the same vertical spacing above and
below as a table. A tall diagram therefore shrinks inline; click it to see the
whole picture in the overlay. Click a diagram to open it in a dimmed
overlay: `+`/`-` or the wheel zoom (1×–3×), `0` resets the zoom and `a` resets
and centres the picture, `h`/`l`/`j`/`k` and mouse drag pan
(both axes, with half a frame of overscroll past every edge), `t` switches the
theme and redraws the diagram in it, and `Esc`, `q`,
`Ctrl+C` or a click outside closes it. Zoom is anchored at the pointer when it is over the diagram, so the
same part of the image stays under the cursor. `q` and `Ctrl+C` only close the overlay; `Ctrl+C` with no overlay open quits
jiaoren. On Linux the window backend reports `Ctrl+C` as egui's `Event::Copy`
rather than `Key::C`; `Cmd+C` on macOS and `Ctrl+Insert` also map to `Copy`, so
they quit too when no overlay is open.

## Local images

A paragraph that contains only one local image is drawn as an image block:

```markdown
![alt text](pics/cat.png "optional title")
```

Relative paths resolve against the document directory. A `"title"` is drawn as
one weak centred caption line under the image; without one the alt text is
used, and empty alt text draws no caption. A destination with spaces can be bracketed:
`![alt](<a b.png>)`. Document image paths are passed literally to the file
loader: `%20` is part of the filename, not an escape for a space.
**PNG, JPEG, GIF (first frame) and WebP** are decoded locally by the `image`
crate.

Remote images are never fetched: `http(s)://`, `data:` and absolute paths keep
the Markdown library's placeholder. Inline images (an image among text), and
images inside tables, lists or blockquotes, also stay on the library path and
are not clickable.

An image is drawn at its intrinsic pixel size (centred when narrower than the
text column), capped to the column width when wider and to 75% of the window
height when taller, keeping its aspect ratio, with the same vertical spacing
above and below as a table. Decoding runs on a background worker with
bounded dimensions/allocation; the pixels are downscaled to the display size
before upload, so a 4000×3000 photo does not keep a full-resolution texture.
The zoom popup re-resizes from the decoded source when zooming in, so `+` stays
sharp.

Click an image to open the same dimmed overlay as a diagram: `+`/`-` or the
wheel zoom (1×–3×), `0` resets the zoom, `a` resets and centres it, `h`/`l`/`j`/`k`
and mouse drag pan, and `Esc`, `q`, `Ctrl+C` or a click outside closes it. A changed file reloads automatically (the texture
is keyed by the file's mtime); a missing or undecodable file draws a red error
line naming the path.

## Cache

Everything managed lives under `~/.cache/jiaoren/`: versioned FiraCode faces,
chromium, `mermaid-<version>.min.js`, and the diagram SVGs (keyed by diagram
content, theme and render version). If neither `XDG_CACHE_HOME` nor `HOME` is
set, the cache goes in a per-UID directory under the system temp dir, created
with mode `0700` and checked for ownership. Reopening a document is instant and
diagrams stay sharp at any zoom.

First-run downloads (fonts, Chromium, mermaid.js) run in the background while
the window is already usable; their phase is shown in the bottom-left status
line, the same place as font progress. Diagram blocks show a short
"preparing diagrams…" box until the browser is ready.

Clear the cache from the CLI:

```bash
jr --clean-cache          # diagram renders + mermaid.js
jr --clean-cache --all    # also the managed Chromium and downloaded fonts
```

Startup also prunes renders and scripts from older Mermaid/Chromium versions.

## Trust boundary

Markdown and diagram source are untrusted input. Diagrams are rasterized by a
headless browser whose render page runs with Mermaid `securityLevel: "strict"`
and with networking blocked (`--host-resolver-rules=MAP * ~NOTFOUND`), so a
document cannot exfiltrate data or load remote resources during rendering.

## Known limitations

- Table columns are always left-aligned. `:---:`, `---:` and `:---` alignment
  markers are accepted and intentionally ignored; the flush-left grid is the
  owner's design choice, not a rendering gap.
- Wide tables wrap instead of getting a horizontal scrollbar, and code lines
  wrap at the reading column (a long token breaks at any character). Use `+` to
  zoom in when the text is hard to read.
- Body paragraphs are justified by a built-in layout: bold/italic/code/strike/
  links are supported, but a paragraph containing an inline image (one that is
  not alone in its paragraph) or raw HTML falls
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

Editing, tabs, a TUI mode, PDF/HTML export, math, remote images,
and mermaid interactivity are out of scope for v1. See
`docs/superpowers/specs/2026-11-20-mdview-design.md` for the full intent,
goals, and constraints.

## License

MIT — see [LICENSE](LICENSE). Copyright (c) 2026 tirzasrwn.
