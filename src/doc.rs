//! Pre-parse a Markdown document, extracting mermaid diagrams, GFM tables,
//! list blocks, headings, fenced code, standalone local images and paragraph
//! placeholders before rendering.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    /// Nested blockquotes keyed by their id.
    pub quotes: HashMap<String, QuoteSpec>,
    /// Standalone local images keyed by [`image_id`].
    pub images: HashMap<String, ImageSpec>,
    /// Headings keyed by [`heading_id`].
    pub headings: HashMap<String, HeadingSpec>,
    /// Fenced code blocks keyed by [`code_id`].
    pub code: HashMap<String, CodeSpec>,
}

/// A top-level heading, parsed out of the Markdown before rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingSpec {
    /// Heading level, 1–6.
    pub level: u8,
    /// Heading text with the markers stripped, inline Markdown intact.
    pub text: String,
    /// Stable id, also the map key and the placeholder's `id="…"`.
    pub id: String,
    /// Whether the document needs a gap above this heading. Decided here so the
    /// renderer keeps the leading-heading and EOF rules in one place.
    pub space_before: bool,
    /// Whether the document needs a gap below this heading (no trailing gap at
    /// EOF).
    pub space_after: bool,
}

/// A fenced code block, extracted verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSpec {
    /// Verbatim block text, without the fences and the trailing newline.
    pub text: String,
    /// First token of the info string, empty when absent.
    pub lang: String,
}

/// A local image extracted from a paragraph that held nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSpec {
    /// Absolute path resolved against the document directory.
    pub src: String,
    /// Alt text exactly as written between the brackets.
    pub alt: String,
}

/// A blockquote block: each line with its nesting depth and inline Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteSpec {
    pub lines: Vec<(u8, String)>,
}

/// Stable id for a quote block.
pub fn quote_id(spec: &QuoteSpec) -> String {
    let mut canon = String::new();
    for (depth, text) in &spec.lines {
        canon.push(depth.to_string().chars().next().unwrap_or('0'));
        canon.push('\u{1f}');
        canon.push_str(text);
        canon.push('\u{1e}');
    }
    let hex = blake3::hash(canon.as_bytes()).to_hex();
    hex.as_str()[..16].to_string()
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
    /// Ordered marker: literal number and `.`/`)` delimiter from the source.
    Ordered(u64, char),
    /// Task marker: checkbox state plus the ordered number/delimiter that
    /// opened it, when present.
    Task {
        done: bool,
        ordered: Option<(u64, char)>,
    },
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
            ListKind::Ordered(number, delim) => {
                canon.push('o');
                canon.push_str(&number.to_string());
                canon.push(delim);
            }
            ListKind::Task { done, ordered } => {
                canon.push(if done { 'x' } else { ' ' });
                if let Some((n, d)) = ordered {
                    canon.push_str(&n.to_string());
                    canon.push(d);
                }
            }
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

/// Stable 16-hex-character id for a heading's level, text and occurrence.
/// GitHub-style anchor slug: lowercase, spaces to `-`, keep letters, digits,
/// `-` and `_`; everything else is dropped.
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            ' ' => out.push('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => out.push(c),
            _ => {}
        }
    }
    out
}

pub fn heading_id(level: u8, text: &str, occurrence: usize) -> String {
    let canon = format!("{level}\u{1f}{text}\u{1f}{occurrence}");
    let hex = blake3::hash(canon.as_bytes()).to_hex();
    hex.as_str()[..16].to_string()
}

/// Stable 16-hex-character id for a code block's language and text.
pub fn code_id(lang: &str, text: &str) -> String {
    let mut canon = String::with_capacity(lang.len() + text.len() + 1);
    canon.push_str(lang);
    canon.push('\u{1f}');
    canon.push_str(text);
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

/// Emit a placeholder as its own HTML block: blank line first, blank line after.
fn push_placeholder(out: &mut String, tag: &str, id: &str) {
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(&format!("<{tag} id=\"{id}\">\n\n"));
}

/// Split an ATX heading line into its level and text. `None` for a line that is
/// not one: 0–3 leading spaces, 1–6 `#`, then a space/tab or end of line.
fn atx_heading(line: &str) -> Option<(u8, &str)> {
    let t = line.trim_end_matches(['\r', '\n']);
    let indent = t.len() - t.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let s = &t[indent..];
    let hashes = s.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = match s.as_bytes().get(hashes) {
        None => "",
        Some(b' ' | b'\t') => &s[hashes + 1..],
        Some(_) => return None,
    };
    // An optional closing sequence: trailing `#`s preceded by a space (or the
    // whole content). Without the space they are part of the text.
    let body = rest.trim_end();
    let closing = body.chars().rev().take_while(|&c| c == '#').count();
    let text = if closing > 0 {
        let before = &body[..body.len() - closing];
        if before.is_empty() || before.ends_with([' ', '\t']) {
            before.trim_end()
        } else {
            body
        }
    } else {
        body
    };
    Some((hashes as u8, text.trim_start()))
}

/// Level of a setext underline (`=` → 1, `-` → 2), `None` when not one.
fn setext_level(line: &str) -> Option<u8> {
    if !is_setext_underline(line) {
        return None;
    }
    match line.trim().chars().next() {
        Some('=') => Some(1),
        Some('-') => Some(2),
        _ => None,
    }
}

/// Open a fence only at 0–3 columns of indent (4+ is an indented code block).
/// A tab counts as the next 4-column stop.
fn fence_open(line: &str) -> Option<(char, usize, &str)> {
    let t = line.trim_end_matches(['\r', '\n']);
    if indent_columns(t) > 3 {
        return None;
    }
    open_fence(t.trim_start_matches([' ', '\t']))
}

/// Visual columns of the leading spaces and tabs (tab = next 4-column stop).
fn indent_columns(s: &str) -> usize {
    let mut col = 0;
    for c in s.chars() {
        match c {
            ' ' => col += 1,
            '\t' => col = (col / 4 + 1) * 4,
            _ => break,
        }
    }
    col
}

/// Strip control characters that would render as tofu and turn Unicode line/
/// paragraph separators into spaces. Code fences keep their bytes verbatim.
pub(crate) fn clean_text(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\u{2028}' | '\u{2029}' => Some(' '),
            c if c.is_control() && c != '\n' && c != '\t' => None,
            c => Some(c),
        })
        .collect()
}

/// Normalize `\r\n` and lone `\r` to `\n`, exactly once, so stored text has
/// clean line offsets.
fn normalize_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

/// Drop the single trailing newline of a code block body (and a CR before it).
fn strip_one_newline(s: &str) -> &str {
    let s = s.strip_suffix('\n').unwrap_or(s);
    s.strip_suffix('\r').unwrap_or(s)
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
/// A paragraph line with trailing CR/LF removed. Two or more trailing spaces
/// mean a hard break: they become a backslash before the join newline, which
/// [`crate::justify::parse_inline`] turns into a forced break.
fn paragraph_line(line: &str) -> String {
    let t = line.trim_end_matches(['\r', '\n']);
    let body = strip_hanging_indent(t);
    // Trailing spaces on an image-only line are layout noise, not a break.
    if parse_standalone_image(body.trim()).is_some() {
        return body.trim().to_string();
    }
    if body.ends_with("  ") {
        format!("{}\\", body.trim_end())
    } else {
        body.to_string()
    }
}

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
    let mut quotes = HashMap::new();
    let mut images = HashMap::new();
    let mut headings = HashMap::new();
    let mut code = HashMap::new();
    // Occurrence counter: identical headings still get distinct ids, so each
    // keeps its own start/EOF gap flags in the map.
    let mut heading_seq = 0usize;
    // (fence char, run length, info string, verbatim open line)
    let mut open: Option<(char, usize, String, String)> = None;
    let mut body = String::new();
    let lines: Vec<&str> = md.split_inclusive('\n').collect();
    let (refs, ref_lines) = collect_reference_definitions(&lines);
    let mut i = 0usize;
    while i < lines.len() {
        if ref_lines.contains(&i) {
            i += 1;
            continue;
        }
        let line = lines[i];
        let t = line.trim();
        match open.take() {
            None => match fence_open(line) {
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
                    // Blockquote that is exactly a table: extract it before the
                    // library sees it. A table nested in a blockquote leaves
                    // egui_commonmark's table flag set, and the next event makes
                    // it swallow the rest of the document (audit T-CM-061).
                    // A Mermaid fence inside a quote: extract it so the library
                    // does not render it as a code block (audit T-MM-081).
                    if let Some((block_end, body)) = quote_mermaid_block(&lines, i) {
                        let id = diagram_id(&body);
                        diagrams.insert(id.clone(), body.trim().to_string());
                        if !out.is_empty() && !out.ends_with("\n\n") {
                            out.push('\n');
                        }
                        out.push_str(&format!("<mermaid id=\"{id}\">\n\n"));
                        i = block_end;
                        continue;
                    }
                    if let Some((block_end, table_start, table_end, mut spec)) =
                        quote_table_split(&lines, i)
                    {
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
                        // Quote text before the table stays a quote; the table
                        // renders at top level because the library's quote
                        // layout clips the reading-column width.
                        if table_start > i {
                            push_verbatim_block(&mut out, &lines[i..table_start]);
                        }
                        if !out.is_empty() && !out.ends_with("\n\n") {
                            out.push('\n');
                        }
                        out.push_str(&format!("<mdtable id=\"{id}\">\n\n"));
                        if table_end < block_end {
                            push_verbatim_block(&mut out, &lines[table_end..block_end]);
                        }
                        i = block_end;
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
                                *c = rewrite_images(&expand_links(&clean_text(c), &refs), doc_dir);
                            }
                            for row in &mut spec.rows {
                                for c in row {
                                    *c = rewrite_images(
                                        &expand_links(&clean_text(c), &refs),
                                        doc_dir,
                                    );
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
                        // Blocks we cannot nest (fenced code) go to the library,
                        // which renders them inside the item properly.
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
                            let level = setext_level(lines[end - 1]).unwrap_or(1);
                            let text = expand_links(
                                &clean_text(&normalize_newlines(
                                    &lines[i..end - 1]
                                        .iter()
                                        .map(|l| l.trim())
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                )),
                                &refs,
                            );
                            let id = heading_id(level, &text, heading_seq);
                            heading_seq += 1;
                            headings.insert(
                                id.clone(),
                                HeadingSpec {
                                    level,
                                    text,
                                    id: id.clone(),
                                    space_before: !out.trim().is_empty(),
                                    space_after: has_content_after(&lines, end),
                                },
                            );
                            push_placeholder(&mut out, "mdheading", &id);
                            i = end;
                            continue;
                        }
                        // CRLF safe: each line is trimmed at its end, then joined.
                        let joined = expand_links(
                            &clean_text(
                                &lines[i..j]
                                    .iter()
                                    .map(|l| paragraph_line(l))
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                            ),
                            &refs,
                        );
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
                    // Blockquote: one placeholder, nested depths kept so the
                    // renderer can draw one bar per level. Lazy continuation
                    // lines are folded into the last quoted line.
                    if strip_quote(line).is_some() {
                        let mut end = i;
                        let mut spec = QuoteSpec { lines: Vec::new() };
                        while end < lines.len() {
                            let Some((depth, rest)) = quote_parts(lines[end]) else {
                                break;
                            };
                            spec.lines
                                .push((depth, expand_links(&clean_text(&rest), &refs)));
                            end += 1;
                        }
                        let mut lazy_end = end;
                        while lazy_end < lines.len() {
                            let l = lines[lazy_end];
                            if l.trim().is_empty()
                                || strip_quote(l).is_some()
                                || is_block_boundary(l)
                            {
                                break;
                            }
                            if let Some(last) = spec.lines.last_mut() {
                                if !last.1.is_empty() {
                                    last.1.push(' ');
                                }
                                last.1.push_str(&expand_links(&clean_text(l.trim()), &refs));
                            }
                            lazy_end += 1;
                        }
                        // Complex quote content (fences, headings, lists,
                        // tables) keeps the library path so those blocks keep
                        // working; plain/nested text uses the quote renderer.
                        let complex = spec.lines.iter().any(|(_, text)| {
                            let t = text.trim();
                            !t.is_empty() && (is_block_boundary(text) || text.contains('|'))
                        });
                        if !complex {
                            let id = quote_id(&spec);
                            quotes.insert(id.clone(), spec);
                            if !out.is_empty() && !out.ends_with("\n\n") {
                                out.push('\n');
                            }
                            out.push_str(&format!("<mdquote id=\"{id}\">\n\n"));
                            i = lazy_end;
                            continue;
                        }
                    }
                    if let Some((level, text)) = atx_heading(line) {
                        let text = expand_links(&clean_text(&normalize_newlines(text)), &refs);
                        let id = heading_id(level, &text, heading_seq);
                        heading_seq += 1;
                        headings.insert(
                            id.clone(),
                            HeadingSpec {
                                level,
                                text,
                                id: id.clone(),
                                space_before: !out.trim().is_empty(),
                                space_after: has_content_after(&lines, i + 1),
                            },
                        );
                        push_placeholder(&mut out, "mdheading", &id);
                        i += 1;
                        continue;
                    }
                    out.push_str(&rewrite_images(line, doc_dir));
                    i += 1;
                }
            },
            Some((ch, len, info, oline)) => {
                if is_close(line, ch, len) {
                    if info.split_whitespace().next() == Some("mermaid") {
                        let id = diagram_id(&body);
                        diagrams.insert(id.clone(), body.trim().to_string());
                        // Trailing blank line terminates the CommonMark HTML block so
                        // a paragraph directly after the fence is not absorbed into it.
                        out.push_str(&format!("<mermaid id=\"{id}\">\n\n"));
                    } else {
                        let lang = info.split_whitespace().next().unwrap_or("").to_string();
                        let text = normalize_newlines(strip_one_newline(&body));
                        let id = code_id(&lang, &text);
                        code.insert(id.clone(), CodeSpec { text, lang });
                        push_placeholder(&mut out, "mdcode", &id);
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
        quotes,
        images,
        headings,
        code,
    }
}

/// Parse a full table block (header, delimiter row, then data rows) into a
/// `TableSpec`. The delimiter row must have the same cell count as the header
/// and each cell must match `:?-{1,}:?`. Malformed blocks return `None`, so
/// the caller can fall back to emitting the lines verbatim.
/// Emit raw block lines between blank lines, so a quote block starts and ends
/// cleanly beside a table placeholder.
fn push_verbatim_block(out: &mut String, lines: &[&str]) {
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    for line in lines {
        out.push_str(line);
    }
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
}

/// A blockquote that is exactly one mermaid fence. Returns the block end and
/// the diagram source; mixed quote content stays on the library path.
fn quote_mermaid_block(lines: &[&str], start: usize) -> Option<(usize, String)> {
    let mut end = start;
    let mut body = Vec::new();
    while end < lines.len() {
        let Some(rest) = strip_quote(lines[end]) else {
            break;
        };
        body.push(rest);
        end += 1;
    }
    if body.len() < 3 {
        return None;
    }
    let (ch, len, info) = open_fence(body[0])?;
    if info.split_whitespace().next() != Some("mermaid") {
        return None;
    }
    if !is_close(body[body.len() - 1], ch, len) {
        return None;
    }
    if body[1..body.len() - 1]
        .iter()
        .any(|l| l.trim_start().starts_with(">"))
    {
        return None;
    }
    let source = body[1..body.len() - 1].join("\n");
    Some((end, source))
}

/// Depth and text of one quote line: counts leading `>` markers.
fn quote_parts(line: &str) -> Option<(u8, String)> {
    let mut text = line.trim_start();
    let mut depth = 0u8;
    while let Some(rest) = text.strip_prefix('>') {
        depth = depth.saturating_add(1);
        text = rest.strip_prefix(' ').unwrap_or(rest);
    }
    (depth > 0).then(|| (depth, text.trim_end().to_string()))
}

fn strip_quote(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest).trim_end())
}

/// Find the first table inside the blockquote starting at `start`. Returns the
/// block end, the table's line range within it, and the parsed spec. Quote text
/// before and after the table is kept by the caller. Extracting the table is
/// what stops egui_commonmark's leaked table flag from swallowing the rest of
/// the document (audit T-CM-061).
fn quote_table_split(lines: &[&str], start: usize) -> Option<(usize, usize, usize, TableSpec)> {
    let mut block_end = start;
    while block_end < lines.len() && strip_quote(lines[block_end]).is_some() {
        block_end += 1;
    }
    if block_end == start {
        return None;
    }
    let mut line = start;
    while line < block_end {
        let Some(cell) = strip_quote(lines[line]) else {
            break;
        };
        if !cell.contains('|') {
            line += 1;
            continue;
        }
        let mut run_end = line;
        let mut body = Vec::new();
        while run_end < block_end {
            let Some(cell) = strip_quote(lines[run_end]) else {
                break;
            };
            if !cell.contains('|') {
                break;
            }
            body.push(cell);
            run_end += 1;
        }
        if let Some(spec) = parse_table_block(&body) {
            return Some((block_end, line, run_end, spec));
        }
        line += 1;
    }
    None
}

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
        // GFM: cells past the header's column count are dropped.
        cells.truncate(ncols);
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
    let info = &t[len..];
    // CommonMark: a backtick fence's info string may not contain a backtick,
    // so such a line is not a fence opener at all.
    if ch == '`' && info.contains('`') {
        return None;
    }
    Some((ch, len, info))
}

/// A closing fence: same char, run at least as long as the opener, at most 3
/// columns of indent, and nothing after the run but whitespace. The raw line is
/// checked so an indented `    ``` inside a block does not end it early.
fn is_close(line: &str, ch: char, open_len: usize) -> bool {
    let t = line.trim_end_matches(['\r', '\n']);
    if indent_columns(t) > 3 {
        return false;
    }
    let body = t.trim_start_matches([' ', '\t']);
    let run = body.chars().take_while(|&c| c == ch).count();
    run >= open_len && body[run..].trim().is_empty()
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
        // A fence inside an item becomes a top-level code block; the library
        // cannot place a block inside a list item.
        if indent >= 1 && fence_open(line).is_some() {
            break;
        }
        if indent >= 2 {
            if !cur.text.is_empty() {
                cur.text.push(' ');
            }
            cur.text.push_str(line.trim());
            j += 1;
            continue;
        }
        // CommonMark lazy continuation: a non-indented line that is not a new
        // block still belongs to the current item.
        if !is_block_boundary(line) && !line.contains('|') && !line.trim_start().starts_with('<') {
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
    for item in &mut items {
        item.text = clean_text(&item.text);
    }
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
                Some(&d @ (b'.' | b')')) => (ListKind::Ordered(number, d as char), digits + 1),
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
        Some((done, text)) => {
            let ordered = match kind {
                ListKind::Ordered(n, d) => Some((n, d)),
                _ => None,
            };
            Some((indent, ListKind::Task { done, ordered }, text))
        }
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
/// Collect `[id]: url "title"` definitions, returning the map and the line
/// indices that must not be rendered. Fences are skipped.
fn collect_reference_definitions(
    lines: &[&str],
) -> (
    std::collections::HashMap<String, String>,
    std::collections::HashSet<usize>,
) {
    let mut defs = std::collections::HashMap::new();
    let mut taken = std::collections::HashSet::new();
    let mut open: Option<(char, usize)> = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some((ch, len)) = open {
            if is_close(line, ch, len) {
                open = None;
            }
            continue;
        }
        if let Some((ch, len, _)) = fence_open(line) {
            open = Some((ch, len));
            continue;
        }
        let t = line.trim();
        let Some(rest) = t.strip_prefix('[') else {
            continue;
        };
        let Some(end) = rest.find("]:") else {
            continue;
        };
        let id = normalize_ref(&rest[..end]);
        let target = rest[end + 2..].trim();
        let url = image_target(target).unwrap_or("");
        if id.is_empty() || url.is_empty() {
            continue;
        }
        defs.entry(id).or_insert_with(|| url.to_string());
        taken.insert(i);
    }
    (defs, taken)
}

fn normalize_ref(id: &str) -> String {
    id.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Resolve reference links/images and linkify bare URLs and emails. Code spans
/// and existing inline links are left alone.
fn expand_links(md: &str, refs: &std::collections::HashMap<String, String>) -> String {
    let mut out = String::with_capacity(md.len());
    let mut i = 0;
    while i < md.len() {
        let rest = &md[i..];
        let c = rest.chars().next().unwrap();
        if c == '`' {
            if let Some(end) = rest[1..].find('`') {
                out.push_str(&rest[..end + 2]);
                i += end + 2;
            } else {
                out.push_str(rest);
                break;
            }
            continue;
        }
        if c == '!' && rest[1..].starts_with('[') {
            if let Some((text, url, used)) = reference_or_inline(rest, refs, true) {
                out.push_str(&text);
                i += used;
                let _ = url;
                continue;
            }
        }
        if c == '[' {
            if let Some((text, _url, used)) = reference_or_inline(rest, refs, false) {
                out.push_str(&text);
                i += used;
                continue;
            }
        }
        if let Some((len, url)) = autolink_at(rest) {
            let text = &rest[..len];
            out.push_str(&format!("[{text}]({url})"));
            i += len;
            continue;
        }
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// `![alt][ref]`, `[text][ref]`, `[text][]`, `[ref]` or inline `[text](url)`.
/// Returns the replacement Markdown and the bytes consumed.
fn reference_or_inline(
    s: &str,
    refs: &std::collections::HashMap<String, String>,
    image: bool,
) -> Option<(String, String, usize)> {
    let bang = usize::from(image);
    let rest = s.get(bang..)?;
    let label_end = rest.find(']')?;
    let label = &rest[1..label_end];
    let after = &rest[label_end + 1..];
    if let Some(inner) = after.strip_prefix('(') {
        let (_, end) = link_inner(inner)?;
        let used = bang + label_end + 1 + 1 + end;
        return Some((s[..used].to_string(), String::new(), used));
    }
    let (id, used_after) = if let Some(rest2) = after.strip_prefix('[') {
        let end = rest2.find(']')?;
        let id = if rest2[..end].is_empty() {
            label.to_string()
        } else {
            rest2[..end].to_string()
        };
        (id, end + 2)
    } else {
        (label.to_string(), 0)
    };
    let url = refs.get(&normalize_ref(&id))?.clone();
    let text = if image {
        format!("![{label}]({url})")
    } else {
        format!("[{label}]({url})")
    };
    Some((text, url, bang + label_end + 1 + used_after))
}

/// A bare `http(s)://`, `www.` or email at the start of `s`: length and target.
fn autolink_at(s: &str) -> Option<(usize, String)> {
    let scheme = ["https://", "http://", "www."]
        .into_iter()
        .find(|p| s.starts_with(p));
    if let Some(prefix) = scheme {
        let end = s
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"'))
            .unwrap_or(s.len());
        let mut text = &s[..end];
        while text
            .chars()
            .last()
            .is_some_and(|c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']'))
        {
            text = &text[..text.len() - 1];
        }
        if text.len() <= prefix.len() {
            return None;
        }
        let url = if prefix == "www." {
            format!("https://{text}")
        } else {
            text.to_string()
        };
        return Some((text.len(), url));
    }
    // Bare email: word@word.tld
    let end = s
        .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"'))
        .unwrap_or(s.len());
    let word = &s[..end];
    let at = word.find('@')?;
    let (user, domain) = (&word[..at], &word[at + 1..]);
    if user.is_empty() || !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.')
    {
        return None;
    }
    Some((word.len(), format!("mailto:{word}")))
}

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
pub(crate) fn parse_standalone_image(source: &str) -> Option<(String, String)> {
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

/// A `file://` URI for the Markdown library's file loader. The path is used
/// literally (the loader does not percent-decode), so a destination with
/// whitespace is angle-bracketed to survive Markdown parsing.
fn file_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if raw.chars().any(char::is_whitespace) {
        format!("<file://{raw}>")
    } else {
        format!("file://{raw}")
    }
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
            // Remote/data/absolute images are never fetched. Show the alt text
            // instead of an image placeholder with no label.
            out.push_str(alt);
            rest = &rest[mid + 2 + end..];
            continue;
        }
        out.push_str(&format!("![{alt}]({})", file_uri(&doc_dir.join(target))));
        rest = &rest[mid + 2 + end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    mod quote_table {
        use super::super::prepare;

        #[test]
        fn mixed_quote_keeps_text_around_the_extracted_table() {
            let p = prepare(
                "> intro line\n> | a | b |\n> | - | - |\n> | 1 | 2 |\n> outro line\n\nafter marker\n",
                std::path::Path::new("/tmp"),
            );
            assert_eq!(p.tables.len(), 1);
            assert!(p.markdown.contains("> intro line"), "{:?}", p.markdown);
            assert!(p.markdown.contains("> outro line"), "{:?}", p.markdown);
            assert!(p.markdown.contains("<mdtable"), "{:?}", p.markdown);
            assert!(p.markdown.contains("<mdpara"), "{:?}", p.markdown);
        }

        #[test]
        fn table_inside_a_blockquote_becomes_a_placeholder_and_keeps_later_content() {
            let p = prepare(
                "> | a | b |\n> | - | - |\n> | 1 | 2 |\n\nafter marker\n",
                std::path::Path::new("/tmp"),
            );
            assert_eq!(p.tables.len(), 1, "the quote table must be extracted");
            assert_eq!(
                p.tables.values().next().unwrap().header,
                vec!["a".to_string(), "b".to_string()]
            );
            assert!(
                p.markdown.contains("<mdtable"),
                "the quote table gets a placeholder: {:?}",
                p.markdown
            );
            assert!(
                p.markdown.contains("<mdpara"),
                "content after the quote must survive: {:?}",
                p.markdown
            );
        }
    }

    mod list_delimiters {
        use super::super::{prepare, ListKind};
        use std::path::Path;

        #[test]
        fn parenthesised_ordered_marker_is_kept() {
            let p = prepare("1) first\n2) second\n", Path::new("/d"));
            let spec = p.lists.values().next().expect("list extracted");
            assert_eq!(spec.items[0].kind, ListKind::Ordered(1, ')'));
            assert_eq!(spec.items[1].kind, ListKind::Ordered(2, ')'));
        }
    }

    mod anchors {
        use super::super::slug;

        #[test]
        fn slugs_match_github_style() {
            assert_eq!(slug("Hello, World!"), "hello-world");
            assert_eq!(slug("C++ & Rust"), "c--rust");
            assert_eq!(slug("snake_case and-dash"), "snake_case-and-dash");
            assert_eq!(slug("日本語 Heading"), "日本語-heading");
        }
    }

    mod hard_break {
        use super::super::prepare;
        use std::path::Path;

        #[test]
        fn two_trailing_spaces_become_a_forced_break() {
            let p = prepare("first line  \nsecond line\n", Path::new("/d"));
            let out = p.paragraphs.values().next().unwrap();
            assert!(out.contains("first line\\\nsecond line"), "{out:?}");
        }
    }

    mod quote_lazy {
        use super::super::prepare;
        use std::path::Path;

        #[test]
        fn unindented_line_stays_inside_a_quote() {
            let p = prepare("> quote line\nlazy line\n\nnext para\n", Path::new("/d"));
            let spec = p.quotes.values().next().expect("quote extracted");
            assert_eq!(spec.lines[0].1, "quote line lazy line");
            assert!(p.markdown.contains("<mdquote"), "{:?}", p.markdown);
            assert!(p.markdown.contains("<mdpara"), "{:?}", p.markdown);
        }
    }

    mod text_hygiene {
        use super::super::{clean_text, prepare};

        #[test]
        fn controls_are_stripped_and_separators_become_spaces() {
            assert_eq!(clean_text("a\u{0}b\u{7}c\u{2028}d\u{2029}e"), "abc d e");
            assert_eq!(clean_text("keep\ttabs\nnewlines"), "keep\ttabs\nnewlines");
            let p = prepare("a\u{0}b\n\n\u{2028}x\n", std::path::Path::new("/tmp"));
            assert!(!p.markdown.contains('\u{0}'));
            assert!(!p.markdown.contains('\u{2028}'));
        }
    }

    mod fences {
        use super::super::*;

        #[test]
        fn backtick_info_string_cannot_contain_a_backtick() {
            // CommonMark: a backtick fence's info string may not contain a
            // backtick; the line is ordinary text, not a fence opener.
            assert_eq!(open_fence("``` `not a fence`"), None);
            assert!(open_fence("```rust").is_some());
            assert!(open_fence("````").is_some());
            // Tilde fences may carry backticks in the info string.
            assert!(open_fence("~~~ `ok`").is_some());
        }

        #[test]
        fn extracts_mermaid_and_code_separately() {
            let md = "text\n\n```mermaid\ngraph TD; A-->B;\n```\n\n```rust\nfn x() {}\n```\n";
            let p = prepare(md, Path::new("/docs"));
            assert_eq!(p.diagrams.len(), 1);
            let id = diagram_id("graph TD; A-->B;");
            assert!(p.markdown.contains(&format!("<mermaid id=\"{id}\">")));
            assert!(!p.markdown.contains("```mermaid"));
            assert_eq!(p.code.len(), 1);
            let spec = p.code.values().next().unwrap();
            assert_eq!(spec.lang, "rust");
            assert_eq!(spec.text, "fn x() {}");
            let code = code_id("rust", "fn x() {}");
            assert!(p.markdown.contains(&format!("<mdcode id=\"{code}\">\n\n")));
            assert!(!p.markdown.contains("```"));
        }

        #[test]
        fn code_fence_without_info_has_empty_lang() {
            let p = prepare("```\nplain\n```\n", Path::new("/d"));
            let spec = p.code.values().next().unwrap();
            assert_eq!(spec.lang, "");
            assert_eq!(spec.text, "plain");
            let id = code_id("", "plain");
            assert!(p.markdown.contains(&format!("<mdcode id=\"{id}\">")));
        }

        #[test]
        fn code_text_is_verbatim_with_leading_spaces_and_blank_lines() {
            let md = "```\n    indented\n\nlast\n```\n";
            let p = prepare(md, Path::new("/d"));
            assert_eq!(p.code.values().next().unwrap().text, "    indented\n\nlast");
        }

        #[test]
        fn tilde_code_fence_extracts() {
            let p = prepare("~~~sh\necho hi\n~~~\n", Path::new("/d"));
            let spec = p.code.values().next().unwrap();
            assert_eq!((spec.lang.as_str(), spec.text.as_str()), ("sh", "echo hi"));
            assert!(p.markdown.contains("<mdcode"));
        }

        #[test]
        fn crlf_code_fence_strips_the_trailing_newline() {
            let p = prepare("```\r\nx\r\n```\r\n", Path::new("/d"));
            assert_eq!(p.code.values().next().unwrap().text, "x");
        }

        #[test]
        fn closing_fence_must_be_bare_and_mostly_unindented() {
            // A `    ``` line inside the block is content, not the close: the
            // tail must not leak out as a paragraph.
            let p = prepare("```\n    ```\nrest\n```\n", Path::new("/d"));
            assert_eq!(p.code.len(), 1);
            let text = &p.code.values().next().unwrap().text;
            assert_eq!(text, "    ```\nrest");
            assert!(!p.markdown.contains("rest"));
        }

        #[test]
        fn closing_fence_with_trailing_text_does_not_close() {
            let p = prepare("```\ncode\n```x\n```\n", Path::new("/d"));
            assert_eq!(p.code.values().next().unwrap().text, "code\n```x");
        }

        #[test]
        fn tab_indented_fence_stays_on_the_library_path() {
            let p = prepare("\t```\n\tcode\n\t```\n", Path::new("/d"));
            assert!(p.code.is_empty());
            assert!(p.markdown.contains("\t```"));
            assert!(p.markdown.contains("\tcode"));
        }

        #[test]
        fn crlf_code_blocks_normalize_interior_newlines() {
            let p = prepare("```\r\n    a\r\n\r\nb\r\n```\r\n", Path::new("/d"));
            let text = &p.code.values().next().unwrap().text;
            assert_eq!(text, "    a\n\nb");
            assert!(!text.contains('\r'));
        }

        #[test]
        fn unclosed_code_fence_at_eof_stays_verbatim() {
            let p = prepare("text\n\n```rust\nfn x() {}", Path::new("/d"));
            assert!(p.code.is_empty());
            assert!(p.markdown.contains("```rust"));
            assert!(p.markdown.contains("fn x() {}"));
        }

        #[test]
        fn code_inside_quotes_and_indented_code_is_untouched() {
            let quoted = prepare("> ```\n> code\n> ```\n", Path::new("/d"));
            assert!(quoted.code.is_empty());
            assert!(quoted.markdown.contains("> ```"));
            let indented = prepare("    ```\n    code\n    ```\n", Path::new("/d"));
            assert!(indented.code.is_empty());
            assert!(indented.markdown.contains("    ```"));
            let listed = prepare("- item\n  ```\n  code\n  ```\n", Path::new("/d"));
            // The fence becomes a top-level code block instead of collapsing
            // into the item text; the item/list itself still extracts.
            assert_eq!(listed.code.len(), 1);
            assert_eq!(listed.lists.len(), 1);
            assert!(listed.markdown.contains("<mdcode"));
        }

        #[test]
        fn code_id_is_stable_and_content_sensitive() {
            assert_eq!(code_id("rust", "x"), code_id("rust", "x"));
            assert_ne!(code_id("rust", "x"), code_id("sh", "x"));
            assert_ne!(code_id("rust", "x"), code_id("rust", "y"));
            assert_eq!(code_id("rust", "x").len(), 16);
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
            assert!(p.headings.values().any(|h| h.text == "h"));
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
        fn extra_cells_are_dropped_per_gfm() {
            let spec = parse_table_block(&["| a | b |", "|---|---|", "| 1 | 2 | 3 |"]).unwrap();
            assert_eq!(spec.rows, vec![vec!["1".to_string(), "2".to_string()]]);
        }

        #[test]
        fn delimiter_cells_must_be_hyphens_with_optional_edge_colons() {
            assert!(is_delim_cell("---"));
            assert!(is_delim_cell(":--"));
            assert!(is_delim_cell("--:"));
            assert!(is_delim_cell(":-:"));
            assert!(!is_delim_cell("-:-"));
            assert!(!is_delim_cell(""));
            assert!(!is_delim_cell("--x"));
            assert_eq!(
                parse_table_block(&["| a | b |", "| :-: | -:- |", "| 1 | 2 |"]),
                None
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
            assert!(p.headings.values().any(|h| h.text == "h | x"));
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
            assert!(matches!(spec.items[3].kind, ListKind::Ordered(1, '.')));
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
            assert!(matches!(all[0].kind, ListKind::Ordered(3, '.')));
            assert!(matches!(all[1].kind, ListKind::Ordered(4, '.')));
            assert_eq!(
                all[2].kind,
                ListKind::Task {
                    done: true,
                    ordered: None
                }
            );
            assert_eq!(
                all[3].kind,
                ListKind::Task {
                    done: false,
                    ordered: None
                }
            );
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
        fn list_then_blank_line_paragraph_keeps_both() {
            let p = prepare("- a\n\nnext para\n", Path::new("/d"));
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
        fn unindented_line_lazily_continues_the_item() {
            let p = prepare("- item\ncontinuation line\n\nnext para\n", Path::new("/d"));
            let spec = p.lists.values().next().unwrap();
            assert_eq!(spec.items.len(), 1);
            assert_eq!(spec.items[0].text, "item continuation line");
            assert_eq!(p.paragraphs.len(), 1, "next paragraph stays separate");
        }

        #[test]
        fn plus_bullets_and_paren_ordered_markers() {
            let p = prepare("+ a\n1) b\n", Path::new("/d"));
            let spec = p.lists.values().next().unwrap();
            assert_eq!(spec.items[0].kind, ListKind::Bullet);
            assert_eq!(spec.items[1].kind, ListKind::Ordered(1, ')'));
        }

        #[test]
        fn task_suffix_accepts_one_to_four_spaces_or_tab() {
            let p = prepare("-  [x] a\n-\t[ ] b\n", Path::new("/d"));
            let spec = p.lists.values().next().unwrap();
            assert_eq!(
                spec.items[0].kind,
                ListKind::Task {
                    done: true,
                    ordered: None
                }
            );
            assert_eq!(spec.items[0].text, "a");
            assert_eq!(
                spec.items[1].kind,
                ListKind::Task {
                    done: false,
                    ordered: None
                }
            );
            assert_eq!(spec.items[1].text, "b");
        }

        #[test]
        fn list_inside_fence_stays_untouched() {
            let p = prepare("```\n- a\n```\n", Path::new("/d"));
            assert!(p.lists.is_empty());
            assert!(p.code.values().any(|c| c.text == "- a"));
        }

        #[test]
        fn ordered_task_item_keeps_its_number() {
            let p = prepare("1. [x] done\n", Path::new("/d"));
            let spec = p.lists.values().next().unwrap();
            assert_eq!(
                spec.items[0].kind,
                ListKind::Task {
                    done: true,
                    ordered: Some((1, '.'))
                }
            );
        }

        #[test]
        fn nested_task_item_keeps_depth_and_state() {
            let p = prepare("- [ ] induk\n  - [x] anak\n", Path::new("/d"));
            let spec = p.lists.values().next().unwrap();
            assert_eq!(
                spec.items[0].kind,
                ListKind::Task {
                    done: false,
                    ordered: None
                }
            );
            assert_eq!(
                (spec.items[1].depth, spec.items[1].kind),
                (
                    1,
                    ListKind::Task {
                        done: true,
                        ordered: None
                    }
                )
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
            assert!(p.headings.values().any(|h| h.text == "h"));
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
            assert!(p.headings.values().any(|h| h.text == "h"));
        }

        #[test]
        fn setext_equals_heading_is_not_a_paragraph() {
            let md = "Title\n===\n";
            let p = prepare(md, Path::new("/d"));
            assert!(p.paragraphs.is_empty());
            assert!(p
                .headings
                .values()
                .any(|h| h.level == 1 && h.text == "Title"));
            assert!(!p.markdown.contains("==="));
        }

        #[test]
        fn setext_dash_heading_is_not_a_paragraph() {
            let md = "Title\n---\n";
            let p = prepare(md, Path::new("/d"));
            assert!(p.paragraphs.is_empty());
            assert!(p
                .headings
                .values()
                .any(|h| h.level == 2 && h.text == "Title"));
            assert!(!p.markdown.contains("Title\n---"));
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

        fn only(p: &Prepared) -> &HeadingSpec {
            assert_eq!(p.headings.len(), 1, "{:?}", p.headings);
            p.headings.values().next().unwrap()
        }

        #[test]
        fn atx_levels_one_to_six() {
            let md = "# h1\n\n## h2\n\n### h3\n\n#### h4\n\n##### h5\n\n###### h6\n";
            let p = prepare(md, Path::new("/d"));
            let mut levels: Vec<(u8, String)> = p
                .headings
                .values()
                .map(|h| (h.level, h.text.clone()))
                .collect();
            levels.sort();
            assert_eq!(
                levels,
                vec![
                    (1, "h1".to_string()),
                    (2, "h2".to_string()),
                    (3, "h3".to_string()),
                    (4, "h4".to_string()),
                    (5, "h5".to_string()),
                    (6, "h6".to_string()),
                ]
            );
            for h in p.headings.values() {
                assert!(p
                    .markdown
                    .contains(&format!("<mdheading id=\"{}\">\n\n", h.id)));
            }
            assert!(!p.markdown.contains("# h1"));
        }

        #[test]
        fn atx_text_drops_closing_hashes_and_markers() {
            assert_eq!(atx_heading("# Title ###"), Some((1, "Title")));
            assert_eq!(atx_heading("### foo#bar"), Some((3, "foo#bar")));
            assert_eq!(atx_heading("  # indented"), Some((1, "indented")));
            assert_eq!(atx_heading("#no-space"), None);
            assert_eq!(atx_heading("####### seven"), None);
        }

        #[test]
        fn setext_equals_is_level_one_and_dash_level_two() {
            let p = prepare("Title\n===\n\nSub\n---\n", Path::new("/d"));
            let mut levels: Vec<(u8, String)> = p
                .headings
                .values()
                .map(|h| (h.level, h.text.clone()))
                .collect();
            levels.sort();
            assert_eq!(
                levels,
                vec![(1, "Title".to_string()), (2, "Sub".to_string())]
            );
            assert!(p.paragraphs.is_empty());
            assert!(!p.markdown.contains("==="));
        }

        #[test]
        fn multiline_setext_title_joins_with_a_space() {
            let p = prepare("Long\nTitle\n===\n", Path::new("/d"));
            assert_eq!(only(&p).text, "Long Title");
        }

        #[test]
        fn heading_at_document_start_has_no_top_gap() {
            let p = prepare("# Title\n\nbody\n", Path::new("/d"));
            let h = only(&p);
            assert!(!h.space_before);
            assert!(h.space_after);
            assert!(p.markdown.starts_with("<mdheading"));
        }

        #[test]
        fn heading_at_eof_has_no_bottom_gap() {
            for md in ["body\n\n## Last", "body\n\n## Last\n\n"] {
                let p = prepare(md, Path::new("/d"));
                let h = only_final(&p);
                assert!(h.space_before, "{md:?}");
                assert!(!h.space_after, "{md:?}");
            }
        }

        fn only_final(p: &Prepared) -> &HeadingSpec {
            p.headings.values().find(|h| h.text == "Last").unwrap()
        }

        #[test]
        fn heading_between_paragraphs_gets_gaps_on_both_sides() {
            let p = prepare("before\n\n# Title\n\nafter\n", Path::new("/d"));
            assert_eq!(p.paragraphs.len(), 2);
            let h = only(&p);
            assert!(h.space_before);
            assert!(h.space_after);
        }

        #[test]
        fn consecutive_headings_keep_both_gaps() {
            let p = prepare("# A\n# B\n", Path::new("/d"));
            let a = p.headings.values().find(|h| h.text == "A").unwrap();
            let b = p.headings.values().find(|h| h.text == "B").unwrap();
            assert!(a.space_after);
            assert!(b.space_before);
        }

        #[test]
        fn setext_heading_at_document_start_has_no_top_gap() {
            let p = prepare("Title\n===\n\nbody\n", Path::new("/d"));
            let h = only(&p);
            assert!(!h.space_before);
            assert!(h.space_after);
            assert!(p.markdown.starts_with("<mdheading"));
        }

        #[test]
        fn thematic_break_is_not_a_heading() {
            let p = prepare("body\n\n---\n", Path::new("/d"));
            assert!(p.headings.is_empty());
            assert!(p.markdown.contains("---"));
        }

        #[test]
        fn indented_atx_heading_becomes_a_heading() {
            let p = prepare("before\n\n  ## indented\n\nafter\n", Path::new("/d"));
            assert_eq!(only(&p).level, 2);
            assert_eq!(only(&p).text, "indented");
        }

        #[test]
        fn four_space_indented_heading_stays_code() {
            let p = prepare("before\n\n    ## code\n\nafter\n", Path::new("/d"));
            assert!(p.headings.is_empty());
            assert!(p.markdown.contains("    ## code"));
        }

        #[test]
        fn raw_html_block_keeps_hash_line_literal() {
            let p = prepare("<div>\n# not a heading\n</div>\n", Path::new("/d"));
            assert!(p.headings.is_empty());
            assert!(p.markdown.contains("<div>\n# not a heading\n</div>"));
            assert!(p.paragraphs.is_empty());
        }

        #[test]
        fn heading_inside_a_fence_is_untouched() {
            let p = prepare("```\n# not a heading\n```\n", Path::new("/d"));
            assert!(p.headings.is_empty());
            assert!(p.code.values().any(|c| c.text == "# not a heading"));
        }

        #[test]
        fn headings_in_quotes_lists_and_indented_code_are_untouched() {
            let p = prepare("> # q\n\n- # item\n\n    # code\n", Path::new("/d"));
            assert!(p.headings.is_empty());
            assert!(p.markdown.contains("> # q"));
            assert!(p.markdown.contains("    # code"));
            assert_eq!(p.lists.values().next().unwrap().items[0].text, "# item");
        }

        #[test]
        fn heading_id_is_stable_and_content_sensitive() {
            assert_eq!(heading_id(1, "x", 0), heading_id(1, "x", 0));
            assert_ne!(heading_id(1, "x", 0), heading_id(2, "x", 0));
            assert_ne!(heading_id(1, "x", 0), heading_id(1, "y", 0));
            assert_ne!(heading_id(1, "x", 0), heading_id(1, "x", 1));
            assert_eq!(heading_id(1, "x", 0).len(), 16);
        }

        /// Heading ids in placeholder order (the map itself is unordered).
        fn ids_in_order(markdown: &str) -> Vec<String> {
            let mut ids = Vec::new();
            let mut rest = markdown;
            while let Some(k) = rest.find("<mdheading id=\"") {
                rest = &rest[k + "<mdheading id=\"".len()..];
                let end = rest.find('"').unwrap();
                ids.push(rest[..end].to_string());
            }
            ids
        }

        #[test]
        fn repeated_heading_at_start_keeps_both_gap_flags() {
            let p = prepare("# A\n\nbody\n\n# A", Path::new("/d"));
            assert_eq!(p.headings.len(), 2, "{:?}", p.headings);
            let ids = ids_in_order(&p.markdown);
            assert_eq!(ids.len(), 2);
            assert_ne!(ids[0], ids[1]);
            let first = &p.headings[&ids[0]];
            let second = &p.headings[&ids[1]];
            // Document start: no top gap. The second occurrence is not the
            // first's overwritten record.
            assert!(!first.space_before);
            assert!(first.space_after);
            assert!(second.space_before);
            assert!(!second.space_after);
        }

        #[test]
        fn repeated_heading_keeps_first_space_after() {
            let p = prepare("body\n\n# A\n\n# A", Path::new("/d"));
            let ids = ids_in_order(&p.markdown);
            assert_eq!(ids.len(), 2);
            assert_ne!(ids[0], ids[1]);
            assert!(p.headings[&ids[0]].space_after);
            assert!(!p.headings[&ids[1]].space_after);
        }

        #[test]
        fn crlf_heading_text_has_no_carriage_returns() {
            let p = prepare("Title\r\n===\r\n", Path::new("/d"));
            let h = only(&p);
            assert_eq!(h.text, "Title");
            assert!(!h.text.contains('\r'));
        }
    }

    mod links {
        use super::super::*;

        #[test]
        fn reference_links_resolve_and_definitions_disappear() {
            let md = "[full][r], [r], [collapsed][] and `[r]`\n\n[r]: https://example.com \"t\"\n[collapsed]: https://example.com/c\n";
            let p = prepare(md, Path::new("/d"));
            let out = p.paragraphs.values().next().unwrap();
            assert!(out.contains("[full](https://example.com)"), "{out}");
            assert!(out.contains("[r](https://example.com)"), "{out}");
            assert!(out.contains("[collapsed](https://example.com/c)"), "{out}");
            assert!(out.contains("`[r]`"), "code span untouched: {out}");
            assert!(!p.markdown.contains("[r]:"), "{:?}", p.markdown);
        }

        #[test]
        fn mermaid_inside_a_blockquote_is_extracted() {
            let md = "> ```mermaid\n> graph TD; A-->B;\n> ```\n\nafter\n";
            let p = prepare(md, Path::new("/d"));
            assert_eq!(p.diagrams.len(), 1, "{:?}", p.markdown);
            assert!(p.diagrams.values().any(|s| s.contains("graph TD; A-->B;")));
            assert!(!p.markdown.contains("```mermaid"));
            assert!(p.markdown.contains("<mdpara"), "{:?}", p.markdown);
        }

        #[test]
        fn reference_images_resolve() {
            let md = "![alt][img]\n\n[img]: pic.png\n";
            let p = prepare(md, Path::new("/d"));
            let spec = p.images.values().next().expect("reference image extracted");
            assert_eq!(spec.src, "/d/pic.png");
            assert_eq!(spec.alt, "alt");
        }

        #[test]
        fn bare_urls_and_emails_are_linkified() {
            let md = "see https://x.test/p. and www.example.com, mail a@b.co now\n";
            let p = prepare(md, Path::new("/d"));
            let out = p.paragraphs.values().next().unwrap();
            assert!(
                out.contains("[https://x.test/p](https://x.test/p)"),
                "{out}"
            );
            assert!(
                out.contains("[www.example.com](https://www.example.com)"),
                "{out}"
            );
            assert!(out.contains("[a@b.co](mailto:a@b.co)"), "{out}");
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
            // Never fetched: absolute and remote images show their alt text.
            assert!(out.contains('b'));
            assert!(out.contains('c'));
            assert!(!out.contains("https://h/c.png"));
            assert!(!out.contains("/abs.png"));
        }

        #[test]
        fn balanced_parens_and_title_in_image_targets() {
            let md = "![a](x(1).png \"caption\") ![b](a#b.png)";
            let p = prepare(md, Path::new("/docs"));
            let out = p.paragraphs.values().next().unwrap();
            assert!(out.contains("![a](file:///docs/x(1).png)"));
            assert!(out.contains("![b](file:///docs/a#b.png)"));
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
        fn remote_data_and_absolute_images_show_alt_text() {
            for target in [
                "https://example.com/x.png",
                "http://example.com/x.png",
                "data:image/png;base64,AAAA",
                "/abs/x.png",
            ] {
                let md = format!("![x]({target})\n");
                let p = prepare(&md, Path::new("/docs"));
                assert!(p.images.is_empty(), "{target} is never fetched");
                assert!(
                    p.paragraphs.values().all(|s| !s.contains(target)),
                    "{target} target must not be rendered"
                );
                assert!(
                    p.paragraphs.values().any(|s| s.contains('x')),
                    "{target} must show its alt text"
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
            assert!(source.contains("![x](<file:///docs/a b.png>)"));
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
