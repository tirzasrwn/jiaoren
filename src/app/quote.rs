//! Blockquote rendering for the `<mdquote>` html hook: one vertical bar per
//! nesting level, inline formatting, and per-line search/selection support.

use std::collections::HashMap;

use super::searchui::SearchUi;
use crate::doc::QuoteSpec;

pub(crate) struct QuoteUi {
    specs: HashMap<String, QuoteSpec>,
    column: f32,
    occurrences: HashMap<String, usize>,
}

impl QuoteUi {
    pub(crate) fn new(specs: HashMap<String, QuoteSpec>) -> Self {
        QuoteUi {
            specs,
            column: 0.0,
            occurrences: HashMap::new(),
        }
    }

    pub(crate) fn begin_frame(&mut self, column: f32) {
        self.column = column;
        self.occurrences.clear();
    }

    pub(crate) fn reset(&mut self, specs: HashMap<String, QuoteSpec>) {
        self.specs = specs;
        self.occurrences.clear();
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str, search: &mut SearchUi) {
        let Some(spec) = self.specs.get(id).cloned() else {
            return;
        };
        let mut lines: &[(u8, String)] = &spec.lines;
        while lines.first().is_some_and(|(_, text)| text.is_empty()) {
            lines = &lines[1..];
        }
        while lines.last().is_some_and(|(_, text)| text.is_empty()) {
            lines = &lines[..lines.len() - 1];
        }
        if lines.is_empty() {
            return;
        }
        let text = lines
            .iter()
            .map(|(_, line)| line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let block = search.record(ui, text);
        let (ranges, current) = search.highlights(block);
        let selection = search.selection_range(block);
        let sel_ranges: Vec<(usize, usize)> = selection.into_iter().collect();
        let row_h = ui.text_style_height(&egui::TextStyle::Body);
        let gap = row_h * 0.45;
        let weak = ui.visuals().weak_text_color();
        let column = self.column;
        let mut chars = Vec::new();
        let mut start = 0usize;
        let mut union: Option<egui::Rect> = None;
        // The html hook lays children out horizontally; a quote must stack.
        ui.vertical(|ui| {
            for (depth, line) in lines {
                let line_start = start;
                let line_len = line.chars().count();
                start += line_len + 1;
                let (local_ranges, local_current) =
                    clip_with_current(&ranges, current, line_start, line_len);
                let local_sel = clip(&sel_ranges, line_start, line_len);
                let runs = crate::justify::parse_inline(line);
                let indent = *depth as f32 * gap;
                let painted = ui
                    .horizontal(|ui| {
                        ui.add_space(indent);
                        crate::justify::draw_left(
                            ui,
                            &runs,
                            (column - indent).max(40.0),
                            crate::justify::MatchHighlight {
                                ranges: &local_ranges,
                                current: local_current,
                                selection: local_sel.first().copied(),
                            },
                        )
                    })
                    .inner;
                let rect = painted.rect;
                union = Some(union.map_or(rect, |u| u.union(rect)));
                let left = rect.left() - indent;
                for level in 0..*depth {
                    let x = left + level as f32 * gap;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::Pos2::new(x, rect.top()),
                            egui::vec2(2.0, rect.height().max(row_h)),
                        ),
                        0.0,
                        weak,
                    );
                }
                for span in painted.chars {
                    chars.push(crate::justify::CharSpan {
                        start: line_start + span.start,
                        len: span.len,
                        rect: span.rect,
                    });
                }
                if line.is_empty() {
                    ui.add_space(row_h * 0.35);
                }
            }
        });
        search.set_char_map(block, chars);
        if let Some(rect) = union {
            search.set_rect(block, rect);
            search.set_height(block, rect.height());
        }
    }
}

/// Clip ranges to one line and report the local index of `current`.
fn clip_with_current(
    ranges: &[(usize, usize)],
    current: Option<usize>,
    start: usize,
    len: usize,
) -> (Vec<(usize, usize)>, Option<usize>) {
    let end = start + len;
    let mut out = Vec::new();
    let mut local_current = None;
    for (index, &(s, e)) in ranges.iter().enumerate() {
        let s = s.max(start);
        let e = e.min(end);
        if s < e {
            if current == Some(index) {
                local_current = Some(out.len());
            }
            out.push((s - start, e - start));
        }
    }
    (out, local_current)
}

/// Clip global highlight ranges to one line and shift them to local offsets.
fn clip(ranges: &[(usize, usize)], start: usize, len: usize) -> Vec<(usize, usize)> {
    let end = start + len;
    ranges
        .iter()
        .filter_map(|&(s, e)| {
            let s = s.max(start);
            let e = e.min(end);
            (s < e).then(|| (s - start, e - start))
        })
        .collect()
}
