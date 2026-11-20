# Final fix wave — report

Branch: `master`. Base: `6f87c05`. Date: 2026-11-20.

## 1. Theme applied to window chrome — DONE

- Added `apply_theme(ctx, theme)` in `src/app.rs:54`: sets `ctx.set_theme(egui::Theme::Light|Dark)` and the matching `Visuals::light()/dark()`.
- `App::new` calls it at startup from `--dark` (`src/app.rs:405`); `App::ui` re-applies whenever `self.theme` differs from `self.applied_theme` (`src/app.rs:569`), so `t` updates chrome and mermaid together.
- `App` stores `applied_theme: Option<Theme>` (`src/app.rs:403`) to avoid setting visuals every frame.

Covered by `app::theme_tests::applying_theme_updates_active_visuals` (`src/app.rs:845`): asserts `ctx.theme()` and `visuals.dark_mode` for both themes.

## 2. First-run mermaid.min.js race — DONE

- `Renderer::ensure_mermaid_js` is now `pub(crate)` (`src/mermaid.rs:145`) and is called once on the UI thread in `App::new` before any worker can spawn (`src/app.rs:424`). Workers still call it, but it is a no-op when the file exists. Atomic tmp+rename kept.
- Added `ensure_mermaid_js_is_idempotent` (`src/mermaid.rs:361`): writes a pre-existing JS file, calls ensure twice, asserts identical mtime and bytes (no rewrite/download).

## 3. `reload` clears the pending queue — DONE

- Added `DiagramUi::reset_for_reload` (`src/app.rs:172`): rebuilds `sources`, clears textures, inflight, failures, placeholders, and `queue.lock().clear()`. Running workers keep their `active_workers` slots until `poll` sees their results.
- `App::reload` calls it (`src/app.rs:517`).
- Added `reload_drops_queued_jobs` (`src/app.rs:789`): enqueues a job, resets for reload, asserts queue/inflight/sources are empty.

## 4. Scroll re-anchoring on placeholder replacement — DONE

- Added pure `reanchor(scroll_y, viewport_top, placeholder_top, delta)` (`src/app.rs:66`): adds `delta` only when the placeholder top was above the viewport top, clamped at 0.
- `DiagramUi` records each placeholder's screen top, height, and viewport top when drawn (`Placeholder`, `src/app.rs:305`). When a PNG replaces it, `draw_png` computes `delta = response height - placeholder height`, accumulates `scroll_adjust`, and requests a repaint (`src/app.rs:284`).
- `App::ui` applies the accumulated adjustment to `scroll_y` after layout (`src/app.rs:661`).
- Tests: `app::scroll_tests::*` (`src/app.rs:818`–`834`) cover above viewport, below viewport, partially visible, zero delta, and negative delta clamping.

## 5. Chromium trust boundary — DONE

- Mermaid `securityLevel` changed to `"strict"` (`src/mermaid.rs:278`).
- Chromium now runs with `--host-resolver-rules=MAP * ~NOTFOUND` (`src/mermaid.rs:87`), blocking network egress from the render page.
- Bumped `RENDER_VERSION` to `2-mermaid11.12.0-htmlLabelsOff-2x-strict` (`src/mermaid.rs:5`) so old cached renders are not reused for the new config.
- Strict rendering verified with both the existing ignored test and a new two-diagram ignored test (flowchart light + sequence dark): `renders_real_diagram` (`src/mermaid.rs:379`) and `renders_two_real_diagrams` (`src/mermaid.rs:388`).
- Added "Trust boundary" section to `README.md:74`.

## 6. Wide tables / long code lines — DONE (documented)

- Added "Known limitations" to `README.md:81`: wide tables and long code lines wrap; use `+` to zoom. No renderer restructuring.

## Minor items — DONE

- `src/doc.rs`: balanced-parenthesis link scanner `link_inner` (`src/doc.rs:86`); optional `"title"` stripped via first whitespace token (`src/doc.rs:120`); rewritten image URI uses `mermaid::file_url` (`src/doc.rs:133`) so spaces/`#` are percent-encoded; removed dead `target.contains('\n')`.
- `src/doc.rs`: mermaid fence matches first whitespace-delimited info token (`src/doc.rs:39`); test `mermaid_fence_matches_first_info_token` (`src/doc.rs:181`).
- `src/mermaid.rs`: playwright dirs sort by numeric suffix, descending (`src/mermaid.rs:198`).
- `src/mermaid.rs`: `find_in_path` requires a regular file with an executable bit (`is_executable`, `src/mermaid.rs:216`).
- `src/app.rs`: `watch_tick` (`src/app.rs:522`) no longer schedules a 100 ms repaint while idle. Watcher callback wakes the UI only for non-access events (`src/app.rs:459`); the 500 ms timer remains only for the no-watcher mtime fallback; debounce/pending reload still schedules its 100 ms wake.
- `README.md`: `a` = "reset zoom to 100%" (`README.md:41`); hardcoded `chromium-1255` removed in favour of newest `chromium-*` under `~/.cache/ms-playwright/` (`README.md:52`); mermaid JS documented as downloaded on first run, not bundled (`README.md:65`).

## Commands and output

```
$ cargo build
EXIT:0
    Finished `dev` profile [unoptimized + debuginfo] target(s)
```
Warning-free.

```
$ cargo test
test result: ok. 36 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out
```

```
$ cargo test renders_real_diagram -- --ignored
cargo test: 1 passed, 37 filtered out (1 suite, 1.83s)
```

```
$ cargo test renders_two_real_diagrams -- --ignored
cargo test: 1 passed, 37 filtered out (1 suite, 2.01s)
```

Smoke run:

```
$ timeout 15 cargo run -- /tmp/mdview-smoke.md
EXIT:124
    Finished `dev` profile [unoptimized + debuginfo] target(s)
     Running `target/debug/mdview /tmp/mdview-smoke.md`
```

Exit 124 is `timeout` stopping the still-open window; there was no panic or error output. New cache files produced for the two diagrams:

```
~/.cache/mdview/2d76bea6f444da6f-light-2-mermaid11.12.0-htmlLabelsOff-2x-strict.png  10.0K
~/.cache/mdview/82674a7fb71805ec-light-2-mermaid11.12.0-htmlLabelsOff-2x-strict.png  2.0K
```

No `.err` files were written.
