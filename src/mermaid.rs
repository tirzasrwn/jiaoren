use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const RENDER_VERSION: &str = "1-mermaid11.12.0-htmlLabelsOff-2x";

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

    pub fn png_path(&self, id: &str, theme: Theme) -> PathBuf {
        self.cache_dir.join(format!("{id}-{}-{RENDER_VERSION}.png", theme.as_str()))
    }

    pub fn err_path(&self, id: &str, theme: Theme) -> PathBuf {
        self.cache_dir.join(format!("{id}-{}-{RENDER_VERSION}.err", theme.as_str()))
    }

    pub fn cached(&self, id: &str, theme: Theme) -> Option<Result<PathBuf, String>> {
        let png = self.png_path(id, theme);
        if png.exists() {
            return Some(Ok(png));
        }
        let err = self.err_path(id, theme);
        if err.exists() {
            let msg = fs::read_to_string(&err).unwrap_or_else(|_| "render failed".to_string());
            return Some(Err(msg));
        }
        None
    }

    pub fn render(&self, id: &str, source: &str, theme: Theme) -> Result<PathBuf, String> {
        let png = self.png_path(id, theme);
        if png.exists() {
            return Ok(png);
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

        let svg = match slice_svg(&stdout) {
            Some(s) => s,
            None => {
                let msg = "no <svg> in chromium output".to_string();
                self.write_err(id, theme, &msg);
                return Err(msg);
            }
        };

        let bytes = rasterize(&svg).map_err(|e| {
            self.write_err(id, theme, &e);
            e
        })?;
        self.atomic_write(&png, &bytes).map_err(|e| format!("write png: {e}"))?;
        Ok(png)
    }

    fn write_err(&self, id: &str, theme: Theme, msg: &str) {
        let p = self.err_path(id, theme);
        self.atomic_write(&p, msg.as_bytes()).ok();
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        // Append rather than replace the extension so .png and .err of the
        // same id get distinct tmp names.
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, path)
    }

    fn ensure_mermaid_js(&self) -> Result<(), String> {
        if self.mermaid_js.exists() {
            return Ok(());
        }
        let tmp = PathBuf::from(format!("{}.tmp", self.mermaid_js.display()));
        let status = Command::new("curl")
            .arg("-fsSL")
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
            Err("failed to download mermaid.min.js".to_string())
        }
    }
}

fn dirs_cache() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| std::env::temp_dir())
}

#[allow(dead_code)] // consumed by app.rs in later tasks
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
    entries.sort_by(|a, b| b.cmp(a)); // descending: newest version first
    for e in entries {
        let chrome = e.join("chrome-linux64/chrome");
        if chrome.exists() {
            return Some(chrome);
        }
    }
    None
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(name);
        if cand.is_file() {
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

fn file_url(path: &Path) -> String {
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
mermaid.initialize({{startOnLoad:false, htmlLabels:false, theme:\"{theme}\", fontFamily:\"sans-serif\",\n\
  flowchart:{{htmlLabels:false}}, securityLevel:\"loose\"}});\n\
mermaid.render(\"d\", document.getElementById(\"src\").textContent)\n\
  .then(r => {{ document.getElementById(\"out\").innerHTML = r.svg; }})\n\
  .catch(e => {{ document.getElementById(\"out\").textContent = \"ERR:\" + e.message; }});\n\
</script>\n",
        theme = theme.as_str()
    )
}

fn slice_svg(stdout: &str) -> Option<&str> {
    let start = stdout.find("<svg")?;
    let end = stdout.rfind("</svg>")? + "</svg>".len();
    Some(&stdout[start..end])
}

fn rasterize(svg: &str) -> Result<Vec<u8>, String> {
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &opt).map_err(|e| format!("parse svg: {e}"))?;
    let size = tree.size();
    let (w, h) = (size.width().ceil() as u32, size.height().ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w * 2, h * 2)
        .ok_or_else(|| "pixmap alloc failed".to_string())?;
    resvg::render(
        &tree,
        resvg::usvg::Transform::from_scale(2.0, 2.0),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| format!("encode png: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html_metacharacters() {
        assert_eq!(html_escape("A-->B & C<D>\"E\""), "A--&gt;B &amp; C&lt;D&gt;\"E\"");
        assert_eq!(html_escape("</pre>"), "&lt;/pre&gt;");
    }

    fn renderer(cache: &Path) -> Renderer {
        Renderer { chromium: None, mermaid_js: cache.join("mermaid.min.js"), cache_dir: cache.to_path_buf() }
    }

    #[test]
    fn cache_paths_include_theme_and_render_version() {
        let d = std::env::temp_dir().join("mdview-test-paths");
        let r = renderer(&d);
        let p = r.png_path("abc", Theme::Dark);
        assert_eq!(p.file_name().unwrap().to_str().unwrap(), format!("abc-dark-{RENDER_VERSION}.png"));
        assert_ne!(p, r.png_path("abc", Theme::Light));
    }

    #[test]
    fn cached_reports_missing_failure_and_success() {
        let d = std::env::temp_dir().join(format!("mdview-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let r = renderer(&d);
        assert!(r.cached("k", Theme::Light).is_none());
        std::fs::write(r.err_path("k", Theme::Light), "boom").unwrap();
        assert!(matches!(r.cached("k", Theme::Light), Some(Err(_))));
        std::fs::write(r.png_path("k", Theme::Light), b"png").unwrap();
        assert!(matches!(r.cached("k", Theme::Light), Some(Ok(_))));
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
    #[ignore]
    fn renders_real_diagram() {
        let r = Renderer::new();
        let p = r.render("smoke", "graph TD; A-->B;", Theme::Light).unwrap();
        assert!(p.exists());
        assert!(fs::metadata(&p).unwrap().len() > 1000);
    }
}
