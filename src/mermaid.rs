//! Locate Chromium, download mermaid.js once, and cache diagram renders as SVG.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const RENDER_VERSION: &str = "5-mermaid11.12.0-htmlLabelsOff-2x-strict-svg-catppuccin-firacode";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
}

pub struct Renderer {
    pub chromium: Option<PathBuf>,
    pub mermaid_js: PathBuf,
    pub cache_dir: PathBuf,
}

impl Renderer {
    pub fn new() -> Self {
        let cache_dir = dirs_cache().join("mdview");
        fs::create_dir_all(&cache_dir).ok();
        let mermaid_js = std::env::var_os("MDVIEW_MERMAID_JS")
            .map(PathBuf::from)
            .unwrap_or_else(|| cache_dir.join("mermaid.min.js"));
        Renderer { chromium: None, mermaid_js, cache_dir }
    }

    pub fn svg_path(&self, id: &str, theme: Theme) -> PathBuf {
        self.cache_dir.join(format!("{id}-{}-{RENDER_VERSION}.svg", theme.as_str()))
    }

    pub fn err_path(&self, id: &str, theme: Theme) -> PathBuf {
        self.cache_dir.join(format!("{id}-{}-{RENDER_VERSION}.err", theme.as_str()))
    }

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

    pub fn render(&self, id: &str, source: &str, theme: Theme) -> Result<PathBuf, String> {
        let svg = self.svg_path(id, theme);
        if svg.exists() {
            return Ok(svg);
        }
        let err = self.err_path(id, theme);
        if err.exists() {
            let msg = fs::read_to_string(&err).unwrap_or_else(|_| "render failed".to_string());
            return Err(msg);
        }

        let chromium = self
            .chromium
            .clone()
            .or_else(find_chromium)
            .ok_or_else(|| "chromium not found; set MDVIEW_CHROMIUM".to_string())?;

        self.ensure_mermaid_js()?;

        let html = self.cache_dir.join(format!("{id}-{}-{RENDER_VERSION}.html", theme.as_str()));
        let page = build_html(&self.mermaid_js, source, theme);
        fs::write(&html, &page).map_err(|e| format!("write html: {e}"))?;

        let out = Command::new(&chromium)
            .arg("--headless")
            .arg("--no-sandbox")
            .arg("--disable-gpu")
            // No network egress from the render page; diagram source is untrusted.
            .arg("--host-resolver-rules=MAP * ~NOTFOUND")
            .arg("--dump-dom")
            .arg("--virtual-time-budget=5000")
            .arg(format!("file://{}", html.display()))
            .output()
            .map_err(|e| format!("spawn chromium {}: {e}", chromium.display()))?;

        let _ = fs::remove_file(&html);

        if !out.status.success() {
            let msg = format!(
                "chromium exited {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
            self.write_err(id, theme, &msg);
            return Err(msg);
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        // The script source itself contains the literal "ERR:", so look only
        // at the output div's rendered text.
        if let Some(pos) = stdout.find("id=\"out\">ERR:") {
            let msg = stdout[pos..].split("</div>").next().unwrap_or("mermaid error").to_string();
            self.write_err(id, theme, &msg);
            return Err(msg);
        }

        let svg_text = match slice_svg(&stdout) {
            Some(s) => s,
            None => {
                let msg = "no <svg> in chromium output".to_string();
                self.write_err(id, theme, &msg);
                return Err(msg);
            }
        };

        self.atomic_write(&svg, svg_text.as_bytes()).map_err(|e| format!("write svg: {e}"))?;
        Ok(svg)
    }

    fn write_err(&self, id: &str, theme: Theme, msg: &str) {
        let p = self.err_path(id, theme);
        self.atomic_write(&p, msg.as_bytes()).ok();
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        // Append rather than replace the extension so .svg and .err of the
        // same id get distinct tmp names.
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, path)
    }

    pub(crate) fn ensure_mermaid_js(&self) -> Result<(), String> {
        if self.mermaid_js.exists() {
            return Ok(());
        }
        let tmp = PathBuf::from(format!("{}.tmp", self.mermaid_js.display()));
        let status = Command::new("curl")
            .arg("-fsSL")
            .arg("--connect-timeout")
            .arg("5")
            .arg("--max-time")
            .arg("60")
            .arg("-o")
            .arg(&tmp)
            .arg("https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.min.js")
            .status()
            .map_err(|e| format!("spawn curl: {e}"))?;
        if status.success() {
            fs::rename(&tmp, &self.mermaid_js).map_err(|e| format!("rename mermaid js: {e}"))?;
            Ok(())
        } else {
            let _ = fs::remove_file(&tmp);
            Err("failed to download mermaid.min.js (network needed on first run only; set MDVIEW_MERMAID_JS to use an existing copy)".to_string())
        }
    }
}

fn dirs_cache() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
}

pub fn find_chromium() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MDVIEW_CHROMIUM") {
        let p = PathBuf::from(p);
        return p.exists().then_some(p);
    }
    for name in ["chromium", "google-chrome", "chrome"] {
        if let Some(p) = find_in_path(name) {
            return Some(p);
        }
    }
    let base = dirs_cache().join("ms-playwright");
    let mut entries: Vec<PathBuf> = fs::read_dir(&base)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("chromium-"))
                .unwrap_or(false)
        })
        .collect();
    // Descending numeric suffix: chromium-1255 is newer than chromium-999.
    entries.sort_by_key(|p| {
        std::cmp::Reverse(
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("chromium-"))
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0),
        )
    });
    for e in entries {
        let chrome = e.join("chrome-linux64/chrome");
        if chrome.exists() {
            return Some(chrome);
        }
    }
    None
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
// $MDVIEW_MERMAID_JS override outside cache_dir still loads.
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
    if let (Some(w), Some(h)) = (tag_attr(tag, "width").and_then(px_value),
                                 tag_attr(tag, "height").and_then(px_value))
    {
        return Some((w, h));
    }
    let vb = tag_attr(tag, "viewBox")?;
    let nums: Vec<&str> = vb.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
    if nums.len() < 4 {
        return None;
    }
    let w = px_value(nums[2]);
    let h = px_value(nums[3]);
    Some((w?, h?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html_metacharacters() {
        assert_eq!(html_escape("A-->B & C<D>\"E\""), "A--&gt;B &amp; C&lt;D&gt;\"E\"");
        assert_eq!(html_escape("</pre>"), "&lt;/pre&gt;");
    }

    fn test_renderer(cache: &Path) -> Renderer {
        Renderer { chromium: None, mermaid_js: cache.join("mermaid.min.js"), cache_dir: cache.to_path_buf() }
    }

    #[test]
    fn cache_paths_include_theme_and_render_version() {
        let d = std::env::temp_dir().join("mdview-test-paths");
        let r = test_renderer(&d);
        let p = r.svg_path("abc", Theme::Dark);
        assert_eq!(p.file_name().unwrap().to_str().unwrap(), format!("abc-dark-{RENDER_VERSION}.svg"));
        assert_ne!(p, r.svg_path("abc", Theme::Light));
    }

    #[test]
    fn cached_reports_missing_failure_and_success() {
        let d = std::env::temp_dir().join(format!("mdview-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let r = test_renderer(&d);
        assert!(r.cached("k", Theme::Light).is_none());
        std::fs::write(r.err_path("k", Theme::Light), "boom").unwrap();
        assert!(matches!(r.cached("k", Theme::Light), Some(Err(_))));
        std::fs::write(r.svg_path("k", Theme::Light), b"svg").unwrap();
        assert!(matches!(r.cached("k", Theme::Light), Some(Ok(_))));
    }

    #[test]
    fn svg_path_uses_svg_extension_and_version() {
        let r = test_renderer(Path::new("/tmp/x"));
        let p = r.svg_path("abc", Theme::Light);
        assert!(p.to_string_lossy().ends_with(&format!("abc-light-{RENDER_VERSION}.svg")));
    }

    #[test]
    fn parses_width_and_height() {
        assert_eq!(parse_svg_size(r#"<svg width="120" height="40" viewBox="0 0 120 40">"#), Some((120.0, 40.0)));
        assert_eq!(parse_svg_size(r#"<svg viewBox="0 0 120 40">"#), Some((120.0, 40.0)));
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
        assert_eq!(parse_svg_size(r#"<svg id="d" width="100%" xmlns="u" style="max-width: 87px;" viewBox="0 0 87 160"><style>#d{font-size:16px}</style></svg>"#), Some((87.0, 160.0)));
        // stroke-width must not be picked up as width.
        assert_eq!(parse_svg_size(r#"<svg stroke-width="2" viewBox="0 0 5 6">"#), Some((5.0, 6.0)));
        // Garbage must not panic.
        assert_eq!(parse_svg_size("<<svg"), None);
        assert_eq!(parse_svg_size(r#"<svg width="auto" height="100%" viewBox="0 0">"#), None);
    }

    #[test]
    fn env_override_wins_for_chromium() {
        std::env::set_var("MDVIEW_CHROMIUM", "/bin/true");
        assert_eq!(find_chromium(), Some(PathBuf::from("/bin/true")));
        std::env::remove_var("MDVIEW_CHROMIUM");
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
        let d = std::env::temp_dir().join(format!("mdview-js-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let r = test_renderer(&d);
        fs::write(&r.mermaid_js, b"// already here").unwrap();
        r.ensure_mermaid_js().unwrap();
        let first = fs::metadata(&r.mermaid_js).unwrap().modified().unwrap();
        let first_bytes = fs::read(&r.mermaid_js).unwrap();
        // A second call must not download or rewrite the file.
        r.ensure_mermaid_js().unwrap();
        assert_eq!(first, fs::metadata(&r.mermaid_js).unwrap().modified().unwrap());
        assert_eq!(first_bytes, fs::read(&r.mermaid_js).unwrap());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    #[ignore]
    fn renders_real_diagram() {
        let r = Renderer::new();
        let p = r.render("smoke", "graph TD; A-->B;", Theme::Light).unwrap();
        assert!(p.exists());
        assert!(fs::metadata(&p).unwrap().len() > 1000);
    }

    #[test]
    #[ignore]
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
