//! Fenced-code rendering for the `<mdcode>` html hook: verbatim monospace
//! lines on a code-coloured background, left-aligned and never wrapped. A line
//! wider than the reading column widens the block so `h`/`l` can pan to it.

use std::collections::HashMap;

use crate::doc::CodeSpec;

/// Shared, interior-mutable state backing the `<mdcode>` render path. Holds the
/// parsed specs and the reading column recorded once per frame.
pub(crate) struct CodeUi {
    specs: HashMap<String, CodeSpec>,
    /// Reading column in effect this frame; `ui.available_width()` inside the
    /// html hook is not that column.
    column: f32,
}

impl CodeUi {
    pub(crate) fn new(specs: HashMap<String, CodeSpec>) -> Self {
        CodeUi { specs, column: 0.0 }
    }

    /// Record the document's reading column once per frame, before the viewer
    /// runs.
    pub(crate) fn begin_frame(&mut self, column: f32) {
        self.column = column;
    }

    pub(crate) fn reset(&mut self, specs: HashMap<String, CodeSpec>) {
        self.specs = specs;
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str) {
        // Clone so the spec outlives the immutable borrow of `self`.
        let spec = match self.specs.get(id) {
            Some(s) => s.clone(),
            None => return, // unknown id: draw nothing, never panic
        };
        let mono = ui
            .style()
            .text_styles
            .get(&egui::TextStyle::Monospace)
            .cloned()
            .unwrap_or_else(|| egui::FontId::monospace(14.0));
        let row_h = ui.fonts_mut(|f| f.row_height(&mono));
        let lines = code_lines(&spec.text);
        let measure = |s: &str| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap(s.to_string(), mono.clone(), egui::Color32::PLACEHOLDER)
                    .size()
                    .x
            })
        };
        let widest = widest_line(&lines, &measure);
        let pad = row_h * 0.5;
        // Widest line plus padding, never narrower than the reading column: the
        // scroll area sees the wider allocation and `h`/`l` can pan to it.
        let width = self.column.max(widest + pad * 2.0);
        let height = block_height(lines.len(), row_h) + pad * 2.0;
        let (_, rect) = ui.allocate_space(egui::vec2(width, height));
        let painter = ui.painter().clone();
        painter.rect_filled(rect, 2.0, ui.visuals().code_bg_color);
        let color = ui.visuals().text_color();
        for (row, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue; // the row is still reserved by `height`
            }
            let galley = ui.fonts_mut(|f| f.layout_no_wrap(line.to_string(), mono.clone(), color));
            let pos = egui::Pos2::new(rect.left() + pad, rect.top() + pad + row as f32 * row_h);
            painter.galley(pos, galley, color);
        }
    }
}

/// The laid-out lines of a code block: spaces and line breaks stay verbatim.
pub(crate) fn code_lines(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

/// Height of a block of `line_count` rows; an empty block still keeps one row.
pub(crate) fn block_height(line_count: usize, row_height: f32) -> f32 {
    line_count.max(1) as f32 * row_height
}

/// Width of the widest line under `measure`.
pub(crate) fn widest_line(lines: &[&str], measure: &dyn Fn(&str) -> f32) -> f32 {
    lines
        .iter()
        .map(|line| measure(line))
        .fold(0.0f32, f32::max)
}

#[cfg(test)]
mod tests {
    mod layout {
        use super::super::{block_height, code_lines, widest_line};

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
        fn height_counts_every_line_including_empty_ones() {
            assert_eq!(block_height(0, 20.0), 20.0);
            assert_eq!(block_height(1, 20.0), 20.0);
            assert_eq!(block_height(4, 20.0), 80.0);
        }

        #[test]
        fn widest_line_measures_each_line() {
            let measure = |s: &str| s.chars().count() as f32 * 10.0;
            let lines = ["fn x() {}", "", "        indented"];
            assert_eq!(widest_line(&lines, &measure), 160.0);
            assert_eq!(widest_line(&[], &measure), 0.0);
        }
    }
}
