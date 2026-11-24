//! Locate Chromium, download mermaid.js once, and cache diagram renders as SVG.

use std::fs;
use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Once;
use std::time::{Duration, Instant};

/// Wall-clock limit for one Chromium render. `--virtual-time-budget` bounds the
/// page, not the process: a hung browser would otherwise burn a worker forever.
const RENDER_TIMEOUT: Duration = Duration::from_secs(20);

/// Mermaid version used for diagram renders. Shown by `jr --version`.
pub const MERMAID_VERSION: &str = "12.3.0";

/// Where `ensure_mermaid_js` downloads that version from.
pub const MERMAID_URL: &str = "https://cdn.jsdelivr.net/npm/mermaid@12.3.0/dist/mermaid.min.js";

/// SHA-256 of [`MERMAID_URL`] at [`MERMAID_VERSION`]. Verified before the
/// download is renamed into place.
const MERMAID_SHA256: &str = "336df803d87a38b70a01e736e666c8f7601d2ce42f5e47c8fafe5d721074b9a3";

/// Hard cap for the mermaid.js download (the pinned file is about 5.3 MB).
const MAX_MERMAID_BYTES: u64 = 16 * 1024 * 1024;

/// Hard cap for the Chrome for Testing zip (the pinned archive is about 126 MB).
const MAX_CHROMIUM_BYTES: u64 = 256 * 1024 * 1024;

/// Cache-key version: bump when the render page or mermaid version changes.
pub const RENDER_VERSION: &str = "6-mermaid12.3.0-htmlLabelsOff-2x-strict-svg-catppuccin-firacode";

/// Colour theme used for the window and diagram renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// Light palette (Catppuccin Latte).
    Light,
    /// Dark palette (Catppuccin Macchiato).
    Dark,
}

impl Theme {
    /// Cache-file token for this theme (`light` or `dark`).
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
}

/// Failure to produce a diagram SVG.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// No Chromium binary was found.
    #[error("chromium not found; set JIAOREN_CHROMIUM")]
    ChromiumMissing,
    /// mermaid.js failed to load, or mermaid reported an error (text passed through).
    #[error("{0}")]
    MermaidJs(String),
    /// Chromium could not be spawned.
    #[error("spawn chromium {0}")]
    Spawn(String),
    /// The temporary HTML page could not be written.
    #[error("write html: {0}")]
    HtmlWrite(String),
    /// Chromium produced output without an `<svg>` element.
    #[error("no <svg> in chromium output")]
    NoSvg,
    /// The SVG could not be written to the cache.
    #[error("write svg: {0}")]
    Raster(String),
}

/// Renders mermaid sources to cached SVGs with headless Chromium.
pub struct Renderer {
    /// Chromium binary, when one has been discovered.
    pub chromium: Option<PathBuf>,
    /// Path to the local mermaid.js copy.
    pub mermaid_js: PathBuf,
    /// Directory holding cached SVGs, error text and the HTML scratch page.
    pub cache_dir: PathBuf,
}

/// Age that a temp file must reach before it is pruned, so a `.tmp` another
/// jr instance is writing right now is never removed.
const TMP_GRACE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Age that an old Chromium version directory must reach before removal, so a
/// still-running older jr binary keeps its browser.
const CHROMIUM_GRACE: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Whether `modified` is older than `grace` relative to `now`.
fn older_than(
    modified: Option<std::time::SystemTime>,
    now: std::time::SystemTime,
    grace: std::time::Duration,
) -> bool {
    modified
        .and_then(|m| now.duration_since(m).ok())
        .is_some_and(|age| age > grace)
}

/// Delete the jiaoren cache from the command line. `all` also removes the
/// managed Chromium; otherwise the browser is kept so the next document does
/// not need a 132 MB re-download. Fonts are kept for the same reason unless
/// `all` is set, which removes everything. Returns `(bytes, files)` removed.
pub fn clean_cache(all: bool) -> std::io::Result<(u64, usize)> {
    clean_cache_dir(&cache_root(), all)
}

fn clean_cache_dir(dir: &Path, all: bool) -> std::io::Result<(u64, usize)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok((0, 0));
    };
    let mut bytes = 0u64;
    let mut files = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !all && (name == "chromium" || name == "fonts") {
            continue;
        }
        let path = entry.path();
        let (entry_bytes, entry_files) = path_size(&path)?;
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
        bytes += entry_bytes;
        files += entry_files;
    }
    Ok((bytes, files))
}

fn path_size(path: &Path) -> std::io::Result<(u64, usize)> {
    if path.is_file() {
        return Ok((fs::metadata(path)?.len(), 1));
    }
    let mut bytes = 0u64;
    let mut files = 0usize;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let (child_bytes, child_files) = path_size(&entry.path())?;
            bytes += child_bytes;
            files += child_files;
        }
    }
    Ok((bytes, files))
}

/// Delete cache entries that belong to older Mermaid/Chromium versions, plus
/// old scratch temps. Current-version renders, in-flight temps and unknown
/// user files stay, so several jr instances can run together.
fn prune_cache(dir: &Path) {
    let current_js = format!("mermaid-{MERMAID_VERSION}.min.js");
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name == "chromium" {
                prune_chromium_versions(&path, now);
            }
            continue;
        }
        let meta = entry.metadata().ok();
        let stale_render = name.contains("-mermaid") && !name.contains(RENDER_VERSION);
        let stale_js =
            name.starts_with("mermaid") && name.ends_with(".min.js") && name != current_js;
        let stale_tmp = name.ends_with(".tmp")
            && older_than(meta.and_then(|m| m.modified().ok()), now, TMP_GRACE);
        if stale_render || stale_js || stale_tmp {
            let _ = fs::remove_file(&path);
        }
    }
}

fn prune_chromium_versions(dir: &Path, now: std::time::SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old_version = entry.file_name().to_string_lossy() != MANAGED_VERSION;
        let modified = entry.metadata().ok().and_then(|m| m.modified().ok());
        if old_version && entry.path().is_dir() && older_than(modified, now, CHROMIUM_GRACE) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Strip the wrapper from the render page's error text and decode the HTML
/// entities it carries, so the window shows `Parse error ...` rather than
/// `id="out">ERR:...&gt;`.
/// Set once Chromium reports that the OS cannot sandbox it (Ubuntu 23.10+
/// AppArmor restricts unprivileged user namespaces), so later renders skip the
/// doomed sandboxed attempt.
static SANDBOX_UNAVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Headless Chromium command for one render page. The sandbox stays on unless
/// the OS cannot provide one, or the user forces `--no-sandbox`.
fn chromium_command(chromium: &Path, html: &Path, sandbox: bool) -> Command {
    let mut cmd = Command::new(chromium);
    cmd.arg("--headless")
        .arg("--disable-gpu")
        // No network egress from the render page; diagram source is untrusted.
        .arg("--host-resolver-rules=MAP * ~NOTFOUND")
        .arg("--dump-dom")
        .arg("--virtual-time-budget=5000")
        .arg(format!("file://{}", html.display()));
    if !sandbox {
        cmd.arg("--no-sandbox");
    }
    cmd
}

/// Run `cmd` with captured output, killing it once `limit` elapses.
fn output_with_timeout(cmd: &mut Command, limit: Duration) -> std::io::Result<Output> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("no stdout pipe"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("no stderr pipe"))?;
    // Drain both pipes concurrently so a chatty render cannot fill a pipe and
    // deadlock the wait loop.
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "render timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    Ok(Output {
        status,
        stdout: out_reader.join().unwrap_or_default(),
        stderr: err_reader.join().unwrap_or_default(),
    })
}

fn clean_error(raw: &str) -> String {
    let text = raw
        .strip_prefix("id=\"out\">ERR:")
        .unwrap_or(raw)
        .split("</div>")
        .next()
        .unwrap_or("mermaid error");
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

impl Renderer {
    /// Renderer using the user cache dir, with no Chromium discovered yet.
    pub fn new() -> Self {
        let cache_dir = cache_root();
        ensure_private_dir(&cache_dir).ok();
        // Startup cleanup: renders and scripts from older Mermaid or Chromium
        // versions are never read again.
        prune_cache(&cache_dir);
        let mermaid_js = std::env::var_os("JIAOREN_MERMAID_JS")
            .map(PathBuf::from)
            // Versioned filename: a bump must not reuse the old script.
            .unwrap_or_else(|| cache_dir.join(format!("mermaid-{MERMAID_VERSION}.min.js")));
        Renderer {
            chromium: None,
            mermaid_js,
            cache_dir,
        }
    }

    /// Cache path of the rendered SVG for `id` in `theme`.
    pub fn svg_path(&self, id: &str, theme: Theme) -> PathBuf {
        self.cache_dir
            .join(format!("{id}-{}-{RENDER_VERSION}.svg", theme.as_str()))
    }

    /// Cache path of the error text for `id` in `theme`.
    pub fn err_path(&self, id: &str, theme: Theme) -> PathBuf {
        self.cache_dir
            .join(format!("{id}-{}-{RENDER_VERSION}.err", theme.as_str()))
    }

    /// Cached outcome for `id` in `theme`; `None` when nothing is cached yet.
    pub fn cached(&self, id: &str, theme: Theme) -> Option<Result<PathBuf, String>> {
        let svg = self.svg_path(id, theme);
        if svg.exists() {
            return Some(Ok(svg));
        }
        let err = self.err_path(id, theme);
        if err.exists() {
            let msg = fs::read_to_string(&err).unwrap_or_else(|_| "render failed".to_string());
            return Some(Err(msg));
        }
        None
    }

    /// Render `source` to an SVG on disk, reusing or writing the cache.
    pub fn render(&self, id: &str, source: &str, theme: Theme) -> Result<PathBuf, RenderError> {
        let svg = self.svg_path(id, theme);
        if svg.exists() {
            return Ok(svg);
        }
        let err = self.err_path(id, theme);
        if err.exists() {
            let msg = fs::read_to_string(&err).unwrap_or_else(|_| "render failed".to_string());
            return Err(RenderError::MermaidJs(msg));
        }

        let chromium = self
            .chromium
            .clone()
            .or_else(|| find_chromium(&self.cache_dir))
            .ok_or(RenderError::ChromiumMissing)?;

        self.ensure_mermaid_js()?;

        let html = self.cache_dir.join(format!(
            "{id}-{}-{RENDER_VERSION}-{}.html",
            theme.as_str(),
            std::process::id()
        ));
        let page = build_html(&self.mermaid_js, source, theme);
        fs::write(&html, &page).map_err(|e| RenderError::HtmlWrite(e.to_string()))?;

        let out = self.run_chromium(&chromium, &html)?;

        let _ = fs::remove_file(&html);

        if !out.status.success() {
            let msg = format!(
                "chromium exited {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
            self.write_err(id, theme, &msg);
            return Err(RenderError::MermaidJs(msg));
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        // The script source itself contains the literal "ERR:", so look only
        // at the output div's rendered text.
        if let Some(pos) = stdout.find("id=\"out\">ERR:") {
            let msg = clean_error(&stdout[pos..]);
            self.write_err(id, theme, &msg);
            return Err(RenderError::MermaidJs(msg));
        }

        let svg_text = match slice_svg(&stdout) {
            Some(s) => s,
            None => {
                let err = RenderError::NoSvg;
                self.write_err(id, theme, &err.to_string());
                return Err(err);
            }
        };

        self.atomic_write(&svg, svg_text.as_bytes())
            .map_err(|e| RenderError::Raster(e.to_string()))?;
        Ok(svg)
    }

    /// Run Chromium on the render page, keeping its sandbox when the OS allows
    /// it. Retries with `--no-sandbox` only when Chromium itself reports that no
    /// usable sandbox exists.
    fn run_chromium(&self, chromium: &Path, html: &Path) -> Result<Output, RenderError> {
        let forced = std::env::var_os("JIAOREN_CHROMIUM_NO_SANDBOX").is_some();
        let sandbox = !forced && !SANDBOX_UNAVAILABLE.get().copied().unwrap_or(false);
        let mut cmd = chromium_command(chromium, html, sandbox);
        let out = output_with_timeout(&mut cmd, RENDER_TIMEOUT)
            .map_err(|e| RenderError::Spawn(format!("{}: {e}", chromium.display())))?;
        if sandbox
            && !out.status.success()
            && String::from_utf8_lossy(&out.stderr).contains("No usable sandbox")
        {
            let _ = SANDBOX_UNAVAILABLE.set(true);
            let mut cmd = chromium_command(chromium, html, false);
            return output_with_timeout(&mut cmd, RENDER_TIMEOUT)
                .map_err(|e| RenderError::Spawn(format!("{}: {e}", chromium.display())));
        }
        Ok(out)
    }

    fn write_err(&self, id: &str, theme: Theme, msg: &str) {
        let p = self.err_path(id, theme);
        self.atomic_write(&p, msg.as_bytes()).ok();
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        // Append rather than replace the extension so .svg and .err of the
        // same id get distinct tmp names.
        // Per-process temp: two jr instances may render the same diagram.
        let tmp = PathBuf::from(format!("{}.{}.tmp", path.display(), std::process::id()));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, path)
    }

    /// Download mermaid.js once, into `mermaid_js`, when it is missing.
    pub(crate) fn ensure_mermaid_js(&self) -> Result<(), RenderError> {
        self.ensure_mermaid_js_with(&mut |_| {})
    }

    /// [`Self::ensure_mermaid_js`] with phase messages for the status line.
    pub(crate) fn ensure_mermaid_js_with(
        &self,
        progress: &mut impl FnMut(&str),
    ) -> Result<(), RenderError> {
        if self.mermaid_js.exists() {
            return Ok(());
        }
        let tmp = PathBuf::from(format!(
            "{}.{}.tmp",
            self.mermaid_js.display(),
            std::process::id()
        ));
        progress(&format!("mermaid: downloading 0% ({MERMAID_VERSION})…"));
        let downloaded =
            crate::download::curl(MERMAID_URL, &tmp, 60, MAX_MERMAID_BYTES, &mut |pct| {
                progress(&format!("mermaid: downloading {pct}% ({MERMAID_VERSION})…"));
            });
        if let Err(e) = downloaded {
            let _ = fs::remove_file(&tmp);
            return Err(RenderError::MermaidJs(e));
        }
        crate::download::verify_sha256(&tmp, MERMAID_SHA256, "mermaid.min.js")
            .map_err(RenderError::MermaidJs)?;
        fs::rename(&tmp, &self.mermaid_js)
            .map_err(|e| RenderError::MermaidJs(format!("rename mermaid js: {e}")))?;
        progress("mermaid: ready");
        Ok(())
    }
}

/// jiaoren's own cache directory (`<XDG_CACHE_HOME or ~/.cache>/jiaoren`).
pub(crate) fn cache_root() -> PathBuf {
    dirs_cache().join("jiaoren")
}

fn dirs_cache() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(xdg);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache");
    }
    private_cache_base()
}

/// Without XDG/HOME, use a per-UID directory under the system temp dir. It is
/// created 0700 and checked for ownership, so another local user cannot plant
/// `mermaid.min.js` or a Chromium binary in it.
fn private_cache_base() -> PathBuf {
    let uid = effective_uid();
    let base = std::env::temp_dir().join(format!("jiaoren-cache-{uid}"));
    if ensure_private_dir(&base).is_ok() {
        return base;
    }
    // A pre-created/wrong-owner path is unusable. Fall back to a per-process
    // path so a later cache error is still confined to this instance.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let fallback = std::env::temp_dir().join(format!(
        "jiaoren-cache-{uid}-{}-{nanos}",
        std::process::id()
    ));
    let _ = ensure_private_dir(&fallback);
    fallback
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// Create `path` (recursively, 0700) and verify it is a real directory owned by
/// the current user, with no group/other access.
fn ensure_private_dir(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    match builder.create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("create {}: {e}", path.display())),
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    if meta.uid() != effective_uid() {
        return Err(format!(
            "{} is owned by uid {}, not {}",
            path.display(),
            meta.uid(),
            effective_uid()
        ));
    }
    if meta.permissions().mode() & 0o777 != 0o700 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("chmod {}: {e}", path.display()))?;
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    if meta.permissions().mode() & 0o777 != 0o700 {
        return Err(format!("{} is not private", path.display()));
    }
    Ok(())
}

/// Chrome for Testing version jiaoren downloads and manages itself.
pub const MANAGED_VERSION: &str = "158.0.8127.36";

/// SHA-256 of the `chrome-headless-shell-linux64.zip` for [`MANAGED_VERSION`].
/// Verified before `unzip` ever sees the archive.
const CHROMIUM_SHA256: &str = "2d3a23017323c54fd457a678e4351f895c4076d8050e39d5aa6247f93448b73a";

/// PATH names tried, in order, before the managed install.
const CHROMIUM_NAMES: [&str; 4] = [
    "chrome-headless-shell",
    "chromium",
    "google-chrome",
    "chrome",
];

/// Path of the managed binary inside jiaoren's cache directory.
fn managed_chromium_path(cache_dir: &Path) -> PathBuf {
    cache_dir
        .join("chromium")
        .join(MANAGED_VERSION)
        .join("chrome-headless-shell-linux64")
        .join("chrome-headless-shell")
}

/// Candidate binaries in detection order for a given cache dir and PATH.
#[cfg(test)]
fn chromium_candidates(cache_dir: &Path, path_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for name in CHROMIUM_NAMES {
        out.extend(path_dirs.iter().map(|d| d.join(name)));
    }
    out.push(managed_chromium_path(cache_dir));
    out
}

/// Locate Chromium: `JIAOREN_CHROMIUM`, then `PATH`, then the managed install
/// under `cache_dir`.
pub fn find_chromium(cache_dir: &Path) -> Option<PathBuf> {
    static WARNED: Once = Once::new();
    if let Some(raw) = std::env::var_os("JIAOREN_CHROMIUM") {
        let p = PathBuf::from(raw);
        if is_executable(&p) {
            return Some(p);
        }
        WARNED.call_once(|| {
            eprintln!(
                "warning: JIAOREN_CHROMIUM={} is not an executable file; ignoring it",
                p.display()
            );
        });
    }
    for name in CHROMIUM_NAMES {
        if let Some(p) = find_in_path(name) {
            return Some(p);
        }
    }
    let managed = managed_chromium_path(cache_dir);
    is_executable(&managed).then_some(managed)
}

/// Return an existing Chromium, or download the pinned `chrome-headless-shell`
/// into `cache_dir` (`~/.cache/jiaoren`) and return its path.
#[cfg(test)] // app uses ensure_chromium_with for progress messages
pub fn ensure_chromium(cache_dir: &Path) -> Result<PathBuf, String> {
    ensure_chromium_with(cache_dir, &mut |_| {})
}

/// [`ensure_chromium`] with phase messages for the app's download status line.
pub fn ensure_chromium_with(
    cache_dir: &Path,
    progress: &mut impl FnMut(&str),
) -> Result<PathBuf, String> {
    if let Some(p) = find_chromium(cache_dir) {
        return Ok(p);
    }
    install_chromium_with(cache_dir, progress)
}

/// Take an exclusive lock covering detect → validate → rename. Blocking: a
/// second instance waits for the first, then re-checks detection and reuses
/// its install.
fn acquire_install_lock(cache_dir: &Path) -> Result<fs::File, String> {
    let dir = cache_dir.join("chromium");
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(".install.lock");
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    loop {
        // SAFETY: `file` owns a valid fd for the duration of the call.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc == 0 {
            return Ok(file);
        }
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!("lock {}: {err}", path.display()));
        }
    }
}

fn pid_is_alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks for existence; no signal is delivered.
    let rc = unsafe { libc::kill(pid, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Remove `.tmp-<pid>` dirs left behind by installs whose process is gone.
fn cleanup_stale_temps(version_dir: &Path) {
    let Ok(entries) = fs::read_dir(version_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.strip_prefix(".tmp-"))
            .and_then(|p| p.parse::<i32>().ok())
        else {
            continue;
        };
        if pid > 0 && !pid_is_alive(pid) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Require `path` to be executable and `--version` to exit 0.
fn validate_binary(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("chmod: {e}"))?;
    let out = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("--version exited {}", out.status))
    }
}

#[cfg(test)]
fn install_chromium(cache_dir: &Path) -> Result<PathBuf, String> {
    install_chromium_with(cache_dir, &mut |_| {})
}

fn install_chromium_with(
    cache_dir: &Path,
    progress: &mut impl FnMut(&str),
) -> Result<PathBuf, String> {
    let _lock = acquire_install_lock(cache_dir)?;
    // Another instance may have installed while we waited for the lock.
    if let Some(p) = find_chromium(cache_dir) {
        return Ok(p);
    }
    let version_dir = cache_dir.join("chromium").join(MANAGED_VERSION);
    let final_dir = version_dir.join("chrome-headless-shell-linux64");
    let final_bin = final_dir.join("chrome-headless-shell");
    fs::create_dir_all(&version_dir)
        .map_err(|e| format!("create {}: {e}", version_dir.display()))?;
    cleanup_stale_temps(&version_dir);
    if final_dir.exists() {
        // Never remove or move a published install out from under a running
        // process; a half-written one can only come from an older build.
        return Err(format!(
            "{} exists but is not a usable chrome-headless-shell; remove it to reinstall",
            final_dir.display()
        ));
    }
    // Build everything in a temp dir beside the target so the last step is an
    // atomic rename and a failed install never leaves a half-written binary.
    let tmp_dir = version_dir.join(format!(".tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp_dir);
    let zip = tmp_dir.join("chrome-headless-shell-linux64.zip");
    let url = format!(
        "https://storage.googleapis.com/chrome-for-testing-public/{MANAGED_VERSION}/linux64/chrome-headless-shell-linux64.zip"
    );

    let fail = |step: &str, e: String| -> String {
        let _ = fs::remove_dir_all(&tmp_dir);
        format!("{step}: {e}")
    };

    fs::create_dir_all(&tmp_dir).map_err(|e| format!("create {}: {e}", tmp_dir.display()))?;
    progress(&format!(
        "chromium: downloading 0% ({MANAGED_VERSION}, 132 MB)…"
    ));
    crate::download::curl(&url, &zip, 600, MAX_CHROMIUM_BYTES, &mut |pct| {
        progress(&format!(
            "chromium: downloading {pct}% ({MANAGED_VERSION})…"
        ));
    })
    .map_err(|e| fail("download", e))?;
    progress("chromium: verifying sha256…");
    crate::download::verify_sha256(&zip, CHROMIUM_SHA256, "chrome-headless-shell zip")
        .map_err(|e| fail("verify archive", e))?;
    progress("chromium: verifying archive…");
    let status = Command::new("unzip")
        .arg("-t")
        .arg(&zip)
        .status()
        .map_err(|e| fail("spawn unzip -t", e.to_string()))?;
    if !status.success() {
        return Err(fail("archive test", format!("unzip -t exited {status}")));
    }
    progress("chromium: extracting…");
    let status = Command::new("unzip")
        .arg("-q")
        .arg("-o")
        .arg(&zip)
        .arg("-d")
        .arg(&tmp_dir)
        .status()
        .map_err(|e| fail("spawn unzip", e.to_string()))?;
    if !status.success() {
        return Err(fail("extract", format!("unzip exited {status}")));
    }
    let _ = fs::remove_file(&zip);
    let tmp_bin = tmp_dir.join("chrome-headless-shell-linux64/chrome-headless-shell");
    if !tmp_bin.is_file() {
        return Err(fail("extract", "chrome-headless-shell missing".to_string()));
    }
    // Validate inside the temp dir: only a working binary gets published, so a
    // killed process leaves either no final dir or a fully validated one.
    progress("chromium: checking binary…");
    validate_binary(&tmp_bin).map_err(|e| fail("binary check", e))?;
    if let Err(e) = fs::rename(tmp_dir.join("chrome-headless-shell-linux64"), &final_dir) {
        return Err(fail("rename into place", e.to_string()));
    }
    let _ = fs::remove_dir_all(&tmp_dir);
    progress("chromium: installed");
    Ok(final_bin)
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(name);
        if is_executable(&cand) {
            return Some(cand);
        }
    }
    None
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

pub(crate) fn file_url(path: &Path) -> String {
    let mut out = String::new();
    for c in path.to_string_lossy().chars() {
        match c {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            '%' => out.push_str("%25"),
            '"' => out.push_str("%22"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            _ => out.push(c),
        }
    }
    out
}

// Build the Chromium page. The JS is referenced by absolute file:// URL so a
// $JIAOREN_MERMAID_JS override outside cache_dir still loads.
fn build_html(mermaid_js: &Path, source: &str, theme: Theme) -> String {
    let abs = mermaid_js
        .canonicalize()
        .unwrap_or_else(|_| mermaid_js.to_path_buf());
    let js_src = format!("file://{}", file_url(&abs));
    let escaped = html_escape(source);
    format!(
        "<!doctype html><meta charset=\"utf-8\"><body style=\"margin:0\"><div id=\"out\"></div>\n\
<pre id=\"src\" hidden>{escaped}</pre>\n\
<script src=\"{js_src}\"></script>\n\
<script>\n\
mermaid.initialize({{startOnLoad:false, htmlLabels:false, theme:\"base\", fontFamily:\"{family}\",\n\
  themeVariables:{{{vars}}},\n\
  flowchart:{{htmlLabels:false}}, securityLevel:\"strict\"}});\n\
mermaid.render(\"d\", document.getElementById(\"src\").textContent)\n\
  .then(r => {{ document.getElementById(\"out\").innerHTML = r.svg; }})\n\
  .catch(e => {{ document.getElementById(\"out\").textContent = \"ERR:\" + e.message; }});\n\
</script>\n",
        vars = crate::theme::mermaid_variables(theme),
        family = crate::fonts::FAMILY
    )
}

fn slice_svg(stdout: &str) -> Option<&str> {
    let start = stdout.find("<svg")?;
    let end = stdout.rfind("</svg>")? + "</svg>".len();
    Some(&stdout[start..end])
}

/// First attribute value for `name` within an opening tag, any quote style,
/// ignoring suffixed names like `stroke-width`.
fn tag_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=");
    let bytes = tag.as_bytes();
    let mut from = 0;
    while let Some(rel) = tag[from..].find(&pat) {
        let p = from + rel;
        let boundary = p == 0 || bytes[p - 1].is_ascii_whitespace();
        if boundary {
            if let Some(v) = quoted_value(&tag[p + pat.len()..]) {
                return Some(v);
            }
        }
        from = p + name.len();
    }
    None
}

fn quoted_value(s: &str) -> Option<&str> {
    let q = s.chars().next()?;
    if q != '"' && q != '\'' {
        return None;
    }
    let end = s[1..].find(q)?;
    Some(&s[1..1 + end])
}

/// Leading float of an attribute value, tolerating unit suffixes (`120px`);
/// rejects non-px units (`100%`), non-positive or non-finite numbers.
fn px_value(raw: &str) -> Option<f32> {
    let s = raw.trim();
    if s.ends_with('%') {
        return None;
    }
    (1..=s.len())
        .rev()
        .filter(|&i| s.is_char_boundary(i))
        .find_map(|i| s[..i].parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// Natural size of an SVG: `width`/`height` attributes in px, falling back to
/// the viewBox's 3rd/4th numbers. `None` when neither is present or parsable;
/// never panics on malformed input.
pub fn parse_svg_size(svg: &str) -> Option<(f32, f32)> {
    let start = svg.find("<svg")?;
    let tag = &svg[start..];
    let tag = &tag[..tag.find('>')?];
    if let (Some(w), Some(h)) = (
        tag_attr(tag, "width").and_then(px_value),
        tag_attr(tag, "height").and_then(px_value),
    ) {
        return Some((w, h));
    }
    let vb = tag_attr(tag, "viewBox")?;
    let nums: Vec<&str> = vb
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .collect();
    if nums.len() < 4 {
        return None;
    }
    let w = px_value(nums[2]);
    let h = px_value(nums[3]);
    Some((w?, h?))
}

#[cfg(test)]
mod tests {
    mod mermaid {
        use super::super::*;
        use std::sync::Mutex;

        /// Every test that mutates process env holds this, so they never race.
        static ENV_LOCK: Mutex<()> = Mutex::new(());

        fn env_lock() -> std::sync::MutexGuard<'static, ()> {
            ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
        }

        /// Restores an environment variable on drop, even when a test panics.
        struct EnvVar(&'static str, Option<std::ffi::OsString>);

        impl EnvVar {
            fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
                let saved = std::env::var_os(name);
                std::env::set_var(name, value);
                Self(name, saved)
            }

            fn unset(name: &'static str) -> Self {
                let saved = std::env::var_os(name);
                std::env::remove_var(name);
                Self(name, saved)
            }
        }

        impl Drop for EnvVar {
            fn drop(&mut self) {
                match &self.1 {
                    Some(v) => std::env::set_var(self.0, v),
                    None => std::env::remove_var(self.0),
                }
            }
        }

        fn test_renderer(cache: &Path) -> Renderer {
            Renderer {
                chromium: None,
                mermaid_js: cache.join("mermaid.min.js"),
                cache_dir: cache.to_path_buf(),
            }
        }

        #[test]
        fn render_error_messages_are_unchanged() {
            assert_eq!(
                RenderError::ChromiumMissing.to_string(),
                "chromium not found; set JIAOREN_CHROMIUM"
            );
            assert_eq!(
                RenderError::MermaidJs("ERR:oops".into()).to_string(),
                "ERR:oops"
            );
            assert_eq!(
                RenderError::Spawn("/x/chromium: nope".into()).to_string(),
                "spawn chromium /x/chromium: nope"
            );
            assert_eq!(
                RenderError::HtmlWrite("e".into()).to_string(),
                "write html: e"
            );
            assert_eq!(
                RenderError::NoSvg.to_string(),
                "no <svg> in chromium output"
            );
            assert_eq!(RenderError::Raster("e".into()).to_string(), "write svg: e");
        }

        #[test]
        fn cache_without_home_or_xdg_uses_a_private_per_uid_base() {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let _guard = env_lock();
            let _xdg = EnvVar::unset("XDG_CACHE_HOME");
            let _home = EnvVar::unset("HOME");
            let root = cache_root();
            let base = root.parent().expect("cache root has a parent");
            assert_eq!(base, private_cache_base().as_path());
            assert_ne!(base, std::env::temp_dir().as_path());
            assert_ne!(base.file_name(), Some(std::ffi::OsStr::new("jiaoren")));
            let uid = effective_uid().to_string();
            assert!(
                base.file_name()
                    .is_some_and(|n| n.to_string_lossy().contains(uid.as_str())),
                "fallback must be per-uid: {}",
                base.display()
            );
            let meta = fs::symlink_metadata(base).unwrap();
            assert!(meta.is_dir());
            assert_eq!(meta.uid(), effective_uid());
            assert_eq!(meta.permissions().mode() & 0o777, 0o700);
            let _ = fs::remove_dir_all(base);
        }

        #[test]
        fn clean_cache_keeps_browser_and_fonts_unless_all_is_set() {
            let base = std::env::temp_dir().join(format!(
                "jiaoren-clean-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(base.join("chromium").join(MANAGED_VERSION)).unwrap();
            fs::write(base.join("abc.svg"), b"12345").unwrap();
            fs::write(
                base.join("chromium").join(MANAGED_VERSION).join("chrome"),
                b"1234567890",
            )
            .unwrap();
            fs::create_dir_all(base.join("fonts").join("3.4.0")).unwrap();
            fs::write(base.join("fonts").join("3.4.0").join("face.ttf"), b"font").unwrap();
            let (bytes, files) = clean_cache_dir(&base, false).unwrap();
            assert_eq!((bytes, files), (5, 1));
            assert!(base.join("chromium").exists());
            assert!(base.join("fonts").join("3.4.0").join("face.ttf").exists());
            assert!(!base.join("abc.svg").exists());
            let (bytes, files) = clean_cache_dir(&base, true).unwrap();
            assert_eq!((bytes, files), (14, 2));
            assert!(!base.join("chromium").exists());
            assert!(!base.join("fonts").exists());
            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn error_text_loses_the_wrapper_and_entities() {
            assert_eq!(
                clean_error("id=\"out\">ERR:Parse error on line 2:\n A --&gt; B</div>tail"),
                "Parse error on line 2:\n A --> B"
            );
            assert_eq!(clean_error("plain &lt;err&gt;"), "plain <err>");
        }

        #[test]
        fn prune_removes_old_versions_and_keeps_current_cache() {
            let dir = std::env::temp_dir().join(format!(
                "jiaoren-prune-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(dir.join("chromium").join(MANAGED_VERSION)).unwrap();
            fs::create_dir_all(dir.join("chromium").join("1.0.0")).unwrap();
            let current = format!("abc-light-{RENDER_VERSION}.svg");
            let stale =
                "abc-light-5-mermaid11.12.0-htmlLabelsOff-2x-strict-svg-catppuccin-firacode.svg";
            let current_js = format!("mermaid-{MERMAID_VERSION}.min.js");
            let stale_js = "mermaid-11.12.0.min.js";
            for name in [
                current.as_str(),
                "abc-dark-5-mermaid11.12.0-htmlLabelsOff-2x-strict-svg-catppuccin-firacode.err",
                current_js.as_str(),
                stale_js,
                "leftover.tmp",
                "notes.txt",
            ] {
                fs::write(dir.join(name), b"x").unwrap();
            }
            prune_cache(&dir);
            assert!(dir.join(&current).exists());
            assert!(dir.join(&current_js).exists());
            assert!(!dir.join(stale).exists());
            assert!(!dir.join(stale_js).exists());
            // A fresh temp may belong to another running jr and stays.
            assert!(dir.join("leftover.tmp").exists());
            assert!(dir.join("notes.txt").exists(), "user files stay");
            assert!(dir.join("chromium").join(MANAGED_VERSION).exists());
            // A fresh old-version browser dir may be in use by an older jr.
            assert!(dir.join("chromium").join("1.0.0").exists());
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn age_must_pass_before_pruning_can_touch_a_file() {
            let now = std::time::SystemTime::now();
            let fresh = now - std::time::Duration::from_secs(60);
            let old = now - TMP_GRACE - std::time::Duration::from_secs(1);
            assert!(!older_than(Some(fresh), now, TMP_GRACE));
            assert!(older_than(Some(old), now, TMP_GRACE));
            assert!(!older_than(None, now, TMP_GRACE));
        }

        #[test]
        fn render_key_and_download_track_the_mermaid_version() {
            assert!(RENDER_VERSION.contains(MERMAID_VERSION));
            assert!(MERMAID_URL.contains(MERMAID_VERSION));
            assert!(!RENDER_VERSION.contains("11.12.0"));
        }

        #[test]
        fn escapes_html_metacharacters() {
            assert_eq!(
                html_escape("A-->B & C<D>\"E\""),
                "A--&gt;B &amp; C&lt;D&gt;\"E\""
            );
            assert_eq!(html_escape("</pre>"), "&lt;/pre&gt;");
        }

        #[test]
        fn cache_paths_include_theme_and_render_version() {
            let d = std::env::temp_dir().join("jiaoren-test-paths");
            let r = test_renderer(&d);
            let p = r.svg_path("abc", Theme::Dark);
            assert_eq!(
                p.file_name().unwrap().to_str().unwrap(),
                format!("abc-dark-{RENDER_VERSION}.svg")
            );
            assert_ne!(p, r.svg_path("abc", Theme::Light));
        }

        #[test]
        fn cached_reports_missing_failure_and_success() {
            let d = std::env::temp_dir().join(format!("jiaoren-test-cache-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            let r = test_renderer(&d);
            assert!(r.cached("k", Theme::Light).is_none());
            std::fs::write(r.err_path("k", Theme::Light), "boom").unwrap();
            match r.cached("k", Theme::Light) {
                // The cached `.err` text reaches the UI verbatim.
                Some(Err(msg)) => assert_eq!(msg, "boom"),
                _ => panic!("expected cached error"),
            }
            std::fs::write(r.svg_path("k", Theme::Light), b"svg").unwrap();
            assert!(matches!(r.cached("k", Theme::Light), Some(Ok(_))));
        }

        #[test]
        fn svg_path_uses_svg_extension_and_version() {
            let r = test_renderer(Path::new("/tmp/x"));
            let p = r.svg_path("abc", Theme::Light);
            assert!(p
                .to_string_lossy()
                .ends_with(&format!("abc-light-{RENDER_VERSION}.svg")));
        }

        #[test]
        fn parses_width_and_height() {
            assert_eq!(
                parse_svg_size(r#"<svg width="120" height="40" viewBox="0 0 120 40">"#),
                Some((120.0, 40.0))
            );
            assert_eq!(
                parse_svg_size(r#"<svg viewBox="0 0 120 40">"#),
                Some((120.0, 40.0))
            );
            assert_eq!(parse_svg_size("<svg>"), None);
        }

        #[test]
        fn viewbox_accepts_comma_separators() {
            assert_eq!(
                parse_svg_size(r#"<svg viewBox="0,0,120,40">"#),
                Some((120.0, 40.0))
            );
            assert_eq!(
                parse_svg_size(r#"<svg viewBox="0, 0, 120, 40">"#),
                Some((120.0, 40.0))
            );
        }

        #[test]
        fn percent_width_and_malformed_fall_back_or_return_none() {
            // Real mermaid output: width="100%", size comes from viewBox.
            assert_eq!(
                parse_svg_size(
                    r#"<svg id="d" width="100%" xmlns="u" style="max-width: 87px;" viewBox="0 0 87 160"><style>#d{font-size:16px}</style></svg>"#
                ),
                Some((87.0, 160.0))
            );
            // stroke-width must not be picked up as width.
            assert_eq!(
                parse_svg_size(r#"<svg stroke-width="2" viewBox="0 0 5 6">"#),
                Some((5.0, 6.0))
            );
            // Garbage must not panic.
            assert_eq!(parse_svg_size("<<svg"), None);
            assert_eq!(
                parse_svg_size(r#"<svg width="auto" height="100%" viewBox="0 0">"#),
                None
            );
        }

        #[test]
        fn candidate_order_is_path_then_managed() {
            use std::os::unix::fs::PermissionsExt;
            let base = std::env::temp_dir().join(format!("jiaoren-cand-{}", std::process::id()));
            let _ = fs::remove_dir_all(&base);
            let bin_dir = base.join("bin");
            let cache = base.join("cache");
            fs::create_dir_all(&bin_dir).unwrap();
            fs::create_dir_all(&cache).unwrap();
            let chrome = bin_dir.join("chromium");
            fs::write(&chrome, b"#!/bin/sh\n").unwrap();
            fs::set_permissions(&chrome, fs::Permissions::from_mode(0o755)).unwrap();

            let cands = chromium_candidates(&cache, std::slice::from_ref(&bin_dir));
            let names: Vec<String> = cands
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            assert_eq!(
                names,
                [
                    "chrome-headless-shell",
                    "chromium",
                    "google-chrome",
                    "chrome",
                    "chrome-headless-shell"
                ]
            );
            assert_eq!(cands.last(), Some(&managed_chromium_path(&cache)));
            assert!(is_executable(&chrome));
            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn managed_install_is_detected_from_passed_cache_dir() {
            use std::os::unix::fs::PermissionsExt;
            let _guard = env_lock();
            let base = std::env::temp_dir().join(format!("jiaoren-managed-{}", std::process::id()));
            let _ = fs::remove_dir_all(&base);
            let cache = base.join("cache");
            let empty_path = base.join("empty-path");
            fs::create_dir_all(&empty_path).unwrap();
            let bin = managed_chromium_path(&cache);
            assert!(bin.ends_with(format!(
                "chromium/{MANAGED_VERSION}/chrome-headless-shell-linux64/chrome-headless-shell"
            )));
            fs::create_dir_all(bin.parent().unwrap()).unwrap();
            fs::write(&bin, b"#!/bin/sh\n").unwrap();
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

            let _env = EnvVar::unset("JIAOREN_CHROMIUM");
            let _path = EnvVar::set("PATH", &empty_path);
            assert_eq!(find_chromium(&cache), Some(bin));
            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn invalid_env_path_falls_through_to_path() {
            use std::os::unix::fs::PermissionsExt;
            let _guard = env_lock();
            let base = std::env::temp_dir().join(format!("jiaoren-env-bad-{}", std::process::id()));
            let _ = fs::remove_dir_all(&base);
            let bin_dir = base.join("bin");
            fs::create_dir_all(&bin_dir).unwrap();
            let chrome = bin_dir.join("chromium");
            fs::write(&chrome, b"#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(&chrome, fs::Permissions::from_mode(0o755)).unwrap();
            let cache = base.join("cache");

            // A directory passed as JIAOREN_CHROMIUM must be ignored, not stored.
            let _env = EnvVar::set("JIAOREN_CHROMIUM", &base);
            let _path = EnvVar::set("PATH", &bin_dir);
            assert_eq!(ensure_chromium(&cache), Ok(chrome));
            assert!(!cache.join("chromium").exists());
            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn ensure_chromium_returns_env_override_without_downloading() {
            use std::os::unix::fs::PermissionsExt;
            let _guard = env_lock();
            let base = std::env::temp_dir().join(format!("jiaoren-env-{}", std::process::id()));
            let _ = fs::remove_dir_all(&base);
            fs::create_dir_all(&base).unwrap();
            let fake = base.join("chrome-headless-shell");
            fs::write(&fake, b"#!/bin/sh\necho 'HeadlessChrome 0.0'\n").unwrap();
            fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
            let cache = base.join("cache");
            let _env = EnvVar::set("JIAOREN_CHROMIUM", &fake);
            assert_eq!(ensure_chromium(&cache), Ok(fake));
            // No managed install was attempted.
            assert!(!cache.join("chromium").exists());
            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn validate_binary_sets_exec_bit_and_requires_zero_exit() {
            let base =
                std::env::temp_dir().join(format!("jiaoren-validate-{}", std::process::id()));
            let _ = fs::remove_dir_all(&base);
            fs::create_dir_all(&base).unwrap();
            let ok = base.join("ok");
            fs::write(&ok, b"#!/bin/sh\nexit 0\n").unwrap();
            // Not executable yet: validate must chmod before running it.
            assert!(validate_binary(&ok).is_ok());
            assert!(is_executable(&ok));
            let bad = base.join("bad");
            fs::write(&bad, b"#!/bin/sh\nexit 7\n").unwrap();
            let err = validate_binary(&bad).unwrap_err();
            assert!(err.contains("--version exited"), "{err}");
            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn stale_temp_dirs_for_dead_pids_are_removed() {
            let base = std::env::temp_dir().join(format!("jiaoren-tmp-{}", std::process::id()));
            let _ = fs::remove_dir_all(&base);
            let dead = base.join(".tmp-2147483647");
            let live = base.join(format!(".tmp-{}", std::process::id()));
            fs::create_dir_all(&dead).unwrap();
            fs::create_dir_all(&live).unwrap();
            cleanup_stale_temps(&base);
            assert!(!dead.exists());
            assert!(live.exists());
            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn ensure_chromium_errors_when_cache_dir_is_a_file() {
            let base = std::env::temp_dir().join(format!("jiaoren-bad-{}", std::process::id()));
            let _ = fs::remove_dir_all(&base);
            fs::write(&base, b"not a dir").unwrap();
            let err = install_chromium(&base.join("cache")).unwrap_err();
            assert!(err.contains("create"), "{err}");
            let _ = fs::remove_file(&base);
        }

        #[test]
        fn html_references_absolute_mermaid_js_path() {
            let js = std::env::temp_dir().join("elsewhere/mermaid.min.js");
            let html = build_html(&js, "graph TD; A-->B;", Theme::Light);
            assert!(html.contains(&format!("file://{}", js.display())));
            assert!(!html.contains("src=\"mermaid.min.js\""));
        }

        #[test]
        fn html_uses_base_theme_with_catppuccin_variables() {
            let js = std::env::temp_dir().join("mermaid.min.js");
            let html = build_html(&js, "graph TD; A-->B;", Theme::Dark);
            assert!(html.contains("theme:\"base\""));
            assert!(html.contains(crate::theme::mermaid_variables(Theme::Dark)));
            assert!(html.contains("themeVariables:{"));
            assert!(!html.contains("theme:\"dark\""));
        }

        #[test]
        fn html_uses_firacode_font_family() {
            let js = std::env::temp_dir().join("mermaid.min.js");
            let html = build_html(&js, "graph TD; A-->B;", Theme::Light);
            assert!(html.contains(&format!("fontFamily:\"{}\"", crate::fonts::FAMILY)));
            assert!(!html.contains("fontFamily:\"sans-serif\""));
        }

        #[test]
        fn ensure_mermaid_js_is_idempotent() {
            let d = std::env::temp_dir().join(format!("jiaoren-js-{}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).unwrap();
            let r = test_renderer(&d);
            fs::write(&r.mermaid_js, b"// already here").unwrap();
            r.ensure_mermaid_js().unwrap();
            let first = fs::metadata(&r.mermaid_js).unwrap().modified().unwrap();
            let first_bytes = fs::read(&r.mermaid_js).unwrap();
            // A second call must not download or rewrite the file.
            r.ensure_mermaid_js().unwrap();
            assert_eq!(
                first,
                fs::metadata(&r.mermaid_js).unwrap().modified().unwrap()
            );
            assert_eq!(first_bytes, fs::read(&r.mermaid_js).unwrap());
            let _ = fs::remove_dir_all(&d);
        }

        #[test]
        #[ignore = "requires headless chromium"]
        fn renders_real_diagram() {
            let r = Renderer::new();
            let p = r.render("smoke", "graph TD; A-->B;", Theme::Light).unwrap();
            assert!(p.exists());
            assert!(fs::metadata(&p).unwrap().len() > 1000);
        }

        #[test]
        #[ignore = "requires headless chromium"]
        fn renders_two_real_diagrams() {
            let r = Renderer::new();
            let docs = [
                ("smoke-flow", "graph LR; A-->B;", Theme::Light),
                (
                    "smoke-seq",
                    "sequenceDiagram\n  A->>B: hello\n  B-->>A: world",
                    Theme::Dark,
                ),
            ];
            for (id, src, theme) in docs {
                let p = r.render(id, src, theme).unwrap();
                assert!(fs::metadata(&p).unwrap().len() > 1000, "{id} too small");
            }
        }
    }
}
