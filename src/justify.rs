//! Inline parsing and word-by-word justified painting for body paragraphs.
//! epaint has no justification, so each word is laid out on its own and the
//! leftover column width is divided between the gaps of every line but the last.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{
    Color32, FontId, Galley, Id, Pos2, Rect, Sense, Stroke, TextStyle,
};

/// One styled piece of a paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub kind: RunKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RunKind {
    Text,
    Bold,
    Italic,
    BoldItalic,
    Code,
    Strike,
    Link(String),
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
        let c = rest.chars().next().unwrap();

        if c == '\\' {
            if let Some(next) = rest[1..].chars().next() {
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
            if let Some(end) = rest[1..].find('`') {
                flush_text(&mut runs, &mut text);
                runs.push(Run {
                    text: rest[1..1 + end].to_string(),
                    kind: RunKind::Code,
                });
                i += end + 2;
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
    if next.is_whitespace() || matches!(next, '*' | '_' | '~' | '`') {
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

/// Length of a code span at the start of `s` (`None` when `s` does not start
/// with a backtick or the span is unterminated).
fn code_span_len(s: &str) -> Option<usize> {
    let rest = s.strip_prefix('`')?;
    Some(rest.find('`')? + 2)
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
        if rest.starts_with(marker)
            && !escaped(s, i)
            && delimiter_can_close(s, i, marker.len(), ch)
        {
            return Some(i);
        }
        i += rest.chars().next().unwrap().len_utf8();
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
        let c = rest.chars().next().unwrap();
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
fn parse_link(s: &str) -> Option<(&str, String, usize)> {
    let mid = s.find("](")?;
    let label = &s[1..mid];
    let (target, end) = link_target(&s[mid + 2..])?;
    let url = target.split_whitespace().next()?;
    if url.is_empty() {
        return None;
    }
    Some((label, url.to_string(), mid + 2 + end))
}

/// Split the text after `](` into the link target and the index just past the
/// closing `)`, balancing parentheses so `x(1).png` is one target.
fn link_target(s: &str) -> Option<(&str, usize)> {
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
pub fn wrap_words(widths: &[f32], gaps: &[f32], column: f32) -> Vec<Vec<usize>> {
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
pub fn split_token(
    word: &str,
    max_width: f32,
    measure: &dyn Fn(&str) -> f32,
) -> Vec<String> {
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
            _ => last_fit.unwrap_or_else(|| {
                start + rest.chars().next().expect("non-empty").len_utf8()
            }),
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
}

/// Layout every word fragment with the real egui font metrics. Words are split
/// on whitespace; a code span stays one atomic word.
fn build_words(ui: &egui::Ui, runs: &[Run]) -> Vec<Word> {
    let body = ui
        .style()
        .text_styles
        .get(&TextStyle::Body)
        .cloned()
        .unwrap_or_else(|| FontId::proportional(14.0));
    let mono = ui
        .style()
        .text_styles
        .get(&TextStyle::Monospace)
        .cloned()
        .unwrap_or_else(|| FontId::monospace(14.0));
    let text_color = ui.visuals().text_color();
    let strong = ui.visuals().strong_text_color();
    let link_color = ui.visuals().hyperlink_color;

    let mut words: Vec<Word> = Vec::new();
    let mut open = false; // a word is being filled

    for run in runs {
        let atomic = matches!(run.kind, RunKind::Code);
        let mut buf = String::new();
        for ch in run.text.chars() {
            if ch.is_whitespace() && !atomic {
                if !buf.is_empty() {
                    push_frag(ui, &mut words, &mut open, &buf, run, &body, &mono, text_color, strong, link_color);
                    buf.clear();
                }
                open = false;
            } else {
                buf.push(ch);
            }
        }
        if !buf.is_empty() {
            push_frag(ui, &mut words, &mut open, &buf, run, &body, &mono, text_color, strong, link_color);
        }
    }
    words
}

#[allow(clippy::too_many_arguments)]
fn push_frag(
    ui: &egui::Ui,
    words: &mut Vec<Word>,
    open: &mut bool,
    text: &str,
    run: &Run,
    body: &FontId,
    mono: &FontId,
    text_color: Color32,
    strong: Color32,
    link_color: Color32,
) {
    let (font, color, italics, underline, strikethrough) = match &run.kind {
        RunKind::Text => (body.clone(), text_color, false, Stroke::NONE, Stroke::NONE),
        RunKind::Bold => (body.clone(), strong, false, Stroke::NONE, Stroke::NONE),
        RunKind::Italic => (body.clone(), text_color, true, Stroke::NONE, Stroke::NONE),
        RunKind::BoldItalic => (body.clone(), strong, true, Stroke::NONE, Stroke::NONE),
        RunKind::Code => (mono.clone(), text_color, false, Stroke::NONE, Stroke::NONE),
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
            Stroke::new(1.0, link_color),
            Stroke::NONE,
        ),
    };
    let format = TextFormat {
        font_id: font,
        color,
        italics,
        underline,
        strikethrough,
        ..Default::default()
    };
    let galley = layout(ui, text, &format);
    let width = galley.size().x;
    if !*open {
        words.push(Word {
            frags: Vec::new(),
            width: 0.0,
            glued: false,
            split: false,
        });
        *open = true;
    }
    let word = words.last_mut().unwrap();
    word.width += width;
    word.frags.push(Frag {
        kind: run.kind.clone(),
        color,
        format,
        text: text.to_string(),
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

/// Paint `runs` as a fully justified paragraph in a `column`-wide slot starting
/// at the current cursor. Every line but the last (and any line without a
/// stretchable gap) is spread so its right edge lands on `cursor.left() + column`.
pub fn draw(ui: &mut egui::Ui, runs: &[Run], column: f32) {
    paint(ui, runs, column, true);
}

/// Left-aligned variant of [`draw`]: same wrapping, splitting and painting, but
/// gaps never stretch and rows hug the left edge. Used by table cells.
pub fn draw_left(ui: &mut egui::Ui, runs: &[Run], column: f32) {
    paint(ui, runs, column, false);
}

fn paint(ui: &mut egui::Ui, runs: &[Run], column: f32, justify: bool) {
    let column = column.max(0.0);
    if column <= 0.0 {
        return;
    }
    let body = ui
        .style()
        .text_styles
        .get(&TextStyle::Body)
        .cloned()
        .unwrap_or_else(|| FontId::proportional(14.0));
    let row_h = ui.fonts_mut(|f| f.row_height(&body));
    let space = ui
        .fonts_mut(|f| f.layout_no_wrap(" ".to_string(), body.clone(), Color32::PLACEHOLDER))
        .size()
        .x;

    let words = split_overwide(ui, build_words(ui, runs), column);
    if words.is_empty() {
        return;
    }
    let widths: Vec<f32> = words.iter().map(|w| w.width).collect();
    let gaps: Vec<f32> = words
        .iter()
        .map(|w| if w.glued { 0.0 } else { space })
        .collect();
    let lines = wrap_words(&widths, &gaps, column);
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
        lines.iter().map(|line| natural_of(line)).fold(column, f32::max)
    } else {
        column
    };
    let (_, rect) = ui.allocate_space(egui::vec2(reserved, used_height));
    let mut painter = ui.painter().clone();
    if !justify {
        let clip = egui::Rect::from_min_size(rect.min, egui::vec2(column, used_height));
        painter.set_clip_rect(ui.clip_rect().intersect(clip));
    }
    let left = rect.left();
    let top = rect.top();
    let split_words: Vec<bool> = words.iter().map(|w| w.split).collect();

    for (row, line) in lines.iter().enumerate() {
        let natural = natural_of(line);
        let extra = (column - natural).max(0.0);
        let last = row + 1 == lines.len();
        let stretchable = line
            .iter()
            .skip(1)
            .filter(|&&i| gaps[i] > 0.0)
            .count();
        let split = touches_split(
            &split_words,
            line,
            lines.get(row + 1).map(Vec::as_slice),
        );
        let grow = justified_grow(justify, last, extra, stretchable, split);

        let mut x = left;
        let y = top + row as f32 * row_h;
        for (k, &wi) in line.iter().enumerate() {
            if k > 0 {
                let gap = gaps[wi];
                x += if gap > 0.0 { gap + grow } else { 0.0 };
            }
            let word = &words[wi];
            for frag in &word.frags {
                let pos = Pos2::new(x, y);
                paint_frag(ui, &painter, pos, frag);
                x += frag.width;
            }
        }
    }
}

/// A line borders a split token when it holds any piece of one (head or
/// continuation) or the next line starts with a piece. Such lines stay
/// left-aligned: stretching gaps next to a broken URL/path looks like inserted
/// spaces.
fn touches_split(split: &[bool], line: &[usize], next: Option<&[usize]>) -> bool {
    line.iter().any(|&i| split[i])
        || next
            .and_then(|l| l.first())
            .is_some_and(|&i| split[i])
}

/// Extra width added to every stretchable gap on a line; zero for the last line,
/// lines with nothing to stretch, and lines touching a split token.
fn justified_grow(
    justify: bool,
    last: bool,
    extra: f32,
    stretchable: usize,
    split: bool,
) -> f32 {
    if justify && !last && stretchable > 0 && !split {
        extra / stretchable as f32
    } else {
        0.0
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
        for part in parts {
            let galley = layout(ui, &part, &frag.format);
            let width = galley.size().x;
            out.push(Word {
                frags: vec![Frag {
                    kind: frag.kind.clone(),
                    color: frag.color,
                    format: frag.format.clone(),
                    text: part,
                    galley,
                    width,
                }],
                width,
                glued: true,
                split: true,
            });
        }
        out[first].glued = false;
    }
    out
}

fn paint_frag(ui: &egui::Ui, painter: &egui::Painter, pos: Pos2, frag: &Frag) {
    let size = frag.galley.size();
    if frag.kind == RunKind::Code {
        let bg = Rect::from_min_size(pos, size).expand2(egui::vec2(1.0, 0.0));
        painter.rect_filled(bg, 2.0, ui.visuals().code_bg_color);
    }
    painter.galley(pos, frag.galley.clone(), frag.color);

    if let RunKind::Link(url) = &frag.kind {
        let rect = Rect::from_min_size(pos, size);
        let id = Id::new(("mdpara-link", url, pos.x.to_bits(), pos.y.to_bits()));
        let response = ui.interact(rect, id, Sense::click());
        if response.clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
        let _ = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_supported_inline_styles() {
        let runs = parse_inline("a **b** c *d* e `f` g ~~h~~ [i](https://x)");
        assert!(runs.iter().any(|r| r.kind == RunKind::Bold && r.text == "b"));
        assert!(runs.iter().any(|r| r.kind == RunKind::Italic && r.text == "d"));
        assert!(runs.iter().any(|r| r.kind == RunKind::Code && r.text == "f"));
        assert!(runs.iter().any(|r| r.kind == RunKind::Strike && r.text == "h"));
        assert!(runs.iter().any(|r| r.kind == RunKind::Link("https://x".into()) && r.text == "i"));
    }

    #[test]
    fn leaves_unbalanced_markers_literal() {
        let runs = parse_inline("a **b");
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), "a **b");
    }

    #[test]
    fn escapes_markers_and_code_wins_over_emphasis() {
        let runs = parse_inline(r"a \*b\* `**c**`");
        assert!(!runs.iter().any(|r| matches!(r.kind, RunKind::Italic | RunKind::Bold)));
        let joined = runs.iter().map(|r| r.text.as_str()).collect::<String>();
        assert_eq!(joined, "a *b* **c**");
        assert!(runs.iter().any(|r| r.kind == RunKind::Code && r.text == "**c**"));
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
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), "abc");
        assert!(runs.iter().any(|r| r.kind == RunKind::BoldItalic && r.text == "b"));
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
        assert_eq!(joined("&quot;q&quot; &#39;s&#39; &apos;t&apos;"), "\"q\" 's' 't'");
        assert_eq!(joined("&bogus; &amp"), "&bogus; &amp");
    }

    #[test]
    fn emphasis_search_skips_code_spans() {
        let runs = parse_inline("**use `a**b` here**");
        assert_eq!(
            runs.iter().map(|r| r.text.as_str()).collect::<String>(),
            "use a**b here"
        );
        assert!(runs.iter().any(|r| r.kind == RunKind::Code && r.text == "a**b"));
        assert!(!runs
            .iter()
            .any(|r| r.kind != RunKind::Code && r.text.contains('*')));
    }

    #[test]
    fn triple_marker_is_bold_italic() {
        let runs = parse_inline("***both***");
        assert_eq!(runs, vec![Run {
            text: "both".into(),
            kind: RunKind::BoldItalic,
        }]);
    }

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

    #[test]
    fn lines_touching_a_split_token_are_not_stretched() {
        let split = [false, true, true, false];
        assert!(touches_split(&split, &[1, 2], None)); // holds split pieces
        assert!(touches_split(&split, &[0], Some(&[1]))); // next line starts it
        assert!(!touches_split(&split, &[0], Some(&[3])));
        assert_eq!(justified_grow(true, false, 50.0, 1, true), 0.0);
        assert_eq!(justified_grow(true, false, 50.0, 2, false), 25.0);
        assert_eq!(justified_grow(true, true, 50.0, 2, false), 0.0); // last line
        assert_eq!(justified_grow(false, false, 50.0, 2, false), 0.0); // draw_left
    }

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
