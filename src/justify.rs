//! Inline parsing and word-by-word justified painting for body paragraphs.
//! epaint has no justification, so each word is laid out on its own and the
//! leftover column width is divided between the gaps of every line but the last.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Galley, Id, Pos2, Rect, Sense, Stroke, TextStyle};

/// One styled piece of a paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// The visible characters of this piece.
    pub text: String,
    /// How the text is styled.
    pub kind: RunKind,
}

/// Inline style of a [`Run`].
#[derive(Clone, Debug, PartialEq)]
pub enum RunKind {
    /// Plain body text.
    Text,
    /// Bold text.
    Bold,
    /// Italic text.
    Italic,
    /// Text that is both bold and italic.
    BoldItalic,
    /// Inline code span.
    Code,
    /// Inline code inside bold emphasis; keeps the code background but uses
    /// the bold face.
    BoldCode,
    /// Struck-through text.
    Strike,
    /// Link text, carrying the target URL. Shared so cloning a run per
    /// painted fragment is a refcount bump, not a string copy.
    Link(Rc<str>),
    /// A forced line break (`  ` + newline or `\\` + newline).
    Break,
}

impl RunKind {
    /// Whether this run is an inline code span (bold or not).
    pub(crate) fn is_code(&self) -> bool {
        matches!(self, RunKind::Code | RunKind::BoldCode)
    }
}

/// Cap on memoised inline parses. Cleared wholesale on overflow: the point is
/// to keep painting cheap, not to keep every block of a huge document.
const MAX_INLINE_CACHE: usize = 2048;

thread_local! {
    /// Parsed runs per source text. Every visible block parses on every frame,
    /// and the parser is quadratic on adversarial input (unclosed `[` or
    /// backticks, dense emphasis), so a document could otherwise freeze the UI.
    static INLINE_CACHE: RefCell<HashMap<String, Rc<Vec<Run>>>> = RefCell::new(HashMap::new());
}

/// [`parse_inline`] memoised by source text, for the per-frame paint paths.
pub(crate) fn parse_inline_cached(md: &str) -> Rc<Vec<Run>> {
    INLINE_CACHE.with(|cache| {
        if let Some(runs) = cache.borrow().get(md) {
            return Rc::clone(runs);
        }
        let runs = Rc::new(parse_inline(md));
        let mut cache = cache.borrow_mut();
        if cache.len() >= MAX_INLINE_CACHE {
            cache.clear();
        }
        cache.insert(md.to_string(), Rc::clone(&runs));
        runs
    })
}

/// Drop the memoised parses; a reloaded document has different text.
pub(crate) fn clear_inline_cache() {
    INLINE_CACHE.with(|cache| cache.borrow_mut().clear());
}

/// Parse the inline Markdown the viewer supports. Single left-to-right scan:
/// backslash escapes, code spans first (they win over emphasis), `**` before
/// `*`, and an unbalanced marker is emitted literally instead of swallowing the
/// rest of the paragraph.
pub fn parse_inline(md: &str) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut text = String::new();
    let mut i = 0;

    while i < md.len() {
        let rest = &md[i..];
        let Some(c) = rest.chars().next() else {
            break;
        };

        if c == '\\' {
            if let Some(next) = rest[1..].chars().next() {
                if next == '\n' {
                    flush_text(&mut runs, &mut text);
                    runs.push(Run {
                        text: String::new(),
                        kind: RunKind::Break,
                    });
                    i += 2;
                    continue;
                }
                text.push(next);
                i += 1 + next.len_utf8();
            } else {
                text.push('\\');
                i += 1;
            }
            continue;
        }

        if c == '&' {
            if let Some((decoded, len)) = decode_entity(rest) {
                text.push(decoded);
                i += len;
                continue;
            }
            text.push('&');
            i += 1;
            continue;
        }

        if c == '`' {
            if let Some((start, end, used)) = code_span_at(rest) {
                flush_text(&mut runs, &mut text);
                runs.push(Run {
                    text: rest[start..end].to_string(),
                    kind: RunKind::Code,
                });
                i += used;
                continue;
            }
            text.push('`');
            i += 1;
            continue;
        }

        if c == '[' {
            if let Some((label, url, len)) = parse_link(rest) {
                flush_text(&mut runs, &mut text);
                runs.push(Run {
                    text: unescape(label),
                    kind: RunKind::Link(url),
                });
                i += len;
                continue;
            }
            text.push('[');
            i += 1;
            continue;
        }

        if rest.starts_with("***") || rest.starts_with("___") {
            let m = &rest[..3];
            if delimiter_can_open(md, i, 3, c) {
                if let Some(end) = find_marker(&rest[3..], m, c) {
                    flush_text(&mut runs, &mut text);
                    let inner = parse_inline(&rest[3..3 + end]);
                    runs.extend(inner.into_iter().map(|r| Run {
                        kind: combine(&RunKind::BoldItalic, r.kind),
                        ..r
                    }));
                    i += end + 6;
                    continue;
                }
            }
            text.push_str(m);
            i += 3;
            continue;
        }

        if rest.starts_with("**") || rest.starts_with("__") {
            let m = &rest[..2];
            if delimiter_can_open(md, i, 2, c) {
                if let Some(end) = find_marker(&rest[2..], m, c) {
                    flush_text(&mut runs, &mut text);
                    let inner = parse_inline(&rest[2..2 + end]);
                    runs.extend(inner.into_iter().map(|r| Run {
                        kind: combine(&RunKind::Bold, r.kind),
                        ..r
                    }));
                    i += end + 4;
                    continue;
                }
            }
            text.push_str(m);
            i += 2;
            continue;
        }

        if c == '*' || c == '_' {
            if delimiter_can_open(md, i, 1, c) {
                if let Some(end) = find_single(&rest[1..], c) {
                    flush_text(&mut runs, &mut text);
                    let inner = parse_inline(&rest[1..1 + end]);
                    runs.extend(inner.into_iter().map(|r| Run {
                        kind: combine(&RunKind::Italic, r.kind),
                        ..r
                    }));
                    i += end + 2;
                    continue;
                }
            }
            text.push(c);
            i += 1;
            continue;
        }

        if let Some(after) = rest.strip_prefix("~~") {
            if delimiter_can_open(md, i, 2, '~') {
                if let Some(end) = find_marker(after, "~~", '~') {
                    flush_text(&mut runs, &mut text);
                    runs.push(Run {
                        text: unescape(&after[..end]),
                        kind: RunKind::Strike,
                    });
                    i += end + 4;
                    continue;
                }
            }
            text.push_str("~~");
            i += 2;
            continue;
        }

        text.push(c);
        i += c.len_utf8();
    }

    flush_text(&mut runs, &mut text);
    runs
}

fn flush_text(runs: &mut Vec<Run>, text: &mut String) {
    if !text.is_empty() {
        runs.push(Run {
            text: std::mem::take(text),
            kind: RunKind::Text,
        });
    }
}

/// Decode the handful of HTML entities the library used to handle, plus the
/// non-breaking space (rendered as a normal space: no visual line break is
/// wanted in cells). Returns the decoded char and the bytes consumed; unknown
/// entities start with `:` and stay literal.
fn decode_entity(s: &str) -> Option<(char, usize)> {
    for (name, ch) in [
        ("&amp;", '&'),
        ("&lt;", '<'),
        ("&gt;", '>'),
        ("&quot;", '"'),
        ("&#39;", '\''),
        ("&apos;", '\''),
        ("&nbsp;", ' '),
    ] {
        if s.starts_with(name) {
            return Some((ch, name.len()));
        }
    }
    None
}

/// A delimiter run may open only when the next character is neither whitespace
/// nor another marker; `_` runs additionally may not be intraword.
fn delimiter_can_open(md: &str, at: usize, len: usize, ch: char) -> bool {
    let prev = md[..at].chars().last();
    let Some(next) = md[at + len..].chars().next() else {
        return false;
    };
    // A delimiter may open before a code span: `**`code`**` is bold text
    // containing inline code. Only a bare marker or whitespace blocks it.
    if next.is_whitespace() || matches!(next, '*' | '_' | '~') {
        return false;
    }
    if ch == '_' && prev.is_some_and(char::is_alphanumeric) && next.is_alphanumeric() {
        return false;
    }
    true
}

/// A delimiter run may close only when the previous character is not
/// whitespace; `_` runs additionally may not be intraword.
fn delimiter_can_close(s: &str, at: usize, len: usize, ch: char) -> bool {
    let prev = if at == 0 {
        Some(ch)
    } else {
        s[..at].chars().last()
    };
    let next = s[at + len..].chars().next();
    let Some(prev) = prev else {
        return false;
    };
    if prev.is_whitespace() {
        return false;
    }
    if ch == '_' && prev.is_alphanumeric() && next.is_some_and(char::is_alphanumeric) {
        return false;
    }
    true
}

/// Length of the backtick run at the start of `s` (0 when it starts with
/// anything else).
fn backtick_run(s: &str) -> usize {
    s.bytes().take_while(|&b| b == b'`').count()
}

/// The code span at the start of `s` as `(interior_start, interior_end, used)`.
/// The closing run must have exactly the opener's length (CommonMark), so
/// `` ``a`b`` `` is one span and `` `a` `` does not close it.
fn code_span_at(s: &str) -> Option<(usize, usize, usize)> {
    let open = backtick_run(s);
    if open == 0 {
        return None;
    }
    let mut i = open;
    while i < s.len() {
        let rest = &s[i..];
        let run = backtick_run(rest);
        if run == open {
            return Some((open, i, i + run));
        }
        let Some(c) = rest.chars().next() else {
            break;
        };
        i += if run > 0 { run } else { c.len_utf8() };
    }
    None
}

/// Length of a code span at the start of `s` (`None` when `s` does not start
/// with a backtick or the span is unterminated). Shared with the document
/// preparser so reference/autolink rewriting also skips code spans.
pub(crate) fn code_span_len(s: &str) -> Option<usize> {
    code_span_at(s).map(|(_, _, used)| used)
}

/// First closing candidate for `marker` in `s`, skipping escaped markers and
/// code spans (code wins over emphasis) and honouring the flanking rules.
fn find_marker(s: &str, marker: &str, ch: char) -> Option<usize> {
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if let Some(n) = code_span_len(rest) {
            i += n;
            continue;
        }
        if rest.starts_with(marker) && !escaped(s, i) && delimiter_can_close(s, i, marker.len(), ch)
        {
            return Some(i);
        }
        let Some(c) = rest.chars().next() else {
            break;
        };
        i += c.len_utf8();
    }
    None
}

/// First unescaped `ch` that is not part of a doubled marker, so `*a **b** c*`
/// closes on the final star rather than the first one of `**`. Code spans are
/// skipped.
fn find_single(s: &str, ch: char) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if let Some(n) = code_span_len(rest) {
            i += n;
            continue;
        }
        let Some(c) = rest.chars().next() else {
            break;
        };
        if c == ch && !escaped(s, i) {
            let prev_double = i > 0 && bytes[i - 1] == ch as u8;
            let next_double = bytes.get(i + 1) == Some(&(ch as u8));
            if !prev_double && !next_double && delimiter_can_close(s, i, 1, ch) {
                return Some(i);
            }
        }
        i += c.len_utf8();
    }
    None
}

/// Whether the byte at `at` is preceded by an odd number of backslashes.
fn escaped(s: &str, at: usize) -> bool {
    let bytes = s.as_bytes();
    let mut n = 0;
    let mut i = at;
    while i > 0 && bytes[i - 1] == b'\\' {
        n += 1;
        i -= 1;
    }
    n % 2 == 1
}

/// `[label](target)` starting at `s[0] == '['`; returns the label, the target
/// (an optional `"title"` is dropped) and the bytes consumed.
fn parse_link(s: &str) -> Option<(&str, Rc<str>, usize)> {
    let mid = s.find("](")?;
    let label = &s[1..mid];
    let (target, end) = link_inner(&s[mid + 2..])?;
    let url = target.split_whitespace().next()?;
    if url.is_empty() {
        return None;
    }
    Some((label, Rc::from(url), mid + 2 + end))
}

/// Split the text after `](` into the link body and the index just past the
/// closing `)`, balancing parentheses so `x(1).png` is one target and a closing
/// paren inside a `"title"` does not end the link. Shared with the document
/// preparser.
pub(crate) fn link_inner(s: &str) -> Option<(&str, usize)> {
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

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            if let Some(next) = it.next() {
                out.push(next);
            } else {
                out.push('\\');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Apply an outer emphasis kind to an inner run. Text becomes the outer style;
/// bold+italic collapses to `BoldItalic`; code and links keep their own kind.
fn combine(outer: &RunKind, inner: RunKind) -> RunKind {
    match (outer, inner) {
        (RunKind::Bold, RunKind::Text) => RunKind::Bold,
        (RunKind::Bold, RunKind::Code) | (RunKind::BoldItalic, RunKind::Code) => RunKind::BoldCode,
        (RunKind::Bold, RunKind::Italic) | (RunKind::Italic, RunKind::Bold) => RunKind::BoldItalic,
        (RunKind::Bold, RunKind::BoldItalic) | (RunKind::Italic, RunKind::BoldItalic) => {
            RunKind::BoldItalic
        }
        (RunKind::Italic, RunKind::Text) => RunKind::Italic,
        (RunKind::BoldItalic, RunKind::Text)
        | (RunKind::BoldItalic, RunKind::Italic)
        | (RunKind::BoldItalic, RunKind::Bold) => RunKind::BoldItalic,
        (_, k) => k,
    }
}

/// Greedy word wrap: a word joins the line only when `gap + word` still fits;
/// a lone over-wide word gets its own line and overflows instead of being cut.
/// `gaps[i]` is the gap before word `i` (ignored at the start of a line), so
/// pieces of one token can be glued together with a zero gap.
#[cfg(test)]
pub fn wrap_words(widths: &[f32], gaps: &[f32], column: f32) -> Vec<Vec<usize>> {
    let breaks = vec![false; widths.len()];
    wrap_words_with_breaks(widths, gaps, &breaks, column)
}

/// [`wrap_words`] with forced line breaks: a `true` entry closes the line after
/// that (zero-width) word.
pub(crate) fn wrap_words_with_breaks(
    widths: &[f32],
    gaps: &[f32],
    breaks: &[bool],
    column: f32,
) -> Vec<Vec<usize>> {
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut used = 0.0;
    for (i, &w) in widths.iter().enumerate() {
        let gap = if cur.is_empty() {
            0.0
        } else {
            gaps.get(i).copied().unwrap_or(0.0)
        };
        if cur.is_empty() || used + gap + w <= column {
            used += gap + w;
            cur.push(i);
        } else {
            lines.push(std::mem::take(&mut cur));
            used = w;
            cur.push(i);
        }
        if breaks.get(i).copied().unwrap_or(false) {
            lines.push(std::mem::take(&mut cur));
            used = 0.0;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// How much a separator is preferred as a break point; `0` means "not a
/// separator". Ties are resolved by taking the latest fitting separator so the
/// line stays as full as possible.
fn break_priority(c: char) -> u8 {
    match c {
        '/' => 3,
        '-' | '_' => 2,
        ':' | '.' => 1,
        ',' | ';' | '?' | '&' | '=' | '\\' => 1,
        _ => 0,
    }
}

/// Split an over-wide token into pieces that each measure at most `max_width`.
/// Prefers cutting just after a separator when that piece is at least half full;
/// otherwise hard-cuts at the largest fitting char boundary. A char wider than
/// `max_width` forms its own piece, so the loop always advances and no character
/// is ever dropped. Prefixes are measured once per candidate, never the whole
/// remaining suffix, so a huge token stays linear in layouts.
pub fn split_token(word: &str, max_width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0;
    while start < word.len() {
        let rest = &word[start..];
        let mut last_fit = None;
        let mut sep_cut: Option<(usize, f32)> = None;
        for (off, ch) in rest.char_indices() {
            let end = start + off + ch.len_utf8();
            let w = measure(&word[start..end]);
            if w > max_width {
                break;
            }
            last_fit = Some(end);
            if break_priority(ch) > 0 {
                sep_cut = Some((end, w));
            }
        }
        if last_fit == Some(word.len()) {
            parts.push(rest.to_string());
            break;
        }
        let cut = match sep_cut {
            Some((end, w)) if w >= max_width * 0.5 => end,
            _ => last_fit.unwrap_or_else(|| start + rest.chars().next().map_or(0, char::len_utf8)),
        };
        parts.push(word[start..cut].to_string());
        start = cut;
    }
    parts
}

struct Frag {
    kind: RunKind,
    color: Color32,
    format: TextFormat,
    text: String,
    /// Char offset of `text` in the block's recorded search text.
    offset: usize,
    galley: Arc<Galley>,
    width: f32,
}

struct Word {
    frags: Vec<Frag>,
    width: f32,
    /// No gap before this word: it is a continuation piece of the previous one.
    glued: bool,
    /// Produced by splitting an over-wide token (head and continuations alike).
    split: bool,
    /// A hard break marker: close the line after this zero-width word.
    force_break: bool,
}

/// Visible text of inline Markdown exactly as the painter lays it out: run
/// texts joined in order, escapes and entities already decoded by
/// [`parse_inline`], link labels kept, inline HTML kept as literal text, and
/// image syntax kept literally (`![alt](x)` becomes `!alt`). Whitespace in
/// non-code runs collapses to one space because the painter splits those runs
/// on whitespace; code spans keep their literal interior characters.
pub fn search_text(md: &str) -> String {
    let mut out = String::new();
    for run in parse_inline(md) {
        match run.kind {
            RunKind::Code | RunKind::BoldCode => out.push_str(&run.text),
            RunKind::Break => push_collapsed(&mut out, " "),
            _ => push_collapsed(&mut out, &run.text),
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Append `text` to `out`, collapsing whitespace runs to one space and skipping
/// leading whitespace.
fn push_collapsed(out: &mut String, text: &str) {
    for c in text.chars() {
        if c.is_whitespace() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
}

/// Drop raw HTML tags (`<b>`, `</b>`, `<br>`, comments) from visible text.
/// `<` not followed by a tag-like character is kept literally.
pub(crate) fn strip_html(s: &str) -> String {
    if !s.contains('<') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if let Some(len) = html_tag_len(rest) {
            i += len;
            continue;
        }
        let Some(c) = rest.chars().next() else {
            break;
        };
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// Byte length of a tag-like `<...>` at the start of `s`, if any.
fn html_tag_len(s: &str) -> Option<usize> {
    let rest = s.strip_prefix('<')?;
    let first = rest.chars().next()?;
    if !(first.is_ascii_alphabetic() || matches!(first, '/' | '!' | '?')) {
        return None;
    }
    rest.find('>').map(|end| end + 2)
}

/// Layout every word fragment with the real egui font metrics. Words are split
/// on whitespace; a code span stays one atomic word. `face` overrides the body
/// font (headings scale it); the strong colour follows the body colour so a
/// heading's bold runs stay one ink.
fn build_words(ui: &egui::Ui, runs: &[Run], face: Option<&(FontId, Color32)>) -> Vec<Word> {
    let body = face.map_or_else(
        || {
            ui.style()
                .text_styles
                .get(&TextStyle::Body)
                .cloned()
                .unwrap_or_else(|| FontId::proportional(14.0))
        },
        |(font, _)| font.clone(),
    );
    let mono_style = ui
        .style()
        .text_styles
        .get(&TextStyle::Monospace)
        .cloned()
        .unwrap_or_else(|| FontId::monospace(14.0));
    // Inline code in a heading scales with it instead of going microscopic.
    let mono = match face {
        Some((font, _)) if font.size > 0.0 => {
            let base = ui
                .style()
                .text_styles
                .get(&TextStyle::Body)
                .map_or(font.size, |b| b.size);
            FontId::new(
                mono_style.size * font.size / base,
                mono_style.family.clone(),
            )
        }
        _ => mono_style,
    };
    let text_color = face.map_or_else(|| ui.visuals().text_color(), |(_, color)| *color);
    let strong = face.map_or_else(|| ui.visuals().strong_text_color(), |(_, color)| *color);
    let link_color = ui.visuals().hyperlink_color;

    let mut words: Vec<Word> = Vec::new();
    let mut open = false; // a word is being filled
    for frag in fragment_plan(runs) {
        if frag.brk {
            words.push(Word {
                frags: Vec::new(),
                width: 0.0,
                glued: false,
                split: false,
                force_break: true,
            });
            open = false;
            continue;
        }
        if frag.gap_before {
            open = false;
        }
        let run = &runs[frag.run];
        push_frag(
            ui,
            &mut words,
            &mut open,
            frag.text,
            frag.offset,
            run,
            &body,
            &mono,
            text_color,
            strong,
            link_color,
        );
    }
    words
}

/// A painted fragment and its char offset in the block's recorded search text.
struct PlanFragment {
    /// Index into the run list.
    run: usize,
    /// The fragment's visible text.
    text: String,
    /// Char offset of `text` in `search_text(runs)`.
    offset: usize,
    /// The fragment starts a new word (whitespace precedes it).
    gap_before: bool,
    /// A forced line break: no text, closes the current word line.
    brk: bool,
}

/// Char cursor over the text model produced by [`search_text`]: whitespace in
/// non-code runs collapses to one space, leading whitespace is skipped, and code
/// runs are appended literally. Keeping the same state here is what makes
/// fragment offsets line up with highlight ranges.
#[derive(Default)]
struct RecordedCursor {
    len: usize,
    ends_space: bool,
}

impl RecordedCursor {
    /// Append a code run literally and return its start. An empty span (``)
    /// changes nothing, not even whether the previous char was a space.
    fn code(&mut self, text: &str) -> usize {
        let start = self.len;
        if text.is_empty() {
            return start;
        }
        self.len += text.chars().count();
        self.ends_space = text.ends_with(' ');
        start
    }

    /// Append one non-whitespace char and return its offset.
    fn push(&mut self, _ch: char) -> usize {
        let at = self.len;
        self.len += 1;
        self.ends_space = false;
        at
    }

    /// Fold one whitespace char into the model: one space between words, none
    /// at the start and none after a space already present.
    fn space(&mut self) {
        if self.len > 0 && !self.ends_space {
            self.len += 1;
            self.ends_space = true;
        }
    }
}

/// Split the runs into painted fragments with offsets in the recorded text.
fn fragment_plan(runs: &[Run]) -> Vec<PlanFragment> {
    let mut plan = Vec::new();
    let mut cursor = RecordedCursor::default();
    let mut open = false;
    for (run_index, run) in runs.iter().enumerate() {
        if matches!(run.kind, RunKind::Break) {
            cursor.space();
            plan.push(PlanFragment {
                run: run_index,
                text: String::new(),
                offset: cursor.len,
                gap_before: true,
                brk: true,
            });
            open = false;
            continue;
        }
        if run.kind.is_code() {
            if run.text.is_empty() {
                // An empty code span paints nothing and owns no word.
                continue;
            }
            let offset = cursor.code(&run.text);
            plan.push(PlanFragment {
                run: run_index,
                text: run.text.clone(),
                offset,
                gap_before: !open,
                brk: false,
            });
            open = true;
            continue;
        }
        let mut buf = String::new();
        let mut buf_start = 0usize;
        for ch in run.text.chars() {
            if ch.is_whitespace() {
                if !buf.is_empty() {
                    plan.push(PlanFragment {
                        run: run_index,
                        text: std::mem::take(&mut buf),
                        offset: buf_start,
                        gap_before: !open,
                        brk: false,
                    });
                }
                cursor.space();
                open = false;
            } else {
                if buf.is_empty() {
                    buf_start = cursor.push(ch);
                } else {
                    cursor.push(ch);
                }
                buf.push(ch);
            }
        }
        if !buf.is_empty() {
            plan.push(PlanFragment {
                run: run_index,
                text: buf,
                offset: buf_start,
                gap_before: !open,
                brk: false,
            });
            open = true;
        }
    }
    plan
}

/// Paint-level word grouping: each entry is `(gap_before, fragment texts)`.
/// Mirrors `build_words`: a fragment with `gap_before` starts a new word, the
/// rest glue into the current one.
#[cfg(test)]
fn word_plan(runs: &[Run]) -> Vec<(bool, Vec<String>)> {
    let mut words: Vec<(bool, Vec<String>)> = Vec::new();
    for fragment in fragment_plan(runs) {
        match words.last_mut() {
            Some((_, texts)) if !fragment.gap_before => texts.push(fragment.text),
            _ => words.push((fragment.gap_before, vec![fragment.text])),
        }
    }
    words
}

/// Lay out one text fragment and append it to the current word, opening a new
/// word when none is in progress.
#[expect(
    clippy::too_many_arguments,
    reason = "one fragment needs font, colours, text and run context"
)]
fn push_frag(
    ui: &egui::Ui,
    words: &mut Vec<Word>,
    open: &mut bool,
    text: String,
    offset: usize,
    run: &Run,
    body: &FontId,
    mono: &FontId,
    text_color: Color32,
    strong: Color32,
    link_color: Color32,
) {
    let (font, color, italics, underline, strikethrough) = match &run.kind {
        RunKind::Text => (body.clone(), text_color, false, Stroke::NONE, Stroke::NONE),
        RunKind::Bold => (
            crate::fonts::bold_font(body.size),
            strong,
            false,
            Stroke::NONE,
            Stroke::NONE,
        ),
        RunKind::Italic => (body.clone(), text_color, true, Stroke::NONE, Stroke::NONE),
        RunKind::BoldItalic => (
            crate::fonts::bold_font(body.size),
            strong,
            true,
            Stroke::NONE,
            Stroke::NONE,
        ),
        RunKind::Code => (mono.clone(), text_color, false, Stroke::NONE, Stroke::NONE),
        RunKind::BoldCode => (
            crate::fonts::bold_font(mono.size),
            strong,
            false,
            Stroke::NONE,
            Stroke::NONE,
        ),
        RunKind::Strike => (
            body.clone(),
            text_color,
            false,
            Stroke::NONE,
            Stroke::new(1.0, text_color),
        ),
        RunKind::Link(_) => (
            body.clone(),
            link_color,
            false,
            // Underline is painted per link span so spaces are covered too.
            Stroke::NONE,
            Stroke::NONE,
        ),
        // Zero-width; build_words turns it into a forced line break.
        RunKind::Break => (body.clone(), text_color, false, Stroke::NONE, Stroke::NONE),
    };
    let format = TextFormat {
        font_id: font,
        color,
        italics,
        underline,
        strikethrough,
        ..Default::default()
    };
    let galley = layout(ui, &text, &format);
    let width = galley.size().x;
    if !*open {
        words.push(Word {
            frags: Vec::new(),
            width: 0.0,
            glued: false,
            split: false,
            force_break: false,
        });
        *open = true;
    }
    let Some(word) = words.last_mut() else {
        return;
    };
    word.width += width;
    word.frags.push(Frag {
        kind: run.kind.clone(),
        color,
        format,
        text,
        offset,
        galley,
        width,
    });
}

fn layout(ui: &egui::Ui, text: &str, format: &TextFormat) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    job.append(text, 0.0, format.clone());
    ui.fonts_mut(|f| f.layout_job(job))
}

/// The glyph drawn to the left of an indented block of text, such as a list
/// item's marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    /// `•`, a top-level bullet.
    Bullet,
    /// `○`, a nested bullet. U+25E6 `◦` is not used: it is missing from
    /// FiraCode Nerd Font and from egui's fallback set, so it renders as tofu.
    Hollow,
    /// `{n}.` or `{n})`, an ordered item keeping its source number/delimiter.
    Number(u64, char),
    /// `☑` or `☐`, a task box, prefixed by `n.`/`n)` when ordered.
    Task(bool, Option<(u64, char)>),
}

impl Marker {
    /// The text painted for this marker.
    pub fn label(self) -> String {
        match self {
            Marker::Bullet => "•".to_string(),
            Marker::Hollow => "○".to_string(),
            Marker::Number(n, delim) => format!("{n}{delim}"),
            Marker::Task(true, ordered) | Marker::Task(false, ordered) => {
                let boxed = if matches!(self, Marker::Task(true, _)) {
                    "☑"
                } else {
                    "☐"
                };
                match ordered {
                    Some((n, d)) => format!("{n}{d} {boxed}"),
                    None => boxed.to_string(),
                }
            }
        }
    }
}

/// Left offset of a list item's marker: two spaces per nesting level, capped at
/// five levels so a deep list never eats the reading column.
pub fn list_indent(depth: u8, space_width: f32) -> f32 {
    depth.min(5) as f32 * 2.0 * space_width
}

/// One painted fragment's char range and screen rect, used for pointer hit
/// testing. `start`/`len` are char offsets in the block's recorded text.
#[derive(Clone, Debug, PartialEq)]
pub struct CharSpan {
    pub start: usize,
    pub len: usize,
    pub rect: Rect,
}

/// Result of painting one block: its used height, screen rect and the char map
/// for selection hit testing.
#[derive(Clone, Debug)]
pub struct PaintedBlock {
    pub height: f32,
    pub rect: Rect,
    pub chars: Vec<CharSpan>,
    /// Link rects: pressing these must not start a text selection.
    pub interactives: Vec<Rect>,
}

impl Default for PaintedBlock {
    fn default() -> Self {
        PaintedBlock {
            height: 0.0,
            rect: Rect::ZERO,
            chars: Vec::new(),
            interactives: Vec::new(),
        }
    }
}

/// Highlight ranges for one block, as char ranges into its recorded search
/// text, plus which range is the current match and the text selection.
#[derive(Clone, Copy, Default)]
pub struct MatchHighlight<'a> {
    /// Block-local char ranges.
    pub ranges: &'a [(usize, usize)],
    /// Index into `ranges` of the current match, if it belongs to this block.
    pub current: Option<usize>,
    /// Selected char range in this block, painted below search hits.
    pub selection: Option<(usize, usize)>,
}

/// Fill colour behind a match: the theme selection colour for all hits, and a
/// stronger (more opaque) version of the same ink for the current hit.
pub(crate) fn highlight_color(ui: &egui::Ui, current: bool) -> Color32 {
    let selection = ui.visuals().selection.bg_fill;
    if current {
        // `bg_fill` is premultiplied; feed its straight-alpha channels back so
        // the current fill is the same lavender, only more opaque.
        let [r, g, b, _] = selection.to_srgba_unmultiplied();
        Color32::from_rgba_unmultiplied(r, g, b, 0xb4)
    } else {
        selection
    }
}

/// Fill colour behind the text selection: the search selection ink at a lower
/// alpha, so search hits stay readable on top.
pub(crate) fn selection_color(ui: &egui::Ui) -> Color32 {
    let [r, g, b, _] = ui.visuals().selection.bg_fill.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, 0x40)
}

/// Ordered draw passes for one painted row. Code backgrounds must be below the
/// selection chips, and both below glyphs/search highlights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowPass {
    CodeBackgrounds,
    Selection,
    Text,
}

fn row_passes(has_code: bool, has_selection: bool) -> Vec<RowPass> {
    let mut passes = Vec::new();
    if has_code {
        passes.push(RowPass::CodeBackgrounds);
    }
    if has_selection {
        passes.push(RowPass::Selection);
    }
    passes.push(RowPass::Text);
    passes
}

/// Intersection of `ranges` with the char span `[start, start + text.chars())`,
/// returned as local char offsets within `text` plus the global range index.
/// Range ends are exclusive. Used by the quote renderer per line.
pub(crate) fn local_ranges(
    start: usize,
    text: &str,
    ranges: &[(usize, usize)],
) -> Vec<(usize, usize, usize)> {
    let end = start + text.chars().count();
    ranges
        .iter()
        .enumerate()
        .filter_map(|(index, &(s, e))| {
            let s = s.max(start);
            let e = e.min(end);
            if s < e {
                Some((s - start, e - start, index))
            } else {
                None
            }
        })
        .collect()
}

/// Justified variant with a hanging indent: the marker sits at `indent` and the
/// text starts one marker width plus one space later, so wrapped lines align
/// under the item's text. `column` is the whole block width.
pub fn draw_indented(
    ui: &mut egui::Ui,
    runs: &[Run],
    column: f32,
    indent: f32,
    marker: Marker,
    highlights: MatchHighlight<'_>,
) -> PaintedBlock {
    let column = column.max(0.0);
    let space = space_width(ui);
    let galley = layout(ui, &marker.label(), &marker_format(ui));
    let marker_width = galley.size().x;
    let offset = indent + marker_width + space;
    let text_width = (column - offset).max(0.0);
    if text_width <= 0.0 {
        return PaintedBlock::default();
    }
    paint_at(
        ui,
        runs,
        text_width,
        true,
        offset,
        Some((indent, galley)),
        None,
        highlights,
        0.0,
    )
}

/// Left-aligned variant of [`draw`]: same wrapping, splitting and painting, but
/// gaps never stretch and rows hug the left edge. Used by table cells.
pub fn draw_left(
    ui: &mut egui::Ui,
    runs: &[Run],
    column: f32,
    highlights: MatchHighlight<'_>,
) -> PaintedBlock {
    paint(ui, runs, column, false, highlights)
}

/// Paint `runs` with an explicit body font and text colour. `justify` selects
/// stretched or ragged rows, exactly like [`draw`] and [`draw_left`]. Used by
/// headings, which scale the body font per level instead of using the style.
pub fn draw_with(
    ui: &mut egui::Ui,
    runs: &[Run],
    column: f32,
    font: FontId,
    color: Color32,
    justify: bool,
    highlights: MatchHighlight<'_>,
) -> PaintedBlock {
    paint_at(
        ui,
        runs,
        column,
        justify,
        0.0,
        None,
        Some((font, color)),
        highlights,
        0.0,
    )
}

fn paint(
    ui: &mut egui::Ui,
    runs: &[Run],
    column: f32,
    justify: bool,
    highlights: MatchHighlight<'_>,
) -> PaintedBlock {
    paint_at(ui, runs, column, justify, 0.0, None, None, highlights, 0.0)
}

/// Like [`draw`] but reserves `gap` points of blank space below the paragraph,
/// so stacked blocks breathe. The painted/recorded rect excludes the gap.
pub fn draw_spaced(
    ui: &mut egui::Ui,
    runs: &[Run],
    column: f32,
    highlights: MatchHighlight<'_>,
    gap: f32,
) -> PaintedBlock {
    paint_at(ui, runs, column, true, 0.0, None, None, highlights, gap)
}

/// Vertical gap between top-level blocks (paragraphs, code, headings).
pub(crate) fn block_gap(ui: &egui::Ui) -> f32 {
    ui.text_style_height(&TextStyle::Body) * 0.7
}

pub(crate) fn body_font(ui: &egui::Ui) -> FontId {
    ui.style()
        .text_styles
        .get(&TextStyle::Body)
        .cloned()
        .unwrap_or_else(|| FontId::proportional(14.0))
}

pub(crate) fn space_width(ui: &egui::Ui) -> f32 {
    space_width_for(ui, &body_font(ui))
}

fn space_width_for(ui: &egui::Ui, font: &FontId) -> f32 {
    ui.fonts_mut(|f| {
        f.layout_no_wrap(" ".to_string(), font.clone(), Color32::PLACEHOLDER)
            .size()
            .x
    })
}

fn marker_format(ui: &egui::Ui) -> TextFormat {
    TextFormat {
        font_id: body_font(ui),
        color: ui.visuals().text_color(),
        ..Default::default()
    }
}

/// The paragraph painter, with an optional hanging indent: `offset` shifts every
/// row's text right of the allocated `rect`, and `marker` (x, galley) is painted
/// once at the first row. `face` overrides the body font and ink colour.
#[expect(
    clippy::too_many_arguments,
    reason = "the painter needs layout, face, marker and highlight context"
)]
fn paint_at(
    ui: &mut egui::Ui,
    runs: &[Run],
    column: f32,
    justify: bool,
    offset: f32,
    marker: Option<(f32, Arc<Galley>)>,
    face: Option<(FontId, Color32)>,
    highlights: MatchHighlight<'_>,
    trailing_gap: f32,
) -> PaintedBlock {
    let column = column.max(0.0);
    if column <= 0.0 {
        return PaintedBlock::default();
    }
    let body = face
        .as_ref()
        .map_or_else(|| body_font(ui), |(font, _)| font.clone());
    let row_h = ui.fonts_mut(|f| f.row_height(&body));
    let space = space_width_for(ui, &body);

    let words = split_overwide(ui, build_words(ui, runs, face.as_ref()), column);
    if words.is_empty() {
        // An empty item still shows its marker and keeps its row.
        if let Some((x, galley)) = marker {
            let (_, rect) = ui.allocate_space(egui::vec2(offset + column, row_h));
            ui.painter().galley(
                Pos2::new(rect.left() + x, rect.top()),
                galley,
                ui.visuals().text_color(),
            );
            return PaintedBlock {
                height: row_h,
                rect,
                chars: Vec::new(),
                interactives: Vec::new(),
            };
        }
        return PaintedBlock::default();
    }
    let widths: Vec<f32> = words.iter().map(|w| w.width).collect();
    let gaps: Vec<f32> = words
        .iter()
        .map(|w| if w.glued { 0.0 } else { space })
        .collect();
    let breaks: Vec<bool> = words.iter().map(|w| w.force_break).collect();
    let lines = wrap_words_with_breaks(&widths, &gaps, &breaks, column);
    let natural_of = |line: &[usize]| {
        line.iter()
            .enumerate()
            .map(|(k, &i)| widths[i] + if k == 0 { 0.0 } else { gaps[i] })
            .sum::<f32>()
    };

    let used_height = lines.len() as f32 * row_h;
    // Reserve the widest line's natural width so a lone char wider than the
    // column stays reachable by horizontal pan instead of being clipped.
    // Left-aligned mode paints inside a fixed-width cell, so it reserves exactly
    // the column and clips: a single over-wide glyph must not widen the table.
    let reserved = if justify {
        lines
            .iter()
            .map(|line| natural_of(line))
            .fold(column, f32::max)
    } else {
        column
    };
    let (_, allocated) =
        ui.allocate_space(egui::vec2(offset + reserved, used_height + trailing_gap));
    let rect = egui::Rect::from_min_size(allocated.min, egui::vec2(allocated.width(), used_height));
    let mut painter = ui.painter().clone();
    if !justify {
        let clip = egui::Rect::from_min_size(rect.min, egui::vec2(column, used_height));
        painter.set_clip_rect(ui.clip_rect().intersect(clip));
    }
    let left = rect.left() + offset;
    let top = rect.top();
    if let Some((x, galley)) = marker {
        painter.galley(
            Pos2::new(rect.left() + x, top),
            galley,
            ui.visuals().text_color(),
        );
    }
    let split_words: Vec<bool> = words.iter().map(|w| w.split).collect();
    let mut chars = Vec::new();
    let mut interactives = Vec::new();

    for (row, line) in lines.iter().enumerate() {
        let natural = natural_of(line);
        let extra = (column - natural).max(0.0);
        let last = row + 1 == lines.len();
        let stretchable = line.iter().skip(1).filter(|&&i| gaps[i] > 0.0).count();
        let split = touches_split(&split_words, line, lines.get(row + 1).map(Vec::as_slice));
        let grow = justified_grow(justify, last, extra, stretchable, split, space);

        let mut x = left;
        let y = top + row as f32 * row_h;
        let mut placements: Vec<(&Frag, f32)> = Vec::new();
        for (k, &wi) in line.iter().enumerate() {
            if k > 0 {
                let gap = gaps[wi];
                x += if gap > 0.0 { gap + grow } else { 0.0 };
            }
            for frag in &words[wi].frags {
                placements.push((frag, x));
                x += frag.width;
            }
        }
        // One chip per visual line, from the first selected glyph to the last,
        // so the spaces between words are covered and the line reads as one
        // continuous highlight — the way a browser or a PDF reader selects.
        let mut selection_span: Option<(f32, f32)> = None;
        if let Some((sel_start, sel_end)) = highlights.selection {
            let mut lo = f32::INFINITY;
            let mut hi = f32::NEG_INFINITY;
            for (frag, frag_x) in &placements {
                let frag_start = frag.offset;
                let frag_end = frag_start + frag.text.chars().count();
                let start = sel_start.max(frag_start);
                let end = sel_end.min(frag_end);
                if start >= end {
                    continue;
                }
                let prefix: String = frag.text.chars().take(start - frag_start).collect();
                let piece: String = frag
                    .text
                    .chars()
                    .skip(start - frag_start)
                    .take(end - start)
                    .collect();
                let prefix_w = layout(ui, &prefix, &frag.format).size().x;
                let piece_w = layout(ui, &piece, &frag.format).size().x;
                lo = lo.min(frag_x + prefix_w);
                hi = hi.max(frag_x + prefix_w + piece_w);
            }
            if hi > lo {
                selection_span = Some((lo, hi));
            }
        }
        let has_code = placements.iter().any(|(frag, _)| frag.kind.is_code());
        for pass in row_passes(has_code, selection_span.is_some()) {
            match pass {
                RowPass::CodeBackgrounds => {
                    for (frag, frag_x) in &placements {
                        if frag.kind.is_code() {
                            paint_code_background(ui, &painter, Pos2::new(*frag_x, y), frag);
                        }
                    }
                }
                RowPass::Selection => {
                    if let Some((lo, hi)) = selection_span {
                        painter.rect_filled(
                            Rect::from_min_size(Pos2::new(lo, y), egui::vec2(hi - lo, row_h)),
                            0.0,
                            selection_color(ui),
                        );
                    }
                }
                RowPass::Text => {
                    // One rect per match, so spaces inside the query are covered
                    // between words too.
                    for (index, &(start, end)) in highlights.ranges.iter().enumerate() {
                        if let Some((lo, hi)) = range_span(ui, &placements, start, end) {
                            let color = highlight_color(ui, highlights.current == Some(index));
                            painter.rect_filled(
                                Rect::from_min_size(Pos2::new(lo, y), egui::vec2(hi - lo, row_h)),
                                0.0,
                                color,
                            );
                        }
                    }
                    for (frag, frag_x) in &placements {
                        let pos = Pos2::new(*frag_x, y);
                        paint_frag(ui, &painter, pos, frag);
                        let len = frag.text.chars().count();
                        if len > 0 {
                            chars.push(CharSpan {
                                start: frag.offset,
                                len,
                                rect: Rect::from_min_size(pos, egui::vec2(frag.width, row_h)),
                            });
                        }
                    }
                    // One underline per link, spanning the spaces between its
                    // words; the same span owns clicks.
                    for (lo, hi, color) in link_spans(&placements) {
                        painter.rect_filled(
                            Rect::from_min_size(
                                Pos2::new(lo, y + row_h - 2.0),
                                egui::vec2(hi - lo, 1.0),
                            ),
                            0.0,
                            color,
                        );
                        interactives.push(Rect::from_min_size(
                            Pos2::new(lo, y),
                            egui::vec2(hi - lo, row_h),
                        ));
                    }
                }
            }
        }
    }
    PaintedBlock {
        height: used_height,
        rect,
        chars,
        interactives,
    }
}

/// A line borders a split token when it holds any piece of one (head or
/// continuation) or the next line starts with a piece. Such lines stay
/// left-aligned: stretching gaps next to a broken URL/path looks like inserted
/// spaces.
fn touches_split(split: &[bool], line: &[usize], next: Option<&[usize]>) -> bool {
    line.iter().any(|&i| split[i]) || next.and_then(|l| l.first()).is_some_and(|&i| split[i])
}

/// Extra width added to every stretchable gap on a line; zero for the last line,
/// lines with nothing to stretch, and lines touching a split token. Growth is
/// also capped at twice the natural `space`: a line that would need more is a
/// sparse line before a long token, and stretching it turns the words into
/// islands, so it stays ragged instead.
fn justified_grow(
    justify: bool,
    last: bool,
    extra: f32,
    stretchable: usize,
    split: bool,
    space: f32,
) -> f32 {
    if !justify || last || stretchable == 0 || split {
        return 0.0;
    }
    let grow = extra / stretchable as f32;
    if grow > space * 2.0 {
        0.0
    } else {
        grow
    }
}

/// Turn any single-fragment word that is wider than the column into glued
/// pieces, so long paths and URLs wrap instead of overflowing. Words made of
/// several fragments (adjacent styled runs) are left alone.
fn split_overwide(ui: &egui::Ui, words: Vec<Word>, column: f32) -> Vec<Word> {
    let mut out = Vec::with_capacity(words.len());
    for word in words {
        if word.width <= column || word.frags.len() != 1 {
            out.push(word);
            continue;
        }
        let frag = &word.frags[0];
        let measure = |s: &str| layout(ui, s, &frag.format).size().x;
        let parts = split_token(&frag.text, column, &measure);
        if parts.len() <= 1 {
            out.push(word);
            continue;
        }
        let first = out.len();
        let mut part_offset = 0usize;
        for part in parts {
            let galley = layout(ui, &part, &frag.format);
            let width = galley.size().x;
            let offset = frag.offset + part_offset;
            part_offset += part.chars().count();
            out.push(Word {
                frags: vec![Frag {
                    kind: frag.kind.clone(),
                    color: frag.color,
                    format: frag.format.clone(),
                    text: part,
                    offset,
                    galley,
                    width,
                }],
                width,
                glued: true,
                split: true,
                force_break: false,
            });
        }
        out[first].glued = false;
    }
    out
}

/// Screen span of one match on a line's placements, including the gap when the
/// match starts/ends in inter-word space.
fn range_span(
    ui: &egui::Ui,
    placements: &[(&Frag, f32)],
    start: usize,
    end: usize,
) -> Option<(f32, f32)> {
    let mut lo: Option<f32> = None;
    let mut hi: Option<f32> = None;
    let mut prev_end: Option<f32> = None;
    let mut last_matched_end: Option<usize> = None;
    for (frag, x) in placements {
        let frag_start = frag.offset;
        let frag_end = frag_start + frag.text.chars().count();
        if frag_end <= start {
            prev_end = Some(*x + frag.width);
            continue;
        }
        if frag_start >= end {
            // Only reach into the following gap when the query itself ends in
            // that gap (`"alpha "`); a match that stops on a glyph must stop at
            // that glyph, not eat the space after it.
            if hi.is_none() || last_matched_end.is_some_and(|e| end > e) {
                hi = Some(*x);
            }
            break;
        }
        let local_start = start.max(frag_start);
        let local_end = end.min(frag_end);
        if local_start >= local_end {
            continue;
        }
        let prefix: String = frag.text.chars().take(local_start - frag_start).collect();
        let piece: String = frag
            .text
            .chars()
            .skip(local_start - frag_start)
            .take(local_end - local_start)
            .collect();
        let x0 = *x + layout(ui, &prefix, &frag.format).size().x;
        let x1 = x0 + layout(ui, &piece, &frag.format).size().x;
        let lo0 = if start < frag_start {
            prev_end.unwrap_or(x0)
        } else {
            x0
        };
        lo = Some(lo.map_or(lo0, |v: f32| v.min(lo0)));
        hi = Some(hi.map_or(x1, |v: f32| v.max(x1)));
        last_matched_end = Some(frag_end);
        prev_end = Some(x1);
    }
    match (lo, hi) {
        (Some(lo), Some(hi)) if hi > lo => Some((lo, hi)),
        _ => None,
    }
}

/// Merged underline spans for links on one line: one `(lo, hi, color)` per
/// link label, covering inter-word spaces. A new span starts on a different
/// URL or a non-link fragment.
fn link_spans(placements: &[(&Frag, f32)]) -> Vec<(f32, f32, Color32)> {
    let mut out = Vec::new();
    let mut open: Option<(&str, f32, f32, usize, Color32)> = None;
    for (frag, x) in placements {
        let RunKind::Link(url) = &frag.kind else {
            if let Some((_, lo, hi, _, color)) = open.take() {
                out.push((lo, hi, color));
            }
            continue;
        };
        let url: &str = url;
        let start = frag.offset;
        let end = start + frag.text.chars().count();
        let extend = open
            .as_ref()
            .is_some_and(|(u, _, _, last, _)| *u == url && start <= last + 1);
        if extend {
            if let Some((_, _, hi, last, _)) = open.as_mut() {
                *hi = *x + frag.width;
                *last = end;
            }
        } else {
            if let Some((_, lo, hi, _, color)) = open.take() {
                out.push((lo, hi, color));
            }
            open = Some((url, *x, *x + frag.width, end, frag.color));
        }
    }
    if let Some((_, lo, hi, _, color)) = open {
        out.push((lo, hi, color));
    }
    out
}

fn paint_code_background(ui: &egui::Ui, painter: &egui::Painter, pos: Pos2, frag: &Frag) {
    let size = frag.galley.size();
    let bg = Rect::from_min_size(pos, size).expand2(egui::vec2(1.0, 0.0));
    painter.rect_filled(bg, 0.0, ui.visuals().code_bg_color);
}

/// Schemes a link may copy to the clipboard. Everything else (javascript:,
/// data:, file:, relative paths, bare anchors) stays inert text.
pub(crate) fn link_is_safe(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

/// One-frame handoff: a link click asked for the `link copied` status chip.
pub(crate) const LINK_COPIED: &str = "jiaoren-link-copied";

/// One-frame handoff: URL of the link under the pointer, read by `o`.
pub(crate) const HOVER_LINK: &str = "jiaoren-hover-link";

/// Copy a safe link and leave the status flag. Inert schemes copy nothing.
fn click_link(ctx: &egui::Context, url: &str) -> bool {
    if !link_is_safe(url) {
        return false;
    }
    ctx.copy_text(url.to_owned());
    ctx.data_mut(|d| d.insert_temp(Id::new(LINK_COPIED), ()));
    true
}

fn paint_frag(ui: &egui::Ui, painter: &egui::Painter, pos: Pos2, frag: &Frag) {
    let size = frag.galley.size();
    painter.galley(pos, frag.galley.clone(), frag.color);

    if let RunKind::Link(url) = &frag.kind {
        let url: &str = url;
        let rect = Rect::from_min_size(pos, size);
        let id = Id::new(("mdpara-link", url, pos.x.to_bits(), pos.y.to_bits()));
        let safe = link_is_safe(url);
        let response = ui.interact(rect, id, Sense::click());
        if response.clicked() {
            click_link(ui.ctx(), url);
        }
        if url.starts_with('#') && response.clicked() {
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("jiaoren-anchor"),
                    url.trim_start_matches('#').to_string(),
                );
            });
        }
        if response.hovered() {
            ui.ctx()
                .data_mut(|d| d.insert_temp(Id::new(HOVER_LINK), url.to_string()));
        }
        // Show the destination so a homoglyph link cannot hide where it goes.
        let response = response.on_hover_text(url);
        if safe {
            let _ = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
    }
}

#[cfg(test)]
mod tests {
    mod copy {
        use super::super::{click_link, link_is_safe, LINK_COPIED};

        #[test]
        fn safe_links_copy_and_flag_the_status() {
            let ctx = egui::Context::default();
            assert!(click_link(&ctx, "https://example.com/docs"));
            let copied = ctx.output(|o| {
                o.commands.iter().any(
                    |c| matches!(c, egui::OutputCommand::CopyText(t) if t == "https://example.com/docs"),
                )
            });
            assert!(copied, "no CopyText command");
            assert!(click_link(&ctx, "mailto:a@b.c"));
            let mailed = ctx.output(|o| {
                o.commands
                    .iter()
                    .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "mailto:a@b.c"))
            });
            assert!(mailed, "no mailto CopyText command");
            let flagged = ctx
                .data_mut(|d| d.remove_temp::<()>(egui::Id::new(LINK_COPIED)))
                .is_some();
            assert!(flagged);
        }

        #[test]
        fn inert_schemes_copy_nothing() {
            let ctx = egui::Context::default();
            for url in [
                "javascript:alert(1)",
                "data:text/html,x",
                "file:///etc/passwd",
                "doc.md",
            ] {
                assert!(!link_is_safe(url));
                assert!(!click_link(&ctx, url), "{url}");
            }
            let commands = ctx.output(|o| o.commands.len());
            assert_eq!(commands, 0);
            let flagged = ctx
                .data_mut(|d| d.get_temp::<()>(egui::Id::new(LINK_COPIED)))
                .is_some();
            assert!(!flagged);
        }
    }

    mod links {
        use super::super::link_is_safe;

        #[test]
        fn only_web_and_mail_schemes_are_clickable() {
            assert!(link_is_safe("https://example.com/x"));
            assert!(link_is_safe("http://example.com"));
            assert!(link_is_safe("mailto:a@b.c"));
            assert!(link_is_safe("HTTPS://EXAMPLE.COM"));
            assert!(!link_is_safe("javascript:alert(1)"));
            assert!(!link_is_safe("JavaScript:alert(1)"));
            assert!(!link_is_safe("data:text/html,<script>"));
            assert!(!link_is_safe("file:///etc/passwd"));
            assert!(!link_is_safe("doc.md"));
            assert!(!link_is_safe("#anchor"));
        }
    }

    mod parse {
        use super::super::*;

        #[test]
        fn parses_the_supported_inline_styles() {
            let runs = parse_inline("a **b** c *d* e `f` g ~~h~~ [i](https://x)");
            assert!(runs
                .iter()
                .any(|r| r.kind == RunKind::Bold && r.text == "b"));
            assert!(runs
                .iter()
                .any(|r| r.kind == RunKind::Italic && r.text == "d"));
            assert!(runs
                .iter()
                .any(|r| r.kind == RunKind::Code && r.text == "f"));
            assert!(runs
                .iter()
                .any(|r| r.kind == RunKind::Strike && r.text == "h"));
            assert!(runs
                .iter()
                .any(|r| r.kind == RunKind::Link("https://x".into()) && r.text == "i"));
        }

        #[test]
        fn leaves_unbalanced_markers_literal() {
            let runs = parse_inline("a **b");
            assert_eq!(
                runs.iter().map(|r| r.text.as_str()).collect::<String>(),
                "a **b"
            );
        }

        #[test]
        fn escapes_markers_and_code_wins_over_emphasis() {
            let runs = parse_inline(r"a \*b\* `**c**`");
            assert!(!runs
                .iter()
                .any(|r| matches!(r.kind, RunKind::Italic | RunKind::Bold)));
            let joined = runs.iter().map(|r| r.text.as_str()).collect::<String>();
            assert_eq!(joined, "a *b* **c**");
            assert!(runs
                .iter()
                .any(|r| r.kind == RunKind::Code && r.text == "**c**"));
        }

        #[test]
        fn code_span_closes_on_an_equal_length_run() {
            assert_eq!(
                parse_inline("``a`b`` c"),
                vec![
                    Run {
                        text: "a`b".into(),
                        kind: RunKind::Code,
                    },
                    Run {
                        text: " c".into(),
                        kind: RunKind::Text,
                    },
                ]
            );
            // A shorter run cannot close a longer opener: no code run may hold
            // the backtick that would have been its closer.
            let runs = parse_inline("``a`b`");
            assert!(
                runs.iter()
                    .filter(|r| r.kind.is_code())
                    .all(|r| !r.text.contains('`')),
                "{runs:?}"
            );
        }

        #[test]
        fn code_span_inside_emphasis_is_skipped_by_marker_search() {
            let runs = parse_inline("**``a**b``**");
            assert!(
                runs.iter().any(|r| r.kind.is_code() && r.text == "a**b"),
                "{runs:?}"
            );
            assert!(!runs.iter().any(|r| r.text.contains("``")), "{runs:?}");
        }

        #[test]
        fn emphasis_may_wrap_a_leading_code_span() {
            let runs = parse_inline("**`document_folder_id` nullable**");
            assert!(runs.iter().any(|r| r.kind == RunKind::Bold));
            assert!(runs
                .iter()
                .any(|r| r.kind.is_code() && r.text == "document_folder_id"));
            let joined = runs.iter().map(|r| r.text.as_str()).collect::<String>();
            assert!(!joined.contains("**"), "{joined:?}");
        }

        #[test]
        fn delimiters_need_flanking_and_underscores_stay_intraword() {
            for s in ["2 * 3 * 4", "snake_case_name", "foo__bar__baz", "a_b"] {
                let runs = parse_inline(s);
                assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), s);
                assert!(runs.iter().all(|r| r.kind == RunKind::Text), "{s}");
            }
        }

        #[test]
        fn bold_with_inner_italics_keeps_every_character() {
            let runs = parse_inline("**a*b*c**");
            assert_eq!(
                runs.iter().map(|r| r.text.as_str()).collect::<String>(),
                "abc"
            );
            assert!(runs
                .iter()
                .any(|r| r.kind == RunKind::BoldItalic && r.text == "b"));
        }

        #[test]
        fn decodes_html_entities_in_text_runs() {
            let joined = |s: &str| {
                parse_inline(s)
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<String>()
            };
            assert_eq!(joined("AT&amp;T"), "AT&T");
            assert_eq!(joined("&lt;b&gt;"), "<b>");
            assert_eq!(joined("a&nbsp;b"), "a b");
            assert_eq!(
                joined("&quot;q&quot; &#39;s&#39; &apos;t&apos;"),
                "\"q\" 's' 't'"
            );
            assert_eq!(joined("&bogus; &amp"), "&bogus; &amp");
        }

        #[test]
        fn emphasis_search_skips_code_spans() {
            let runs = parse_inline("**use `a**b` here**");
            assert_eq!(
                runs.iter().map(|r| r.text.as_str()).collect::<String>(),
                "use a**b here"
            );
            assert!(runs.iter().any(|r| r.kind.is_code() && r.text == "a**b"));
            assert!(!runs
                .iter()
                .any(|r| !r.kind.is_code() && r.text.contains('*')));
        }

        #[test]
        fn triple_marker_is_bold_italic() {
            let runs = parse_inline("***both***");
            assert_eq!(
                runs,
                vec![Run {
                    text: "both".into(),
                    kind: RunKind::BoldItalic,
                }]
            );
        }
    }

    mod search_text {
        use super::super::{parse_inline, search_text};

        #[test]
        fn soft_wrap_becomes_one_space() {
            assert_eq!(
                search_text("tables and mermaid\ndiagrams render"),
                "tables and mermaid diagrams render"
            );
        }

        #[test]
        fn inline_markup_matches_the_painted_run_text() {
            assert_eq!(
                search_text("a **bold** and *it* word"),
                "a bold and it word"
            );
            assert_eq!(search_text("see [label](https://x/y)"), "see label");
            assert_eq!(search_text("run `code()` now"), "run code() now");
            // Image syntax is painted literally by this viewer: `!alt`.
            assert_eq!(search_text("![alt text](i.png)"), "!alt text");
            assert_eq!(search_text("![](i.png)"), "!");
            // Inline HTML is painted as text, not stripped.
            assert_eq!(search_text("<b>x</b> and <br> y"), "<b>x</b> and <br> y");
            assert_eq!(search_text("1 < 2"), "1 < 2");
            // Numbered heading with escaped dot and bold, as the index shows it.
            assert_eq!(
                search_text("**3\\. Tujuan dan kriteria keberhasilan**"),
                "3. Tujuan dan kriteria keberhasilan"
            );
        }

        #[test]
        fn recorded_text_equals_the_concatenated_run_texts() {
            let md = "**b** `a  b` [t](u) &amp; ![alt](x)";
            let painted: String = parse_inline(md).iter().map(|r| r.text.as_str()).collect();
            assert_eq!(search_text(md), painted);
            assert_eq!(search_text(md), "b a  b t & !alt");
        }

        #[test]
        fn code_span_keeps_its_literal_interior() {
            assert_eq!(search_text("`a  b`"), "a  b");
            assert_eq!(search_text("x `  ` y"), "x   y");
        }

        #[test]
        fn whitespace_runs_collapse_and_edges_trim() {
            assert_eq!(search_text("  padded \n\t text  "), "padded text");
            assert_eq!(search_text(""), "");
        }
    }

    mod highlights {
        use super::super::{local_ranges, row_passes, RowPass};

        #[test]
        fn row_passes_paint_code_background_then_selection_then_text() {
            assert_eq!(
                row_passes(true, true),
                vec![RowPass::CodeBackgrounds, RowPass::Selection, RowPass::Text]
            );
            assert_eq!(
                row_passes(true, false),
                vec![RowPass::CodeBackgrounds, RowPass::Text]
            );
            assert_eq!(
                row_passes(false, true),
                vec![RowPass::Selection, RowPass::Text]
            );
            assert_eq!(row_passes(false, false), vec![RowPass::Text]);
        }

        #[test]
        fn maps_block_ranges_to_fragment_offsets() {
            // Fragment starts at block char 10 and is 5 chars long. Ranges
            // entirely before or after it are dropped, not subtracted.
            assert_eq!(
                local_ranges(10, "abcde", &[(0, 5), (8, 12), (12, 14), (15, 20)]),
                vec![(0, 2, 1), (2, 4, 2)]
            );
        }

        #[test]
        fn partial_fragment_gets_local_prefix_and_tail() {
            assert_eq!(local_ranges(0, "abcdef", &[(2, 4)]), vec![(2, 4, 0)]);
        }

        #[test]
        fn unicode_fragment_offsets_are_chars() {
            // "é東x" is 3 chars; the range starts one char into the fragment.
            assert_eq!(local_ranges(4, "é東x", &[(5, 7)]), vec![(1, 3, 0)]);
        }
    }

    mod fragment_offsets {
        use super::super::{fragment_plan, parse_inline, search_text, word_plan, Run, RunKind};

        #[test]
        fn every_fragment_offset_matches_its_text_in_the_model() {
            for source in [
                "a  b",
                "a `` b",
                "`x ` y",
                "**b**  t",
                "[t](u)  z",
                "&amp;  q",
                "![alt](x)  z",
                "``a`b``  z",
            ] {
                let model: Vec<char> = search_text(source).chars().collect();
                let mut after = 0usize;
                for fragment in fragment_plan(&parse_inline(source)) {
                    let text: Vec<char> = fragment.text.chars().collect();
                    let last = model.len().saturating_sub(text.len());
                    let found = (after..=last).find(|&i| model[i..i + text.len()] == text[..]);
                    assert_eq!(
                        found,
                        Some(fragment.offset),
                        "{source:?}: fragment {:?} at model {:?}",
                        fragment.text,
                        model.iter().collect::<String>()
                    );
                    after = fragment.offset + text.len();
                }
            }
        }

        #[test]
        fn bare_backtick_run_is_literal_text() {
            // One run of backticks has no equal-length closer, so CommonMark
            // renders it literally; it is an ordinary word for wrapping.
            let runs = parse_inline("a `` b");
            assert_eq!(
                runs,
                vec![Run {
                    text: "a `` b".into(),
                    kind: RunKind::Text,
                }]
            );
            assert_eq!(
                word_plan(&runs),
                vec![
                    (true, vec!["a".to_string()]),
                    (true, vec!["``".to_string()]),
                    (true, vec!["b".to_string()]),
                ]
            );
        }
    }

    mod wrap {
        use super::super::*;

        #[test]
        fn wraps_greedily() {
            let widths = vec![30.0, 30.0, 30.0, 30.0];
            let gaps = vec![0.0, 10.0, 10.0, 10.0];
            let lines = wrap_words(&widths, &gaps, 100.0);
            // 30 + 10 + 30 = 70 fits; adding the third word needs 110 > 100.
            assert_eq!(lines, vec![vec![0, 1], vec![2, 3]]);
        }

        #[test]
        fn an_overwide_word_gets_its_own_line() {
            let lines = wrap_words(&[30.0, 200.0, 30.0], &[0.0, 10.0, 10.0], 100.0);
            assert_eq!(lines, vec![vec![0], vec![1], vec![2]]);
        }

        #[test]
        fn glued_words_share_a_line_break_budget() {
            assert_eq!(
                wrap_words(&[60.0, 60.0], &[0.0, 0.0], 100.0),
                vec![vec![0], vec![1]]
            );
        }
    }

    mod marker {
        use super::super::*;

        #[test]
        fn labels_match_the_marker_kind() {
            assert_eq!(Marker::Bullet.label(), "•");
            assert_eq!(Marker::Hollow.label(), "○");
            assert_eq!(Marker::Number(3, '.').label(), "3.");
            assert_eq!(Marker::Number(4, ')').label(), "4)");
            assert_eq!(Marker::Task(true, None).label(), "☑");
            assert_eq!(Marker::Task(false, None).label(), "☐");
            assert_eq!(Marker::Task(true, Some((2, '.'))).label(), "2. ☑");
        }

        #[test]
        fn bullet_and_task_labels_are_single_supported_chars() {
            for label in [
                Marker::Bullet.label(),
                Marker::Hollow.label(),
                Marker::Task(true, None).label(),
                Marker::Task(false, None).label(),
            ] {
                assert_eq!(label.chars().count(), 1, "{label}");
            }
            // Exact code points so a future glyph swap is deliberate: U+2022,
            // U+25CB, U+2611, U+2610 all exist in FiraCode Nerd Font.
            assert_eq!(Marker::Bullet.label(), "\u{2022}");
            assert_eq!(Marker::Hollow.label(), "\u{25cb}");
            assert_eq!(Marker::Task(true, None).label(), "\u{2611}");
            assert_eq!(Marker::Task(false, None).label(), "\u{2610}");
        }

        #[test]
        fn indent_steps_two_spaces_per_level_and_stops_at_five() {
            assert_eq!(list_indent(0, 8.0), 0.0);
            assert_eq!(list_indent(2, 8.0), 32.0);
            assert_eq!(list_indent(9, 8.0), 80.0);
        }
    }

    mod paint {
        use super::super::*;

        #[test]
        fn lines_touching_a_split_token_are_not_stretched() {
            let split = [false, true, true, false];
            assert!(touches_split(&split, &[1, 2], None)); // holds split pieces
            assert!(touches_split(&split, &[0], Some(&[1]))); // next line starts it
            assert!(!touches_split(&split, &[0], Some(&[3])));
            assert_eq!(justified_grow(true, false, 50.0, 1, true, 20.0), 0.0);
            assert_eq!(justified_grow(true, false, 50.0, 2, false, 20.0), 25.0);
            assert_eq!(justified_grow(true, true, 50.0, 2, false, 20.0), 0.0); // last line

            // draw_left
            assert_eq!(justified_grow(false, false, 50.0, 2, false, 20.0), 0.0);
        }

        #[test]
        fn gaps_grow_at_most_twice_the_space_width() {
            // extra 100 over 2 gaps, cap 17.6 -> no stretch
            assert_eq!(justified_grow(true, false, 100.0, 2, false, 8.8), 0.0);
            // extra 10 over 2 gaps -> 5.0
            assert_eq!(justified_grow(true, false, 10.0, 2, false, 8.8), 5.0);
            // last line and split-token lines still 0
            assert_eq!(justified_grow(true, true, 10.0, 2, false, 8.8), 0.0);
            assert_eq!(justified_grow(true, false, 10.0, 2, true, 8.8), 0.0);
        }
    }

    mod split {
        use super::super::*;

        #[test]
        fn splits_prefer_separators_and_lose_nothing() {
            let m = |s: &str| s.chars().count() as f32 * 10.0; // fake measure
            let parts = split_token("src/db/schema.ts:293-311", 100.0, &m);
            assert!(parts.len() > 1);
            assert_eq!(parts.concat(), "src/db/schema.ts:293-311");
            assert!(parts.iter().all(|p| m(p) <= 100.0));
        }

        #[test]
        fn hard_splits_when_no_separator_and_terminates() {
            let m = |s: &str| s.chars().count() as f32 * 10.0;
            for n in [300, 500] {
                let word = "a".repeat(n);
                let parts = split_token(&word, 100.0, &m);
                assert_eq!(parts.concat(), word);
                assert!(parts.iter().all(|p| m(p) <= 100.0));
            }
        }

        #[test]
        fn keeps_whole_tokens_that_fit_and_splits_slash_runs() {
            let m = |s: &str| s.chars().count() as f32 * 10.0;
            assert_eq!(split_token("abcdefghij", 100.0, &m), vec!["abcdefghij"]);
            assert_eq!(split_token("alpha/", 60.0, &m), vec!["alpha/"]);
            let parts = split_token("//////////", 30.0, &m);
            assert_eq!(parts.concat(), "//////////");
            assert!(parts.iter().all(|p| m(p) <= 30.0));
        }

        #[test]
        fn breaks_after_separator_not_before() {
            let m = |s: &str| s.chars().count() as f32 * 10.0;
            let parts = split_token("alpha/beta/gamma", 60.0, &m);
            assert_eq!(parts, vec!["alpha/", "beta/", "gamma"]);
        }

        #[test]
        fn skips_a_separator_that_leaves_the_piece_less_than_half_full() {
            let m = |s: &str| s.chars().count() as f32 * 10.0;
            let word = format!("a-{}", "b".repeat(40));
            let parts = split_token(&word, 100.0, &m);
            assert_eq!(parts[0], "a-bbbbbbbb");
            assert_eq!(parts.concat(), word);
        }

        #[test]
        fn measures_only_short_prefixes_for_huge_tokens() {
            let longest = std::cell::Cell::new(0usize);
            let m = |s: &str| {
                longest.set(longest.get().max(s.chars().count()));
                s.chars().count() as f32 * 10.0
            };
            let word = "a/".repeat(50_000);
            let parts = split_token(&word, 100.0, &m);
            assert_eq!(parts.concat(), word);
            assert!(longest.get() <= 11, "measured {} chars", longest.get());
        }

        #[test]
        fn splits_multibyte_without_panicking() {
            let m = |s: &str| s.chars().count() as f32 * 10.0;
            let word = "数据库查询接口超长标识符😀😀😀";
            let parts = split_token(word, 40.0, &m);
            assert_eq!(parts.concat(), word);
            assert!(parts.iter().all(|p| m(p) <= 40.0), "{parts:?}");
            assert_eq!(parts[0].chars().count(), 4);
        }
    }

    mod inline_cache {
        use super::super::{
            clear_inline_cache, parse_inline, parse_inline_cached, MAX_INLINE_CACHE,
        };

        #[test]
        fn repeats_share_one_parse_and_texts_stay_distinct() {
            clear_inline_cache();
            let a = parse_inline_cached("**a** `b`");
            let b = parse_inline_cached("**a** `b`");
            assert!(std::rc::Rc::ptr_eq(&a, &b), "same text must hit the cache");
            assert_eq!(*a, parse_inline("**a** `b`"), "cache must not change runs");
            let c = parse_inline_cached("**a** `c`");
            assert!(!std::rc::Rc::ptr_eq(&a, &c));
            clear_inline_cache();
        }

        #[test]
        fn cache_is_bounded() {
            clear_inline_cache();
            for i in 0..MAX_INLINE_CACHE + 10 {
                let text = format!("word {i}");
                assert_eq!(parse_inline_cached(&text).len(), 1);
            }
            let len = super::super::INLINE_CACHE.with(|c| c.borrow().len());
            assert!(len <= MAX_INLINE_CACHE, "cache holds {len} entries");
            clear_inline_cache();
        }
    }
}
