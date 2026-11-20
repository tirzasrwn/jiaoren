# mdview: Rust best-practices refactor

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Bring the codebase to Apollo's Rust best practices: a green `cargo clippy --all-targets --all-features --locked -- -D warnings` gate, typed errors instead of `Result<_, String>`, no production `unwrap`, app.rs split by responsibility, and documentation/tests organised per unit — with zero behaviour change.

**Architecture:** Add `thiserror` for module-level error enums (`doc::DocError`, `mermaid::RenderError`) and `anyhow` at the binary boundary (`main`). Replace lock `unwrap`s with a poison-tolerant helper and remove the duplicate file read. Split `src/app.rs` (1435 lines) into `src/app/{mod,diagram,table,keys,placeholder}.rs` and move pure parsing/geometry helpers next to their users. Enable lints in `Cargo.toml` so the gate is enforced by the compiler, not by habit.

**Tech Stack:** Rust 1.100, `thiserror` 2, `anyhow` 1 — the only two new dependencies, both endorsed by the handbook (ch. 4).

**Spec:** `docs/superpowers/specs/2026-11-20-mdview-design.md` (behaviour is frozen there; this plan changes structure only).

## Global Constraints

- **No behaviour change.** All 108 existing tests keep passing; every visual/CLI behaviour stays identical. Any test whose assertions must change because an error type changed keeps the same expected text via `to_string()`.
- `cargo clippy --all-targets --all-features --locked -- -D warnings` must exit 0 at the end of every task.
- `cargo test` (plain) green at the end of every task; `cargo build` warning-free.
- New deps exactly: `thiserror = "2"`, `anyhow = "1"`. No others.
- Cache formats, `RENDER_VERSION`, `.err` file contents (error `Display` text), CLI exit codes (2 for usage/read errors), and window behaviour are unchanged.
- `#[expect(...)]` with a reason comment instead of `#[allow(...)]` where a lint suppression is genuinely needed; no blanket module/file allows except `#![allow(dead_code)]`-free code (remove stale ones).
- Module docs (`//!`) on every module file; `///` on every `pub` item.
- Public API stays crate-internal: use `pub(crate)` unless a module is genuinely a boundary (`main` items stay as-is).

## Review Focus

- Every one of the 108 tests still exists and asserts the same thing; no test deleted to make the gate pass.
- Error text shown to users (CLI, banner, inline diagram error, `.err` cache) is byte-identical after the typed-error change.
- `app.rs` split: no duplicated state, no new `Rc<RefCell>` layers, the hook still handles `<mdpara>`, `<mdtable>`, `<mermaid>` in that order.
- Poisoned-mutex handling cannot deadlock or silently drop queued renders.
- The clippy gate is real: `Cargo.toml` lints are set, and the command in the README/constraints actually fails on a reintroduced warning (verify by a scratch warning, then revert).

---

### Task 1: Lint gate, mechanical clippy fixes, module docs

**Files:** `Cargo.toml`, all `src/*.rs` (docs only + small lint fixes).

**Interfaces:** none change.

- [ ] **Step 1:** Add to `Cargo.toml`:

```toml
[lints.clippy]
all = { level = "deny", priority = 10 }
perf = { level = "deny", priority = 9 }
```

and run `cargo clippy --all-targets --all-features --locked -- -D warnings` to capture the exact failing list (7 today).

- [ ] **Step 2:** Fix each finding without changing behaviour: collapse the `if` into its `match`, replace redundant closures with method references, use `is_multiple_of`, use `strip_prefix`, introduce `type` aliases for the two complex types, and fix the test-only `redundant_pattern_matching` with `assert!(…is_none())`.
- [ ] **Step 3:** Add a `//!` module doc to every file (`main`, `app`, `doc`, `fonts`, `justify`, `mermaid`, `theme`) stating its responsibility in one or two sentences.
- [ ] **Step 4:** `cargo clippy …-- -D warnings` green, `cargo test` green, `cargo build` clean; commit.

---

### Task 2: Typed errors, poison-tolerant locks, single file read

**Files:** `Cargo.toml`, `src/doc.rs`, `src/mermaid.rs`, `src/app.rs`, `src/main.rs`.

**Interfaces:**
- `pub enum DocError { Io { path: PathBuf, source: std::io::Error }, NotUtf8 { path: PathBuf } }` (`thiserror`), `impl DocError { pub fn path(&self) -> &Path }` not required.
- `pub fn doc::read_doc(path: &Path) -> Result<String, DocError>` (moved from `app.rs`).
- `pub enum RenderError { ChromiumMissing, MermaidJs(String), Spawn(String), HtmlWrite(String), NoSvg, Raster(String) }` (`thiserror`); `Renderer::render` returns `Result<PathBuf, RenderError>`; `Renderer::cached` keeps `Option<Result<PathBuf, String>>` (it reads the `.err` file text).
- `fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T>` in `app` — `unwrap_or_else(std::sync::PoisonError::into_inner)`.
- `main` returns `anyhow::Result<()>`, `parse_args` keeps `Result<Options, String>`.

- [ ] **Step 1:** Add deps; write failing tests: `read_doc` error text for missing file and non-UTF-8 (assert `to_string()` equals today's messages), and `RenderError::NoSvg.to_string()`.
- [ ] **Step 2:** Implement the enums with `#[error("…")]` messages that reproduce today's strings exactly (`cannot read <path>: <err>`, `<path> is not valid UTF-8`).
- [ ] **Step 3:** Thread the types through `mermaid::render`, `app` (worker result map stores `String` via `to_string()`), `.err` writes (`err.to_string()`), and the banner/error box.
- [ ] **Step 4:** Replace every production `.lock().unwrap()` with `lock(&…)`; replace `occurrences.get_mut(id).unwrap()` with the `entry` API; remove the double `read_doc` by reading once in `main` and passing the text into `App::new`.
- [ ] **Step 5:** Gate + tests green; verify user-visible error strings by running `mdview /nope.md`, `mdview .`, and a non-UTF-8 file; report outputs. Commit.

---

### Task 3: Split `app.rs` by responsibility

**Files:** create `src/app/mod.rs`, `src/app/diagram.rs`, `src/app/table.rs`, `src/app/keys.rs`, `src/app/placeholder.rs`, `src/app/geometry.rs`; delete `src/app.rs`; adjust `main.rs`.

**Interfaces:** crate-visible names keep their current paths as far as possible (`app::App`, `app::keys::apply_key`, …). Re-exports in `app/mod.rs` where `main.rs` or tests use them.

- [ ] **Step 1:** Move `Action`, `KeyState`, `apply_key`, `GG_WINDOW` and their tests to `src/app/keys.rs`; `App::new/update` wiring imports from there. Run tests.
- [ ] **Step 2:** Move `DiagramUi`, `Raster`, `RasterKey`, raster column/worker functions and their tests to `src/app/diagram.rs`. Run tests.
- [ ] **Step 3:** Move `TableUi`, `render_cell`, `cell_needs_library_fallback` and their tests to `src/app/table.rs`. Run tests.
- [ ] **Step 4:** Move `parse_placeholder`, `parse_table_placeholder`, `parse_paragraph_placeholder`, `html_escape`-adjacent helpers into `src/app/placeholder.rs` and unify the three into one `Placeholder<'a> { kind, id }` parser used by the hook (same accepted inputs, same rejections).
- [ ] **Step 5:** Move `content_column`, `bottom_pad`, `column_widths`, `reanchor`, `bucket_for`, `bucket_scale`, `raster_key` into `src/app/geometry.rs` with their tests.
- [ ] **Step 6:** `app/mod.rs` keeps `App`, the eframe impl, theme application, watcher, scroll/layout, the html hook, and `read_doc` re-export; no file over ~500 lines.
- [ ] **Step 7:** Gate + tests green; run the app once and report the screenshot/no-panic evidence. Commit.

---

### Task 4: Test organisation and public-item docs

**Files:** all `src/**/*.rs` test modules and `///` docs.

- [ ] **Step 1:** Group each file's tests under `#[cfg(test)] mod tests { mod <unit> { … } }` per tested unit (`doc::tables`, `doc::paragraphs`, `doc::fences`, `justify::parse`, `justify::wrap`, `justify::paint`, `app::keys`, `app::geometry`, `app::table`, `theme`, `fonts`, `mermaid`), without renaming tests en masse.
- [ ] **Step 2:** Rename only the generic names (`parses_width_and_height`, `cache_paths_include_theme_and_render_version` stay; `test_*`, `handles_*` become `<unit>_<behaviour>`), keeping each test's assertions untouched.
- [ ] **Step 3:** Add `///` docs to every `pub` item that lacks one (`Prepared`, `TableSpec`, `Run`, `RunKind`, `Palette`, `Rgb`, `Renderer`, `Theme`, `find_chromium`, `parse_args`, `Options`, `App::new`, `DocError`, `RenderError`).
- [ ] **Step 4:** Add `#[ignore = "requires headless chromium"]` to the two ignored tests (currently bare `#[ignore]`).
- [ ] **Step 5:** Final gate: clippy `-D warnings`, `cargo test`, `cargo build`, `cargo install --path . --locked`, run the app; commit.
