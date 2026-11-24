//! Heading rendering for the `<mdheading>` html hook: one left-aligned,
//! wrapped row per heading, scaled by level and painted in the strong colour.

use std::collections::HashMap;

use super::searchui::SearchUi;
use crate::doc::HeadingSpec;
use crate::justify;

/// Shared, interior-mutable state backing the `<mdheading>` render path. Holds
/// the parsed specs and the reading column recorded once per frame.
pub(crate) struct HeadingUi {
    specs: HashMap<String, HeadingSpec>,
    /// Reading column in effect this frame; `ui.available_width()` inside the
    /// html hook is not that column.
    column: f32,
}

impl HeadingUi {
    pub(crate) fn new(specs: HashMap<String, HeadingSpec>) -> Self {
        HeadingUi { specs, column: 0.0 }
    }

    /// Record the document's reading column once per frame, before the viewer
    /// runs.
    pub(crate) fn begin_frame(&mut self, column: f32) {
        self.column = column;
    }

    pub(crate) fn reset(&mut self, specs: HashMap<String, HeadingSpec>) {
        self.specs = specs;
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str, search: &mut SearchUi) {
        // Borrow the spec for the frame; nothing in `self` is mutated here.
        let Some(spec) = self.specs.get(id) else {
            return; // unknown id: draw nothing, never panic
        };
        let column = self.column;
        let body = justify::body_font(ui);
        let size = heading_size(spec.level, body.size);
        let font = egui::FontId::new(size, body.family.clone());
        let color = ui.visuals().strong_text_color();
        let runs = justify::parse_inline(&spec.text);
        let row_h = ui.text_style_height(&egui::TextStyle::Body);
        // Vertical breathing room in body rows. The gap above stays at one row
        // (it matches the old `<mdspace>` spacing); the gap below is tightened
        // so a heading does not float away from the body that belongs to it.
        let before = row_h * 0.55;
        let after = row_h * 1.2;
        // The html hook runs in a wrapping left-to-right layout, where a bare
        // `add_space` would advance horizontally; the vertical wrapper makes the
        // gaps above and below real vertical space.
        ui.vertical(|ui| {
            if spec.space_before {
                ui.add_space(before);
            }
            // Record at the heading's text top, after the gap above, so a jump
            // does not leave a blank band.
            let text = search.cached_text(id, || justify::search_text(&spec.text));
            let block = search.record(ui, text);
            search.record_anchor(&crate::doc::slug(&spec.text), block);
            search.record_heading(&spec.id, block);
            let (ranges, current) = search.highlights(block);
            let highlights = justify::MatchHighlight {
                ranges: &ranges,
                current,
                selection: search.selection_range(block),
            };
            if runs.is_empty() {
                // `#` still owns a heading row, not just the gaps.
                let row_h = ui.fonts_mut(|f| f.row_height(&font));
                ui.allocate_space(egui::vec2(column, row_h));
                search.set_height(block, row_h);
            } else {
                let painted = justify::draw_with(ui, &runs, column, font, color, false, highlights);
                search.set_height(block, painted.height);
                search.set_rect(block, painted.rect);
                search.set_char_map(block, painted.chars);
                for rect in painted.interactives {
                    search.add_control(rect, egui::CursorIcon::PointingHand);
                }
            }
            if spec.space_after {
                ui.add_space(after);
            }
        });
    }
}

/// Font size of a heading at `body` size: H1 = 1.6×, H2 1.4×, H3 1.2×,
/// H4 1.1×, H5/H6 1.0×.
pub(crate) fn heading_size(level: u8, body: f32) -> f32 {
    let scale = match level {
        1 => 1.6,
        2 => 1.4,
        3 => 1.2,
        4 => 1.1,
        _ => 1.0,
    };
    body * scale
}

#[cfg(test)]
mod tests {
    mod size {
        use super::super::heading_size;

        #[test]
        fn scales_per_level() {
            let approx = |level: u8, body: f32, want: f32| {
                let got = heading_size(level, body);
                assert!((got - want).abs() < 1e-4, "{level}: {got} != {want}");
            };
            approx(1, 14.0, 22.4);
            approx(2, 14.0, 19.6);
            approx(3, 14.0, 16.8);
            approx(4, 14.0, 15.4);
            approx(5, 14.0, 14.0);
            approx(6, 14.0, 14.0);
            // Out-of-range levels fall back to body size, never panic.
            assert_eq!(heading_size(0, 10.0), 10.0);
            assert_eq!(heading_size(7, 10.0), 10.0);
        }
    }
}
