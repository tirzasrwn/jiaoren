# Copy link on click — design

Clicking a link in jiaoren does nothing today: `eframe` is built with
`default-features = false` and no `links` feature, so `ctx.open_url` only logs
"feature links not enabled". Rather than adding the `webbrowser` dependency,
a click should copy the destination to the clipboard and confirm it in the
existing bottom-left status chip.

## Scope

- Left click on a `http://`, `https://` or `mailto:` link → `ctx.copy_text(url)`
  and a 1.5 s status line `link copied`.
- Works wherever links are painted today (paragraphs, list items, table cells,
  quotes) because they all go through `justify::paint_frag`.
- `#anchor` links keep jumping to the heading.
- `javascript:`, `data:`, `file:` and relative paths stay inert: no copy and
  no chip. Their cursor is the same pointing hand as before this feature; the
  hover tooltip still shows the raw destination.

## How

`paint_frag` already has `ui` and the url. On a click of a safe link it calls
`ui.ctx().copy_text(url)` and inserts a temp flag under
`egui::Id::new("jiaoren-link-copied")` — the same one-frame handoff the
`jiaoren-anchor` link click already uses. `App::ui` removes that flag and calls
`set_status("link copied")`, so the renderer stays free of app state and no
function signatures change.

## Non-goals

- Opening a browser (the current `open_url` call is removed).
- Copy on middle click, or a "copy link" context menu.
- Per-link status text (the URL is already on hover; the chip stays one word).

## Verification

- Unit test: inserting the flag then draining it reports the copy once.
- `link_is_safe` test already covers the scheme allowlist; extend it with
  `mailto:`.
- Manual: `jr examples/demo.md`, click the inline and the `[reference link][docs]`
  (resolved to `https://example.com/docs`), paste into a terminal, and confirm
  the status chip appears.
