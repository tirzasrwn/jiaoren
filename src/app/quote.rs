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
        // Catppuccin mantle behind the quote and a surface2 accent bar on its
        // left edge, like a callout panel.
        let bg = ui.visuals().faint_bg_color;
        let bar_color = ui.visuals().widgets.active.bg_fill;
        let bar_w = (row_h * 0.2).clamp(2.5, 4.0);
        let bar_inset = (row_h * 0.4).clamp(5.0, 10.0);
        let gap_after_bar = (row_h * 0.7).max(10.0);
        let left_margin = (bar_w + gap_after_bar).round().clamp(12.0, 100.0) as i8;
        let right_margin = (row_h * 0.55).round().clamp(6.0, 100.0) as i8;
        let pad_y = (row_h * 0.4).round().clamp(4.0, 100.0) as i8;
        let column = self.column;
        let inner_w = (column - left_margin as f32 - right_margin as f32).max(40.0);
        let mut chars = Vec::new();
        let mut start = 0usize;
        let mut frame_rect = None;
        // One bar per nesting level, spanning the lines that still belong to
        // that level (blank quoted lines included).
        let mut depth_spans: Vec<(usize, f32, f32)> = Vec::new();
        // The html hook lays children out horizontally; a quote must stack.
        ui.vertical(|ui| {
            let frame = egui::Frame::new()
                .fill(bg)
                .corner_radius(egui::CornerRadius::ZERO)
                .inner_margin(egui::Margin {
                    left: left_margin,
                    right: right_margin,
                    top: pad_y,
                    bottom: pad_y,
                })
                .show(ui, |ui| {
                    ui.set_width(inner_w);
                    for (depth, line) in lines {
                        let line_top = ui.cursor().top();
                        let line_start = start;
                        let line_len = line.chars().count();
                        start += line_len + 1;
                        let mapped = crate::justify::local_ranges(line_start, line, &ranges);
                        let local_ranges: Vec<(usize, usize)> =
                            mapped.iter().map(|&(s, e, _)| (s, e)).collect();
                        let local_current = mapped
                            .iter()
                            .position(|&(_, _, index)| Some(index) == current);
                        let local_sel: Vec<(usize, usize)> =
                            crate::justify::local_ranges(line_start, line, &sel_ranges)
                                .into_iter()
                                .map(|(s, e, _)| (s, e))
                                .collect();
                        let runs = crate::justify::parse_inline_cached(line);
                        let indent = *depth as f32 * gap;
                        let painted = ui
                            .horizontal(|ui| {
                                ui.add_space(indent);
                                crate::justify::draw_left(
                                    ui,
                                    &runs,
                                    (inner_w - indent).max(40.0),
                                    crate::justify::MatchHighlight {
                                        ranges: &local_ranges,
                                        current: local_current,
                                        selection: local_sel.first().copied(),
                                    },
                                )
                            })
                            .inner;
                        for rect in &painted.interactives {
                            search.add_control(*rect, egui::CursorIcon::PointingHand);
                        }
                        for span in painted.chars {
                            chars.push(crate::justify::CharSpan {
                                start: line_start + span.start,
                                len: span.len,
                                rect: span.rect,
                            });
                        }
                        let line_bottom = if line.is_empty() {
                            ui.add_space(row_h * 0.35);
                            line_top + row_h * 0.35
                        } else {
                            painted.rect.bottom()
                        };
                        depth_spans.push((*depth as usize, line_top, line_bottom));
                    }
                });
            let rect = frame.response.rect;
            let max_depth = depth_spans.iter().map(|&(d, ..)| d).max().unwrap_or(0);
            for level in 0..max_depth {
                let mut top = f32::INFINITY;
                let mut bottom = f32::NEG_INFINITY;
                for &(depth, line_top, line_bottom) in &depth_spans {
                    if depth > level {
                        top = top.min(line_top);
                        bottom = bottom.max(line_bottom);
                    }
                }
                if bottom <= top {
                    continue;
                }
                let x = rect.left() + bar_inset + level as f32 * gap;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::Pos2::new(x, top),
                        egui::vec2(bar_w, bottom - top),
                    ),
                    0.0,
                    bar_color,
                );
            }
            frame_rect = Some(rect);
        });
        search.set_char_map(block, chars);
        if let Some(rect) = frame_rect {
            search.set_rect(block, rect);
            search.set_height(block, rect.height());
        }
    }
}
