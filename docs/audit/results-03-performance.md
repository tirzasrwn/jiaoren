# Audit 03 — Performance and Scale: Results

**Status:** completed. One critical scale defect (P3 code blocks), no crash or OOM kill.
**Target:** `jiaoren 0.1.0` (`~/.cargo/bin/jr`), repository commit `3489862`.
**Document:** `docs/audit/audit-03-performance.md` — 3.1 MB, 19,948 lines, 121 Mermaid blocks,
2,000-row table, 2 MB paragraph, 400 math expressions.

## Environment

| Item | Value |
|---|---|
| CPU / RAM | 8 logical cores, 15.7 GiB RAM (ThinkPad T14 Gen 2i) |
| Display | 1920x1080 @ 60 Hz, `DISPLAY=:0`, i3, workspace 9 |
| Window | floating, requested 1200x900 at 40,40; client geometry 1190x890 |
| Binary | `target/debug/jr` (debug build, 297.8 MB) |
| Mermaid | 11.12.0 (cache filenames), bundled Chromium/headless shell |
| Date | 2026-11-22 |

## Method

- **First paint:** launch with `setsid env DISPLAY=:0 ./target/debug/jr <file>`; poll
  `xdotool search --name jiaoren`, require `xwininfo` `Map State: IsViewable`, capture with
  `import -window`, and accept the first image whose ImageMagick grayscale
  `standard_deviation > 0.008` (non-blank). Timestamp is taken when that capture is read.
- **Full render:** after first paint, send `G`; poll the window image every 10–20 s until the
  final line `End of Part 3. If you can read this, the document fully rendered.` is visible and
  the image stops changing.
- **Page scroll:** `date +%s%N` immediately before `xdotool key Page_Down`, then poll the window
  until its image MD5 changes (key → visible new frame), 1 s polling granularity.
- **RSS:** `ps -o rss= -p <pid>` sampled every 0.2–1 s, plus process census
  (`pgrep -c -x jr`, `pgrep -c -f 'chromium|headless'`). Screenshots are in
  `docs/audit/evidence-03-performance/`. `/usr/bin/time -v` was not used for first paint because a
  `timeout` run would be killed before paint; polling plus RSS is the measurement above.

The app creates its real window only after reading and preparing the document and completing the
first frame, so “window mapped” and “first paint” coincide; there was no separate blank-window
stage to measure.

## Key metrics

| Metric | Result |
|---|---|
| First paint, full 3.1 MB document | **141.2 s** (runs: 138.6, 140.1, 141.2, 146.8, 149.8 s) |
| RSS at first paint | 2.6 GiB (2,750,524 KiB) |
| Final confirmation line visible | **~397 s after launch** (253 s after `G`; `G` sent ~3 s after first paint) |
| Time to stable bottom frame | +317 s after `G` (≈ 461 s after launch) |
| PageDown, 10 samples | 66.7, 124.4, 52.8, 6.3, 2.6, 2.7, 1.4, 53.9, 1.3, 16.9 s |
| PageDown median / worst / best | **11.6 s / 124.4 s / 1.3 s** (≈ 0.09 page/s median) |
| Peak RSS, observed | **7.3 GiB** (7,638,436 KiB; other full runs 5.4 GiB) |
| Processes | 1 × `jr`; 3 × Chromium/headless during the P4 diagram batch; 0 Chromium at idle bottom |
| Theme switch, full document | 96.8 s to first dark frame (input queued behind a frame) |
| Theme switch, P4 subset | 484 ms to dark background; diagram placeholder, dark diagram by 25 s |
| Crash / panic / OOM | None observed; `q` exited with status 0 after the queue drained |

First-paint screenshot `01-first-paint.png`/`05-full-launch.png` also shows a second defect: the
first frame paints only the original 800x600 surface. After the window is floated to 1200x900 the
right and bottom bands stay black until a later full frame (`03-floated-1200x900.png`,
`06-full-floated-top.png`).

## Release-binary cross-check (same document, same machine)

The metrics above were taken with `target/debug/jr` (debug build). Repeating the key measurements
with the installed release binary (`~/.cargo/bin/jr`) gives much smaller first-paint and `G`
latency, but the full document is still not usable interactively:

| Metric | Debug (`target/debug/jr`) | Release (`~/.cargo/bin/jr`) |
|---|---:|---:|
| First paint, full 3.1 MB | 141.2 s | **8.3 s** |
| `G` → first new frame | 253 s | **2.3 s** |
| `G` → frame stops changing | ~317 s after `G` | **11.9 s** |
| PageDown page 1 | 52–124 s | **9.5 s** |
| PageDown pages 2–5 | 1.3–53.9 s | **>48 s each** (no new frame within the 48 s poll window) |
| Peak RSS observed | 7.3 GiB | **7.6 GiB** |

Conclusion unchanged in kind: at section scale the release binary is fine, but a full 3.2 MB
stress document still has multi-second-to-minute input latency and ~7.6 GiB peak RSS. The debug
numbers overstate first paint by ~17x; quote the release row when judging ship-readiness.

## Per-section observations

The full-document time is dominated by P3. Isolated-section runs (same binary, same display)
give the attribution:

| Section | Isolated first paint | Peak RSS | Observation |
|---|---|---|---|
| P1 prose (400 sections) + P2 table (2000×8) | 5.2 s | 194 MiB | Prose and the full table render; table is complete at its bottom (`32-p2-table-bottom.png`) |
| P3 code (60 blocks × 150 lines) | **132–159 s** | **2.33–2.37 GiB** at first paint; 3.01 GiB after 40 s | Dominant cost; subsequent frames 25 s and 7.7 s. Code text is laid out/wrapped every frame (`27-p3-code.png`) |
| P4 Mermaid (120 diagrams) | 0.45–0.56 s | ~166 MiB | Placeholders paint immediately; first diagram visible by 5 s; 3 headless Chromium processes (`21-p4-batch-5s.png`) |
| P5 one large diagram (500 nodes, 700 edges) | 0.34–0.64 s | 130 MiB | Mermaid rejects it: `Edge limit exceeded. 500 edges found, but the limit is 500.` Error box; no raster, no crash (`10-p5-paint.png`) |
| P6 3000-item + 30-level lists | 1.0 s | 133 MiB | Renders (`28-p6-lists.png`) |
| P7 300 footnotes, 500 links, 300 images | 1.2 s | 133 MiB | Renders (`29-p7-footnotes-links-images.png`) |
| P8 2 MB single-line paragraph | 2.5 s | 422 MiB | Renders; largest isolated memory use after P3 (`30-p8-paragraph.png`) |
| P9 math (300 inline, 100 block) | 0.67 s | 130 MiB | Math is literal `$$…$$` by design (documented non-goal), not a performance failure (`31-p9-math-literal.png`) |

The full-document page sequence reached the P9 tail and the final line
(`19-g-130s-window.png`, `20-bottom-final-line.png`). P9 therefore is reachable and complete;
its literal display is expected.

## Scroll and input behaviour

- PageDown timing is bimodal: 52–124 s while the P3 blocks dominate layout, then 1.3–6.3 s once
  font/glyph caches are warm. One 53.9 s sample shows that a later page can still fall back into
  the slow path.
- Input is not dropped, it is **queued**. During the full-document run, `G` took 253 s to produce a
  new frame; in another run `G` had still not produced a frame after 100 s while RSS rose from
  2.6 GiB to 4.4 GiB. `q` likewise sometimes needed >15 s; it eventually exited 0.
- During the P4 batch alone the UI is usable: one `jr` plus 3 Chromium workers, RSS flat at
  ~166 MiB, and theme switch applied in 484 ms.
- On theme switch the old light diagram is **not reused**: the P4 diagram is replaced by a
  `rendering diagram…` placeholder (`23-p4-dark-early.png`) and re-rendered dark by 25 s
  (`25-p4-dark-25s.png`). New rasters are generated per theme; the disk cache stores both
  themes, but the in-memory view shows a placeholder during the switch.
- Layout is not incremental at document scale: the whole prepared Markdown is laid out in each
  frame, and the final line only appears after a complete frame, not as a viewport-local update.

## Crash, hang and known texture-limit risk

- No panic text, abort or OOM kill occurred. The process stayed single-threaded on the UI side
  and exited cleanly after `q`.
- The known failure mode (raster texture limit above 16384 px per side) was **not exercised**:
  P5 is rejected by Mermaid's `maxEdges` limit before a raster is produced. No other diagram in
  this document produced an oversized raster. The limit remains a code-level risk, not a
  finding from this file.
- The observable “hang” is latency, not deadlock: every queued key was eventually processed.

## Post-fix re-measure (commit `8ecd827`, release binary)

`wrap_line` now accumulates per-char advances instead of re-measuring every growing prefix, and the
raster side is capped. Re-runs on the same machine, `target/release/jr`:

| Metric | Before fixes (release) | After fixes (release) |
|---|---:|---:|
| P3 isolated (739 KB, 9,188 lines): first frame / stable | — | **1.1 s / 3.9 s**, 432 MB RSS |
| Full 3.2 MB document: first stable frame | 5.7 s | **5.7 s** |
| Full document: `G` first frame / settled | 2.3 s / 11.9 s | **0.62 s / 4.1 s** |
| Full document: theme toggle first frame / settled | — | **0.61 s / 4.8 s** (Mermaid dark rasters already cached) |
| Peak RSS observed | 7.6 GiB | 0.74 GiB on the full-document first frame |

With a cold Mermaid cache the first theme switch still has to render 121 diagrams for the new theme;
the placeholder and scrolling stay responsive while that runs. The old debug-build numbers above are
kept as the historical baseline; they overstate release behaviour by ~17x on first paint.

**Post-fix verdict:** the P3-dominated hang is gone. The full document opens in ~5.7 s and scroll/jump
respond in under a second (settling in ~4 s). The remaining slow action is a cold-cache theme switch,
which is bounded diagram work rather than a deadlock.

## Verdict

**Acceptable at section scale:** P1/P2/P4/P5/P6/P7/P8/P9 each render in ≤ 5.2 s in isolation;
Mermaid work is asynchronous, theme switching is under 0.5 s when the document is small, and
there are no crashes, panics or OOM kills.

**Critical scale defect:** the complete 3.2 MB document takes ~141 s to first paint, ~6.6 minutes
to reach and display the final line, peaks at 7.3 GiB RSS, and has median page latency of 11.6 s
(worst 124 s) with keyboard input queued for minutes. P3 (9,000 lines of fenced code) is the
cause; its per-frame code wrapping alone takes 132–159 s and uses ~2.3 GiB before the first paint.
At the suite's severity scale this is an app-level hang in practice even though no deadlock or
crash occurs.

Companion: P5 is a Mermaid configuration limit (`maxEdges`) rather than a renderer performance
defect; P9 math being literal is by design.

## Evidence index

| File | Shows |
|---|---|
| `01-first-paint.png`, `05-full-launch.png` | First non-blank full-document frame |
| `03-floated-1200x900.png`, `06-full-floated-top.png` | Only the old 800x600 surface painted after resize |
| `07-page-1.png` … `07-page-10.png` | Ten PageDown results |
| `10-p5-paint.png` | P5 Mermaid edge-limit error |
| `19-g-130s-window.png`, `20-bottom-final-line.png` | Final confirmation line at the bottom |
| `21-p4-batch-5s.png` | P4 diagrams during the batch |
| `23-p4-dark-early.png`, `25-p4-dark-25s.png` | Theme switch placeholder, then dark diagram |
| `26-p1-p2-top.png` … `31-p9-math-literal.png`, `32-p2-table-bottom.png` | Per-section isolated renders; P2 rows 1987–1999 |
