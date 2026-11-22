//! Locate, cache and install the FiraCode Nerd Font faces used by the window.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

/// Font family name as reported by `fc-list`; reused by the mermaid `fontFamily`
/// setting so diagrams and the window chrome agree.
pub const FAMILY: &str = "FiraCode Nerd Font";

/// Pinned nerd-fonts release; also the cache subdirectory. Bumping it selects a
/// new cache dir and lets startup prune the old faces.
pub const FONT_VERSION: &str = "3.4.0";

/// Where the pinned FiraCode faces live in the nerd-fonts repository.
const FONT_BASE: &str =
    "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/v3.4.0/patched-fonts/FiraCode";

/// Explicit font directory; skips the download entirely.
pub const OVERRIDE_ENV: &str = "JIAOREN_FONT_DIR";

const REGULAR: &str = "FiraCodeNerdFont-Regular";
const BOLD: &str = "FiraCodeNerdFont-Bold";

/// Family holding only the Bold face, so `**strong**` runs can ask for a real
/// bold weight. egui has no per-run weight flag: a face listed as a *fallback*
/// is only used for missing glyphs, so bold text needs its own family.
pub const BOLD_FAMILY: &str = "FiraCode Bold";

/// System fallbacks for scripts FiraCode lacks (CJK, Arabic). Loaded when
/// present so proxied documents do not render tofu; missing files simply keep
/// the egui defaults.
const FALLBACK_FILES: &[&str] = &[
    "NotoSansCJK-Regular.ttc",
    "NotoSansArabic-Regular.ttf",
    "NotoNaskhArabic-Regular.ttf",
];
const FALLBACK_DIRS: &[&str] = &[
    "/usr/share/fonts/opentype/noto",
    "/usr/share/fonts/truetype/noto",
    "/usr/share/fonts/truetype",
];

/// A background cache/download notification; the app polls it once per frame.
pub enum FontMsg {
    /// Downloaded percentage, 0..=100.
    Progress(u8),
    /// Final definitions to install, or why the download failed.
    Done(Result<egui::FontDefinitions, String>),
}

/// The font id for bold text at `size`.
pub fn bold_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(BOLD_FAMILY.into()))
}

/// Whether `font` asks for the bold family.
#[cfg(test)]
pub fn is_bold(font: &egui::FontId) -> bool {
    matches!(&font.family, egui::FontFamily::Name(name) if &**name == BOLD_FAMILY)
}

/// Cache directory for the pinned font version.
pub fn cache_dir(cache_root: &Path) -> PathBuf {
    cache_root.join("fonts").join(FONT_VERSION)
}

/// Definitions from the managed cache, when the Regular face is present.
pub fn cached(cache_root: &Path) -> Option<egui::FontDefinitions> {
    load_from_dir(&cache_dir(cache_root))
}

/// Definitions from the override directory, then the system font dirs.
pub fn fallback() -> Option<egui::FontDefinitions> {
    override_dir()
        .and_then(|dir| load_from_dir(&dir))
        .or_else(|| {
            candidate_dirs()
                .into_iter()
                .find_map(|dir| load_from_dir(&dir))
        })
}

/// One-line source and version for `jr --version`.
pub fn version_line(cache_root: &Path) -> String {
    let cached = cache_dir(cache_root).join(face(REGULAR)).is_file();
    let system = candidate_dirs()
        .iter()
        .any(|dir| dir.join(face(REGULAR)).is_file());
    version_line_with(Some(cached), system, override_dir().as_deref())
}

/// Delete cached font versions other than [`FONT_VERSION`]. Best-effort.
pub fn prune(cache_root: &Path) {
    let root = cache_root.join("fonts");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != FONT_VERSION {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Ensure the pinned faces are cached, downloading them once if needed.
pub fn download_cached(
    cache_root: &Path,
    progress: &mut dyn FnMut(u8),
) -> Result<egui::FontDefinitions, String> {
    if let Some(defs) = override_dir().and_then(|dir| load_from_dir(&dir)) {
        return Ok(defs);
    }
    if let Some(defs) = cached(cache_root) {
        return Ok(defs);
    }
    let dir = cache_dir(cache_root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    progress(0);
    let regular_url = format!("{FONT_BASE}/Regular/{REGULAR}.ttf");
    download_face(&regular_url, &dir, &face(REGULAR), progress)?;
    // Bold is optional: without it bold runs fall back to Regular.
    let bold_url = format!("{FONT_BASE}/Bold/{BOLD}.ttf");
    let _ = download_face(&bold_url, &dir, &face(BOLD), &mut |_| {});
    let regular =
        std::fs::read(dir.join(face(REGULAR))).map_err(|e| format!("read cached regular: {e}"))?;
    let bold = std::fs::read(dir.join(face(BOLD))).ok();
    Ok(defs_from_faces(regular, bold))
}

fn face(name: &str) -> String {
    format!("{name}.ttf")
}

fn override_dir() -> Option<PathBuf> {
    std::env::var_os(OVERRIDE_ENV)
        .map(PathBuf::from)
        .filter(|dir| dir.join(face(REGULAR)).is_file())
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

fn parse_percent(line: &str) -> Option<u8> {
    line.split_whitespace()
        .next()?
        .parse::<u8>()
        .ok()
        .filter(|p| *p <= 100)
}

fn is_font(bytes: &[u8]) -> bool {
    let Some(magic) = bytes.get(..4) else {
        return false;
    };
    [&[0u8, 1, 0, 0][..], b"OTTO", b"true", b"ttcf"]
        .iter()
        .any(|want| magic == *want)
}

/// Validate `bytes` and atomically move them to `dest` via a temp file.
fn store_face(dir: &Path, dest: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 10_000 || !is_font(bytes) {
        return Err("not a font".to_string());
    }
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("bad font file name")?;
    let tmp = dir.join(format!("{name}.tmp-{}", std::process::id()));
    if let Err(e) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("write {}: {e}", tmp.display()));
    }
    if let Err(e) = std::fs::rename(&tmp, dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("rename {}: {e}", dest.display()));
    }
    Ok(())
}

/// Download one face with `curl`, reporting the meter's percentage.
fn download_face(
    url: &str,
    dir: &Path,
    name: &str,
    progress: &mut dyn FnMut(u8),
) -> Result<(), String> {
    let dest = dir.join(name);
    let tmp = dir.join(format!("{name}.dl-{}", std::process::id()));
    let mut child = Command::new("curl")
        .args([
            "-fL",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "5",
            "--max-time",
            "120",
        ])
        .arg("-o")
        .arg(&tmp)
        .arg(url)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn curl: {e}"))?;
    let mut stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = child.kill();
            let _ = std::fs::remove_file(&tmp);
            return Err("curl stderr unavailable".to_string());
        }
    };
    let mut chunk = [0u8; 4096];
    let mut pending = String::new();
    let mut last = 0u8;
    loop {
        let n = stderr
            .read(&mut chunk)
            .map_err(|e| format!("read curl progress: {e}"))?;
        if n == 0 {
            break;
        }
        pending.push_str(&String::from_utf8_lossy(&chunk[..n]));
        while let Some(at) = pending.find(['\r', '\n']) {
            let line = pending[..at].to_string();
            pending.drain(..=at);
            if let Some(pct) = parse_percent(&line) {
                if pct != last {
                    last = pct;
                    progress(pct);
                }
            }
        }
    }
    if let Some(pct) = parse_percent(&pending) {
        if pct != last {
            progress(pct);
        }
    }
    let status = child.wait().map_err(|e| format!("wait curl: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("curl exited {status}"));
    }
    let bytes = match std::fs::read(&tmp) {
        Ok(bytes) => bytes,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("read download: {e}"));
        }
    };
    if bytes.len() < 10_000 || !is_font(&bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err("download is not a font".to_string());
    }
    let moved = std::fs::rename(&tmp, &dest);
    let _ = std::fs::remove_file(&tmp);
    moved.map_err(|e| format!("rename {}: {e}", dest.display()))
}

/// Build [`egui::FontDefinitions`] from already-read face bytes.
fn defs_from_faces(regular: Vec<u8>, bold: Option<Vec<u8>>) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    defs.font_data.insert(
        REGULAR.to_owned(),
        Arc::new(egui::FontData::from_owned(regular)),
    );
    // List the Bold face as its own family so bold runs get a real bold
    // weight rather than the Regular face plus a darker colour.
    let faces = [REGULAR.to_owned()];
    // Registered even without the Bold file, so `bold_font` always resolves;
    // it then falls back to Regular.
    let mut bold_family = Vec::new();
    if let Some(b) = bold {
        defs.font_data
            .insert(BOLD.to_owned(), Arc::new(egui::FontData::from_owned(b)));
        bold_family.push(BOLD.to_owned());
    }
    // Script fallbacks (CJK, Arabic) sit after FiraCode so missing glyphs
    // resolve instead of rendering tofu; absent files keep egui's defaults.
    let mut fallbacks: Vec<String> = Vec::new();
    for file in FALLBACK_FILES {
        let Some(path) = FALLBACK_DIRS
            .iter()
            .map(|dir| Path::new(dir).join(file))
            .find(|path| path.is_file())
        else {
            continue;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let name = file.trim_end_matches(".ttc").trim_end_matches(".ttf");
        defs.font_data
            .insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        fallbacks.push(name.to_owned());
    }
    bold_family.push(REGULAR.to_owned());
    bold_family.extend(fallbacks.iter().cloned());
    bold_family.extend(
        defs.families[&egui::FontFamily::Proportional]
            .iter()
            .cloned(),
    );
    defs.families
        .insert(egui::FontFamily::Name(BOLD_FAMILY.into()), bold_family);

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = defs.families.entry(family).or_default();
        let defaults = std::mem::take(list);
        list.extend(faces.iter().cloned());
        list.extend(fallbacks.iter().cloned());
        list.extend(defaults);
    }
    defs
}

/// Install the best available font once at startup. Temporary bridge until the
/// app owns cache/fallback selection; a missing font leaves egui's defaults.
pub fn install(ctx: &egui::Context) {
    if let Some(defs) = cached(&crate::mermaid::cache_root()).or_else(fallback) {
        ctx.set_fonts(defs);
    }
}

/// Build [`egui::FontDefinitions`] from the Regular (and, if present, Bold)
/// faces in `dir`. FiraCode becomes the primary for both `Proportional` and
/// `Monospace`; the built-in egui fonts stay as fallbacks. `None` when the
/// Regular face is missing or unreadable.
pub fn load_from_dir(dir: &Path) -> Option<egui::FontDefinitions> {
    let regular = std::fs::read(dir.join(face(REGULAR))).ok()?;
    let bold = std::fs::read(dir.join(face(BOLD))).ok();
    Some(defs_from_faces(regular, bold))
}

/// Search order for a locally installed copy: `~/.fonts`,
/// `$XDG_DATA_HOME/fonts`, `~/.local/share/fonts`, `/usr/share/fonts/truetype`.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var_os("HOME");
    if let Some(h) = &home {
        dirs.push(PathBuf::from(h).join(".fonts"));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(xdg).join("fonts"));
    }
    if let Some(h) = &home {
        dirs.push(PathBuf::from(h).join(".local/share/fonts"));
    }
    dirs.push(PathBuf::from("/usr/share/fonts/truetype"));
    dirs
}

#[cfg(test)]
mod tests {
    mod fonts {
        use super::super::*;

        #[test]
        fn finds_the_regular_face_on_this_machine() {
            let defs = fallback().expect("FiraCode Nerd Font is installed under ~/.fonts");
            assert!(defs.font_data.keys().any(|k| k.contains("FiraCode")));
            assert_eq!(
                defs.families[&egui::FontFamily::Proportional]
                    .first()
                    .unwrap(),
                "FiraCodeNerdFont-Regular"
            );
            assert_eq!(
                defs.families[&egui::FontFamily::Monospace].first().unwrap(),
                "FiraCodeNerdFont-Regular"
            );
        }

        #[test]
        fn fallbacks_cover_cjk_and_arabic() {
            let defs = fallback().expect("font installed");
            let proportional = &defs.families[&egui::FontFamily::Proportional];
            assert!(
                proportional.iter().any(|name| name.contains("CJK")),
                "CJK fallback missing: {proportional:?}"
            );
            assert!(
                proportional.iter().any(|name| name.contains("Arabic")),
                "Arabic fallback missing: {proportional:?}"
            );
        }

        #[test]
        fn missing_files_return_none_instead_of_panicking() {
            assert!(load_from_dir(std::path::Path::new("/nonexistent/fonts")).is_none());
        }

        #[test]
        fn bold_face_has_its_own_family_after_regular() {
            let defs = fallback().expect("font installed");
            let mono = &defs.families[&egui::FontFamily::Monospace];
            assert_eq!(mono[0], "FiraCodeNerdFont-Regular");
            let bold = &defs.families[&egui::FontFamily::Name(BOLD_FAMILY.into())];
            assert_eq!(bold[0], "FiraCodeNerdFont-Bold");
            assert_eq!(bold[1], "FiraCodeNerdFont-Regular");
        }

        #[test]
        fn bold_font_selects_the_bold_family() {
            assert!(is_bold(&bold_font(14.0)));
            assert!(!is_bold(&egui::FontId::proportional(14.0)));
            assert!(!is_bold(&egui::FontId::monospace(14.0)));
        }

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
    }
}
