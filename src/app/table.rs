//! Table rendering for the `<mdtable>` html hook: a grid of cells painted by
//! the justify module, with the library as a fallback for images and raw HTML.

use std::collections::HashMap;

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use crate::doc::TableSpec;

use super::geometry::column_widths;

/// Shared, interior-mutable state backing the `<mdtable>` render path in the
/// html hook. Holds the parsed specs, a per-cell `CommonMarkCache` separate
/// from the outer viewer's (a cell never re-borrows the outer cache), and a
/// per-id occurrence counter so identical tables each get a distinct egui `Id`.
pub(crate) struct TableUi {
    tables: HashMap<String, TableSpec>,
    cell_cache: CommonMarkCache,
    occurrences: HashMap<String, usize>,
}

impl TableUi {
    pub(crate) fn new(tables: HashMap<String, TableSpec>) -> Self {
        TableUi {
            tables,
            cell_cache: CommonMarkCache::default(),
            occurrences: HashMap::new(),
        }
    }

    /// Restart occurrence counters at the top of the document, once per frame.
    pub(crate) fn begin_frame(&mut self) {
        self.occurrences.clear();
    }

    pub(crate) fn reset(&mut self, tables: HashMap<String, TableSpec>) {
        self.tables = tables;
        self.cell_cache = CommonMarkCache::default();
        self.occurrences.clear();
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str) {
        // Clone so the spec outlives the immutable borrow while `cell_cache` is
        // borrowed mutably below. Tables are small; the clone is the cheap fix.
        let spec = match self.tables.get(id) {
            Some(s) => s.clone(),
            None => return, // unknown id: draw nothing, never panic
        };
        let ncols = spec.header.len();
        if ncols == 0 {
            return;
        }
        let occurrence = self.occurrences.entry(id.to_string()).or_insert(0);
        let current = *occurrence;
        *occurrence += 1;
        let occurrence = current;
        let w = column_widths(ui.available_width(), ui.spacing().item_spacing.x, ncols);
        // A distinct id per occurrence so two identical tables never share state.
        let grid_id = ui.id().with(("mdtable", id, occurrence));
        egui::Grid::new(grid_id).striped(true).show(ui, |ui| {
            for cell in &spec.header {
                render_cell(ui, w, cell, &mut self.cell_cache);
            }
            ui.end_row();
            for row in &spec.rows {
                for cell in row {
                    render_cell(ui, w, cell, &mut self.cell_cache);
                }
                ui.end_row();
            }
        });
    }
}

/// Cells with images or raw HTML still need the library: our mini-parser cannot
/// express them. Everything else (plain text, inline styles, code, links) goes
/// through the token-splitting left-aligned painter so long paths wrap.
fn cell_needs_library_fallback(md: &str) -> bool {
    md.contains("![") || md.contains('<')
}

/// Render one cell, capped at `w` so text wraps and inline formatting (bold,
/// code, links) still applies. Long tokens break at separators, so a cell never
/// widens the grid past the reading column.
fn render_cell(ui: &mut egui::Ui, w: f32, cell_md: &str, cache: &mut CommonMarkCache) {
    // A vertical scope reports the real content height back to the grid; a
    // zero-height `allocate_ui_with_layout` would collapse every row to 0 and
    // let wrapped text paint over the next row.
    ui.vertical(|ui| {
        ui.set_width(w);
        if cell_needs_library_fallback(cell_md) {
            CommonMarkViewer::new().show(ui, cache, cell_md);
            return;
        }
        let runs = crate::justify::parse_inline(cell_md);
        if runs.is_empty() {
            // Keep an all-blank row from collapsing to zero height.
            ui.add_space(ui.spacing().interact_size.y);
            return;
        }
        crate::justify::draw_left(ui, &runs, w);
    });
}

#[cfg(test)]
mod tests {
    use super::cell_needs_library_fallback;

    #[test]
    fn cell_falls_back_only_for_images_and_html() {
        assert!(cell_needs_library_fallback("see ![x](a.png)"));
        assert!(cell_needs_library_fallback("a<br>b"));
        assert!(!cell_needs_library_fallback("**bold** and `code`"));
        assert!(!cell_needs_library_fallback("src/db/schema.ts:293-311"));
    }
}
