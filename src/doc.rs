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
    #[allow(dead_code)] // rendered by app.rs in a later task
    pub paragraphs: HashMap<String, String>,
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
pub fn paragraph_id(source: &str) -> String {
    let hex = blake3::hash(source.trim().as_bytes()).to_hex();
    hex.as_str()[..16].to_string()
}

/// True when `line` may open a top-level paragraph: it starts at column 0, is
/// non-blank, and carries no block marker the library must own.
pub fn is_paragraph_start(line: &str) -> bool {
    let t = line.trim_end_matches(['\r', '\n', ' ', '\t']);
    let first = match t.chars().next() {
        Some(c) => c,
        None => return false,
    };
    if first == ' ' || first == '\t' {
        return false; // indented: code block, list continuation, ...
    }
    if open_fence(t).is_some() || is_thematic_break(t) || is_setext_underline(t) {
        return false;
    }
    match first {
        '#' | '>' | '<' => return false,
        '-' | '*' | '+' => {
            let rest = &t[1..];
            if rest.is_empty() || rest.starts_with([' ', '\t']) {
                return false; // bullet (or a lone marker)
            }
        }
        _ => {}
    }
    // Ordered list marker: digits then `.`/`)` then space (or end of line).
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && digits < t.len() {
        let next = t.as_bytes()[digits];
        if matches!(next, b'.' | b')')
            && (digits + 1 == t.len()
                || matches!(t.as_bytes()[digits + 1], b' ' | b'\t' | b'\r'))
        {
            return false;
        }
    }
    // ponytail: pipe lines stay with the library (table detection ran first and
    // rejected this one) so an inline `|` is never half-claimed.
    !t.contains('|')
}

fn is_thematic_break(t: &str) -> bool {
    let marks: Vec<char> = t.chars().filter(|c| !c.is_whitespace()).collect();
    match marks.first() {
        Some(&m) if matches!(m, '-' | '*' | '_') => {
            marks.len() >= 3 && marks.iter().all(|&c| c == m)
        }
        _ => false,
    }
}

/// A setext underline: one or more `=` or `-` and nothing else (up to 3 spaces
/// of indent). It turns the preceding line into a heading, so the library must
/// own both lines instead of us extracting that line as a paragraph.
fn is_setext_underline(line: &str) -> bool {
    let t = line.trim();
    match t.chars().next() {
        Some('=') => t.chars().all(|c| c == '='),
        Some('-') => t.chars().all(|c| c == '-'),
        _ => false,
    }
}

/// A paragraph continuation may be indented 1–3 spaces (CommonMark allows a
/// hanging indent); 4+ spaces is code or a nested block, so the run ends there.
fn is_paragraph_continuation(line: &str) -> bool {
    let t = line.trim_end_matches(['\r', '\n', ' ', '\t']);
    let indent = t.len() - t.trim_start_matches(' ').len();
    match indent {
        0 => is_paragraph_start(t),
        1..=3 => is_paragraph_start(t.trim_start_matches(' ')),
        _ => false,
    }
}

/// Drop the 1–3 space hanging indent CommonMark allows on paragraph
/// continuation lines, so it never becomes a stray gap when the paragraph is
/// re-wrapped by the justified painter.
fn strip_hanging_indent(line: &str) -> &str {
    let n = line.len() - line.trim_start_matches(' ').len();
    &line[n.min(3)..]
}

#[allow(dead_code)] // consumed by app.rs in later tasks
pub fn prepare(md: &str, doc_dir: &Path) -> Prepared {
    let mut out = String::with_capacity(md.len());
    let mut diagrams = HashMap::new();
    let mut tables = HashMap::new();
    let mut paragraphs = HashMap::new();
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
                    // line (GFM headers may omit the leading pipe); block-level
                    // structures end the run. ponytail: an emphasis header like
                    // `*a | b*` counts as a boundary and is missed; loosen the
                    // marker set if that style shows up.
                    if !is_block_boundary(line) && t.contains('|') {
                        let mut j = i + 1;
                        while j < lines.len()
                            && lines[j].trim().contains('|')
                            && !is_block_boundary(lines[j])
                        {
                            j += 1;
                        }
                        let block: Vec<&str> = lines[i..j].iter().map(|l| l.trim()).collect();
                        if let Some(mut spec) = parse_table_block(&block) {
                            // Cells are ordinary Markdown: give them the same
                            // relative-image rewrite every other line gets.
                            for c in &mut spec.header {
                                *c = rewrite_images(c, doc_dir);
                            }
                            for row in &mut spec.rows {
                                for c in row {
                                    *c = rewrite_images(c, doc_dir);
                                }
                            }
                            let id = table_id(&spec);
                            tables.insert(id.clone(), spec);
                            // Blank line before the placeholder: type-7 HTML can
                            // never interrupt a paragraph, so separate it first.
                            if !out.is_empty() && !out.ends_with("\n\n") {
                                out.push('\n');
                            }
                            // Trailing blank line keeps a following paragraph out
                            // of the placeholder's HTML block, as with mermaid.
                            out.push_str(&format!("<mdtable id=\"{id}\">\n\n"));
                            i = j;
                            continue;
                        }
                    }
                    // Not a table (or malformed delimiter): emit as before.
                    // A paragraph is claimed only after table detection, so the
                    // two never cover the same lines.
                    if is_paragraph_start(line) {
                        let mut j = i + 1;
                        while j < lines.len() && is_paragraph_continuation(lines[j]) {
                            j += 1;
                        }
                        // A setext underline anywhere in/after the run makes the
                        // library's heading win: emit this line verbatim and let
                        // the underline end the block on the next iteration.
                        let setext = (j < lines.len() && is_setext_underline(lines[j]))
                            || lines[i + 1..j].iter().any(|l| is_setext_underline(l));
                        if setext {
                            out.push_str(&rewrite_images(line, doc_dir));
                            i += 1;
                            continue;
                        }
                        // CRLF safe: each line is trimmed at its end, then joined.
                        let joined = lines[i..j]
                            .iter()
                            .map(|l| strip_hanging_indent(l.trim_end()))
                            .collect::<Vec<_>>()
                            .join("\n");
                        let source = rewrite_images(&joined, doc_dir);
                        let id = paragraph_id(&source);
                        paragraphs.insert(id.clone(), source);
                        if !out.is_empty() && !out.ends_with("\n\n") {
                            out.push('\n');
                        }
                        // Same trailing blank line as tables: keeps a following
                        // paragraph out of the placeholder's HTML block.
                        out.push_str(&format!("<mdpara id=\"{id}\">\n\n"));
                        i = j;
                        continue;
                    }
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
        paragraphs,
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

/// A table run ends (and cannot start) on such a line: fenced code, headings,
/// blockquotes, list items, or indented code (≥4 spaces or a tab). A blank
/// line qualifies too; it simply has no `|`, so the run stops at it anyway.
fn is_block_boundary(line: &str) -> bool {
    let t = line.trim_start();
    if line.len() - t.len() >= 4 || line.starts_with('\t') {
        return true; // indented code
    }
    let t = t.trim_end();
    if open_fence(t).is_some() {
        return true; // ``` / ~~~ fence line (may carry an info string with `|`)
    }
    match t.as_bytes().first() {
        None => true,
        Some(b'#') | Some(b'>') => true,
        // Bullets stop a run — but a delimiter row (`--- | ---`) is part of the
        // table, not a bullet.
        Some(b'-') | Some(b'+') | Some(b'*') => {
            let cells = split_row(t);
            !(cells.len() > 1 && cells.iter().all(|c| is_delim_cell(c)))
        }
        Some(_) => {
            let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            digits > 0 && digits < t.len() && matches!(t.as_bytes()[digits], b'.' | b')')
        }
    }
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
        let p = prepare(md, Path::new("/docs"));
        // An image-only line is a paragraph now; its source is rewritten.
        let out = p.paragraphs.values().next().unwrap();
        assert!(out.contains("![a](file:///docs/img/x.png)"));
        assert!(out.contains("![b](/abs.png)"));
        assert!(out.contains("![c](https://h/c.png)"));
    }

    #[test]
    fn balanced_parens_and_title_in_image_targets() {
        let md = "![a](x(1).png \"caption\") ![b](a#b.png)";
        let p = prepare(md, Path::new("/docs"));
        let out = p.paragraphs.values().next().unwrap();
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
        assert!(p
            .paragraphs
            .values()
            .any(|src| src == "This paragraph must survive."));
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
        assert!(p.paragraphs.values().any(|src| src == "after"));
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

    #[test]
    fn pipeless_gfm_header_table_extracts() {
        let md = "a | b\n--- | ---\n1 | 2\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.tables.len(), 1);
        let spec = p.tables.values().next().unwrap();
        assert_eq!(spec.header, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(spec.rows, vec![vec!["1".to_string(), "2".to_string()]]);
        assert!(!p.markdown.contains("a | b"));
    }

    #[test]
    fn block_structures_stop_a_table_run() {
        let md = "| a | b |\n|---|---|\n| 1 | 2 |\n## h | x\n- list | item\n> q | r\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.tables.len(), 1);
        let spec = p.tables.values().next().unwrap();
        assert_eq!(spec.rows.len(), 1);
        assert!(p.markdown.contains("## h | x"));
        assert!(p.markdown.contains("- list | item"));
        assert!(p.markdown.contains("> q | r"));
        // Indented code after a row is a new block, not a data row.
        let md2 = "| a | b |\n|---|---|\n| 1 | 2 |\n    | code |\n";
        let p2 = prepare(md2, Path::new("/d"));
        assert_eq!(p2.tables.values().next().unwrap().rows.len(), 1);
        assert!(p2.markdown.contains("| code |"));
    }

    #[test]
    fn images_in_cells_get_file_url_rewrite() {
        let md = "| img | txt |\n|---|---|\n| ![x](rel.png) | y |\n";
        let p = prepare(md, Path::new("/docs"));
        let spec = p.tables.values().next().unwrap();
        assert_eq!(spec.rows[0][0], "![x](file:///docs/rel.png)");
        assert_eq!(spec.rows[0][1], "y");
    }

    #[test]
    fn shallowly_indented_table_extracts() {
        let md = "  | a | b |\n  |---|---|\n  | 1 | 2 |\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.tables.len(), 1);
    }

    #[test]
    fn placeholder_after_paragraph_gets_blank_line() {
        let md = "intro\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        let p = prepare(md, Path::new("/d"));
        // Both neighbours survive: the paragraph placeholder is followed by a
        // blank line, so the table placeholder starts its own HTML block.
        let id = p.paragraphs.keys().next().unwrap();
        assert_eq!(p.paragraphs.values().next().unwrap(), "intro");
        assert!(p
            .markdown
            .contains(&format!("<mdpara id=\"{id}\">\n\n<mdtable id=\"")));
    }

    #[test]
    fn extracts_a_top_level_paragraph() {
        let md = "# h\n\nHello *world*, this is a paragraph.\nStill the same paragraph.\n\n- list\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.paragraphs.len(), 1);
        let id = p.paragraphs.keys().next().unwrap();
        assert!(p.markdown.contains(&format!("<mdpara id=\"{id}\">")));
        assert!(p.markdown.contains("- list"));
        assert!(!p.markdown.contains("world\", this is a paragraph"));
    }

    #[test]
    fn skips_lists_quotes_headings_and_fences() {
        let md = "- item one\n\n> quote\n\n## h\n\n```\ncode\n```\n";
        assert!(prepare(md, Path::new("/d")).paragraphs.is_empty());
    }

    #[test]
    fn keeps_blank_line_separation() {
        let md = "first para\n\nsecond para\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.paragraphs.len(), 2);
        assert!(p.markdown.ends_with("\n\n"));
        for id in p.paragraphs.keys() {
            assert!(p.markdown.contains(&format!("<mdpara id=\"{id}\">\n\n")));
        }
    }

    #[test]
    fn paragraph_at_eof_without_trailing_newline() {
        let p = prepare("# h\n\nlast line", Path::new("/d"));
        assert_eq!(p.paragraphs.len(), 1);
        assert_eq!(p.paragraphs.values().next().unwrap(), "last line");
        assert!(p.markdown.ends_with("\n\n"));
    }

    #[test]
    fn paragraph_directly_after_heading_and_after_table() {
        let md = "## h\nbody right after\n\n| a |\n|---|\n| 1 |\nafter table\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.tables.len(), 1);
        assert_eq!(p.paragraphs.len(), 2);
        assert!(p.paragraphs.values().any(|s| s == "body right after"));
        assert!(p.paragraphs.values().any(|s| s == "after table"));
        assert!(p.markdown.contains("## h"));
        assert!(!p.markdown.contains("body right after"));
    }

    #[test]
    fn crlf_paragraph_extracts() {
        let md = "alpha beta\r\ngamma\r\n\r\ndelta\r\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.paragraphs.len(), 2);
        assert!(p.paragraphs.values().any(|s| s == "alpha beta\ngamma"));
        assert!(p.paragraphs.values().any(|s| s == "delta"));
    }

    #[test]
    fn paragraph_id_is_stable_and_content_sensitive() {
        assert_eq!(paragraph_id("a b"), paragraph_id("a b"));
        assert_ne!(paragraph_id("a b"), paragraph_id("a c"));
        assert_eq!(paragraph_id("x").len(), 16);
    }

    #[test]
    fn thematic_break_and_indented_line_are_not_paragraphs() {
        let md = "---\n\n***\n\n  indented\n";
        assert!(prepare(md, Path::new("/d")).paragraphs.is_empty());
    }

    #[test]
    fn list_and_heading_end_a_paragraph_run() {
        let md = "para\n- item\n\npara2\n# h\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.paragraphs.len(), 2);
        assert!(p.paragraphs.values().any(|s| s == "para"));
        assert!(p.paragraphs.values().any(|s| s == "para2"));
        assert!(p.markdown.contains("- item"));
        assert!(p.markdown.contains("# h"));
    }

    #[test]
    fn setext_equals_heading_is_not_a_paragraph() {
        let md = "Title\n===\n";
        let p = prepare(md, Path::new("/d"));
        assert!(p.paragraphs.is_empty());
        assert!(p.markdown.contains("Title\n==="));
    }

    #[test]
    fn setext_dash_heading_is_not_a_paragraph() {
        let md = "Title\n---\n";
        let p = prepare(md, Path::new("/d"));
        assert!(p.paragraphs.is_empty());
        assert!(p.markdown.contains("Title\n---"));
    }

    #[test]
    fn hanging_indent_continuation_stays_in_the_paragraph() {
        let md = "Hello world\n  continued here\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.paragraphs.len(), 1);
        let src = p.paragraphs.values().next().unwrap();
        assert!(src.contains("Hello world"));
        assert!(src.contains("continued here"));
    }

    #[test]
    fn hanging_indent_is_stripped_from_the_stored_source() {
        let md = "Hello world\n   continued here\n  and more\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(
            p.paragraphs.values().next().unwrap(),
            "Hello world\ncontinued here\nand more"
        );
    }

    #[test]
    fn four_space_indent_is_not_a_continuation() {
        let md = "Hello world\n    code\n";
        let p = prepare(md, Path::new("/d"));
        assert_eq!(p.paragraphs.len(), 1);
        assert_eq!(p.paragraphs.values().next().unwrap(), "Hello world");
        assert!(p.markdown.contains("    code"));
    }
}
