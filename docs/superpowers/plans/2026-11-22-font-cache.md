# Font Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Download and cache FiraCode Nerd Font on first run, install it live with visible progress, and report its version in `jr --version`.

**Architecture:** `fonts.rs` gains a versioned cache under `~/.cache/jiaoren/fonts/<version>/`, a curl-based downloader that parses the stderr progress meter, and a definitions builder reused by cache, override and system paths. `App` spawns the download on a thread, polls it in `logic`, shows the percentage in the existing bottom-left status area, and calls `ctx.set_fonts` when done.

**Tech Stack:** Rust 2021, egui/eframe 0.36, std `mpsc` + `std::thread`, `curl` subprocess (already used for `mermaid.min.js`). No new crates.

**Spec:** `docs/superpowers/specs/2026-11-22-font-cache-design.md`

## Global Constraints

- No new crate dependencies.
- HTTPS only: `curl --proto =https --proto-redir =https`.
- Font version pin: `FONT_VERSION = "3.4.0"`.
- Cache root is `mermaid::cache_root()` = `<XDG_CACHE_HOME or ~/.cache>/jiaoren`.
- Download must never block the first frame; no retry until the next process start.
- A failed or partial download leaves no readable cached face.
- System CJK/Arabic Noto fallbacks stay system-only, unchanged.
- `--clean-cache` keeps the font cache.
- All new UI strings are English, matching the rest of the app.

## Review Focus

- First run offline: window still opens, uses system/egui font, shows a short failure line, no panic.
- Curl meter changes per second; a stalled or missing meter must not freeze the UI or hide the window.
- A truncated/replaced HTTP body (proxy error page, 0-byte file) must be rejected before rename.
- Font install happens mid-session: existing widget sizes must re-layout after `ctx.set_fonts`, with no restart.
- `--version` with only a system font must not claim the pinned version was the one loaded.

---

### Task 1: Cache, downloader and font definitions in `src/fonts.rs`

**Files:**
- Modify: `src/fonts.rs`
- Modify: `src/mermaid.rs` (expose cache root)
- Test: inline `mod tests` in `src/fonts.rs`

**Interfaces:**
- Produces:
  - `pub const FONT_VERSION: &str`
  - `pub fn cache_dir(cache_root: &Path) -> PathBuf`
  - `pub fn cached(cache_root: &Path) -> Option<egui::FontDefinitions>`
  - `pub fn fallback() -> Option<egui::FontDefinitions>`
  - `pub fn download_cached(cache_root: &Path, progress: &mut dyn FnMut(u8)) -> Result<egui::FontDefinitions, String>`
  - `pub fn version_line(cache_root: &Path) -> String`
  - `pub fn prune(cache_root: &Path)`
  - `pub enum FontMsg { Progress(u8), Done(Result<egui::FontDefinitions, String>) }`
- Consumes: existing `load_from_dir`, `candidate_dirs`, Noto fallback scan.
- Removes: `pub fn install(ctx)` (the app now chooses cache/fallback itself); update the two system-font tests that call `load()` to call `fallback()` instead.

- [ ] **Step 1: Write failing tests**

Add to `src/fonts.rs` tests:
```rust
#[test]
fn parses_curl_progress_lines() {
    assert_eq!(parse_percent(" 12  249k   12  249k"), Some(12));
    assert_eq!(parse_percent("100  2.6M  100  2.6M    0     0"), Some(100));
    assert_eq!(parse_percent("  0     0    0     0"), Some(0));
    assert_eq!(parse_percent("  % Total    % Received"), None);
    assert_eq!(parse_percent("101  x"), None);
    assert_eq!(parse_percent(""), None);
}

#[test]
fn cache_path_is_versioned() {
    assert_eq!(
        cache_dir(Path::new("/tmp/c")),
        Path::new("/tmp/c/fonts").join(FONT_VERSION)
    );
}

#[test]
fn store_face_rejects_short_or_non_font_bytes() {
    let dir = std::env::temp_dir().join(format!("jiaoren-face-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join("face.ttf");
    assert!(store_face(&dir, &dest, b"<html>nope</html>").is_err());
    assert!(!dest.exists());
    let mut good = vec![0u8, 1, 0, 0];
    good.resize(10_000, 0);
    store_face(&dir, &dest, &good).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), good);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn version_line_names_cache_system_and_default() {
    // Pure checks with an explicit root; no process env or system fonts touched.
    assert_eq!(version_line_with(Some(true), false, None), "3.4.0 (cache)");
    assert_eq!(
        version_line_with(None, true, None),
        "3.4.0 (system; may differ)"
    );
    assert_eq!(version_line_with(None, false, None), "none (egui default)");
    assert_eq!(
        version_line_with(None, false, Some(Path::new("/x/fonts"))),
        "3.4.0 (override: /x/fonts)"
    );
}
```
- [ ] **Step 2: Run tests, confirm they fail**

Run: `cargo test fonts::tests`
Expected: FAIL (`parse_percent`, `cache_dir`, `store_face`, `version_line_with` not found).

- [ ] **Step 3: Implement constants, paths and pure helpers**

In `src/fonts.rs`:
```rust
pub const FONT_VERSION: &str = "3.4.0";
const FONT_BASE: &str = "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/v3.4.0/patched-fonts/FiraCode";
pub const OVERRIDE_ENV: &str = "JIAOREN_FONT_DIR";

pub fn cache_dir(cache_root: &Path) -> PathBuf {
    cache_root.join("fonts").join(FONT_VERSION)
}

fn parse_percent(line: &str) -> Option<u8> {
    line.split_whitespace().next()?.parse::<u8>().ok().filter(|p| *p <= 100)
}

fn version_line_with(cached: Option<bool>, system: bool, override_dir: Option<&Path>) -> String {
    if let Some(dir) = override_dir {
        return format!("{FONT_VERSION} (override: {})", dir.display());
    }
    if cached == Some(true) {
        return format!("{FONT_VERSION} (cache)");
    }
    if system {
        return format!("{FONT_VERSION} (system; may differ)");
    }
    "none (egui default)".to_string()
}
```
`store_face(dir, dest, bytes)` writes `dir/<dest-name>.tmp-<pid>`, rejects `bytes.len() < 10_000` or a magic not in `[[0,1,0,0], b"OTTO", b"true", b"ttcf"]`, then renames. Remove the temp on every error.

- [ ] **Step 4: Implement download, definitions builder, cache/fallback/version/prune**

- `fn defs_from_faces(regular: Vec<u8>, bold: Option<Vec<u8>>) -> egui::FontDefinitions`: move the body of `load_from_dir` after the two `fs::read` calls here; Noto scan stays.
- `load_from_dir(dir)` becomes `let regular = fs::read(...).ok()?; let bold = ...; Some(defs_from_faces(regular, bold))`.
- `fn download_face(url: &str, dir: &Path, dest_name: &str, progress: &mut dyn FnMut(u8)) -> Result<(), String>`: spawn `curl -fL --proto =https --proto-redir =https --connect-timeout 5 --max-time 120 -o <tmp> <url>` with `stderr(Stdio::piped())`; read stderr in 4 KiB chunks, split on `\r`/`\n`, call `progress` when `parse_percent` yields a new value; on exit success read tmp, call `store_face`, else remove tmp and return `Err("curl exited <status>")`.
- `pub fn download_cached(...)`: if `OVERRIDE_ENV` names a readable dir, return `load_from_dir`; if `cache_dir` has Regular, return `cached`; create dir; download Regular (required), then Bold (best-effort); return `defs_from_faces`.
- `pub fn cached(cache_root)` / `pub fn fallback()` use the same read/build helpers; `fallback` tries override dir, then `candidate_dirs()`.
- `pub fn version_line(cache_root)`: compute `cached = cache_dir(cache_root).join(REGULAR).is_file()`, `system = candidate_dirs().any(|d| d.join(REGULAR).is_file())`, override dir from env; call `version_line_with`.
- `pub fn prune(cache_root)`: remove `cache_root/fonts/*` directories whose name != `FONT_VERSION`.
- `pub enum FontMsg { Progress(u8), Done(Result<egui::FontDefinitions, String>) }`.
- In `src/mermaid.rs`, rename `fn dirs_cache()` to `pub(crate) fn cache_root() -> PathBuf` returning `dirs_cache().join("jiaoren")`; `Renderer::new` uses `cache_root()`.

- [ ] **Step 5: Run tests and the gate**

Run: `cargo test fonts::tests && cargo fmt --check && cargo clippy --all-targets --all-features --locked -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/fonts.rs src/mermaid.rs
git commit -m "feat: cache and download the FiraCode font"
```

---

### Task 2: Live download progress in the app

**Files:**
- Modify: `src/app/mod.rs`
- Test: inline `mod tests` (spawn/poll helpers only)

**Interfaces:**
- Consumes: `fonts::FontMsg`, `fonts::download_cached`, `fonts::cached`, `fonts::fallback`, `fonts::prune`, `fonts::cache_dir`, `mermaid::cache_root`.
- Produces: `App` fields `font_rx: Option<std::sync::mpsc::Receiver<fonts::FontMsg>>` and `font_progress: Option<u8>`; status line `Downloading FiraCode font… {pct}% (first run only)`.

- [ ] **Step 1: Write a failing test for the status text helper**

```rust
#[test]
fn font_progress_text_has_percent_and_reason() {
    assert_eq!(font_progress_text(0), "Downloading FiraCode font… 0% (first run only)");
    assert_eq!(font_progress_text(42), "Downloading FiraCode font… 42% (first run only)");
}
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test font_progress_text -- --nocapture`
Expected: FAIL (`font_progress_text` not found).

- [ ] **Step 3: Implement startup selection, polling and status**

In `src/app/mod.rs`:
- Add `fn font_progress_text(pct: u8) -> String` with the exact format above.
- Replace `crate::fonts::install(ctx);` in `App::new` with:
```rust
let cache_root = mermaid::cache_root();
crate::fonts::prune(&cache_root);
let mut font_rx = None;
match crate::fonts::cached(&cache_root) {
    Some(defs) => ctx.set_fonts(defs),
    None => {
        if let Some(defs) = crate::fonts::fallback() {
            ctx.set_fonts(defs);
        }
        let (tx, rx) = mpsc::channel();
        let root = cache_root.clone();
        std::thread::spawn(move || {
            let mut send = |pct| {
                let _ = tx.send(crate::fonts::FontMsg::Progress(pct));
            };
            let result = crate::fonts::download_cached(&root, &mut send);
            let _ = tx.send(crate::fonts::FontMsg::Done(result));
        });
        font_rx = Some(rx);
    }
}
```
Note `mpsc` is already imported; keep the existing name.
- Add fields to `App` and initialize them (`font_rx`, `font_progress: None`).
- In `logic`, poll:
```rust
if let Some(rx) = &self.font_rx {
    let mut done = false;
    while let Ok(msg) = rx.try_recv() {
        match msg {
            crate::fonts::FontMsg::Progress(pct) => self.font_progress = Some(pct),
            crate::fonts::FontMsg::Done(Ok(defs)) => {
                ctx.set_fonts(defs);
                self.font_progress = None;
                done = true;
            }
            crate::fonts::FontMsg::Done(Err(_)) => {
                self.font_progress = None;
                done = true;
                self.set_status("font download failed; using fallback font");
            }
        }
    }
    if done {
        self.font_rx = None;
    } else {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}
```
- In `draw_status`, after the transient `Status` branch and before the search-hit branch, draw the progress line and return:
```rust
if let Some(pct) = self.font_progress {
    let text = font_progress_text(pct);
    status_area(ctx, |ui| {
        ui.add(egui::Label::new(egui::RichText::new(text).color(status_color(self.theme)))
            .wrap_mode(egui::TextWrapMode::Extend));
    });
    return;
}
```

- [ ] **Step 4: Run tests, gate, and a real first-run check**

Run: `cargo test font_progress_text && cargo fmt --check && cargo clippy --all-targets --all-features --locked -- -D warnings && cargo test`
Expected: PASS.
Manual: `XDG_CACHE_HOME=/tmp/jr-fresh cargo run --release -- examples/*.md` on a networked machine; confirm the status percentage advances, the window reflows to FiraCode, and `/tmp/jr-fresh/jiaoren/fonts/3.4.0/` holds both faces. Offline: same command must open and show the failure line.

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs
git commit -m "feat: show font download progress in the status line"
```

---

### Task 3: Version output, README and final gate

**Files:**
- Modify: `src/main.rs` (`version_text`, tests around line 205)
- Modify: `README.md` (Typography section, `--clean-cache` note)
- Test: inline `mod tests` in `src/main.rs`

**Interfaces:**
- Consumes: `fonts::version_line`, `mermaid::cache_root`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn version_text_names_the_font() {
    let text = version_text();
    assert!(
        text.lines().any(|l| l.starts_with("font ")),
        "font line missing: {text}"
    );
}
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test version_text_names_the_font`
Expected: FAIL (no `font` line).

- [ ] **Step 3: Implement the font line and README**

- `version_text()` appends `format!("\nfont {}", crate::fonts::version_line(&mermaid::cache_root()))`.
- README Typography: the font is downloaded once into
  `~/.cache/jiaoren/fonts/3.4.0/`; fallback order `JIAOREN_FONT_DIR` → cache →
  system dirs → egui defaults; `jr --version` prints the font version and
  source; mermaid labels still need the system font.
- README `--clean-cache`: keep the font cache.

- [ ] **Step 4: Run the full gate and install**

Run: `cargo fmt --check && cargo clippy --all-targets --all-features --locked -- -D warnings && cargo test && cargo install --path . --locked && jr --version`
Expected: 365+ tests pass; `jr --version` lists `font 3.4.0 (cache|system; may differ|...)`.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs README.md
git commit -m "feat: report the font version in --version"
```

---

## Self-review

- Spec coverage: cache/download (Task 1), progress + live install + failure fallback (Task 2), `--version` + README + install (Task 3); pruning and HTTPS/atomic rules are in Task 1.
- System-font version claim: `version_line_with` labels it `(system; may differ)`; Review Focus records it.
- No network in this environment: live download is a manual step, parser/atomicity are unit-tested.
