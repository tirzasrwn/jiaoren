# Changelog

All notable changes to jiaoren. Versions follow [SemVer](https://semver.org/).

## 0.5.3 — 2026-11-26

### Changed

- Refreshed `docs/assets/demo.gif`: it now also shows the heading index with
  fold and jump, the `ctrl+g` path status, the `?` shortcut overlay, zoom and
  pan, `yy`, and the theme switching inside the diagram overlay. The recorder
  runs on a private Xvfb display, so it no longer needs a desktop or i3.

## 0.5.2 — 2026-11-26

### Changed

- Scrolling units follow the rendered text: `j`/`k` move one body line and
  `h`/`l` one monospace column at every zoom instead of a fixed 40 px. A held
  key now repeats only scrolling; `yy`, `gg`, `G`, `r`, `t`, `]`, `[`, Tab and
  quit wait for a fresh press.

### Fixed

- A document with deeply nested emphasis no longer overflows the stack; nesting
  past 32 levels renders the markers literally.
- `    ---` / `    ===` at four-space indent stay a paragraph instead of
  becoming a setext heading and dropping the line.
- Image syntax inside inline code is no longer rewritten to a `file://` URL.
- The table copy button and table links are clickable again: controls are
  sorted by top before the lookup.
- Typing in `/` no longer copies the whole document per keystroke; block texts
  are borrowed and one char buffer is reused.
- Quote blocks no longer clone their spec and re-join their text every frame.
- An invalid-UTF-8 SVG keeps its bytes so the parse error names the encoding.
- Reading a document with a BOM no longer copies the whole file.

## 0.5.1 — 2026-11-26

### Fixed

- `]` / `[` home the new document's reading column again. The horizontal
  offset only homed on the very first frame of a session, so a navigated
  document drew its column one pad to the right.

## 0.5.0 — 2026-11-26

### Changed

- The heading index no longer paints the shortcut hint chip; the `?` overlay
  still lists the keys.

### Fixed

- A window resize keeps the content that was in the middle of the viewport in
  the middle when the reader has panned sideways; at home the column stays
  anchored to its left edge. Above 1× zoom the wider column is no longer
  snapped back to its left edge.
- `t` still switches the theme while a diagram or image zoom overlay is open;
  the overlay redraws in the other theme instead of ignoring the key.

## 0.4.1 — 2026-11-25

### Changed

- The release profile strips symbols and uses thin LTO, and the direct `resvg`
  matches the version `egui_extras` already pulls in: the binary is 15.1 MiB
  instead of 21.3 MB.

### Fixed

- Inline runs are memoised per block and cleared on reload, so a pathological
  line (many unclosed `[` or backticks, dense emphasis) no longer re-parses
  every frame and freezes the window.
- A document larger than 32 MiB is rejected with a message instead of being
  read into memory.
- Opening a link reaps the browser process instead of leaving a zombie per
  click.
- A failed render removes its HTML scratch page and `.tmp` file, and
  `--clean-cache` leaves another instance's fresh `.tmp` files alone.

## 0.4.0 — 2026-11-25

### Added

- `]` / `[` open the next/previous Markdown file in the document's folder,
  wrapping, at the top of the file.
- `o` opens the hovered `http(s)`/`mailto` link in the desktop browser
  (`$BROWSER`, `xdg-open`, `gio open`, `sensible-browser`); other schemes stay
  inert.
- Zoom and theme persist between sessions in
  `$XDG_CONFIG_HOME/jiaoren/state.toml` (or `~/.config/jiaoren/state.toml`);
  `--dark` still wins over a saved light theme.
- A window resize re-homes the reading column, so a resize no longer leaves the
  document off-centre until `a` is pressed.
- `tests/cli.rs` end-to-end CLI tests.
- SHA-256 pinning for every downloaded artifact (mermaid.js, the Chromium zip,
  both FiraCode faces), verified before use; downloads have byte and stall
  caps.
- `examples/demo-2.md` and a refreshed `docs/assets/demo.gif`.

### Changed

- Zoom-driven image re-resizes and SVG raster caps run off the UI thread.
- Block text, cell text and scaled text styles are cached per document instead
  of rebuilt every frame; hit testing searches only the blocks under the
  pointer; off-screen code blocks skip per-line work.
- `+` accepts Shift (Shift+= on most layouts); plain bindings ignore modifier
  chords.
- Per-UID `0700` cache fallback when neither `XDG_CACHE_HOME` nor `HOME` is set.

### Fixed

- Drag-to-select works from gaps and margins, and releasing the drag copies the
  selection while keeping it (so `Esc` clears it instead of quitting).
- Only one highlight at a time: table hover and search fills yield to a live
  selection.
- `h`/`l` in the heading index no longer panic on a document without headings.
- `Ctrl+G` shows the file name first, then the full path.
- Mermaid fences after a plain line get their leading blank line, so diagrams
  no longer silently disappear; indented reference-like lines are kept, a UTF-8
  BOM is stripped, hard breaks at EOF no longer paint a backslash, quoted tables
  expand links like top-level cells, multi-backtick code spans close on an
  equal-length run.
- Chromium renders are killed after a 20 s wall clock; the sandbox is kept when
  the OS can provide one and `--no-sandbox` is only a fallback.

### Known limits

- Per-frame character maps are still recomputed; live search copies the block
  texts once per keystroke.
