# SQLite Store Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move persisted app state and cache metadata from TOML/files into one SQLite database, with a one-time legacy import.

**Architecture:** `rusqlite` with bundled SQLite owns `prefs`, `positions`, `cache_artifacts` and `meta`. `prefs` delegates to the store; cache modules register/prune artifact rows while files remain the source of truth for renders. A failed or missing database never blocks the viewer.

**Tech Stack:** Rust 2021, `rusqlite` 0.43 (bundled), SQLite WAL.

**Spec:** `docs/superpowers/specs/2026-11-28-sqlite-store-design.md`

## Global Constraints

- No new system package; SQLite is bundled and compiled by `cc`.
- The app must still open and render when the database is locked, corrupt or
  unwritable: state falls back to defaults, cache files still work.
- No TOML is written after this change. CSV remains a clipboard format only.
- Cache payload files stay files: SVG, `mermaid.min.js`, TTF, Chromium.
- Keep the current 32-document position cap and MRU order.
- Follow the local gate: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`.
- Do not add CI, bump the version, or edit README/CHANGELOG outside Task 4.

## Review Focus

- A locked or corrupt database must not stop `jr` from opening a document.
- A migration that fails halfway must leave `state.toml` in place and not lose saved positions.
- Two instances saving at once must not corrupt the DB or block forever.
- Deleting the DB must not invalidate cached diagrams; the index rebuilds from files.
- `--clean-cache` must never delete the state database, and `--all` must clear managed rows.

---

### Task 1: Store foundation

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`
- Create: `src/store.rs`
- Modify: `src/main.rs` (module declaration)

**Interfaces:**
- Produces: `store::State { zoom: f32, theme: Option<String>, positions: Vec<(PathBuf, f32)> }`
- Produces: `store::CacheKind { Svg, MermaidJs, Font, Chromium }` with `as_str`
- Produces: `store::load_state() -> Option<State>`
- Produces: `store::save_state(&State) -> bool`
- Produces: `store::record_artifact(kind: CacheKind, version: &str, theme: &str, path: &Path)`
- Produces: `store::forget_artifact(path: &Path)`
- Produces: `store::clear_artifacts(kinds: &[CacheKind])`
- Produces: `store::rebuild_cache_index(cache_root: &Path)`
- Produces: `store::db_path() -> Option<PathBuf>` (private)

- [ ] **Step 1: Add the dependency**

```toml
rusqlite = { version = "0.43", default-features = false, features = ["bundled"] }
```

Run `cargo check --locked` (unlock first with `cargo check --offline` if the
lock needs the new package), confirm the bundled build links with `cc`.

- [ ] **Step 2: Write failing store tests**

Create `src/store.rs` with tests that build a temp DB via a private
`open_at(path: &Path) -> rusqlite::Result<Connection>`:

```rust
#[test]
fn schema_is_version_one() {
    let (_dir, conn) = temp_db();
    assert_eq!(user_version(&conn), 1);
    assert!(table_exists(&conn, "prefs"));
    assert!(table_exists(&conn, "positions"));
    assert!(table_exists(&conn, "cache_artifacts"));
}

#[test]
fn state_round_trips_and_keeps_mru_order() {
    let state = State { zoom: 1.25, theme: Some("dark".into()),
        positions: vec![(PathBuf::from("/a.md"), 10.0), (PathBuf::from("/b.md"), 20.0)] };
    save_state_at(&conn, &state).unwrap();
    assert_eq!(load_state_at(&conn).unwrap(), state);
}

#[test]
fn positions_are_capped_at_32_on_save() { /* 40 entries -> 32 stored */ }

#[test]
fn corrupt_database_degrades_to_none() { /* write garbage, load_state_at returns Err */ }

#[test]
fn artifacts_upsert_forget_and_clear() { /* record twice = one row; forget removes; clear by kind */ }

#[test]
fn index_rebuild_scans_svg_js_font_and_chromium() { /* temp cache tree -> 4 rows */ }
```

- [ ] **Step 3: Run the tests and verify they fail on missing functions**

Run: `cargo test store:: -- --nocapture`
Expected: compile errors for the new API.

- [ ] **Step 4: Implement the schema and connection**

- DB path: `$XDG_DATA_HOME/jiaoren/jiaoren.db`, else
  `$HOME/.local/share/jiaoren/jiaoren.db`; without either, `None`.
- `open_at`: create parent, open, `PRAGMA journal_mode=WAL`,
  `PRAGMA busy_timeout=2000`, `PRAGMA user_version`, create tables from the
  spec, set `user_version=1`.
- Every public function opens its own connection; any error returns
  `None`/`false`/no-op. No `unwrap` on I/O.

- [ ] **Step 5: Implement state and artifact operations**

- Save state in one transaction: upsert `prefs`, delete `positions`, insert the
  first 32 with descending `updated_at`.
- `save_state` commits before returning `true`.
- Artifacts: upsert by `path`, store `bytes` from metadata and `created_at` as
  Unix seconds. `clear_artifacts` deletes the listed kinds.
- `rebuild_cache_index` scans only top-level `*.svg`, `mermaid-*.min.js`,
  `fonts/<version>` and `chromium/<version>` under the cache root; upserts rows
  and leaves unrelated files alone.

- [ ] **Step 6: Run tests and grep for old format writers**

Run: `cargo test store::` → PASS.
Run: `grep -rn "state.toml\|fs::write" src/prefs.rs` → only legacy read paths and tests.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/store.rs src/main.rs
git commit -m "feat: add a sqlite store for state and cache metadata"
```

---

### Task 2: Move prefs and app state onto the store (parallel after Task 1)

**Files:**
- Modify: `src/prefs.rs`
- Test: existing `#[cfg(test)] mod tests` in `src/prefs.rs`
- Modify if needed: `src/app/mod.rs` call sites only

**Interfaces:**
- Consumes: Task 1 `store::{load_state, save_state, State}`.
- Produces: unchanged `prefs::{load, save, Prefs::resume_scroll}` signatures.

- [ ] **Step 1: Write failing migration tests**

```rust
#[test]
fn legacy_toml_migrates_once_and_is_removed() { /* parse -> State -> tempdb -> file gone */ }

#[test]
fn failed_migration_keeps_the_toml() { /* unwritable/failed save -> file stays */ }

#[test]
fn state_conversion_preserves_theme_zoom_and_positions() { /* Prefs <-> store::State */ }
```

- [ ] **Step 2: Run them and verify they fail**

Run: `cargo test prefs:: -- --nocapture` → FAIL on missing conversion/migration.

- [ ] **Step 3: Implement**

- `load`: `store::load_state()` → `Prefs`; when absent, read the legacy
  `state.toml` with the existing parser, `store::save_state`, remove the TOML
  only after `save_state` returns `true`.
- `save`: build `State`, call `store::save_state`; no file write, no TOML.
- Keep `state_path()` only for legacy reads (name it `legacy_path`).
- Convert `theme` to/from `Theme` with the existing strings.

- [ ] **Step 4: Run the gate**

Run: `cargo test prefs::`, `cargo clippy --all-targets --locked -- -D warnings`.
Expected: PASS, no dead code for the old render path.

- [ ] **Step 5: Commit**

```bash
git add src/prefs.rs src/app/mod.rs
git commit -m "feat: store prefs and positions in sqlite"
```

---

### Task 3: Index cache artifacts (parallel with Task 2)

**Files:**
- Modify: `src/mermaid.rs`, `src/fonts.rs`, `src/download.rs`, `src/app/raster.rs`, `src/main.rs`
- Test: unit tests in the touched files

**Interfaces:**
- Consumes: Task 1 artifact functions.

- [ ] **Step 1: Write failing tests**

Add tests that a temp cache tree yields expected artifact rows after the
migration scan, and that pruning an old version removes its row while keeping
the current one. Use the store's `open_at` test helper only from `store` tests;
cache tests assert file behavior and call the same prune/clean functions, with
no global DB assertions.

- [ ] **Step 2: Run them and verify they fail**

Run: `cargo test mermaid::tests::prune`, `cargo test fonts::tests::prune`.
Expected: FAIL where row behavior is asserted.

- [ ] **Step 3: Implement registration**

- SVG: after `render` successfully writes the SVG, record `Svg` with
  `RENDER_VERSION`, theme, path.
- `mermaid.min.js`: after rename, record `MermaidJs` with `MERMAID_VERSION`.
- Fonts: after each face rename, record `Font` with `FONT_VERSION`.
- Chromium: after install rename, record `Chromium` with `MANAGED_VERSION`.
- Prune/clean paths: call `forget_artifact` for each removed path; `clean_cache`
  with `all=false` clears Svg + MermaidJs rows, `all=true` also Font + Chromium.
- At startup, call `store::rebuild_cache_index(&mermaid::cache_root())` once
  (in `App::new` or `main`), so a deleted DB is repopulated from files.

- [ ] **Step 4: Confirm cache behavior is unchanged**

Run: `cargo test`, including the ignored Chromium tests only if a render is
already cached. Confirm a second launch still reuses an existing SVG.

- [ ] **Step 5: Commit**

```bash
git add src/mermaid.rs src/fonts.rs src/download.rs src/app/raster.rs src/main.rs
git commit -m "feat: index cache artifacts in sqlite"
```

---

### Task 4: Docs and final gate (parallel with Task 3)

**Files:**
- Modify: `README.md`, `CHANGELOG.md`

- [ ] **Step 1: Update README**

- Cache section: one SQLite DB at the data path holds zoom/theme/positions and
  the cache index; payload files stay in `~/.cache/jiaoren`.
- State section: remove the TOML path and the “saved to state.toml” wording.
- `--clean-cache`: state DB is not a cache and is never removed.
- Migration: existing `state.toml` is imported once and then deleted.

- [ ] **Step 2: Update CHANGELOG**

Add an Unreleased/next-version entry: SQLite store, one-time TOML migration,
cache index; no TOML writes.

- [ ] **Step 3: Run the whole gate**

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 4: Manual SQLite smoke**

```bash
XDG_DATA_HOME=/tmp/jr-data XDG_CACHE_HOME=/tmp/jr-cache \
  timeout 8 xvfb-run -a jr examples/demo.md
sqlite3 /tmp/jr-data/jiaoren/jiaoren.db '.tables'
sqlite3 /tmp/jr-data/jiaoren/jiaoren.db 'select * from prefs; select count(*) from cache_artifacts;'
```

Expected: window opens, prefs row exists, artifact rows > 0, no `state.toml`.

- [ ] **Step 5: Commit**

```bash
git add README.md CHANGELOG.md
git commit -m "docs: describe the sqlite store"
```
