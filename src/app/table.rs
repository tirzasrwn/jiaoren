//! Table rendering for the `<mdtable>` html hook: a grid of cells painted by
//! the justify module, with the library as a fallback for images and raw HTML.

use std::collections::HashMap;

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use crate::doc::TableSpec;

use super::geometry::table_column_widths;

/// Shared, interior-mutable state backing the `<mdtable>` render path in the
/// html hook. Holds the parsed specs, a per-cell `CommonMarkCache` separate
/// from the outer viewer's (a cell never re-borrows the outer cache), a
/// per-id occurrence counter so identical tables each get a distinct egui `Id`,
/// and the last measured column widths per table.
pub(crate) struct TableUi {
    tables: HashMap<String, TableSpec>,
    cell_cache: CommonMarkCache,
    occurrences: HashMap<String, usize>,
    widths: HashMap<String, (i32, i32, Vec<f32>)>,
}

impl TableUi {
    pub(crate) fn new(tables: HashMap<String, TableSpec>) -> Self {
        TableUi {
            tables,
            cell_cache: CommonMarkCache::default(),
            occurrences: HashMap::new(),
            widths: HashMap::new(),
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
        self.widths.clear();
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
        let available = ui.available_width();
        let spacing = ui.spacing().item_spacing.x;
        let widths = self.column_widths(ui, id, &spec, available, spacing);
        // A distinct id per occurrence so two identical tables never share state.
        let grid_id = ui.id().with(("mdtable", id, occurrence));
        let body_row_height = ui.text_style_height(&egui::TextStyle::Body);
        // The html hook runs in a wrapping left-to-right layout, where a bare
        // `add_space` would advance horizontally; the vertical wrapper turns the
        // padding into real space above and below the grid.
        ui.vertical(|ui| {
            ui.add_space(table_pad(body_row_height));
            egui::Grid::new(grid_id).striped(true).show(ui, |ui| {
                for (i, cell) in spec.header.iter().enumerate() {
                    render_cell(ui, cell_width(&widths, i), cell, &mut self.cell_cache);
                }
                ui.end_row();
                for row in &spec.rows {
                    for (i, cell) in row.iter().enumerate() {
                        render_cell(ui, cell_width(&widths, i), cell, &mut self.cell_cache);
                    }
                    ui.end_row();
                }
            });
            ui.add_space(table_pad(body_row_height));
        });
    }

    /// Content-proportional widths for one table, cached by id and layout so
    /// the text measurement runs only when the column width or DPI changes.
    fn column_widths(
        &mut self,
        ui: &egui::Ui,
        id: &str,
        spec: &TableSpec,
        available: f32,
        spacing: f32,
    ) -> Vec<f32> {
        let available_key = available.round() as i32;
        let ppp_key = (ui.ctx().pixels_per_point() * 100.0).round() as i32;
        if let Some((a, p, widths)) = self.widths.get(id) {
            if *a == available_key && *p == ppp_key {
                return widths.clone();
            }
        }
        let body = egui::TextStyle::Body.resolve(ui.style());
        let cap = 0.6 * available;
        let ncols = spec.header.len();
        let desired: Vec<f32> = (0..ncols)
            .map(|i| {
                spec.header
                    .get(i)
                    .into_iter()
                    .chain(spec.rows.iter().filter_map(|row| row.get(i)))
                    .map(|md| measure(ui, md, &body))
                    .fold(1.0_f32, f32::max)
                    .min(cap)
            })
            .collect();
        let widths = table_column_widths(&desired, available, spacing, 80.0);
        self.widths
            .insert(id.to_string(), (available_key, ppp_key, widths.clone()));
        widths
    }
}

/// Vertical breathing room above and below a table: three quarters of a body
/// row reads as balanced without pushing tables apart from their captions.
pub(crate) fn table_pad(row_height: f32) -> f32 {
    row_height * 0.75
}

/// Single-line width of a cell's raw Markdown. Markup characters are counted,
/// so this overestimates styled text slightly; it only steers the proportions.
fn measure(ui: &egui::Ui, md: &str, font: &egui::FontId) -> f32 {
    ui.fonts_mut(|f| {
        f.layout_no_wrap(md.to_owned(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
    })
}

/// Width for column `i`, falling back to the last measured width for rows that
/// carry more cells than the header.
fn cell_width(widths: &[f32], i: usize) -> f32 {
    widths
        .get(i)
        .or_else(|| widths.last())
        .copied()
        .unwrap_or(80.0)
}

/// Cells with images or raw HTML still need the library: our mini-parser cannot
/// express them. Everything else (plain text, inline styles, code, links) goes
/// through the token-splitting left-aligned painter so long paths wrap.
fn cell_needs_library_fallback(md: &str) -> bool {
    md.contains("![") || md.contains('<')
}

/// Render one cell, capped at `w` so text wraps and inline formatting (bold,
/// code, links) still applies. Long tokens break at separators, so a cell never
/// widens the grid past its column.
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
    mod table {
        use super::super::cell_needs_library_fallback;
        use super::super::table_pad;

        #[test]
        fn table_pad_is_three_quarters_of_a_row() {
            assert_eq!(table_pad(20.0), 15.0);
        }

        #[test]
        fn cell_falls_back_only_for_images_and_html() {
            assert!(cell_needs_library_fallback("see ![x](a.png)"));
            assert!(cell_needs_library_fallback("a<br>b"));
            assert!(!cell_needs_library_fallback("**bold** and `code`"));
            assert!(!cell_needs_library_fallback("src/db/schema.ts:293-311"));
        }
    }
}
