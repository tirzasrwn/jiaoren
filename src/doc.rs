use std::collections::HashMap;
use std::path::Path;

use crate::mermaid::file_url;

#[allow(dead_code)] // fields consumed by app.rs in later tasks
pub struct Prepared {
    #[allow(dead_code)] // read by app.rs render pass
    pub markdown: String,
    #[allow(dead_code)] // read by app.rs render pass
    pub diagrams: HashMap<String, String>,
    #[allow(dead_code)] // rendered by app.rs in a later task
    pub tables: HashMap<String, TableSpec>,
}

/// A top-level GFM table, parsed out of the Markdown before rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSpec {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[allow(dead_code)] // consumed by app.rs in later tasks
pub fn diagram_id(source: &str) -> String {
    let hex = blake3::hash(source.trim().as_bytes()).to_hex();
    hex.as_str()[..16].to_string()
}

#[allow(dead_code)] // consumed by app.rs in later tasks
pub fn table_id(spec: &TableSpec) -> String {
    let mut canon = spec.header.join("\u{1f}");
    for row in &spec.rows {
        canon.push('\u{1e}');
        canon.push_str(&row.join("\u{1f}"));
    }
    let hex = blake3::hash(canon.as_bytes()).to_hex();
    hex.as_str()[..16].to_string()
}

#[allow(dead_code)] // consumed by app.rs in later tasks
pub fn prepare(md: &str, doc_dir: &Path) -> Prepared {
    let mut out = String::with_capacity(md.len());
    let mut diagrams = HashMap::new();
    let mut tables = HashMap::new();
    // (fence char, run length, info string, verbatim open line)
    let mut open: Option<(char, usize, String, String)> = None;
    let mut body = String::new();
    let lines: Vec<&str> = md.split_inclusive('\n').collect();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        match open.take() {
            None => match open_fence(t) {
                Some((ch, len, info)) => {
                    open = Some((ch, len, info.to_string(), line.to_string()));
                    i += 1;
                }
                None => {
                    // Table block: consecutive `|`-lines starting at a top-level
                    // pipe-first line. ponytail: GFM headers without a leading
                    // pipe (`a | b`) are not detected; add when seen in the wild.
                    let mut j = i + 1;
                    if !line.starts_with(char::is_whitespace) && t.starts_with('|') {
                        while j < lines.len() && lines[j].trim().contains('|') {
                            j += 1;
                        }
                        let block: Vec<&str> = lines[i..j].iter().map(|l| l.trim()).collect();
                        if let Some(spec) = parse_table_block(&block) {
                            let id = table_id(&spec);
                            tables.insert(id.clone(), spec);
                            // Trailing blank line keeps a following paragraph out
                            // of the placeholder's HTML block, as with mermaid.
                            out.push_str(&format!("<mdtable id=\"{id}\">\n\n"));
                            i = j;
                            continue;
                        }
                    }
                    // Not a table (or malformed delimiter): emit as before.
                    out.push_str(&rewrite_images(line, doc_dir));
                    i += 1;
                }
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
                i += 1;
            }
        }
    }
    // Unclosed fence at EOF: emit as ordinary fenced text, never drop content.
    if let Some((_, _, _, oline)) = open {
        out.push_str(&oline);
        out.push_str(&body);
    }

    Prepared {
        markdown: out,
        diagrams,
        tables,
    }
}

/// Parse a full table block (header, delimiter row, then data rows) into a
/// `TableSpec`. The delimiter row must have the same cell count as the header
/// and each cell must match `:?-{1,}:?`. Malformed blocks return `None`, so
/// the caller can fall back to emitting the lines verbatim.
pub fn parse_table_block(lines: &[&str]) -> Option<TableSpec> {
    if lines.len() < 2 {
        return None;
    }
    let header = split_row(lines[0]);
    let ncols = header.len();
    let delimiter = split_row(lines[1]);
    if delimiter.len() != ncols || delimiter.iter().any(|c| !is_delim_cell(c)) {
        return None;
    }
    let mut rows = Vec::with_capacity(lines.len() - 2);
    for line in &lines[2..] {
        let mut cells = split_row(line);
        while cells.len() < ncols {
            cells.push(String::new());
        }
        // Extra cells beyond the header are kept, never dropped.
        rows.push(cells);
    }
    Some(TableSpec { header, rows })
}

/// Split one table row into trimmed cells. Only unescaped `|` splits; a
/// leading and one trailing (unescaped) `|` wrap the cells.
fn split_row(line: &str) -> Vec<String> {
    let mut s = line.trim();
    if s.ends_with('|') {
        // A pipe preceded by an odd number of backslashes is escaped.
        let bytes = s.as_bytes();
        let mut slashes = 0;
        let mut k = bytes.len() - 1;
        while k > 0 && bytes[k - 1] == b'\\' {
            slashes += 1;
            k -= 1;
        }
        if slashes % 2 == 0 {
            s = &s[..s.len() - 1];
        }
    }
    if let Some(rest) = s.strip_prefix('|') {
        // A leading pipe is unambiguous and simply wraps the row.
        s = rest;
    }
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut slashes = 0usize;
    for c in s.chars() {
        if c == '|' && slashes % 2 == 0 {
            cells.push(cur.trim().to_string());
            cur.clear();
            slashes = 0;
        } else if c == '\\' {
            cur.push(c);
            slashes += 1;
        } else {
            cur.push(c);
            slashes = 0;
        }
    }
    cells.push(cur.trim().to_string());
    cells.iter_mut().for_each(|c| *c = c.replace("\\|", "|"));
    cells
}

fn is_delim_cell(c: &str) -> bool {
    let core = c.strip_prefix(':').unwrap_or(c);
    let core = core.strip_suffix(':').unwrap_or(core);
    !core.is_empty() && core.chars().all(|ch| ch == '-')
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

    #[test]
    fn converts_a_long_table_to_a_placeholder() {
        let md = "| a | b |\n|---|---|\n| satu | dua |\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.tables.len(), 1);
        let id = p.tables.keys().next().unwrap();
        assert!(p.markdown.contains(&format!("<mdtable id=\"{id}\">")));
        assert!(!p.markdown.contains("| a |"));
    }

    #[test]
    fn keeps_escaped_pipes_and_uneven_rows() {
        let spec = parse_table_block(&["| a \\| b | c |", "|---|---|", "| 1 |"]).unwrap();
        assert_eq!(spec.header, vec!["a | b".to_string(), "c".to_string()]);
        assert_eq!(spec.rows, vec![vec!["1".to_string(), "".to_string()]]);
    }

    #[test]
    fn leaves_tables_inside_code_alone() {
        let md = "```\n| a | b |\n|---|---|\n```\n";
        let p = prepare(md, Path::new("/d"));
        assert!(p.tables.is_empty());
    }

    #[test]
    fn table_at_eof_without_trailing_newline_extracts() {
        let p = prepare("| a | b |\n|---|---|", Path::new("/d"));
        assert_eq!(p.tables.len(), 1);
        assert!(p.markdown.contains("<mdtable"));
    }

    #[test]
    fn table_after_heading_extracts_and_neighbours_survive() {
        let md = "# h\n\n| a | b |\n|---|---|\n| c | d |\n\nafter\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.tables.len(), 1);
        assert!(p.markdown.contains("# h"));
        assert!(p.markdown.contains("after"));
        assert!(!p.markdown.contains("| a |"));
    }

    #[test]
    fn two_tables_in_a_row_extract() {
        let md = "| a |\n|---|\n| 1 |\n\n| b |\n|---|\n| 2 |\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.tables.len(), 2);
    }

    #[test]
    fn pipes_in_paragraph_list_quote_indent_stay_verbatim() {
        let out = prepare("a | b alone\n", Path::new("/d")).markdown;
        assert!(out.contains("a | b alone"));
        let md = "- | a | b |\n  |---|---|\n> | q |\n\n    | code |\n";
        let p = prepare(md, Path::new("/d"));
        assert!(p.tables.is_empty());
        assert!(p.markdown.contains("- | a | b |"));
    }

    #[test]
    fn malformed_delimiter_falls_back_to_verbatim() {
        let md = "| a | b |\n|---|\n| c |\n";
        let p = prepare(md, Path::new("/d"));
        assert!(p.tables.is_empty());
        assert!(p.markdown.contains("| a | b |"));
        assert!(p.markdown.contains("|---|"));
    }

    #[test]
    fn crlf_table_extracts() {
        let md = "| a |\r\n|---|\r\n| b |\r\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.tables.len(), 1);
        let spec = p.tables.values().next().unwrap();
        assert_eq!(spec.header, vec!["a".to_string()]);
        assert_eq!(spec.rows, vec![vec!["b".to_string()]]);
    }

    #[test]
    fn extra_cells_are_kept_not_dropped() {
        let spec = parse_table_block(&["| a | b |", "|---|---|", "| 1 | 2 | 3 |"]).unwrap();
        assert_eq!(
            spec.rows,
            vec![vec!["1".to_string(), "2".to_string(), "3".to_string()]]
        );
    }
}
