//! SQLite store for persisted view state and the cache index.
//!
//! Every public call is best effort: a missing, locked or broken database
//! yields defaults, `false` or a no-op, never a panic. Each call opens its own
//! connection so concurrent `jr` instances cannot corrupt shared state.
#![allow(dead_code)] // Wired into prefs and the cache modules by later tasks.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};

/// The whole persisted view state.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub zoom: f32,
    pub theme: Option<String>,
    /// Reading positions per document, most recent first.
    pub positions: Vec<(PathBuf, f32)>,
}

/// A kind of cache artifact mirrored in the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    Svg,
    MermaidJs,
    Font,
    Chromium,
}

impl CacheKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::MermaidJs => "mermaid",
            Self::Font => "font",
            Self::Chromium => "chromium",
        }
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS prefs (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    zoom REAL NOT NULL,
    theme TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS positions (
    path TEXT PRIMARY KEY,
    scroll REAL NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS cache_artifacts (
    kind TEXT NOT NULL,
    version TEXT NOT NULL,
    theme TEXT NOT NULL,
    path TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (path)
);
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', '1');
";

/// `$XDG_DATA_HOME/jiaoren/jiaoren.db`, else
/// `$HOME/.local/share/jiaoren/jiaoren.db`, else `None`.
fn db_path() -> Option<PathBuf> {
    let base = if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(xdg)
    } else {
        PathBuf::from(std::env::var_os("HOME")?)
            .join(".local")
            .join("share")
    };
    Some(base.join("jiaoren").join("jiaoren.db"))
}

/// Open (and create) the database, applying the schema on first use.
fn open_at(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| rusqlite::Error::InvalidPath(path.to_path_buf()))?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(Duration::from_millis(2000))?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 1 {
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", 1)?;
    }
    Ok(conn)
}

fn open() -> Option<Connection> {
    // Unit tests exercise `open_at` directly; public calls must never touch the
    // developer's real state while `cargo test` runs.
    #[cfg(test)]
    return None;
    #[cfg(not(test))]
    open_at(&db_path()?).ok()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn load_state_at(conn: &Connection) -> rusqlite::Result<Option<State>> {
    let prefs = conn
        .query_row("SELECT zoom, theme FROM prefs WHERE id = 1", [], |row| {
            Ok((row.get::<_, f64>(0)?, row.get::<_, String>(1)?))
        })
        .optional()?;
    let Some((zoom, theme)) = prefs else {
        return Ok(None);
    };
    let mut stmt = conn.prepare("SELECT path, scroll FROM positions ORDER BY updated_at DESC")?;
    let positions = stmt
        .query_map([], |row| {
            Ok((
                PathBuf::from(row.get::<_, String>(0)?),
                row.get::<_, f64>(1)? as f32,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(State {
        zoom: zoom as f32,
        theme: (!theme.is_empty()).then_some(theme),
        positions,
    }))
}

fn save_state_at(conn: &Connection, state: &State) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO prefs (id, zoom, theme) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET zoom = excluded.zoom, theme = excluded.theme",
        params![state.zoom as f64, state.theme.as_deref().unwrap_or("")],
    )?;
    tx.execute("DELETE FROM positions", [])?;
    let now = now_secs();
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO positions (path, scroll, updated_at) VALUES (?1, ?2, ?3)",
        )?;
        for (i, (path, scroll)) in state.positions.iter().take(32).enumerate() {
            stmt.execute(params![
                path.to_string_lossy(),
                *scroll as f64,
                now - i as i64
            ])?;
        }
    }
    tx.commit()
}

fn record_artifact_at(
    conn: &Connection,
    kind: CacheKind,
    version: &str,
    theme: &str,
    path: &Path,
) -> rusqlite::Result<()> {
    let bytes = std::fs::metadata(path).map_or(0, |m| m.len() as i64);
    conn.execute(
        "INSERT INTO cache_artifacts (kind, version, theme, path, bytes, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(path) DO UPDATE SET kind = excluded.kind, version = excluded.version,
             theme = excluded.theme, bytes = excluded.bytes",
        params![
            kind.as_str(),
            version,
            theme,
            path.to_string_lossy(),
            bytes,
            now_secs()
        ],
    )?;
    Ok(())
}

fn forget_artifact_at(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM cache_artifacts WHERE path = ?1",
        [path.to_string_lossy()],
    )?;
    Ok(())
}

fn clear_artifacts_at(conn: &Connection, kinds: &[CacheKind]) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    for kind in kinds {
        tx.execute(
            "DELETE FROM cache_artifacts WHERE kind = ?1",
            [kind.as_str()],
        )?;
    }
    tx.commit()
}

fn rebuild_cache_index_at(conn: &Connection, cache_root: &Path) -> rusqlite::Result<()> {
    let Ok(entries) = std::fs::read_dir(cache_root) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            let kind = match name.as_str() {
                "fonts" => CacheKind::Font,
                "chromium" => CacheKind::Chromium,
                _ => continue,
            };
            let Ok(versions) = std::fs::read_dir(&path) else {
                continue;
            };
            for version in versions.flatten() {
                let dir = version.path();
                if dir.is_dir() {
                    let version = version.file_name().to_string_lossy().into_owned();
                    record_artifact_at(conn, kind, &version, "", &dir)?;
                }
            }
        } else if name.ends_with(".svg") {
            record_artifact_at(conn, CacheKind::Svg, "", "", &path)?;
        } else if let Some(version) = name
            .strip_prefix("mermaid-")
            .and_then(|n| n.strip_suffix(".min.js"))
        {
            record_artifact_at(conn, CacheKind::MermaidJs, version, "", &path)?;
        }
    }
    Ok(())
}

/// The saved state, or `None` when the database is missing or unreadable.
pub fn load_state() -> Option<State> {
    load_state_at(&open()?).ok().flatten()
}

/// Persist the state in one transaction. `false` on any failure.
pub fn save_state(state: &State) -> bool {
    open().is_some_and(|conn| save_state_at(&conn, state).is_ok())
}

/// Add or refresh one cache artifact row. Best effort.
pub fn record_artifact(kind: CacheKind, version: &str, theme: &str, path: &Path) {
    if let Some(conn) = open() {
        let _ = record_artifact_at(&conn, kind, version, theme, path);
    }
}

/// Drop one artifact row by path. Best effort.
pub fn forget_artifact(path: &Path) {
    if let Some(conn) = open() {
        let _ = forget_artifact_at(&conn, path);
    }
}

/// Drop every artifact row of the listed kinds. Best effort.
pub fn clear_artifacts(kinds: &[CacheKind]) {
    if let Some(conn) = open() {
        let _ = clear_artifacts_at(&conn, kinds);
    }
}

/// Mirror the files under `cache_root` into the index. Best effort.
pub fn rebuild_cache_index(cache_root: &Path) {
    if let Some(conn) = open() {
        let _ = rebuild_cache_index_at(&conn, cache_root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir(tag: &str) -> TempDir {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "jiaoren-store-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn temp_db() -> (TempDir, Connection) {
        let dir = temp_dir("db");
        let conn = open_at(&dir.0.join("jiaoren.db")).unwrap();
        (dir, conn)
    }

    fn user_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap()
    }

    fn table_exists(conn: &Connection, table: &str) -> bool {
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            > 0
    }

    fn artifact_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM cache_artifacts", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn schema_is_version_one() {
        let (_dir, conn) = temp_db();
        assert_eq!(user_version(&conn), 1);
        assert!(table_exists(&conn, "prefs"));
        assert!(table_exists(&conn, "positions"));
        assert!(table_exists(&conn, "cache_artifacts"));
        assert!(table_exists(&conn, "meta"));
    }

    #[test]
    fn state_round_trips_and_keeps_mru_order() {
        let (_dir, conn) = temp_db();
        let state = State {
            zoom: 1.25,
            theme: Some("dark".into()),
            positions: vec![
                (PathBuf::from("/a.md"), 10.0),
                (PathBuf::from("/b.md"), 20.0),
            ],
        };
        save_state_at(&conn, &state).unwrap();
        assert_eq!(load_state_at(&conn).unwrap(), Some(state));
    }

    #[test]
    fn state_round_trips_without_a_theme() {
        let (_dir, conn) = temp_db();
        let state = State {
            zoom: 0.8,
            theme: None,
            positions: Vec::new(),
        };
        save_state_at(&conn, &state).unwrap();
        assert_eq!(load_state_at(&conn).unwrap(), Some(state));
    }

    #[test]
    fn positions_are_capped_at_32_on_save() {
        let (_dir, conn) = temp_db();
        let positions: Vec<(PathBuf, f32)> = (0..40)
            .map(|i| (PathBuf::from(format!("/p{i}.md")), i as f32))
            .collect();
        let state = State {
            zoom: 1.0,
            theme: Some("light".into()),
            positions,
        };
        save_state_at(&conn, &state).unwrap();
        let loaded = load_state_at(&conn).unwrap().unwrap();
        assert_eq!(loaded.positions.len(), 32);
        assert_eq!(loaded.positions[0], (PathBuf::from("/p0.md"), 0.0));
        assert_eq!(loaded.positions[31], (PathBuf::from("/p31.md"), 31.0));
    }

    #[test]
    fn corrupt_database_degrades_to_none() {
        let dir = temp_dir("corrupt");
        let path = dir.0.join("bad.db");
        std::fs::write(&path, b"this is not a sqlite database").unwrap();
        assert!(open_at(&path).is_err());
    }

    #[test]
    fn artifacts_upsert_forget_and_clear() {
        let (dir, conn) = temp_db();
        let svg = dir.0.join("diagram.svg");
        std::fs::write(&svg, b"<svg></svg>").unwrap();
        let js = dir.0.join("mermaid-12.3.0.min.js");
        std::fs::write(&js, b"js").unwrap();

        record_artifact_at(&conn, CacheKind::Svg, "v1", "dark", &svg).unwrap();
        record_artifact_at(&conn, CacheKind::Svg, "v2", "dark", &svg).unwrap();
        assert_eq!(artifact_count(&conn), 1);
        let (version, bytes): (String, i64) = conn
            .query_row(
                "SELECT version, bytes FROM cache_artifacts WHERE path = ?1",
                [svg.to_string_lossy().as_ref()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(version, "v2");
        assert_eq!(bytes, 11);

        record_artifact_at(&conn, CacheKind::MermaidJs, "12.3.0", "", &js).unwrap();
        assert_eq!(artifact_count(&conn), 2);

        forget_artifact_at(&conn, &svg).unwrap();
        assert_eq!(artifact_count(&conn), 1);

        clear_artifacts_at(&conn, &[CacheKind::MermaidJs]).unwrap();
        assert_eq!(artifact_count(&conn), 0);
    }

    #[test]
    fn index_rebuild_scans_svg_js_font_and_chromium() {
        let (dir, conn) = temp_db();
        let root = dir.0.join("cache");
        std::fs::create_dir_all(root.join("fonts/3.4.0")).unwrap();
        std::fs::create_dir_all(root.join("chromium/123")).unwrap();
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("rendered.svg"), b"svg").unwrap();
        std::fs::write(root.join("mermaid-12.3.0.min.js"), b"js").unwrap();
        std::fs::write(root.join("fonts/3.4.0/face.ttf"), b"font").unwrap();
        std::fs::write(root.join("chromium/123/chrome"), b"bin").unwrap();
        std::fs::write(root.join("notes.txt"), b"ignore").unwrap();
        std::fs::write(root.join("nested/nested.svg"), b"ignore").unwrap();

        rebuild_cache_index_at(&conn, &root).unwrap();

        assert_eq!(artifact_count(&conn), 4);
        let kinds: Vec<String> = conn
            .prepare("SELECT kind FROM cache_artifacts ORDER BY kind")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(kinds, ["chromium", "font", "mermaid", "svg"]);
    }
}
