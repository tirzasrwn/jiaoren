use std::collections::HashMap;
use std::path::Path;

use crate::mermaid::file_url;

#[allow(dead_code)] // fields consumed by app.rs in later tasks
pub struct Prepared {
    #[allow(dead_code)] // read by app.rs render pass
    pub markdown: String,
    #[allow(dead_code)] // read by app.rs render pass
    pub diagrams: HashMap<String, String>,
}

#[allow(dead_code)] // consumed by app.rs in later tasks
pub fn diagram_id(source: &str) -> String {
    let hex = blake3::hash(source.trim().as_bytes()).to_hex();
    hex.as_str()[..16].to_string()
}

#[allow(dead_code)] // consumed by app.rs in later tasks
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
                    if info.split_whitespace().next() == Some("mermaid") {
                        let id = diagram_id(&body);
                        diagrams.insert(id.clone(), body.trim().to_string());
                        // Trailing blank line terminates the CommonMark HTML block so
                        // a paragraph directly after the fence is not absorbed into it.
                        out.push_str(&format!("<mermaid id=\"{id}\">\n\n"));
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

/// Split the text after `](` into the link body and the index just past the
/// closing `)`. Parentheses are balanced, so `x(1).png` is one target; a
/// closing paren inside a `"title"` does not end the link.
fn link_inner(s: &str) -> Option<(&str, usize)> {
    let mut depth = 0usize;
    let mut in_quote = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quote = !in_quote,
            '(' if !in_quote => depth += 1,
            ')' if !in_quote => {
                if depth == 0 {
                    return Some((&s[..i], i + 1));
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
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
        let (inner, end) = match link_inner(&rest[mid + 2..]) {
            Some(v) => v,
            None => break,
        };
        let alt = &rest[2..mid];
        // Drop an optional `"title"` (and any other trailing link text).
        let target = inner.split_whitespace().next().unwrap_or("");
        let skip = target.is_empty()
            || target.starts_with('/')
            || target.starts_with('#')
            || target.starts_with("data:")
            || target.contains("://");
        if skip {
            out.push_str("![");
            rest = &rest[2..];
            continue;
        }
        out.push_str(&format!(
            "![{alt}](file://{})",
            file_url(&doc_dir.join(target))
        ));
        rest = &rest[mid + 2 + end..];
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

    #[test]
    fn balanced_parens_and_title_in_image_targets() {
        let md = "![a](x(1).png \"caption\") ![b](a#b.png)";
        let out = prepare(md, Path::new("/docs")).markdown;
        assert!(out.contains("![a](file:///docs/x(1).png)"));
        assert!(out.contains("![b](file:///docs/a%23b.png)"));
    }

    #[test]
    fn mermaid_fence_matches_first_info_token() {
        let md = "```mermaid title=\"x\"\ngraph TD; A-->B;\n```\n";
        let p = prepare(md, Path::new("/x"));
        assert_eq!(p.diagrams.len(), 1);
        assert!(p.markdown.contains("<mermaid"));
    }

    #[test]
    fn unclosed_mermaid_fence_at_eof_keeps_content() {
        // Data-loss guard: unclosed fence must be emitted as ordinary fenced text.
        let md = "text\n\n```mermaid\ngraph TD; A-->B;";
        let p = prepare(md, Path::new("/docs"));
        assert!(p.diagrams.is_empty());
        assert!(p.markdown.contains("```mermaid"));
        assert!(p.markdown.contains("graph TD; A-->B;"));
    }

    #[test]
    fn tilde_mermaid_fences_extract() {
        let md = "~~~mermaid\ngraph TD;\n~~~\n";
        let p = prepare(md, Path::new("/x"));
        assert_eq!(p.diagrams.len(), 1);
        assert!(p.markdown.contains("<mermaid"));
    }

    #[test]
    fn extracts_two_diagrams() {
        let md = "```mermaid\nA\n```\n\n```mermaid\nB\n```\n";
        let p = prepare(md, Path::new("/x"));
        assert_eq!(p.diagrams.len(), 2);
        assert!(!p.markdown.contains("```"));
    }

    #[test]
    fn handles_crlf_input() {
        let md = "```mermaid\r\ngraph TD;\r\n```\r\n";
        let p = prepare(md, Path::new("/x"));
        assert_eq!(p.diagrams.len(), 1);
        let id = diagram_id("graph TD;");
        assert!(p.markdown.contains(&format!("<mermaid id=\"{id}\">")));
    }

    #[test]
    fn paragraph_after_mermaid_fence_survives() {
        // A non-blank paragraph directly after the fence must not be swallowed by
        // the placeholder's HTML block; the placeholder ends with a blank line.
        let md = "```mermaid\ngraph TD; A-->B;\n```\nThis paragraph must survive.\n";
        let p = prepare(md, Path::new("/x"));
        let id = diagram_id("graph TD; A-->B;");
        assert!(p.markdown.contains(&format!("<mermaid id=\"{id}\">\n\n")));
        assert!(p.markdown.contains("This paragraph must survive."));
    }
}
