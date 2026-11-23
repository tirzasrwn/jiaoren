# Selection copy, file path and key help

## Why

Three gaps came out of normal reading and copying. Ctrl+C never copied: the
cancel ladder only cleared the selection, so a long drag down the page followed
by the usual copy key threw the selection away — the one gesture every other
application copies with. A code block also painted a hover tint on the row under
the pointer while a selection was being dragged, so two different highlights were
on screen at once and it was unclear what would land on the clipboard. And there
was no way to see which file was open or to read the key bindings from inside
the viewer.

## What changed

Ctrl+C and `y` now share one copy path. The cancel ladder is prompt, then copy
the selection, then clear the search, then quit, so Ctrl+C behaves like a text
editor: it copies whenever something is selected, even while the drag button is
still down. The copy clears the highlight, which keeps the next Ctrl+C moving
down the ladder instead of re-copying the same text forever.

A code block suppresses its hover row while a selection is being dragged or is
still active. The hover tint stays for plain reading, which is what it is for.

The selection tint is one chip per visual line box, from the first selected glyph
to the last, so the spaces between words and the stretched gaps of a justified
line are inside it. Painting one chip per word instead was a regression: on a
justified paragraph the leftmost word chip of every line lines up, so a selection
reads as a stripe of word boxes down the left edge with the spaces left blank.
Nothing paints in the margins or in the vertical gaps between blocks: a chip is
one row tall and starts at the first glyph it covers.

Table cells stay separate blocks, so a selection across a table is one chip per
cell and inherits the cell padding. Library-routed blocks (blockquotes, raw HTML,
inline images) are not searchable, so their markers and bars are never part of a
selection.

Scroll sources during a selection drag: holding the pointer in the 24 px top or
bottom strip of the window auto-scrolls (pixels per frame ramp up over the strip
and clamp at its edge), and the wheel scrolls too. The scroll area itself ignores
the wheel while a widget owns the drag, so the app applies the wheel delta to its
own offset in that state and consumes the event. A drag that runs past the window
keeps growing the selection from the remembered pointer position.

Ctrl+G puts the open document's absolute path in the bottom-left status chip for
four seconds — longer than the clipboard messages, since a path takes longer to
read. The app canonicalises the path it was given, so a file named relatively on
the command line still shows (and reloads) the real location.

`?` toggles a centred overlay holding the key list, and the new `jr --keys` flag
prints exactly the same text to stdout. Both read one shared constant, so the
flag and the overlay cannot drift. While the overlay is up it owns the keyboard:
Esc, `q`, `?`, Ctrl+C or a click closes it, and every other key is dropped, so
nothing scrolls or quits from behind a help screen.

A selection drag held in the top or bottom strip of the window scrolls the
document, so a selection is not limited to what fits on screen when the drag
starts. Pixels per frame ramp up over the strip and clamp at its edge; the
pointer position is remembered for a frame, so a drag that runs past the window
keeps growing the selection.

## What is left to the builder

The overlay is plain monospace text with no wrapping; a window narrower than the
longest line clips it. Two columns, or wrapping to the window width, is the
upgrade if that turns out to matter. The key list has no per-key descriptions
beyond the one line each; richer help belongs here only if the table in the
README stops being enough.
