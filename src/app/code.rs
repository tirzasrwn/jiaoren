//! Fenced-code rendering for the `<mdcode>` html hook: verbatim monospace text
//! on a code-coloured background, soft-wrapped to the reading column with a
//! right-aligned line-number gutter. Continuation rows keep an empty gutter.

use std::collections::HashMap;

use super::searchui::SearchUi;
use super::widgets;
use crate::doc::CodeSpec;

/// Shared, interior-mutable state backing the `<mdcode>` render path. Holds the
/// parsed specs and the reading column recorded once per frame.
pub(crate) struct CodeUi {
    specs: HashMap<String, CodeSpec>,
    /// Reading column in effect this frame; `ui.available_width()` inside the
    /// html hook is not that column.
    column: f32,
    /// Per-id occurrence counter so identical code blocks get distinct ids.
    occurrences: HashMap<String, usize>,
    /// Set when a block's copy button was clicked this frame.
    copied: bool,
}

impl CodeUi {
    pub(crate) fn new(specs: HashMap<String, CodeSpec>) -> Self {
        CodeUi {
            specs,
            column: 0.0,
            occurrences: HashMap::new(),
            copied: false,
        }
    }

    /// Record the document's reading column once per frame, before the viewer
    /// runs.
    pub(crate) fn begin_frame(&mut self, column: f32) {
        self.column = column;
        self.occurrences.clear();
    }

    pub(crate) fn reset(&mut self, specs: HashMap<String, CodeSpec>) {
        self.specs = specs;
        self.occurrences.clear();
        self.copied = false;
    }

    /// Take the "a code block was copied" flag for the frame, if the button
    /// fired.
    pub(crate) fn take_copied(&mut self) -> bool {
        std::mem::take(&mut self.copied)
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str, search: &mut SearchUi) {
        // Clone so the spec outlives the immutable borrow of `self`.
        let spec = match self.specs.get(id) {
            Some(s) => s.clone(),
            None => return, // unknown id: draw nothing, never panic
        };
        let occurrence = {
            let count = self.occurrences.entry(id.to_string()).or_insert(0);
            let occurrence = *count;
            *count += 1;
            occurrence
        };
        let mono = ui
            .style()
            .text_styles
            .get(&egui::TextStyle::Monospace)
            .cloned()
            .unwrap_or_else(|| egui::FontId::monospace(14.0));
        let block = search.record(ui, spec.text.clone());
        let (ranges, current) = search.highlights(block);
        let selection = search.selection_range(block);
        // A drag selection (or an active one) owns the visual: the hover row
        // would otherwise light up a second time and read as a second
        // highlight on top of the selection.
        let selecting = search.selection.dragging || search.selection.is_active();
        let row_h = ui.fonts_mut(|f| f.row_height(&mono));
        let pad = row_h * 0.5;
        let top_pad = code_top_pad(row_h);
        let lines = code_lines(&spec.text);
        let spans = code_line_spans(&spec.text);
        let line_marks = line_highlights(&spec.text, &ranges, current);
        let sel_ranges: Vec<(usize, usize)> = selection.into_iter().collect();
        let sel_marks = line_highlights(&spec.text, &sel_ranges, None);
        let other_fill = crate::justify::highlight_color(ui, false);
        let current_fill = crate::justify::highlight_color(ui, true);
        let selection_fill = crate::justify::selection_color(ui);
        let cell = line_width(ui, "M", &mono);
        // The number column plus one cell of gap; the text starts after it.
        let gutter = gutter_width(lines.len(), cell);
        // Leave the right padding inside the column so glyphs never spill past
        // the block's background.
        let content = content_width(self.column, gutter, pad, cell);
        let rows = layout_rows(&spec.text, content, &|s| line_width(ui, s, &mono));
        let width = self.column.max(gutter + cell);
        let content_height = rows.len().max(1) as f32 * row_h + top_pad + pad;
        let gap = crate::justify::block_gap(ui);
        // Reserve the inter-block gap inside the allocation; the background and
        // the block's recorded height stay on the content area.
        let (_, rect) = ui.allocate_space(egui::vec2(width, content_height + gap));
        search.set_height(block, content_height);
        search.set_rect(block, rect);
        let painter = ui.painter().clone();
        let content = egui::Rect::from_min_size(rect.min, egui::vec2(width, content_height));
        painter.rect_filled(content, 2.0, ui.visuals().code_bg_color);
        // Like a table row: only the visual code line under the pointer gets the
        // theme's hover fill, painted above the block background and below the
        // glyphs/selection.
        if let Some(pos) = ui.ctx().pointer_hover_pos() {
            if !selecting && content.contains(pos) {
                let rel = (pos.y - (rect.top() + top_pad)).max(0.0);
                let hovered_row = (rel / row_h).floor() as usize;
                if let Some(row) = rows.get(hovered_row) {
                    // A wrapped source line owns several visual rows; the hover
                    // covers every row that belongs to the same source line.
                    let first = rows.iter().position(|r| r.line == row.line).unwrap_or(0);
                    let last = rows
                        .iter()
                        .rposition(|r| r.line == row.line)
                        .unwrap_or(first);
                    let row_rect = egui::Rect::from_min_size(
                        egui::Pos2::new(
                            content.left(),
                            rect.top() + top_pad + first as f32 * row_h,
                        ),
                        egui::vec2(content.width(), (last + 1 - first) as f32 * row_h),
                    );
                    painter.rect_filled(row_rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
            }
        }
        let color = ui.visuals().text_color();
        let number_color = ui.visuals().weak_text_color();
        let text_left = rect.left() + pad + gutter;
        let mut chars = Vec::new();
        for (row_index, row) in rows.iter().enumerate() {
            let source = lines.get(row.line).copied().unwrap_or("");
            let text: String = source.chars().skip(row.start).take(row.len).collect();
            let pos = egui::Pos2::new(text_left, rect.top() + top_pad + row_index as f32 * row_h);
            // The number sits on the first visual row of its source line only;
            // wrapped continuations keep an empty gutter.
            if row.start == 0 {
                let number = format!("{}", row.line + 1);
                let galley = ui.fonts_mut(|f| f.layout_no_wrap(number, mono.clone(), number_color));
                let x = text_left - cell * 0.5 - galley.size().x;
                painter.galley(egui::Pos2::new(x, pos.y), galley, number_color);
            }
            // Selection first, then search hits, both behind the glyphs.
            for &(mark_start, mark_end, _) in &clip_marks(&sel_marks[row.line], row) {
                let x = pos.x + line_width(ui, &prefix(&text, mark_start), &mono);
                let w = line_width(ui, &slice(&text, mark_start, mark_end), &mono);
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::Pos2::new(x, pos.y), egui::vec2(w, row_h)),
                    2.0,
                    selection_fill,
                );
            }
            for &(mark_start, mark_end, is_current) in &clip_marks(&line_marks[row.line], row) {
                let x = pos.x + line_width(ui, &prefix(&text, mark_start), &mono);
                let w = line_width(ui, &slice(&text, mark_start, mark_end), &mono);
                let fill = if is_current { current_fill } else { other_fill };
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::Pos2::new(x, pos.y), egui::vec2(w, row_h)),
                    2.0,
                    fill,
                );
            }
            if text.is_empty() {
                continue; // the row is still reserved by `height`
            }
            let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.clone(), mono.clone(), color));
            let line_w = galley.size().x.max(1.0);
            chars.push(crate::justify::CharSpan {
                start: spans.get(row.line).map_or(0, |&(start, _)| start) + row.start,
                len: row.len,
                rect: egui::Rect::from_min_size(pos, egui::vec2(line_w, row_h)),
            });
            painter.galley(pos, galley, color);
        }
        search.set_char_map(block, chars);
        // Overlay the copy icon inside the block's top-right corner, at the
        // reading-column edge for over-wide code, so it never takes layout
        // space or moves with horizontal panning.
        let right = (rect.left() + self.column).min(rect.right()) - pad - 2.0;
        let icon = egui::Rect::from_min_size(
            egui::Pos2::new(right - 14.0, rect.top() + 2.0),
            egui::vec2(14.0, 14.0),
        );
        let backdrop = ui.visuals().code_bg_color;
        let (clicked, control) = widgets::copy_button_at(
            ui,
            icon,
            ui.id().with(("code-copy", id, occurrence)),
            "Copy code",
            backdrop,
        );
        search.add_control(control, egui::CursorIcon::Default);
        if clicked {
            ui.ctx().copy_text(spec.text.clone());
            self.copied = true;
        }
    }
}

/// Split `line` into visual rows no wider than `max_width`, as `(start, len)`
/// char ranges in order. An empty line is one empty row; a single char wider
/// than `max_width` still gets a row of its own.
pub(crate) fn wrap_line(
    line: &str,
    max_width: f32,
    measure: &dyn Fn(&str) -> f32,
) -> Vec<(usize, usize)> {
    let total = line.chars().count();
    let mut rows = Vec::new();
    let mut start = 0usize; // char index of the current row
                            // Add per-char advances instead of re-laying out every growing prefix:
                            // the old loop was O(n^2) layout work per line, and the performance audit
                            // showed it dominating a large document. Per-char widths are cached by the
                            // font atlas, so this is O(n) cheap lookups. A tab counts as four cells;
                            // ligatures make the real row narrower, never wider.
    let cell = measure("M");
    let mut row_w = 0.0f32;
    for (index, (_, c)) in line.char_indices().enumerate() {
        let w = if c == '\t' {
            cell * 4.0
        } else {
            measure(c.encode_utf8(&mut [0u8; 4]))
        };
        if index > start && row_w + w > max_width {
            rows.push((start, index - start));
            start = index;
            row_w = 0.0;
        }
        row_w += w;
    }
    if start < total || rows.is_empty() {
        rows.push((start, total - start));
    }
    rows
}

/// One painted visual row: which source line it belongs to and the char range
/// within that line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub line: usize,
    pub start: usize,
    pub len: usize,
}

/// Visual rows for a whole code block, in paint order.
pub(crate) fn layout_rows(text: &str, max_width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<Row> {
    let mut rows = Vec::new();
    for (line, body) in text.split('\n').enumerate() {
        for (start, len) in wrap_line(body, max_width, measure) {
            rows.push(Row { line, start, len });
        }
    }
    rows
}

/// Wrap width left for code text inside `column`: the gutter and a pad on each
/// side are reserved, never less than one cell.
pub(crate) fn content_width(column: f32, gutter: f32, pad: f32, cell: f32) -> f32 {
    (column - gutter - 2.0 * pad).max(cell)
}

/// Top padding: at least enough to clear the 14 px copy icon at the block's
/// top-right, which would otherwise sit on the first visual row.
pub(crate) fn code_top_pad(row_height: f32) -> f32 {
    (row_height * 0.5).max(18.0)
}

/// Width of the line-number gutter: as many cells as digits in the largest
/// line number, plus one cell of gap.
pub(crate) fn gutter_width(line_count: usize, cell: f32) -> f32 {
    let digits = line_count.max(1).ilog10() + 1;
    digits as f32 * cell + cell
}

/// Clip line-local highlight marks to one visual row and shift them to the
/// row's own char origin. Empty intersections are dropped.
pub(crate) fn clip_marks(marks: &[(usize, usize, bool)], row: &Row) -> Vec<(usize, usize, bool)> {
    let end = row.start + row.len;
    marks
        .iter()
        .filter_map(|&(start, stop, current)| {
            let start = start.max(row.start);
            let stop = stop.min(end);
            (start < stop).then(|| (start - row.start, stop - row.start, current))
        })
        .collect()
}

/// `(start, len)` for every code line, in chars. Empty lines keep their slot so
/// later offsets cannot drift.
fn code_line_spans(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for line in text.split('\n') {
        let len = line.chars().count();
        out.push((start, len));
        start += len + 1; // the newline itself
    }
    out
}

/// Highlight ranges for each code line: `(start, end, is_current)` in local
/// chars. A range crossing a newline is clipped to each line and resumes on the
/// next, so a cross-line match highlights the remainder per line.
pub(crate) fn line_highlights(
    text: &str,
    ranges: &[(usize, usize)],
    current: Option<usize>,
) -> Vec<Vec<(usize, usize, bool)>> {
    let mut out = Vec::new();
    for (line_start, line_len) in code_line_spans(text) {
        let line_end = line_start + line_len;
        let mut local = Vec::new();
        for (index, &(start, end)) in ranges.iter().enumerate() {
            let start = start.max(line_start);
            let end = end.min(line_end);
            if start < end {
                local.push((start - line_start, end - line_start, current == Some(index)));
            }
        }
        out.push(local);
    }
    out
}

/// The first `count` chars of `text`.
fn prefix(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

/// `count` chars of `text` starting at `start`.
fn slice(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

/// Width of `text` in the code font, for prefix/substring highlight offsets.
fn line_width(ui: &egui::Ui, text: &str, mono: &egui::FontId) -> f32 {
    ui.fonts_mut(|f| {
        f.layout_no_wrap(text.to_string(), mono.clone(), egui::Color32::PLACEHOLDER)
            .size()
            .x
    })
}

/// The laid-out lines of a code block: spaces and line breaks stay verbatim.
pub(crate) fn code_lines(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

#[cfg(test)]
mod tests {
    mod layout {
        use super::super::{
            clip_marks, code_line_spans, code_lines, code_top_pad, content_width, gutter_width,
            layout_rows, line_highlights, wrap_line, Row,
        };

        #[test]
        fn wrap_line_splits_on_width_in_chars() {
            // Every char is 10 px wide.
            let m = |s: &str| s.chars().count() as f32 * 10.0;
            assert_eq!(wrap_line("abcdef", 1000.0, &m), vec![(0, 6)]);
            assert_eq!(wrap_line("abcdef", 30.0, &m), vec![(0, 3), (3, 3)]);
            assert_eq!(wrap_line("", 30.0, &m), vec![(0, 0)]);
            // Leading spaces are kept and count toward the width.
            assert_eq!(wrap_line("  abc", 20.0, &m), vec![(0, 2), (2, 2), (4, 1)]);
            // One char wider than the row still gets its own row.
            assert_eq!(wrap_line("ab", 5.0, &m), vec![(0, 1), (1, 1)]);
            // Multi-byte chars keep char offsets, not byte offsets.
            assert_eq!(wrap_line("ééé", 20.0, &m), vec![(0, 2), (2, 1)]);
            // A tab counts as four cells, like the monospace painter.
            assert_eq!(wrap_line("a\tb", 20.0, &m), vec![(0, 1), (1, 1), (2, 1)]);
        }

        #[test]
        fn rows_split_long_lines_and_keep_line_numbers() {
            let m = |s: &str| s.chars().count() as f32 * 10.0;
            let rows = layout_rows("abcdef\nx", 30.0, &m);
            assert_eq!(
                rows,
                vec![
                    Row {
                        line: 0,
                        start: 0,
                        len: 3
                    },
                    Row {
                        line: 0,
                        start: 3,
                        len: 3
                    },
                    Row {
                        line: 1,
                        start: 0,
                        len: 1
                    },
                ]
            );
            // An empty source line still owns one row.
            let rows = layout_rows("a\n", 100.0, &m);
            assert_eq!(
                rows,
                vec![
                    Row {
                        line: 0,
                        start: 0,
                        len: 1
                    },
                    Row {
                        line: 1,
                        start: 0,
                        len: 0
                    },
                ]
            );
        }

        #[test]
        fn wrapped_rows_have_disjoint_char_spans() {
            let m = |s: &str| s.chars().count() as f32 * 10.0;
            let rows = layout_rows("abcdef", 20.0, &m);
            let offsets: Vec<usize> = rows.iter().map(|r| r.start).collect();
            assert_eq!(offsets, vec![0, 2, 4]);
            // Row starts plus lengths tile the line without gaps or overlap.
            assert_eq!(
                rows.iter().map(|r| (r.start, r.len)).collect::<Vec<_>>(),
                vec![(0, 2), (2, 2), (4, 2)]
            );
        }

        #[test]
        fn pointer_on_a_continuation_row_maps_to_that_rows_chars() {
            use crate::app::selection::hit_test;
            use crate::justify::CharSpan;
            let spans = vec![
                CharSpan {
                    start: 0,
                    len: 2,
                    rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(20.0, 10.0)),
                },
                CharSpan {
                    start: 2,
                    len: 2,
                    rect: egui::Rect::from_min_size(egui::pos2(0.0, 10.0), egui::vec2(20.0, 10.0)),
                },
            ];
            // Midpoint of the second row: char 3, not char 1.
            assert_eq!(hit_test(&spans, egui::pos2(10.0, 15.0)), Some(3));
        }

        #[test]
        fn content_width_keeps_both_pads() {
            assert_eq!(content_width(1000.0, 20.0, 8.0, 9.0), 964.0);
            // Never narrower than one cell, even when the column is tiny.
            assert_eq!(content_width(30.0, 30.0, 8.0, 9.0), 9.0);
        }

        #[test]
        fn top_pad_clears_the_copy_icon() {
            assert!(code_top_pad(17.0) >= 16.0);
            assert_eq!(code_top_pad(40.0), 20.0);
        }

        #[test]
        fn gutter_is_digits_plus_one_cell() {
            assert_eq!(gutter_width(1, 8.0), 16.0);
            assert_eq!(gutter_width(9, 8.0), 16.0);
            assert_eq!(gutter_width(10, 8.0), 24.0);
            assert_eq!(gutter_width(999, 8.0), 32.0);
            assert_eq!(gutter_width(0, 8.0), 16.0);
        }

        #[test]
        fn marks_are_clipped_and_shifted_to_the_row() {
            let row = Row {
                line: 0,
                start: 3,
                len: 3,
            };
            assert_eq!(
                clip_marks(&[(1, 5, false), (4, 6, true), (8, 9, true)], &row),
                vec![(0, 2, false), (1, 3, true)]
            );
            assert_eq!(clip_marks(&[], &row), Vec::new());
        }

        #[test]
        fn lines_preserve_leading_spaces_and_breaks() {
            assert_eq!(
                code_lines("    fn main() {\n\tprintln!();\n\n}"),
                vec!["    fn main() {", "\tprintln!();", "", "}"]
            );
            // A trailing newline is stripped in doc.rs; an empty block is one row.
            assert_eq!(code_lines(""), vec![""]);
        }

        #[test]
        fn cross_line_hit_is_split_per_line() {
            // "ab\ncd": chars 1..4 covers `b`, the newline, and `c`.
            let marks = line_highlights("ab\ncd", &[(1, 4), (3, 4)], Some(1));
            assert_eq!(
                marks,
                vec![vec![(1, 2, false)], vec![(0, 1, false), (0, 1, true)]]
            );
            let marks = line_highlights("ab\ncd", &[(1, 4)], Some(0));
            assert_eq!(marks, vec![vec![(1, 2, true)], vec![(0, 1, true)]]);
        }

        #[test]
        fn no_ranges_means_no_marks() {
            assert_eq!(line_highlights("", &[], None), vec![Vec::new()]);
            assert_eq!(
                line_highlights("a\nb", &[(9, 12)], None),
                vec![vec![], vec![]]
            );
        }

        #[test]
        fn empty_line_does_not_drift_char_offsets() {
            assert_eq!(code_line_spans("a\n\nb"), vec![(0, 1), (2, 0), (3, 1)]);
            // A range on `b` stays on the third line even after the empty row.
            assert_eq!(
                line_highlights("a\n\nb", &[(3, 4)], Some(0)),
                vec![vec![], vec![], vec![(0, 1, true)]]
            );
        }
    }
}
