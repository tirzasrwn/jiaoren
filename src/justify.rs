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

        if rest.starts_with("~~") {
            if delimiter_can_open(md, i, 2, '~') {
                if let Some(end) = find_marker(&rest[2..], "~~", '~') {
                    flush_text(&mut runs, &mut text);
                    runs.push(Run {
                        text: unescape(&rest[2..2 + end]),
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

/// Greedy word wrap: a word joins the line only when `line + space + word`
/// fits; a lone over-wide word gets its own line and overflows instead of being
/// cut. Returns the word indices per line and the natural space width used.
pub fn wrap_words(widths: &[f32], space: f32, column: f32) -> (Vec<Vec<usize>>, f32) {
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut used = 0.0;
    for (i, &w) in widths.iter().enumerate() {
        if cur.is_empty() {
            used = w;
            cur.push(i);
        } else if used + space + w <= column {
            used += space + w;
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
    (lines, space)
}

struct Frag {
    kind: RunKind,
    color: Color32,
    galley: Arc<Galley>,
    width: f32,
}

struct Word {
    frags: Vec<Frag>,
    width: f32,
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
    let (font, color, italics, underline, strike) = match &run.kind {
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
    let galley = layout(ui, text, font, color, italics, underline, strike);
    let width = galley.size().x;
    if !*open {
        words.push(Word {
            frags: Vec::new(),
            width: 0.0,
        });
        *open = true;
    }
    let word = words.last_mut().unwrap();
    word.width += width;
    word.frags.push(Frag {
        kind: run.kind.clone(),
        color,
        galley,
        width,
    });
}

fn layout(
    ui: &egui::Ui,
    text: &str,
    font_id: FontId,
    color: Color32,
    italics: bool,
    underline: Stroke,
    strikethrough: Stroke,
) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    job.append(
        text,
        0.0,
        TextFormat {
            font_id,
            color,
            italics,
            underline,
            strikethrough,
            ..Default::default()
        },
    );
    ui.fonts_mut(|f| f.layout_job(job))
}

/// Paint `runs` as a fully justified paragraph in a `column`-wide slot starting
/// at the current cursor. Every line but the last (and any single-word line) is
/// spread so its right edge lands on `cursor.left() + column`.
pub fn draw(ui: &mut egui::Ui, runs: &[Run], column: f32) {
    let column = column.max(0.0);
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

    let words = build_words(ui, runs);
    if words.is_empty() || column <= 0.0 {
        return;
    }
    let widths: Vec<f32> = words.iter().map(|w| w.width).collect();
    let (lines, _) = wrap_words(&widths, space, column);

    let used_height = lines.len() as f32 * row_h;
    // Reserve the widest line's natural width so an over-wide word stays
    // reachable by horizontal pan instead of being silently clipped.
    let reserved = lines
        .iter()
        .map(|line| {
            line.iter().map(|&i| widths[i]).sum::<f32>()
                + space * line.len().saturating_sub(1) as f32
        })
        .fold(column, f32::max);
    let (_, rect) = ui.allocate_space(egui::vec2(reserved, used_height));
    let left = rect.left();
    let top = rect.top();
    let painter = ui.painter().clone();

    for (row, line) in lines.iter().enumerate() {
        let natural: f32 = line.iter().map(|&i| widths[i]).sum::<f32>()
            + space * line.len().saturating_sub(1) as f32;
        let extra = (column - natural).max(0.0);
        let last = row + 1 == lines.len();
        let gap = space
            + if last || line.len() == 1 {
                0.0
            } else {
                extra / (line.len() - 1) as f32
            };

        let mut x = left;
        let y = top + row as f32 * row_h;
        for &wi in line {
            let word = &words[wi];
            for frag in &word.frags {
                let pos = Pos2::new(x, y);
                paint_frag(ui, &painter, pos, frag);
                x += frag.width;
            }
            x += gap;
        }
    }
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
        let (lines, _) = wrap_words(&widths, 10.0, 100.0);
        // 30 + 10 + 30 = 70 fits; adding the third word needs 110 > 100.
        assert_eq!(lines, vec![vec![0, 1], vec![2, 3]]);
    }

    #[test]
    fn an_overwide_word_gets_its_own_line() {
        let (lines, _) = wrap_words(&[30.0, 200.0, 30.0], 10.0, 100.0);
        assert_eq!(lines, vec![vec![0], vec![1], vec![2]]);
    }
}
