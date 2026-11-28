# SQLite store — design

## Intent

Replace the TOML state file and the ad-hoc cache bookkeeping with one SQLite
database. Persisted app state and the cache index live in SQLite; cache payload
files stay on disk because they are executable/runtime artifacts.

## Goals

- Zoom, theme and per-document reading positions live only in SQLite.
- The rendered-diagram cache and managed asset versions are indexed in SQLite.
- Existing `state.toml` and cache files migrate once, transparently.
- Several `jr` instances can run at once; a locked or broken database never
  stops the viewer.
- No new system package: link SQLite through a bundled build.

## Non-goals

- Moving SVG, `mermaid.min.js`, font TTF or the Chromium binary into BLOBs.
  Chromium must exist as an executable file; the rest are runtime assets.
- Changing the CSV clipboard export for tables. It is not persisted data.
- Changing the layout of the cache directory or diagram cache keys.

## Approaches

- **A. `rusqlite` with the bundled SQLite (chosen).** One dependency, static
  SQLite, no `sqlite3.h` requirement. The host has no development headers, so
  the bundled build is the only portable choice. Cost: one C build, larger
  dependency tree.
- **B. `rusqlite` against the system library.** Rejected: `/usr/include/sqlite3.h`
  is absent on this machine.
- **C. Shelling out to the `sqlite3` CLI.** Rejected: fragile quoting and
  parsing, no typed transactions, and the CLI is not guaranteed at runtime.

The cache index is **advisory**: files remain the source of truth and the table
mirrors them for pruning/listing. If the database is lost, the next startup
rescans the cache and rebuilds the index; no cached render is invalidated.

## Design

### Location and schema

Database: `$XDG_DATA_HOME/jiaoren/jiaoren.db`, else
`~/.local/share/jiaoren/jiaoren.db`; without either variable, state is not
persisted (the same rule the old TOML store used). The cache root stays
`$XDG_CACHE_HOME/jiaoren` (or its existing fallback).

Schema version 1, applied on open:

```sql
CREATE TABLE prefs (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    zoom REAL NOT NULL,
    theme TEXT NOT NULL
);
CREATE TABLE positions (
    path TEXT PRIMARY KEY,
    scroll REAL NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE cache_artifacts (
    kind TEXT NOT NULL,
    version TEXT NOT NULL,
    theme TEXT NOT NULL,
    path TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (path)
);
CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

`meta` holds `schema_version`. `positions` keeps the existing 32-newest rule;
`path` is the canonical document path.

### Migration

On first open, if no `prefs` row exists:

1. Parse the legacy `state.toml` if present, insert zoom, theme and positions.
2. Scan the cache root once and insert artifact rows for rendered SVGs,
   `mermaid-*.min.js`, font versions and Chromium versions.
3. Remove the legacy TOML only after the insert commits.

If migration fails, the database is discarded/left empty and defaults apply;
the TOML is kept. An existing database never reads the TOML again.

### Runtime

- `prefs::load`/`save` become store calls. Saving writes `prefs` and `positions`
  in one transaction. There are no TOML writes.
- Startup pruning deletes stale artifact rows and their files together.
  `--clean-cache` clears the relevant rows; `--all` clears the managed rows too.
- Cache writes register the artifact after the file rename succeeds.
- Open with WAL, `busy_timeout(2000)` and foreign keys off. State failures are
  ignored; the cache index is best-effort. File existence still decides whether
  a cached diagram is used, so a missing database changes no render result.
- If the artifact table is empty while the cache root has files, rebuild it by
  scanning; this keeps a deleted database from orphaning files.

### Tests

- In-memory schema creation and `user_version` check.
- Prefs round-trip, theme, zoom clamping, position upsert, 32-entry cap.
- Legacy TOML migration, including removal after success and retention after
  failure.
- Artifact upsert, prune and cache-root rescan.
- Existing prefs tests continue to pass through the new API.

## Left to the builder

- Exact module split (`store.rs` vs extending `prefs.rs`).
- Whether the connection is held for the process or reopened per save.
- Precise SQLite pragmas and retry timing beyond WAL plus 2 s busy timeout.
