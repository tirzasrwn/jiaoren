# Font cache design

## Intent

Today `src/fonts.rs` only loads FiraCode Nerd Font from system directories
(`~/.fonts`, `$XDG_DATA_HOME/fonts`, `~/.local/share/fonts`,
`/usr/share/fonts/truetype`). A machine without the font silently falls back to
egui defaults. Put the font in jiaoren's own cache instead, like `mermaid.js`
and the managed Chromium, so a fresh install gets the intended typography.

## Goals

- First run downloads `FiraCodeNerdFont-Regular.ttf` and `-Bold.ttf` into
  `~/.cache/jiaoren/fonts/<version>/`, atomically.
- The window opens immediately; the download runs on a background thread and a
  status line shows `Downloading FiraCode font… NN% (first run only)`.
- On success the fonts are installed into the live egui context without a
  restart; on failure jiaoren keeps the system font if present, else egui's
  defaults, and shows a short warning.
- No new crate dependency.

## Non-goals

- Bundling the TTFs in the binary (repo/binary would grow ~5.3 MB).
- Downloading the Noto CJK/Arabic fallbacks; they stay system-only.
- Making Chromium/mermaid use the cached font (`FiraCode Nerd Font` must still
  be installed system-wide for diagram labels; not changed here).
- Removing the font cache from `--clean-cache` (it is an asset, not a derived
  cache). Stale font versions are pruned at startup.

## Approach chosen

Background download, cache first, system fallback, then egui defaults.

1. `fonts::ensure_cached(cache_dir, tx)` runs on a thread spawned by
   `App::new`.
   - Cache hit: send the two paths and finish.
   - Cache miss: download each face with
     `curl -fsSL --proto =https --proto-redir =https -o <tmp> <url>`.
     `curl`'s default progress meter is written to stderr; lines start with
     `NN` percent (`% Received` table). stderr is read incrementally, split on
     `\r`/`\n`, and a `u8` percent is sent on change. This is why no new HTTP
     crate is needed. (wget can do the same but is not a dependency; curl is
     already used for `mermaid.min.js`.)
   - A face is written to `<name>.tmp-<pid>` and renamed only after a size and
     TTF-magic check, so a killed download leaves no readable half file.
   - Regular is required; Bold is best-effort and may remain absent.
2. Pinned source: `FONT_VERSION` and two
   `raw.githubusercontent.com/ryanoasis/nerd-fonts/v<version>/patched-fonts/...`
   URLs. Bumping the version selects a new cache directory.
3. Loading order becomes: `JIAOREN_FONT_DIR` (explicit override, no download) →
   cached version dir → system dirs → egui defaults.
4. `App` keeps the progress receiver and a font status state. `update()` polls
   it, requests a repaint while active, draws the progress line through the
   existing bottom-left `status_area`, and on `Done` calls `ctx.set_fonts` with
   the bytes (not paths, so no file is kept open) then repaints.
5. README's Typography section is updated: the font is downloaded once, the
   cache path, the `JIAOREN_FONT_DIR` override, and that diagrams still need
   the system font.

## Parts decided

- Progress is shown in-app, not only on stderr: jr is a GUI and may be launched
  from a desktop entry with no terminal.
- `--clean-cache` keeps the font cache; only a version bump removes old faces.
- No retry loop: a failed first download falls back for this run and is retried
  on the next start.

## Testing

- Unit: curl meter parser accepts real meter lines and ignores speed/header
  fields; percent is monotonic; partial temp file never becomes the cache file.
- Unit: cache dir is preferred over system dirs; missing cache falls through.
- Manual: run with an empty `XDG_CACHE_HOME` on a machine with network and
  confirm the status line advances and the window reflows to FiraCode; run
  offline and confirm the warning + system/default fallback. This environment
  has no network egress, so the live download path is manual-only.
