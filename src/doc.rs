#![allow(dead_code)] // consumed by app.rs in later tasks

use std::collections::HashMap;
use std::path::Path;

pub struct Prepared {
    pub markdown: String,
    pub diagrams: HashMap<String, String>,
}

pub fn diagram_id(source: &str) -> String {
    let hex = blake3::hash(source.trim().as_bytes()).to_hex();
    hex.as_str()[..16].to_string()
}

pub fn prepare(md: &str, doc_dir: &Path) -> Prepared {
    let mut out = String::with_capacity(md.len());
    let mut diagrams = HashMap::new();
    // (fence char, run length, info string, verbatim open line)
    let mut open: Option<(char, usize, String, String)> = None;
    let mut body = String::new();

    for line in md.split_inclusive('\n') {
        let t = line.trim();
        match open.take() {
            None => match open_fence(t) {
                Some((ch, len, info)) => {
                    open = Some((ch, len, info.to_string(), line.to_string()));
                }
                None => out.push_str(&rewrite_images(line, doc_dir)),
            },
            Some((ch, len, info, oline)) => {
                if is_close(t, ch, len) {
                    if info.trim() == "mermaid" {
                        let id = diagram_id(&body);
                        diagrams.insert(id.clone(), body.trim().to_string());
                        out.push_str(&format!("<mermaid id=\"{id}\">\n"));
                    } else {
                        out.push_str(&oline);
                        out.push_str(&body);
                        out.push_str(line);
                    }
                    body.clear();
                } else {
                    body.push_str(line);
                    open = Some((ch, len, info, oline));
                }
            }
        }
    }
    // Unclosed fence at EOF: emit as ordinary fenced text, never drop content.
    if let Some((_, _, _, oline)) = open {
        out.push_str(&oline);
        out.push_str(&body);
    }

    Prepared { markdown: out, diagrams }
}

fn open_fence(t: &str) -> Option<(char, usize, &str)> {
    let ch = t.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = t.chars().take_while(|&c| c == ch).count();
    if len < 3 {
        return None;
    }
    Some((ch, len, &t[len..]))
}

fn is_close(t: &str, ch: char, open_len: usize) -> bool {
    t.chars().all(|c| c == ch) && t.chars().count() >= open_len
}

fn rewrite_images(mut rest: &str, doc_dir: &Path) -> String {
    let mut out = String::with_capacity(rest.len());
    while let Some(start) = rest.find("![") {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let mid = match rest.find("](") {
            Some(j) => j,
            None => break,
        };
        let end = match rest[mid..].find(')') {
            Some(k) => mid + k,
            None => break,
        };
        let alt = &rest[2..mid];
        let target = &rest[mid + 2..end];
        let skip = target.is_empty()
            || target.contains('\n')
            || target.starts_with('/')
            || target.starts_with('#')
            || target.starts_with("data:")
            || target.contains("://");
        if skip {
            out.push_str("![");
            rest = &rest[2..];
            continue;
        }
        out.push_str(&format!("![{alt}](file://{})", doc_dir.join(target).display()));
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_mermaid_and_leaves_other_code() {
        let md = "text\n\n```mermaid\ngraph TD; A-->B;\n```\n\n```rust\nfn x() {}\n```\n";
        let p = prepare(md, Path::new("/docs"));
        assert_eq!(p.diagrams.len(), 1);
        let id = diagram_id("graph TD; A-->B;");
        assert!(p.markdown.contains(&format!("<mermaid id=\"{id}\">")));
        assert!(p.markdown.contains("```rust\nfn x() {}"));
        assert!(!p.markdown.contains("```mermaid"));
    }

    #[test]
    fn id_is_stable_and_content_sensitive() {
        assert_eq!(diagram_id("graph TD; A-->B;"), diagram_id("graph TD; A-->B;"));
        assert_ne!(diagram_id("graph TD; A-->B;"), diagram_id("graph TD; A-->C;"));
        assert_eq!(diagram_id("x").len(), 16);
    }

    #[test]
    fn rewrites_relative_images_only() {
        let md = "![a](img/x.png) ![b](/abs.png) ![c](https://h/c.png)";
        let out = prepare(md, Path::new("/docs")).markdown;
        assert!(out.contains("![a](file:///docs/img/x.png)"));
        assert!(out.contains("![b](/abs.png)"));
        assert!(out.contains("![c](https://h/c.png)"));
    }
}
