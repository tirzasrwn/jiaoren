//! Pre-parse a Markdown document, extracting mermaid diagrams, GFM tables,
//! list blocks, standalone local images and paragraph placeholders before
//! rendering.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::mermaid::file_url;

/// Failure to load a Markdown document from disk.
#[derive(Debug, thiserror::Error)]
pub enum DocError {
    /// The file could not be read at all.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        /// Path the read was attempted on.
        path: PathBuf,
        /// Underlying OS error.
        source: std::io::Error,
    },
    /// The file was read but is not UTF-8.
    #[error("{} is not valid UTF-8", path.display())]
    NotUtf8 {
        /// Path that contained invalid UTF-8.
        path: PathBuf,
    },
}

/// Read `path` as UTF-8 text, naming the file in every error message.
pub fn read_doc(path: &Path) -> Result<String, DocError> {
    let bytes = std::fs::read(path).map_err(|source| DocError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    String::from_utf8(bytes).map_err(|_| DocError::NotUtf8 {
        path: path.to_path_buf(),
    })
}

/// A Markdown document split into the library-rendered text and the
/// placeholders extracted from it.
pub struct Prepared {
    /// Markdown with diagrams, tables and paragraphs replaced by placeholders.
    pub markdown: String,
    /// Mermaid source keyed by [`diagram_id`].
    pub diagrams: HashMap<String, String>,
    /// Parsed tables keyed by [`table_id`].
    pub tables: HashMap<String, TableSpec>,
    /// Parsed list blocks keyed by [`list_id`].
    pub lists: HashMap<String, ListSpec>,
    /// Paragraph Markdown keyed by [`paragraph_id`].
    pub paragraphs: HashMap<String, String>,
    /// Standalone local images keyed by [`image_id`].
    pub images: HashMap<String, ImageSpec>,
}

/// A local image extracted from a paragraph that held nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSpec {
    /// Absolute path resolved against the document directory.
    pub src: String,
    /// Alt text exactly as written between the brackets.
    pub alt: String,
}

/// A top-level list block, parsed out of the Markdown before rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListSpec {
    /// Items in document order, with nesting depth and marker kind.
    pub items: Vec<ListItem>,
}

/// One list item: its nesting depth, marker and inline Markdown text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    /// Nesting depth (leading spaces / 2, capped at 5).
    pub depth: u8,
    /// Marker that opened the item.
    pub kind: ListKind,
    /// Item text with continuation lines joined by a single space.
    pub text: String,
}

/// The marker that opened a list item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    /// `-`, `*` or `+`.
    Bullet,
    /// Ordered marker, keeping the literal number from the source.
    Ordered(u64),
    /// Task marker; `true` when the box is checked.
    Task(bool),
}

/// A top-level GFM table, parsed out of the Markdown before rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSpec {
    /// Header cells, trimmed.
    pub header: Vec<String>,
    /// Data rows; short rows are padded with empty cells, long rows kept.
    pub rows: Vec<Vec<String>>,
}

/// Stable 16-hex-character id for a mermaid diagram source.
pub fn diagram_id(source: &str) -> String {
    let hex = blake3::hash(source.trim().as_bytes()).to_hex();
    hex.as_str()[..16].to_string()
}

/// Stable 16-hex-character id for a table's header and cell contents.
pub fn table_id(spec: &TableSpec) -> String {
    let mut canon = spec.header.join("\u{1f}");
    for row in &spec.rows {
        canon.push('\u{1e}');
        canon.push_str(&row.join("\u{1f}"));
    }
    let hex = blake3::hash(canon.as_bytes()).to_hex();
    hex.as_str()[..16].to_string()
}

/// Stable 16-hex-character id for a paragraph's rewritten Markdown source.
pub fn paragraph_id(source: &str) -> String {
    let hex = blake3::hash(source.trim().as_bytes()).to_hex();
    hex.as_str()[..16].to_string()
}

/// Stable 16-hex-character id for a list block's items.
pub fn list_id(spec: &ListSpec) -> String {
    let mut canon = String::new();
    for (n, item) in spec.items.iter().enumerate() {
        if n > 0 {
            canon.push('\u{1e}');
        }
        canon.push_str(&item.depth.to_string());
        canon.push('\u{1f}');
        match item.kind {
            ListKind::Bullet => canon.push('b'),
            ListKind::Ordered(number) => {
                canon.push('o');
                canon.push_str(&number.to_string());
            }
            ListKind::Task(done) => canon.push(if done { 'x' } else { ' ' }),
        }
        canon.push('\u{1f}');
        canon.push_str(item.text.trim_end());
    }
    let hex = blake3::hash(canon.as_bytes()).to_hex();
    hex.as_str()[..16].to_string()
}

/// Stable 16-hex-character id for an image's resolved path and alt text.
pub fn image_id(src: &str, alt: &str) -> String {
    let mut canon = String::with_capacity(src.len() + alt.len() + 1);
    canon.push_str(src.trim());
    canon.push('\u{1f}');
    canon.push_str(alt.trim());
    let hex = blake3::hash(canon.as_bytes()).to_hex();
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
            && (digits + 1 == t.len() || matches!(t.as_bytes()[digits + 1], b' ' | b'\t' | b'\r'))
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

/// An ATX heading: 0–3 leading spaces, 1–6 `#`, then a space or tab (or end
/// of line). Four or more leading spaces is an indented code block.
fn is_atx_heading(line: &str) -> bool {
    let t = line.trim_end_matches(['\r', '\n']);
    let indent = t.len() - t.trim_start_matches(' ').len();
    if indent > 3 {
        return false;
    }
    let s = &t[indent..];
    let hashes = s.chars().take_while(|&c| c == '#').count();
    (1..=6).contains(&hashes) && matches!(s.as_bytes().get(hashes), None | Some(b' ' | b'\t'))
}

/// Emit a spacer placeholder before a heading, unless the document has no
/// content yet (a leading heading needs no gap).
fn push_space_before(out: &mut String) {
    if out.trim().is_empty() {
        return;
    }
    // Spot the placeholder as its own HTML block: a blank line first.
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str("<mdspace>\n\n");
}

/// Emit a spacer placeholder after a heading.
///
/// `<mdspace>` is deliberately an empty HTML block: the library renders each
/// block as exactly one row, and that row is the breathing room. The html hook
/// runs in a left-to-right layout where `ui.add_space` has no vertical effect,
/// so the space must come from the block's own row.
fn push_space_after(out: &mut String) {
    out.push_str("<mdspace>\n\n");
}

/// True when `lines[from..]` still holds a non-blank line, i.e. a spacer
/// emitted now would not sit at the end of the document.
fn has_content_after(lines: &[&str], from: usize) -> bool {
    lines[from..].iter().any(|l| !l.trim().is_empty())
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

/// Extract mermaid fences, GFM tables and top-level paragraphs from `md`,
/// rewriting relative image links against `doc_dir`.
pub fn prepare(md: &str, doc_dir: &Path) -> Prepared {
    let mut out = String::with_capacity(md.len());
    let mut diagrams = HashMap::new();
    let mut tables = HashMap::new();
    let mut lists = HashMap::new();
    let mut paragraphs = HashMap::new();
    let mut images = HashMap::new();
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
                    // Raw HTML block: the line starts with `<` at ≤3 spaces of
                    // indent and is not an inline tag. Emit through to the next
                    // blank line verbatim and run no Markdown logic inside, so
                    // a spacer's blank line can never terminate the block and
                    // expose `# …` as a heading.
                    if opens_raw_html_block(line) {
                        let mut j = i + 1;
                        while j < lines.len() && !lines[j].trim().is_empty() {
                            j += 1;
                        }
                        for l in &lines[i..j] {
                            out.push_str(l);
                        }
                        i = j;
                        continue;
                    }
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
                    // List block: a marker at column 0 opens it, indented
                    // markers nest, indented non-markers continue the item.
                    // Claimed before paragraphs so a bullet is never a
                    // paragraph. Items the library must own (images, raw HTML)
                    // leave the whole block verbatim.
                    if let Some((j, spec)) = parse_list_block(&lines, i) {
                        let fallback = spec
                            .items
                            .iter()
                            .any(|it| it.text.contains("![") || it.text.contains('<'));
                        if fallback {
                            for l in &lines[i..j] {
                                out.push_str(&rewrite_images(l, doc_dir));
                            }
                        } else {
                            let id = list_id(&spec);
                            lists.insert(id.clone(), spec);
                            if !out.is_empty() && !out.ends_with("\n\n") {
                                out.push('\n');
                            }
                            out.push_str(&format!("<mdlist id=\"{id}\">\n\n"));
                        }
                        i = j;
                        continue;
                    }
                    // Not a table (or malformed delimiter): emit as before.
                    // A paragraph is claimed only after table detection, so the
                    // two never cover the same lines.
                    if is_paragraph_start(line) || is_standalone_image_start(line) {
                        let mut j = i + 1;
                        while j < lines.len() && is_paragraph_continuation(lines[j]) {
                            j += 1;
                        }
                        // A setext underline anywhere in/after the run makes the
                        // library's heading win: emit the title and underline
                        // verbatim, wrapped in spacers like ATX headings.
                        let setext_end = if j < lines.len() && is_setext_underline(lines[j]) {
                            Some(j + 1)
                        } else {
                            lines[i + 1..j]
                                .iter()
                                .position(|l| is_setext_underline(l))
                                .map(|k| i + 2 + k)
                        };
                        if let Some(end) = setext_end {
                            push_space_before(&mut out);
                            for l in &lines[i..end] {
                                out.push_str(&rewrite_images(l, doc_dir));
                            }
                            if has_content_after(&lines, end) {
                                push_space_after(&mut out);
                            }
                            i = end;
                            continue;
                        }
                        // CRLF safe: each line is trimmed at its end, then joined.
                        let joined = lines[i..j]
                            .iter()
                            .map(|l| strip_hanging_indent(l.trim_end()))
                            .collect::<Vec<_>>()
                            .join("\n");
                        // A paragraph that is only one local image becomes a
                        // placeholder, like a mermaid fence. Inline images,
                        // remote/data URIs and images inside tables, lists or
                        // quotes stay on the library path.
                        if let Some((alt, target)) = parse_standalone_image(&joined) {
                            if is_local_image(&target) {
                                let src = doc_dir.join(target).to_string_lossy().into_owned();
                                let id = image_id(&src, &alt);
                                images.insert(id.clone(), ImageSpec { src, alt });
                                if !out.is_empty() && !out.ends_with("\n\n") {
                                    out.push('\n');
                                }
                                out.push_str(&format!("<mdimg id=\"{id}\">\n\n"));
                                i = j;
                                continue;
                            }
                        }
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
                    if is_atx_heading(line) {
                        push_space_before(&mut out);
                        out.push_str(&rewrite_images(line, doc_dir));
                        if has_content_after(&lines, i + 1) {
                            push_space_after(&mut out);
                        }
                        i += 1;
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
        lists,
        paragraphs,
        images,
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
        if c == '|' && slashes.is_multiple_of(2) {
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

/// HTML elements CommonMark treats as inline. A line starting with one of
/// these stays ordinary paragraph text; any other `<…>` line opens a raw HTML
/// block (types 1–7, simplified here to "ends at the next blank line").
const INLINE_HTML_TAGS: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "br", "cite", "code", "data", "del", "dfn", "em", "i", "img",
    "ins", "kbd", "mark", "q", "rp", "rt", "ruby", "s", "samp", "small", "span", "strong", "sub",
    "sup", "time", "u", "var", "wbr",
];

/// True when `line` opens a raw HTML block: at most three leading spaces, then
/// `<`, and either no tag name (comment, declaration, processing instruction,
/// CDATA) or a block-level tag name.
fn opens_raw_html_block(line: &str) -> bool {
    let t = line.trim_end_matches(['\r', '\n']);
    let indent = t.len() - t.trim_start_matches(' ').len();
    if indent > 3 {
        return false;
    }
    let Some(rest) = t[indent..].strip_prefix('<') else {
        return false;
    };
    let body = rest.strip_prefix('/').unwrap_or(rest);
    let name: String = body
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    !INLINE_HTML_TAGS
        .iter()
        .any(|tag| tag.eq_ignore_ascii_case(&name))
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

/// Parse a list block starting at `start` (which must carry a column-0 marker)
/// into its items. Returns the index just past the block and the spec. Nested
/// markers (even indent / 2, capped at 5) stay in the same block; an indented
/// non-marker line joins the current item; a blank, column-0 non-marker or
/// odd-indent marker ends the block.
fn parse_list_block(lines: &[&str], start: usize) -> Option<(usize, ListSpec)> {
    let (indent, kind, text) = list_marker(lines[start])?;
    if indent != 0 {
        return None;
    }
    let mut items = Vec::new();
    let mut cur = ListItem {
        depth: 0,
        kind,
        text: text.to_string(),
    };
    let mut j = start + 1;
    while j < lines.len() {
        let line = lines[j];
        if line.trim().is_empty() {
            break;
        }
        if let Some((indent, kind, text)) = list_marker(line) {
            // Only even indents are item markers: a 1-space (or other odd)
            // marker ends the block and stays on the library path.
            if indent % 2 == 1 {
                break;
            }
            let depth = (indent / 2).min(5) as u8;
            items.push(std::mem::replace(
                &mut cur,
                ListItem {
                    depth,
                    kind,
                    text: text.to_string(),
                },
            ));
            j += 1;
            continue;
        }
        let indent = line.len() - line.trim_start_matches(' ').len();
        if indent >= 2 {
            if !cur.text.is_empty() {
                cur.text.push(' ');
            }
            cur.text.push_str(line.trim());
            j += 1;
            continue;
        }
        break;
    }
    items.push(cur);
    Some((j, ListSpec { items }))
}

/// Split a line into its marker: leading spaces, kind, and remaining text.
/// Ordered markers keep their literal number; task suffixes are recognised
/// after a marker and 1–4 spaces (or one tab).
fn list_marker(line: &str) -> Option<(usize, ListKind, &str)> {
    let t = line.trim_end_matches(['\r', '\n', ' ', '\t']);
    let indent = t.len() - t.trim_start_matches(' ').len();
    let s = &t[indent..];
    if is_thematic_break(s) {
        return None;
    }
    let b = s.as_bytes();
    let (kind, end) = match b.first()? {
        b'-' | b'*' | b'+' => (ListKind::Bullet, 1),
        c if c.is_ascii_digit() => {
            let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
            let number = s[..digits].parse().ok()?;
            match b.get(digits) {
                Some(b'.') | Some(b')') => (ListKind::Ordered(number), digits + 1),
                _ => return None,
            }
        }
        _ => return None,
    };
    if !(end == s.len() || matches!(b[end], b' ' | b'\t')) {
        return None;
    }
    let rest = &s[end..];
    match task_suffix(rest) {
        Some((done, text)) => Some((indent, ListKind::Task(done), text)),
        None => Some((indent, kind, rest.trim_start())),
    }
}

/// Task suffix after a marker: 1–4 spaces (or one tab), then `[ ]`, `[x]` or
/// `[X]`, then whitespace or end of line.
fn task_suffix(rest: &str) -> Option<(bool, &str)> {
    let body = match rest.strip_prefix('\t') {
        Some(r) => r,
        None => {
            let spaces = rest.len() - rest.trim_start_matches(' ').len();
            if !(1..=4).contains(&spaces) {
                return None;
            }
            &rest[spaces..]
        }
    };
    let done = match body.get(..3)? {
        "[ ]" => false,
        "[x]" | "[X]" => true,
        _ => return None,
    };
    match body[3..].chars().next() {
        None | Some(' ') | Some('\t') => Some((done, body[3..].trim_start())),
        _ => None,
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

/// True when `line` is one standalone image paragraph, allowing CommonMark's
/// up-to-three-space paragraph indent. A leading tab is four columns, and four
/// or more columns stay indented code.
fn is_standalone_image_start(line: &str) -> bool {
    let t = line.trim_end_matches(['\r', '\n']);
    let body = t.trim_start_matches([' ', '\t']);
    let lead = &t[..t.len() - body.len()];
    !lead.contains('\t') && lead.len() <= 3 && parse_standalone_image(body).is_some()
}

/// The destination of an image/link body: `<a b.png>` keeps its spaces, an
/// unbracketed destination ends at the first whitespace. Returns `None` for an
/// empty destination.
fn image_target(inner: &str) -> Option<&str> {
    let s = inner.trim();
    if let Some(rest) = s.strip_prefix('<') {
        let end = rest.find('>')?;
        let target = &rest[..end];
        return (!target.is_empty()).then_some(target);
    }
    let target = s.split_whitespace().next()?;
    (!target.is_empty()).then_some(target)
}

/// Parse `source` as exactly one Markdown image, optionally wrapped in
/// whitespace/newlines. Returns its alt text and link target with any
/// `"title"` stripped; text outside the image makes this return `None`.
fn parse_standalone_image(source: &str) -> Option<(String, String)> {
    let s = source.trim();
    let rest = s.strip_prefix("![")?;
    let mid = rest.find("](")?;
    let alt = &rest[..mid];
    let tail = &rest[mid + 2..];
    let (inner, consumed) = link_inner(tail)?;
    if !tail[consumed..].trim().is_empty() {
        return None;
    }
    let target = image_target(inner)?;
    Some((alt.to_string(), target.to_string()))
}

/// Whether an image target is a relative local path. Remote (`http(s)://`,
/// other schemes), `data:` and absolute paths stay on the library path, which
/// shows its placeholder and never fetches anything.
fn is_local_image(src: &str) -> bool {
    !src.is_empty()
        && !src.starts_with('/')
        && !src.starts_with('#')
        && !src.starts_with("data:")
        && !src.contains("://")
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
        // Drop an optional `"title"` and keep a bracketed destination's spaces.
        let Some(target) = image_target(inner) else {
            out.push_str("![");
            rest = &rest[2..];
            continue;
        };
        let skip = target.starts_with('/')
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
    mod fences {
        use super::super::*;

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
            assert_eq!(
                diagram_id("graph TD; A-->B;"),
                diagram_id("graph TD; A-->B;")
            );
            assert_ne!(
                diagram_id("graph TD; A-->B;"),
                diagram_id("graph TD; A-->C;")
            );
            assert_eq!(diagram_id("x").len(), 16);
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
        fn crlf_fences_extract() {
            let md = "```mermaid\r\ngraph TD;\r\n```\r\n";
            let p = prepare(md, Path::new("/x"));
            assert_eq!(p.diagrams.len(), 1);
            let id = diagram_id("graph TD;");
            assert!(p.markdown.contains(&format!("<mermaid id=\"{id}\">")));
        }
    }

    mod tables {
        use super::super::*;

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
        fn pipes_in_list_quote_indent_are_never_tables() {
            let out = prepare("a | b alone\n", Path::new("/d")).markdown;
            assert!(out.contains("a | b alone"));
            let md = "- | a | b |\n  |---|---|\n> | q |\n\n    | code |\n";
            let p = prepare(md, Path::new("/d"));
            assert!(p.tables.is_empty());
            // Lists own their lines now; the quote and indented code stay verbatim.
            assert_eq!(p.lists.len(), 1);
            let spec = p.lists.values().next().unwrap();
            assert!(spec.items[0].text.contains("| a | b |"));
            assert!(p.markdown.contains("> | q |"));
            assert!(p.markdown.contains("    | code |"));
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
            assert_eq!(p.lists.len(), 1);
            assert_eq!(
                p.lists.values().next().unwrap().items[0].text,
                "list | item"
            );
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
    }

    mod lists {
        use super::super::*;

        #[test]
        fn extracts_bullets_with_depth_and_numbers() {
            let md = "- satu\n- dua\n  - anak\n    1. cucu\n";
            let p = prepare(md, Path::new("/d"));
            let spec = p.lists.values().next().unwrap();
            assert_eq!(spec.items.len(), 4);
            assert_eq!(
                (spec.items[0].depth, &spec.items[0].kind),
                (0, &ListKind::Bullet)
            );
            assert_eq!(spec.items[2].depth, 1);
            assert!(matches!(spec.items[3].kind, ListKind::Ordered(1)));
            assert!(p.markdown.contains("<mdlist id="));
            assert!(!p.markdown.contains("- satu"));
        }

        #[test]
        fn joins_continuation_lines() {
            let md = "- item yang panjang\n  lanjutan baris pertama\n  dan kedua\n";
            let spec = prepare(md, Path::new("/d"))
                .lists
                .into_values()
                .next()
                .unwrap();
            assert_eq!(spec.items.len(), 1);
            assert_eq!(
                spec.items[0].text,
                "item yang panjang lanjutan baris pertama dan kedua"
            );
        }

        #[test]
        fn task_lists_and_ordered_start() {
            let md = "3. tiga\n4. empat\n- [x] selesai\n- [ ] belum\n";
            let p = prepare(md, Path::new("/d"));
            let specs: Vec<_> = p.lists.into_values().collect();
            let all: Vec<_> = specs.iter().flat_map(|s| &s.items).collect();
            assert!(matches!(all[0].kind, ListKind::Ordered(3)));
            assert!(matches!(all[1].kind, ListKind::Ordered(4)));
            assert_eq!(all[2].kind, ListKind::Task(true));
            assert_eq!(all[3].kind, ListKind::Task(false));
        }

        #[test]
        fn image_items_fall_back_to_library() {
            let md = "- biasa\n- dengan ![gambar](a.png)\n";
            let p = prepare(md, Path::new("/d"));
            assert!(p.lists.is_empty());
            assert!(p.markdown.contains("- dengan"));
        }

        #[test]
        fn blockquote_lists_are_not_extracted() {
            assert!(prepare("> - satu\n", Path::new("/d")).lists.is_empty());
        }

        #[test]
        fn html_items_fall_back_to_library() {
            let p = prepare("- a<br>b\n- c\n", Path::new("/d"));
            assert!(p.lists.is_empty());
            assert!(p.markdown.contains("- a<br>b"));
            assert!(p.markdown.contains("- c"));
        }

        #[test]
        fn list_then_paragraph_keeps_both() {
            let p = prepare("- a\nnext para\n", Path::new("/d"));
            assert_eq!(p.lists.len(), 1);
            assert_eq!(p.paragraphs.len(), 1);
            assert!(p.paragraphs.values().any(|s| s == "next para"));
            assert!(p.markdown.contains("<mdlist id="));
            assert!(p.markdown.contains("<mdpara id="));
        }

        #[test]
        fn list_after_table_extracts_both() {
            let p = prepare("| a |\n|---|\n| 1 |\n- x\n", Path::new("/d"));
            assert_eq!(p.tables.len(), 1);
            assert_eq!(p.lists.len(), 1);
        }

        #[test]
        fn crlf_list_extracts_and_joins() {
            let p = prepare("- satu\r\n  lanjut\r\n- dua\r\n", Path::new("/d"));
            let spec = p.lists.values().next().unwrap();
            assert_eq!(spec.items.len(), 2);
            assert_eq!(spec.items[0].text, "satu lanjut");
            assert_eq!(spec.items[1].text, "dua");
        }

        #[test]
        fn list_at_eof_without_newline_extracts() {
            let p = prepare("- satu", Path::new("/d"));
            assert_eq!(p.lists.values().next().unwrap().items.len(), 1);
        }

        #[test]
        fn star_emphasis_line_is_not_a_list() {
            let p = prepare("*emphasis*\n", Path::new("/d"));
            assert!(p.lists.is_empty());
            assert_eq!(p.paragraphs.len(), 1);
        }

        #[test]
        fn dash_inside_sentence_is_not_a_list() {
            let p = prepare("a - b\n", Path::new("/d"));
            assert!(p.lists.is_empty());
            assert_eq!(p.paragraphs.len(), 1);
        }

        #[test]
        fn odd_indent_marker_ends_the_block() {
            let p = prepare("- a\n - b\n", Path::new("/d"));
            assert_eq!(p.lists.values().next().unwrap().items.len(), 1);
            assert!(p.markdown.contains(" - b"));
        }

        #[test]
        fn plus_bullets_and_paren_ordered_markers() {
            let p = prepare("+ a\n1) b\n", Path::new("/d"));
            let spec = p.lists.values().next().unwrap();
            assert_eq!(spec.items[0].kind, ListKind::Bullet);
            assert_eq!(spec.items[1].kind, ListKind::Ordered(1));
        }

        #[test]
        fn task_suffix_accepts_one_to_four_spaces_or_tab() {
            let p = prepare("-  [x] a\n-\t[ ] b\n", Path::new("/d"));
            let spec = p.lists.values().next().unwrap();
            assert_eq!(spec.items[0].kind, ListKind::Task(true));
            assert_eq!(spec.items[0].text, "a");
            assert_eq!(spec.items[1].kind, ListKind::Task(false));
            assert_eq!(spec.items[1].text, "b");
        }

        #[test]
        fn list_inside_fence_stays_untouched() {
            let p = prepare("```\n- a\n```\n", Path::new("/d"));
            assert!(p.lists.is_empty());
            assert!(p.markdown.contains("- a"));
        }

        #[test]
        fn nested_task_item_keeps_depth_and_state() {
            let p = prepare("- [ ] induk\n  - [x] anak\n", Path::new("/d"));
            let spec = p.lists.values().next().unwrap();
            assert_eq!(spec.items[0].kind, ListKind::Task(false));
            assert_eq!(
                (spec.items[1].depth, spec.items[1].kind),
                (1, ListKind::Task(true))
            );
        }

        #[test]
        fn depth_is_capped_at_five() {
            let p = prepare("- a\n                - b\n", Path::new("/d"));
            let spec = p.lists.values().next().unwrap();
            assert_eq!(spec.items[1].depth, 5);
        }

        #[test]
        fn list_id_is_stable_and_content_sensitive() {
            let a = ListSpec {
                items: vec![ListItem {
                    depth: 0,
                    kind: ListKind::Bullet,
                    text: "x".to_string(),
                }],
            };
            let mut b = a.clone();
            b.items[0].text = "y".to_string();
            assert_eq!(list_id(&a), list_id(&a));
            assert_ne!(list_id(&a), list_id(&b));
            assert_eq!(list_id(&a).len(), 16);
        }
    }

    mod paragraphs {
        use super::super::*;

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
        fn extracts_a_top_level_paragraph() {
            let md =
                "# h\n\nHello *world*, this is a paragraph.\nStill the same paragraph.\n\n- list\n";
            let p = prepare(md, Path::new("/d"));
            assert_eq!(p.paragraphs.len(), 1);
            let id = p.paragraphs.keys().next().unwrap();
            assert!(p.markdown.contains(&format!("<mdpara id=\"{id}\">")));
            assert!(p.markdown.contains("<mdlist id="));
            assert_eq!(p.lists.len(), 1);
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
            assert_eq!(p.lists.len(), 1);
            assert!(p.markdown.contains("<mdlist id="));
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

    mod headings {
        use super::super::*;

        fn spaces(out: &str) -> usize {
            out.matches("<mdspace>").count()
        }

        #[test]
        fn heading_between_paragraphs_gets_spacers_on_both_sides() {
            let p = prepare("before\n\n# Title\n\nafter\n", Path::new("/d"));
            assert_eq!(p.paragraphs.len(), 2);
            assert_eq!(spaces(&p.markdown), 2);
            let before = p.markdown.find("<mdspace>").unwrap();
            let title = p.markdown.find("# Title").unwrap();
            let after = p.markdown.rfind("<mdspace>").unwrap();
            assert!(before < title && title < after);
        }

        #[test]
        fn consecutive_headings_get_two_spacers() {
            // Each `<mdspace>` is one empty HTML row, so `# A` then `# B` gets
            // two rows between them. Deliberately not deduplicated.
            let p = prepare("# A\n# B\n", Path::new("/d"));
            assert_eq!(spaces(&p.markdown), 2);
            assert!(p.markdown.contains("<mdspace>\n\n<mdspace>"));
        }

        #[test]
        fn document_starting_with_a_heading_has_no_leading_spacer() {
            let p = prepare("# Title\n\nbody\n", Path::new("/d"));
            assert_eq!(spaces(&p.markdown), 1);
            assert!(p.markdown.starts_with("# Title"));
        }

        #[test]
        fn setext_heading_at_document_start_has_no_leading_spacer() {
            let p = prepare("Title\n===\n\nbody\n", Path::new("/d"));
            assert_eq!(spaces(&p.markdown), 1);
            assert!(p.markdown.starts_with("Title\n==="));
            assert!(p.markdown.find("<mdspace>").unwrap() > p.markdown.find("===").unwrap());
        }

        #[test]
        fn heading_at_eof_without_newline_has_no_trailing_spacer() {
            let p = prepare("body\n\n## Last", Path::new("/d"));
            assert_eq!(spaces(&p.markdown), 1);
            assert!(p.markdown.ends_with("## Last"));
        }

        #[test]
        fn heading_at_eof_with_trailing_blank_line_has_no_trailing_spacer() {
            let p = prepare("body\n\n## Last\n\n", Path::new("/d"));
            assert_eq!(spaces(&p.markdown), 1);
            assert!(p.markdown.ends_with("## Last\n\n"));
        }

        #[test]
        fn thematic_break_after_blank_line_gets_no_spacers() {
            let p = prepare("body\n\n---\n", Path::new("/d"));
            assert_eq!(spaces(&p.markdown), 0);
            assert!(p.markdown.contains("---"));
        }

        #[test]
        fn indented_atx_heading_gets_spacers() {
            let p = prepare("before\n\n  ## indented\n\nafter\n", Path::new("/d"));
            assert_eq!(spaces(&p.markdown), 2);
            assert!(p.markdown.contains("  ## indented"));
        }

        #[test]
        fn four_space_indented_heading_stays_code() {
            let p = prepare("before\n\n    ## code\n\nafter\n", Path::new("/d"));
            assert_eq!(spaces(&p.markdown), 0);
            assert!(p.markdown.contains("    ## code"));
        }

        #[test]
        fn raw_html_block_keeps_hash_line_literal() {
            let p = prepare("<div>\n# not a heading\n</div>\n", Path::new("/d"));
            assert_eq!(spaces(&p.markdown), 0);
            assert!(p.markdown.contains("<div>\n# not a heading\n</div>"));
            assert!(p.paragraphs.is_empty());
        }

        #[test]
        fn heading_inside_a_fence_is_untouched() {
            let p = prepare("```\n# not a heading\n```\n", Path::new("/d"));
            assert_eq!(spaces(&p.markdown), 0);
            assert!(p.markdown.contains("# not a heading"));
        }

        #[test]
        fn setext_heading_gets_spacers_around_title_and_underline() {
            let p = prepare("before\n\nTitle\n===\n\nafter\n", Path::new("/d"));
            assert_eq!(p.paragraphs.len(), 2);
            assert_eq!(spaces(&p.markdown), 2);
            let before = p.markdown.find("<mdspace>").unwrap();
            let title = p.markdown.find("Title\n===").unwrap();
            let after = p.markdown.rfind("<mdspace>").unwrap();
            assert!(before < title && title < after);
        }

        #[test]
        fn headings_in_quotes_lists_and_indented_code_are_untouched() {
            let p = prepare("> # q\n\n- # item\n\n    # code\n", Path::new("/d"));
            assert_eq!(spaces(&p.markdown), 0);
            assert!(p.markdown.contains("> # q"));
            assert!(p.markdown.contains("    # code"));
            assert_eq!(p.lists.values().next().unwrap().items[0].text, "# item");
        }
    }

    mod images {
        use super::super::*;

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
    }

    mod local_images {
        use super::super::*;

        #[test]
        fn standalone_local_image_becomes_a_placeholder() {
            let md = "intro\n\n![the cat](pics/cat.png \"A cat\")\n\nafter\n";
            let p = prepare(md, Path::new("/docs"));
            assert_eq!(p.images.len(), 1);
            let spec = p.images.values().next().unwrap();
            assert_eq!(spec.src, "/docs/pics/cat.png");
            assert_eq!(spec.alt, "the cat");
            let id = image_id("/docs/pics/cat.png", "the cat");
            assert!(p.markdown.contains(&format!("<mdimg id=\"{id}\">\n\n")));
            assert!(!p.markdown.contains("![the cat]"));
            assert!(p.paragraphs.values().any(|s| s == "intro"));
            assert!(p.paragraphs.values().any(|s| s == "after"));
        }

        #[test]
        fn inline_images_stay_on_the_library_path() {
            let md = "before ![x](a.png) after\n";
            let p = prepare(md, Path::new("/docs"));
            assert!(p.images.is_empty());
            let source = p.paragraphs.values().next().unwrap();
            assert!(source.contains("![x](file:///docs/a.png)"));
            assert!(source.contains("before"));
        }

        #[test]
        fn remote_and_data_images_are_not_extracted() {
            for target in [
                "https://example.com/x.png",
                "http://example.com/x.png",
                "data:image/png;base64,AAAA",
                "/abs/x.png",
            ] {
                let md = format!("![x]({target})\n");
                let p = prepare(&md, Path::new("/docs"));
                assert!(
                    p.images.is_empty(),
                    "{target} must stay on the library path"
                );
                assert!(
                    p.paragraphs.values().any(|s| s.contains(target)),
                    "{target} must be untouched"
                );
            }
        }

        #[test]
        fn title_is_stripped_and_alt_is_captured() {
            let p = prepare("![a *b*](x.png \"title here\")\n", Path::new("/d"));
            let spec = p.images.values().next().unwrap();
            assert_eq!(spec.src, "/d/x.png");
            assert_eq!(spec.alt, "a *b*");
        }

        #[test]
        fn image_id_is_stable_and_content_sensitive() {
            assert_eq!(image_id("/d/a.png", "x"), image_id("/d/a.png", "x"));
            assert_ne!(image_id("/d/a.png", "x"), image_id("/d/a.png", "y"));
            assert_ne!(image_id("/d/a.png", "x"), image_id("/d/b.png", "x"));
            assert_eq!(image_id("/d/a.png", "x").len(), 16);
        }

        #[test]
        fn crlf_and_surrounding_whitespace_extract() {
            let md = "text\r\n\r\n  ![alt](pic.png)  \r\n\r\nmore\r\n";
            let p = prepare(md, Path::new("/docs"));
            assert_eq!(p.images.len(), 1);
            let spec = p.images.values().next().unwrap();
            assert_eq!(spec.src, "/docs/pic.png");
            assert_eq!(spec.alt, "alt");
            assert!(p.markdown.contains("<mdimg id="));
        }

        #[test]
        fn image_inside_list_table_or_quote_is_untouched() {
            let md = "- ![x](a.png)\n\n> ![q](b.png)\n\n| i |\n|---|\n| ![t](c.png) |\n";
            let p = prepare(md, Path::new("/docs"));
            assert!(p.images.is_empty());
        }

        #[test]
        fn four_space_indented_image_is_code_not_an_image() {
            let p = prepare("    ![x](a.png)\n", Path::new("/docs"));
            assert!(p.images.is_empty());
            assert!(p.markdown.contains("    ![x](file:///docs/a.png)"));
        }

        #[test]
        fn tab_indented_image_stays_on_the_library_path() {
            let p = prepare("\t![x](a.png)\n", Path::new("/docs"));
            assert!(p.images.is_empty());
            assert!(p.markdown.contains("\t![x](file:///docs/a.png)"));
        }

        #[test]
        fn angle_bracket_destination_keeps_its_spaces() {
            let md = "![x](<a b.png> \"title\")\n";
            let p = prepare(md, Path::new("/docs"));
            let spec = p.images.values().next().unwrap();
            assert_eq!(spec.src, "/docs/a b.png");
            assert_eq!(spec.alt, "x");
            assert!(p.markdown.contains("<mdimg id="));
        }

        #[test]
        fn inline_angle_bracket_destination_is_rewritten_not_truncated() {
            let md = "before ![x](<a b.png>) after\n";
            let p = prepare(md, Path::new("/docs"));
            assert!(p.images.is_empty());
            let source = p.paragraphs.values().next().unwrap();
            assert!(source.contains("![x](file:///docs/a%20b.png)"));
        }
    }

    mod read {
        use super::super::*;

        #[test]
        fn read_doc_missing_file_message_is_unchanged() {
            let path = Path::new("/nope/missing.md");
            let io = std::fs::read(path).unwrap_err();
            let err = read_doc(path).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("cannot read {}: {io}", path.display())
            );
        }

        #[test]
        fn read_doc_invalid_utf8_message_is_unchanged() {
            let path =
                std::env::temp_dir().join(format!("jiaoren-bad-utf8-{}.md", std::process::id()));
            std::fs::write(&path, [0xff, 0xfe]).unwrap();
            let err = read_doc(&path).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("{} is not valid UTF-8", path.display())
            );
            let _ = std::fs::remove_file(&path);
        }
    }
}
