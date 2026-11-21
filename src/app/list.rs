//! List rendering for the `<mdlist>` html hook: one justified, hanging-indented
//! row per item, with the marker painted in the gutter.

use std::collections::HashMap;

use crate::doc::{ListKind, ListSpec};
use crate::justify::{self, Marker};

use super::searchui::SearchUi;
use super::table::table_pad;

/// Shared, interior-mutable state backing the `<mdlist>` render path. Holds the
/// parsed specs and the reading column recorded once per frame.
pub(crate) struct ListUi {
    specs: HashMap<String, ListSpec>,
    /// Reading column in effect this frame; `ui.available_width()` inside the
    /// html hook is not that column.
    column: f32,
}

impl ListUi {
    pub(crate) fn new(specs: HashMap<String, ListSpec>) -> Self {
        ListUi { specs, column: 0.0 }
    }

    /// Record the document's reading column once per frame, before the viewer
    /// runs.
    pub(crate) fn begin_frame(&mut self, column: f32) {
        self.column = column;
    }

    pub(crate) fn reset(&mut self, specs: HashMap<String, ListSpec>) {
        self.specs = specs;
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str, search: &mut SearchUi) {
        // Clone so the spec outlives the borrow while the closure borrows `ui`.
        let spec = match self.specs.get(id) {
            Some(s) => s.clone(),
            None => return, // unknown id: draw nothing, never panic
        };
        let column = self.column;
        let space = justify::space_width(ui);
        let pad = table_pad(ui.text_style_height(&egui::TextStyle::Body));
        // A bare `add_space` would advance horizontally in the hook's wrapping
        // layout; the vertical wrapper makes the padding real space.
        ui.vertical(|ui| {
            // Half the normal item gap reads as one block, not loose items.
            ui.spacing_mut().item_spacing.y *= 0.5;
            ui.add_space(pad);
            for item in &spec.items {
                // One searchable block per item, at the row it starts on.
                let block = search.record(ui, crate::justify::search_text(&item.text));
                let (ranges, current) = search.highlights(block);
                let highlights = justify::MatchHighlight {
                    ranges: &ranges,
                    current,
                };
                let marker = match item.kind {
                    ListKind::Bullet if item.depth % 2 == 1 => Marker::Hollow,
                    ListKind::Bullet => Marker::Bullet,
                    ListKind::Ordered(n) => Marker::Number(n),
                    ListKind::Task(done) => Marker::Task(done),
                };
                let runs = justify::parse_inline(&item.text);
                let indent = justify::list_indent(item.depth, space);
                let height = justify::draw_indented(ui, &runs, column, indent, marker, highlights);
                search.set_height(block, height);
            }
            ui.add_space(pad);
        });
    }
}
