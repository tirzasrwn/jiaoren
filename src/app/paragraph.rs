//! Justified body paragraphs for the `<mdpara>` html hook: the built-in
//! renderer, with a library fallback for images and raw HTML.

use std::collections::HashMap;

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use super::searchui::SearchUi;

/// Whether a paragraph is painted by the library instead of the justify
/// painter: images and raw HTML. Those blocks are not searchable because their
/// painted glyphs are not the recorded run text.
pub(crate) fn paragraph_needs_library(source: &str) -> bool {
    source.contains("![") || source.contains('<')
}

/// Shared, interior-mutable state backing the `<mdpara>` render path. Body
/// paragraphs are painted justified by `justify`; anything the mini-parser
/// cannot express (images, raw HTML) falls back to the library with a cache of
/// its own, separate from the outer viewer's.
pub(crate) struct ParagraphUi {
    paragraphs: HashMap<String, String>,
    cache: CommonMarkCache,
    /// Reading column in effect this frame, set from the same `col` the viewer
    /// is given via `ui.set_width(col)`.
    column: f32,
}

impl ParagraphUi {
    pub(crate) fn new(paragraphs: HashMap<String, String>) -> Self {
        ParagraphUi {
            paragraphs,
            cache: CommonMarkCache::default(),
            column: 0.0,
        }
    }

    /// Record the document's reading column once per frame, before the viewer
    /// runs. `ui.available_width()` inside the html hook is not that column.
    pub(crate) fn begin_frame(&mut self, column: f32) {
        self.column = column;
    }

    pub(crate) fn reset(&mut self, paragraphs: HashMap<String, String>) {
        self.paragraphs = paragraphs;
        self.cache = CommonMarkCache::default();
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str, search: &mut SearchUi) {
        // Disjoint fields: the source borrow ends before the cache is mutated.
        let Some(source) = self.paragraphs.get(id) else {
            return; // unknown id: draw nothing, never panic
        };
        // Library-routed paragraphs paint glyphs this module does not model, so
        // they are not searchable: skip recording rather than index text that is
        // not on screen.
        if paragraph_needs_library(source) {
            let gap = crate::justify::block_gap(ui);
            ui.vertical(|ui| {
                CommonMarkViewer::new().show(ui, &mut self.cache, source);
                ui.add_space(gap);
            });
            return;
        }
        let block = search.record(ui, crate::justify::search_text(source));
        let (ranges, current) = search.highlights(block);
        let selection = search.selection_range(block);
        let runs = crate::justify::parse_inline(source);
        let gap = crate::justify::block_gap(ui);
        let painted = crate::justify::draw_spaced(
            ui,
            &runs,
            self.column,
            crate::justify::MatchHighlight {
                ranges: &ranges,
                current,
                selection,
            },
            gap,
        );
        search.set_height(block, painted.height);
        search.set_rect(block, painted.rect);
        search.set_char_map(block, painted.chars);
        for rect in painted.interactives {
            search.add_control(rect, egui::CursorIcon::PointingHand);
        }
    }
}
