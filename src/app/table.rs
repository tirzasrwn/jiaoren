//! Table rendering for the `<mdtable>` html hook: a grid of cells painted by
//! the justify module, with the library as a fallback for images and raw HTML.

use std::collections::HashMap;

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use crate::doc::TableSpec;

use super::geometry::{cap_column_widths, table_column_widths};
use super::searchui::SearchUi;

/// Shared, interior-mutable state backing the `<mdtable>` render path in the
/// html hook. Holds the parsed specs, a per-cell `CommonMarkCache` separate
/// from the outer viewer's (a cell never re-borrows the outer cache), a
/// per-id occurrence counter so identical tables each get a distinct egui `Id`,
/// and the last measured column widths per table.
pub(crate) struct TableUi {
    tables: HashMap<String, TableSpec>,
    cell_cache: CommonMarkCache,
    occurrences: HashMap<String, usize>,
    widths: HashMap<String, (i32, i32, i32, Vec<f32>)>,
    /// Set when a table's `copy csv` button was clicked this frame.
    copied: bool,
}

impl TableUi {
    pub(crate) fn new(tables: HashMap<String, TableSpec>) -> Self {
        TableUi {
            tables,
            cell_cache: CommonMarkCache::default(),
            occurrences: HashMap::new(),
            widths: HashMap::new(),
            copied: false,
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
        self.copied = false;
    }

    /// Take the "a table was copied" flag for the frame, if the button fired.
    pub(crate) fn take_copied(&mut self) -> bool {
        std::mem::take(&mut self.copied)
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str, search: &mut SearchUi) {
        // Clone so the spec outlives the immutable borrow while `cell_cache` is
        // borrowed mutably below. Tables are small; the clone is the cheap fix.
        let spec = match self.tables.get(id) {
            Some(s) => s.clone(),
            None => return, // unknown id: draw nothing, never panic
        };
        let ncols = spec
            .header
            .len()
            .max(spec.rows.iter().map(Vec::len).max().unwrap_or(0));
        if ncols == 0 {
            return;
        }
        let occurrence = self.occurrences.entry(id.to_string()).or_insert(0);
        let current = *occurrence;
        *occurrence += 1;
        let occurrence = current;
        let available = ui.available_width();
        let spacing = ui.spacing().item_spacing.x;
        let widths = self.column_widths(ui, id, &spec, ncols, available, spacing);
        // A distinct id per occurrence so two identical tables never share state.
        let grid_id = ui.id().with(("mdtable", id, occurrence));
        let body_row_height = ui.text_style_height(&egui::TextStyle::Body);
        let dark = ui.visuals().dark_mode;
        // The html hook runs in a wrapping left-to-right layout, where a bare
        // `add_space` would advance horizontally; the vertical wrapper turns the
        // padding into real space above and below the grid.
        ui.vertical(|ui| {
            ui.add_space(table_pad(body_row_height));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (clicked, rect) = super::widgets::copy_button(ui, "Copy table as CSV");
                search.add_control(rect);
                if clicked {
                    ui.ctx().copy_text(table_to_csv(&spec));
                    self.copied = true;
                }
            });
            // The row colour paints the full table width behind the cells, so
            // the header reads as one bar instead of per-cell chips; data rows
            // alternate from the first one.
            egui::Grid::new(grid_id)
                .with_row_color(move |row, _style| row_fill(dark, row))
                .show(ui, |ui| {
                    for i in 0..ncols {
                        let cell = spec.header.get(i).map_or("", String::as_str);
                        // One block per cell, row-major; the row top is enough
                        // to jump to it. Cells routed to the library are not
                        // searchable, so they are not recorded.
                        let (ranges, current, block) = if cell_needs_library_fallback(cell) {
                            (Vec::new(), None, None)
                        } else {
                            let block = search.record(ui, crate::justify::search_text(cell));
                            let (ranges, current) = search.highlights(block);
                            (ranges, current, Some(block))
                        };
                        let selection = block.and_then(|b| search.selection_range(b));
                        let painted = render_cell(
                            ui,
                            cell_width(&widths, i),
                            cell,
                            &mut self.cell_cache,
                            &ranges,
                            current,
                            selection,
                        );
                        if let Some(block) = block {
                            search.set_height(block, painted.height);
                            search.set_char_map(block, painted.chars);
                            for rect in painted.interactives {
                                search.add_control(rect);
                            }
                        }
                    }
                    ui.end_row();
                    for row in &spec.rows {
                        for i in 0..ncols {
                            let cell = row.get(i).map_or("", String::as_str);
                            let (ranges, current, block) = if cell_needs_library_fallback(cell) {
                                (Vec::new(), None, None)
                            } else {
                                let block = search.record(ui, crate::justify::search_text(cell));
                                let (ranges, current) = search.highlights(block);
                                (ranges, current, Some(block))
                            };
                            let selection = block.and_then(|b| search.selection_range(b));
                            let painted = render_cell(
                                ui,
                                cell_width(&widths, i),
                                cell,
                                &mut self.cell_cache,
                                &ranges,
                                current,
                                selection,
                            );
                            if let Some(block) = block {
                                search.set_height(block, painted.height);
                                search.set_char_map(block, painted.chars);
                                for rect in painted.interactives {
                                    search.add_control(rect);
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
            ui.add_space(table_pad(body_row_height));
        });
    }

    /// Content-proportional widths for one table, cached by id and layout so
    /// the text measurement runs only when the column width, DPI or zoom
    /// changes.
    fn column_widths(
        &mut self,
        ui: &egui::Ui,
        id: &str,
        spec: &TableSpec,
        ncols: usize,
        available: f32,
        spacing: f32,
    ) -> Vec<f32> {
        let available_key = available.round() as i32;
        let ppp_key = (ui.ctx().pixels_per_point() * 100.0).round() as i32;
        let body = egui::TextStyle::Body.resolve(ui.style());
        let font_key = (body.size * 100.0).round() as i32;
        if let Some((a, p, f, widths)) = self.widths.get(id) {
            if *a == available_key && *p == ppp_key && *f == font_key {
                return widths.clone();
            }
        }
        let target = (available - spacing * (ncols - 1) as f32).max(0.0);
        let max = if ncols == 1 { target } else { 0.6 * target };
        let desired: Vec<f32> = (0..ncols)
            .map(|i| {
                spec.header
                    .get(i)
                    .into_iter()
                    .chain(spec.rows.iter().filter_map(|row| row.get(i)))
                    .map(|md| measure(ui, &visible_text(md), &body))
                    .fold(1.0_f32, f32::max)
                    .min(max)
            })
            .collect();
        let widths = table_column_widths(&desired, available, spacing, 80.0);
        let widths = cap_column_widths(&widths, max);
        self.widths.insert(
            id.to_string(),
            (available_key, ppp_key, font_key, widths.clone()),
        );
        widths
    }
}

/// Vertical breathing room above and below a table: three quarters of a body
/// row reads as balanced without pushing tables apart from their captions.
pub(crate) fn table_pad(row_height: f32) -> f32 {
    row_height * 0.75
}

/// Background fills for a table: the alternating data-row fill and the header
/// fill, taken from the active Catppuccin palette. Both sit on top of the
/// theme's `base`, so they read as a distinct block rather than loose text.
/// The light theme's header uses `crust`: in Latte the surface roles get
/// darker with elevation, so `surface2` (#acb0be) is darker than `surface1`
/// and would lower the body-text contrast to 3.69:1. `crust` (#dce0e8) is
/// lighter and reaches 6.04:1. The dark theme's `surface1` already passes AA
/// at 5.59:1.
fn table_fills(dark: bool) -> (egui::Color32, egui::Color32) {
    let theme = if dark {
        crate::mermaid::Theme::Dark
    } else {
        crate::mermaid::Theme::Light
    };
    let p = crate::theme::palette(theme);
    let header = if dark { p.surface1 } else { p.crust };
    (p.surface0.into(), header.into())
}

/// Background fill for `row_index`: the header row, then `surface0` for every
/// data row, so the table reads as one block distinct from `base`.
fn row_fill(dark: bool, row_index: usize) -> Option<egui::Color32> {
    let (body, header) = table_fills(dark);
    if row_index == 0 {
        Some(header)
    } else {
        Some(body)
    }
}

/// CSV for one table: the header row, then every data row, `\n`-separated.
/// Cells are rendered Markdown reduced to plain text, then RFC-4180 quoted.
pub(crate) fn table_to_csv(spec: &TableSpec) -> String {
    let ncols = spec
        .header
        .len()
        .max(spec.rows.iter().map(Vec::len).max().unwrap_or(0));
    if ncols == 0 {
        return String::new();
    }
    let mut out = String::new();
    push_csv_row(&mut out, &spec.header, ncols, true);
    for row in &spec.rows {
        push_csv_row(&mut out, row, ncols, false);
    }
    out
}

/// Append one CSV row, padding short rows with empty cells. `first` marks the
/// header row so an all-empty header still emits its leading blank line.
fn push_csv_row(out: &mut String, cells: &[String], ncols: usize, first: bool) {
    if !first {
        out.push('\n');
    }
    for i in 0..ncols {
        if i > 0 {
            out.push(',');
        }
        let cell = cells.get(i).map_or("", String::as_str);
        out.push_str(&csv_field(&plain_text(cell)));
    }
}

/// Visible text of a Markdown cell: inline markup dropped, code and link
/// labels kept, image syntax reduced to its alt text, raw HTML tags removed.
fn plain_text(md: &str) -> String {
    let mut out = String::new();
    let mut prev_was_text_with_bang = false;
    for run in crate::justify::parse_inline(md) {
        let is_link = matches!(run.kind, crate::justify::RunKind::Link(_));
        let next_prev_bang =
            matches!(run.kind, crate::justify::RunKind::Text) && run.text.ends_with('!');
        let mut text = run.text;
        if !matches!(run.kind, crate::justify::RunKind::Code) {
            text = crate::justify::strip_html(&text);
        }
        // `parse_inline` has no image rule: `![alt](u)` arrives as a literal
        // `!` text run followed by a link run. Dropping that `!` leaves the alt
        // text (empty alt included); a bang at the end of a link label is not
        // touched.
        if is_link && prev_was_text_with_bang && out.ends_with('!') {
            out.pop();
        }
        out.push_str(&text);
        prev_was_text_with_bang = next_prev_bang;
    }
    out
}

/// Quote a CSV field when it contains a comma, quote or line break.
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Visible text of a cell, with Markdown markup removed before measuring:
/// emphasis markers, backticks, HTML tags, and link/image targets. The raw
/// cell text is still what gets rendered.
fn visible_text(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut i = 0;
    while i < md.len() {
        let rest = &md[i..];
        let c = rest.chars().next().unwrap();
        match c {
            '\\' => {
                if let Some(next) = rest[1..].chars().next() {
                    out.push(next);
                    i += 1 + next.len_utf8();
                } else {
                    i += 1;
                }
            }
            '`' | '*' | '_' => i += 1,
            '<' => {
                if let Some(end) = rest.find('>') {
                    i += end + 1;
                } else {
                    out.push('<');
                    i += 1;
                }
            }
            '!' if rest.starts_with("![") => {
                if let Some((label, len)) = bracket(&rest[1..]) {
                    out.push_str(&visible_text(label));
                    i += 1 + len;
                } else {
                    out.push('!');
                    i += 1;
                }
            }
            '[' => {
                if let Some((label, len)) = bracket(rest) {
                    out.push_str(&visible_text(label));
                    i += len;
                } else {
                    out.push('[');
                    i += 1;
                }
            }
            _ => {
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

/// `[label](target)` at the start of `s`: the label and bytes consumed.
fn bracket(s: &str) -> Option<(&str, usize)> {
    let mid = s[1..].find("](")? + 1;
    let end = link_end(&s[mid + 2..])?;
    Some((&s[1..mid], mid + 2 + end))
}

/// Byte index just past the closing `)` of a link target, balancing nested
/// parentheses and ignoring those inside a quoted title.
fn link_end(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_quote = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quote = !in_quote,
            '(' if !in_quote => depth += 1,
            ')' if !in_quote => {
                if depth == 0 {
                    return Some(i + 1);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Single-line width of a cell's visible text. The measurement approximates
/// what the painter will lay out; it only steers the column proportions.
fn measure(ui: &egui::Ui, text: &str, font: &egui::FontId) -> f32 {
    ui.fonts_mut(|f| {
        f.layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
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
fn render_cell(
    ui: &mut egui::Ui,
    w: f32,
    cell_md: &str,
    cache: &mut CommonMarkCache,
    highlights: &[(usize, usize)],
    current: Option<usize>,
    selection: Option<(usize, usize)>,
) -> crate::justify::PaintedBlock {
    // A vertical scope reports the real content height back to the grid; a
    // zero-height `allocate_ui_with_layout` would collapse every row to 0 and
    // let wrapped text paint over the next row.
    ui.vertical(|ui| {
        ui.set_width(w);
        if cell_needs_library_fallback(cell_md) {
            CommonMarkViewer::new().show(ui, cache, cell_md);
            return crate::justify::PaintedBlock {
                height: ui.min_rect().height(),
                chars: Vec::new(),
                interactives: Vec::new(),
            };
        }
        let runs = crate::justify::parse_inline(cell_md);
        if runs.is_empty() {
            // Keep an all-blank row from collapsing to zero height.
            ui.add_space(ui.spacing().interact_size.y);
            return crate::justify::PaintedBlock {
                height: ui.min_rect().height(),
                chars: Vec::new(),
                interactives: Vec::new(),
            };
        }
        crate::justify::draw_left(
            ui,
            &runs,
            w,
            crate::justify::MatchHighlight {
                ranges: highlights,
                current,
                selection,
            },
        )
    })
    .inner
}

#[cfg(test)]
mod tests {
    mod table {
        use super::super::cell_needs_library_fallback;
        use super::super::row_fill;
        use super::super::table_fills;
        use super::super::table_pad;
        use super::super::table_to_csv;
        use super::super::visible_text;
        use crate::doc::TableSpec;

        fn spec(header: &[&str], rows: &[&[&str]]) -> TableSpec {
            TableSpec {
                header: header.iter().map(|s| s.to_string()).collect(),
                rows: rows
                    .iter()
                    .map(|row| row.iter().map(|s| s.to_string()).collect())
                    .collect(),
            }
        }

        #[test]
        fn csv_quotes_commas_quotes_and_newlines() {
            let s = spec(&["a,b", "q\"x", "n\nl"], &[]);
            assert_eq!(table_to_csv(&s), "\"a,b\",\"q\"\"x\",\"n\nl\"");
        }

        #[test]
        fn csv_includes_header_then_rows() {
            let s = spec(&["h1", "h2"], &[&["1", "2"], &["3", "4"]]);
            assert_eq!(table_to_csv(&s), "h1,h2\n1,2\n3,4");
        }

        #[test]
        fn csv_strips_markdown_and_keeps_code_and_link_text() {
            let s = spec(&["**b**", "`c`"], &[&["[t](https://x/y)", "plain"]]);
            assert_eq!(table_to_csv(&s), "b,c\nt,plain");
        }

        #[test]
        fn csv_pads_empty_cells_and_short_rows() {
            let s = spec(&["a", ""], &[&[""], &["x"]]);
            assert_eq!(table_to_csv(&s), "a,\n,\nx,");
        }

        #[test]
        fn csv_strips_images_and_html() {
            let s = spec(&["![alt](u.png)", "![](u.png)"], &[&["<b>x</b>", "a<br>b"]]);
            assert_eq!(table_to_csv(&s), "alt,\nx,ab");
        }

        #[test]
        fn csv_keeps_bang_at_end_of_a_link_label() {
            let s = spec(&["[a!](u)[b](v)"], &[]);
            assert_eq!(table_to_csv(&s), "a!b");
        }

        #[test]
        fn csv_keeps_non_tag_angle_brackets() {
            let s = spec(&["1 < 2"], &[]);
            assert_eq!(table_to_csv(&s), "1 < 2");
        }

        #[test]
        fn csv_all_empty_header_still_emits_the_header_row() {
            let s = spec(&[""], &[&["x"], &["y"]]);
            assert_eq!(table_to_csv(&s), "\nx\ny");
        }

        #[test]
        fn csv_single_column_has_no_commas() {
            let s = spec(&["h"], &[&["1"], &["2"]]);
            assert_eq!(table_to_csv(&s), "h\n1\n2");
        }

        #[test]
        fn visible_text_drops_markup_for_measurement() {
            let md = "**Bold** and `code`, [link text](https://x/y), ![alt text](i.png) <b>x</b>";
            assert_eq!(visible_text(md), "Bold and code, link text, alt text x");
        }

        #[test]
        fn table_fills_use_surface0_rows_and_theme_header() {
            let hex = |c: egui::Color32| crate::theme::Rgb(c.r(), c.g(), c.b()).to_hex();
            let (row, header) = table_fills(false);
            assert_eq!(hex(row), "#ccd0da");
            assert_eq!(hex(header), "#dce0e8");
            let (row, header) = table_fills(true);
            assert_eq!(hex(row), "#363a4f");
            assert_eq!(hex(header), "#494d64");
        }

        #[test]
        fn row_fill_maps_header_then_surface0_for_every_data_row() {
            let hex = |c: egui::Color32| crate::theme::Rgb(c.r(), c.g(), c.b()).to_hex();
            let l = |s: &str| Some(s.to_string());
            let light: Vec<_> = (0..=3).map(|r| row_fill(false, r).map(hex)).collect();
            assert_eq!(
                light,
                [l("#dce0e8"), l("#ccd0da"), l("#ccd0da"), l("#ccd0da")]
            );
            let dark: Vec<_> = (0..=3).map(|r| row_fill(true, r).map(hex)).collect();
            assert_eq!(
                dark,
                [l("#494d64"), l("#363a4f"), l("#363a4f"), l("#363a4f")]
            );
        }

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
