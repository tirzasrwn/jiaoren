//! The zathura-style index overlay: headings up to depth 3, foldable with
//! `h`/`l`, navigated with `j`/`k`, jumped to with Enter/Space.

use std::collections::HashSet;

use crate::doc::TocEntry;
use crate::mermaid::Theme;
use crate::theme;

/// Maximum heading depth shown in the index.
const MAX_DEPTH: u8 = 3;

/// Headings shown while collapsed: the index itself.
#[derive(Default)]
pub(crate) struct TocState {
    pub(crate) open: bool,
    /// Index into the full TOC vector.
    selected: usize,
    collapsed: HashSet<usize>,
    /// First `g` of a `gg` press in the index.
    last_g: Option<std::time::Instant>,
    /// Whether the overlay has been opened before; later opens keep the state.
    initialized: bool,
    /// Ask the next paint to reveal the selected row.
    scroll_to_selected: bool,
}

impl TocState {
    /// Open or close the index. `ys` holds the content y of each TOC entry's
    /// heading block, parallel to `toc`; `anchor` is the viewport's vertical
    /// middle in content coordinates. Every open selects the section being
    /// read, not the last one.
    pub(crate) fn toggle_open(&mut self, toc: &[TocEntry], ys: &[Option<f32>], anchor: f32) {
        self.open = !self.open;
        if self.open {
            self.last_g = None;
            if !self.initialized {
                self.initialized = true;
                // Level-one sections start open; deeper parents start collapsed
                // so the first view is the H1/H2 outline.
                self.collapsed = (0..toc.len())
                    .filter(|&i| {
                        toc[i].level > 1 && toc[i].level <= MAX_DEPTH && self.has_children(toc, i)
                    })
                    .collect();
            }
            self.select_current(toc, ys, anchor);
            // Keep the choice on screen: unfold only the ancestors that hide it.
            self.reveal_selected(toc);
            self.clamp_selected(toc);
            self.scroll_to_selected = true;
        }
    }

    /// Select the nearest visible heading at or above `anchor`; before every
    /// heading, the first visible one.
    fn select_current(&mut self, toc: &[TocEntry], ys: &[Option<f32>], anchor: f32) {
        let current = toc
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.level <= MAX_DEPTH)
            .filter_map(|(index, _)| {
                let y = ys.get(index).copied().flatten()?;
                (y <= anchor).then_some((index, y))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        self.selected = current
            .map(|(index, _)| index)
            .or_else(|| toc.iter().position(|entry| entry.level <= MAX_DEPTH))
            .unwrap_or(self.selected);
    }

    /// Drop index state when a different document is loaded.
    pub(crate) fn reset(&mut self) {
        *self = TocState::default();
    }

    /// Keep folds and the selected heading across a reload by heading id, since
    /// edits elsewhere in the document can shift indices. Removed headings drop
    /// their fold, and a removed selection falls back to the first row.
    pub(crate) fn remap(&mut self, old: &[TocEntry], new: &[TocEntry]) {
        let selected = old.get(self.selected).map(|entry| entry.id.as_str());
        let collapsed: HashSet<&str> = self
            .collapsed
            .iter()
            .filter_map(|&index| old.get(index))
            .map(|entry| entry.id.as_str())
            .collect();
        self.collapsed = new
            .iter()
            .enumerate()
            .filter(|(_, entry)| collapsed.contains(entry.id.as_str()))
            .map(|(index, _)| index)
            .collect();
        self.selected = selected
            .and_then(|id| new.iter().position(|entry| entry.id == id))
            .unwrap_or(0);
        self.clamp_selected(new);
    }

    fn reveal_selected(&mut self, toc: &[TocEntry]) {
        let Some(entry) = toc.get(self.selected) else {
            self.selected = 0;
            return;
        };
        let mut level = entry.level;
        let mut j = self.selected;
        while j > 0 {
            j -= 1;
            if toc[j].level < level {
                self.collapsed.remove(&j);
                level = toc[j].level;
            }
        }
    }

    /// `gg`: first visible heading.
    pub(crate) fn select_first(&mut self, toc: &[TocEntry]) {
        if let Some(&first) = self.visible(toc).first() {
            self.selected = first;
            self.scroll_to_selected = true;
        }
    }

    /// `G`: last visible heading.
    pub(crate) fn select_last(&mut self, toc: &[TocEntry]) {
        if let Some(&last) = self.visible(toc).last() {
            self.selected = last;
            self.scroll_to_selected = true;
        }
    }

    /// Track a `g` press; returns true when the second one completes `gg`.
    pub(crate) fn gg(&mut self, now: std::time::Instant) -> bool {
        let complete = self
            .last_g
            .is_some_and(|at| now.duration_since(at) < std::time::Duration::from_millis(500));
        self.last_g = (!complete).then_some(now);
        complete
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    /// TOC indices currently on screen, in document order.
    pub(crate) fn visible(&self, toc: &[TocEntry]) -> Vec<usize> {
        (0..toc.len())
            .filter(|&i| toc[i].level <= MAX_DEPTH && !self.hidden(toc, i))
            .collect()
    }

    fn hidden(&self, toc: &[TocEntry], index: usize) -> bool {
        let mut level = toc[index].level;
        let mut j = index;
        while j > 0 {
            j -= 1;
            if toc[j].level < level {
                if self.collapsed.contains(&j) {
                    return true;
                }
                level = toc[j].level;
            }
        }
        false
    }

    /// Move the selection by `delta` rows within the visible list.
    pub(crate) fn move_by(&mut self, toc: &[TocEntry], delta: isize) {
        let visible = self.visible(toc);
        if visible.is_empty() {
            return;
        }
        let pos = visible
            .iter()
            .position(|&i| i == self.selected)
            .unwrap_or(0);
        let next = (pos as isize + delta).clamp(0, visible.len() as isize - 1);
        self.selected = visible[next as usize];
        self.scroll_to_selected = true;
    }

    /// `h` collapses, `l` expands the selected heading.
    pub(crate) fn fold(&mut self, toc: &[TocEntry], expand: bool) {
        if expand {
            self.collapsed.remove(&self.selected);
        } else if self.has_children(toc, self.selected) {
            self.collapsed.insert(self.selected);
        }
        self.clamp_selected(toc);
    }

    /// Keep the selection on a row that is actually visible: a stale index (or
    /// a selection hidden by a fold) moves to the closest visible row above,
    /// else the first one. An entirely empty list keeps the selection as-is;
    /// Enter is a no-op there.
    fn clamp_selected(&mut self, toc: &[TocEntry]) {
        let visible = self.visible(toc);
        if visible.is_empty() || visible.contains(&self.selected) {
            return;
        }
        let target = visible
            .iter()
            .rev()
            .find(|&&index| index < self.selected)
            .or_else(|| visible.first())
            .copied()
            .unwrap_or(self.selected);
        self.selected = target;
    }

    /// Whether the selected heading has children to expand.
    pub(crate) fn selected_has_children(&self, toc: &[TocEntry]) -> bool {
        self.has_children(toc, self.selected)
    }

    fn has_children(&self, toc: &[TocEntry], index: usize) -> bool {
        // An empty TOC (or a stale selection) must not index out of bounds:
        // `h`/`l` in the index overlay would otherwise panic.
        let Some(entry) = toc.get(index) else {
            return false;
        };
        let level = entry.level;
        toc.iter()
            .skip(index + 1)
            .take_while(|entry| entry.level > level)
            .any(|entry| entry.level <= MAX_DEPTH)
    }

    /// The heading the selection points at, if it is actually on screen. A
    /// selection hidden by a fold or a heading deeper than [`MAX_DEPTH`] is not
    /// jumpable, so Enter on an empty list is a no-op.
    pub(crate) fn selected_entry<'a>(&self, toc: &'a [TocEntry]) -> Option<&'a TocEntry> {
        let entry = toc.get(self.selected)?;
        self.visible(toc).contains(&self.selected).then_some(entry)
    }
}

/// Left edge of the disclosure triangle for a row.
fn rect_marker_x(left: f32, indent_chars: f32, char_w: f32) -> f32 {
    left + 14.0 + indent_chars * char_w
}

/// Paint the overlay. Returns the TOC index the user clicked to jump to, if any.
pub(crate) fn draw(
    ctx: &egui::Context,
    toc: &[TocEntry],
    state: &mut TocState,
    theme: Theme,
) -> Option<usize> {
    let screen = ctx.content_rect();
    let palette = theme::palette(theme);
    let base = egui::Color32::from(palette.base);
    let mut jump = None;
    egui::Area::new(egui::Id::new("jiaoren-toc"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.left_top())
        .fade_in(false)
        .interactable(true)
        .show(ctx, |ui| {
            // Area sizing follows content; paint the whole window first so no
            // document glyphs show through the index.
            ui.set_min_size(screen.size());
            ui.painter().rect_filled(screen, 0.0, base);
            egui::Frame::new()
                .fill(base)
                .stroke(egui::Stroke::new(
                    1.0,
                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                ))
                // No side margin: the selection bar spans the full window.
                .inner_margin(egui::Margin {
                    left: 0,
                    right: 0,
                    top: 10,
                    bottom: 10,
                })
                .show(ui, |ui| {
                    ui.set_width(screen.width());
                    ui.set_height(screen.height() - 20.0);
                    let visible = state.visible(toc);
                    egui::ScrollArea::vertical()
                        // Wheel-only list: the bar itself is visual noise and
                        // its thumb clips against the window edge.
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .auto_shrink([false, false])
                        .max_height(screen.height() - 74.0)
                        .show(ui, |ui| {
                            ui.add_space(8.0);
                            if visible.is_empty() {
                                ui.add(egui::Label::new(
                                    egui::RichText::new("no headings (depth 3)")
                                        .monospace()
                                        .size(14.0)
                                        .color(ui.visuals().weak_text_color()),
                                ));
                            }
                            for &index in &visible {
                                let entry = &toc[index];
                                let has_children = state.has_children(toc, index);
                                let collapsed = state.collapsed.contains(&index);
                                let marker = if has_children {
                                    if collapsed {
                                        "▸"
                                    } else {
                                        "▾"
                                    }
                                } else {
                                    " "
                                };
                                let indent = "  ".repeat((entry.level - 1) as usize);
                                let indent_chars = indent.chars().count() as f32;
                                let plain = crate::justify::search_text(&entry.text);
                                let text = format!("{indent}{marker} {plain}");
                                let text_chars = text.chars().count() as f32;
                                // Painted row: square selection box, no outline.
                                // Fixed size: document zoom must not inflate
                                // the index.
                                let font = egui::FontId::monospace(14.0);
                                let color = if index == state.selected {
                                    ui.visuals().selection.stroke.color
                                } else {
                                    ui.visuals().text_color()
                                };
                                let galley = ui.painter().layout_no_wrap(text, font, color);
                                let char_w = galley.size().x / text_chars.max(1.0);
                                // Full-width row: the selection bar spans the
                                // window, zathura style.
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), galley.size().y + 2.0),
                                    egui::Sense::click(),
                                );
                                let marker_x = rect_marker_x(rect.left(), indent_chars, char_w);
                                let response =
                                    response.on_hover_cursor(egui::CursorIcon::PointingHand);
                                if index == state.selected {
                                    ui.painter().rect_filled(
                                        rect,
                                        0.0,
                                        ui.visuals().selection.bg_fill,
                                    );
                                }
                                if response.clicked() {
                                    state.selected = index;
                                    state.scroll_to_selected = false;
                                    let on_marker =
                                        ui.ctx().pointer_interact_pos().is_some_and(|p| {
                                            p.x >= marker_x - 4.0 && p.x <= marker_x + char_w + 6.0
                                        });
                                    if has_children && on_marker {
                                        // Triangle: fold without leaving the index.
                                        state.fold(toc, collapsed);
                                    } else {
                                        // Row: reveal a collapsed section and jump.
                                        if has_children && collapsed {
                                            state.fold(toc, true);
                                        }
                                        jump = Some(index);
                                    }
                                    ui.ctx().request_repaint();
                                }
                                let text_pos = egui::pos2(
                                    rect.left() + 14.0,
                                    rect.center().y - galley.size().y / 2.0,
                                );
                                ui.painter().galley(text_pos, galley, color);
                                if index == state.selected && state.scroll_to_selected {
                                    response.scroll_to_me(Some(egui::Align::Center));
                                    state.scroll_to_selected = false;
                                }
                            }
                            ui.add_space(8.0);
                        });
                });
        });
    jump
}

#[cfg(test)]
mod tests {
    mod toc {
        use super::super::{TocState, MAX_DEPTH};
        use crate::doc::TocEntry;

        fn entry(level: u8, text: &str) -> TocEntry {
            TocEntry {
                level,
                text: text.to_string(),
                id: text.to_string(),
            }
        }

        fn doc() -> Vec<TocEntry> {
            vec![
                entry(1, "one"),
                entry(2, "one-a"),
                entry(3, "one-a-i"),
                entry(4, "too-deep"),
                entry(2, "one-b"),
                entry(1, "two"),
            ]
        }

        fn ys(values: &[f32]) -> Vec<Option<f32>> {
            values.iter().map(|&y| Some(y)).collect()
        }

        #[test]
        fn opening_selects_heading_at_viewport_top() {
            let toc = doc();
            let mut state = TocState::default();
            state.toggle_open(&toc, &ys(&[0.0, 100.0, 200.0, 250.0, 300.0, 400.0]), 200.0);
            assert_eq!(state.selected, 2);
        }

        #[test]
        fn opening_selects_heading_above_mid_viewport() {
            let toc = doc();
            let mut state = TocState::default();
            state.toggle_open(&toc, &ys(&[0.0, 100.0, 200.0, 250.0, 300.0, 400.0]), 250.0);
            assert_eq!(state.selected, 2);
        }

        #[test]
        fn opening_selects_heading_at_bottom() {
            let toc = doc();
            let mut state = TocState::default();
            state.toggle_open(&toc, &ys(&[0.0, 100.0, 200.0, 250.0, 300.0, 400.0]), 900.0);
            assert_eq!(state.selected, 5);
        }

        #[test]
        fn opening_without_a_heading_above_top_selects_first_visible() {
            let toc = doc();
            let mut state = TocState::default();
            state.toggle_open(&toc, &ys(&[10.0, 100.0, 200.0, 250.0, 300.0, 400.0]), 5.0);
            assert_eq!(state.selected, 0);
        }

        #[test]
        fn opening_unfolds_the_chosen_headings_ancestors() {
            let toc = vec![
                entry(1, "a"),
                entry(2, "a-1"),
                entry(3, "a-1-i"),
                entry(1, "b"),
            ];
            let mut state = TocState {
                initialized: true,
                collapsed: [1].into_iter().collect(),
                ..TocState::default()
            };
            state.toggle_open(&toc, &ys(&[0.0, 10.0, 20.0, 30.0]), 20.0);
            assert_eq!(state.selected, 2);
            assert!(!state.collapsed.contains(&1));
            assert!(state.visible(&toc).contains(&2));
        }

        #[test]
        fn opening_preserves_unrelated_folds() {
            let toc = vec![
                entry(1, "a"),
                entry(2, "a-1"),
                entry(3, "a-1-i"),
                entry(1, "b"),
                entry(2, "b-1"),
                entry(3, "b-1-i"),
            ];
            let mut state = TocState {
                initialized: true,
                collapsed: [1, 4].into_iter().collect(),
                ..TocState::default()
            };
            state.toggle_open(&toc, &ys(&[0.0, 10.0, 20.0, 30.0, 40.0, 50.0]), 20.0);
            assert_eq!(state.selected, 2);
            assert!(!state.collapsed.contains(&1));
            assert!(state.collapsed.contains(&4));
            assert!(!state.visible(&toc).contains(&5));
        }

        #[test]
        fn empty_toc_stays_empty_when_opened() {
            let mut state = TocState::default();
            state.toggle_open(&[], &[], 0.0);
            assert!(state.visible(&[]).is_empty());
            assert!(state.selected_entry(&[]).is_none());
        }

        #[test]
        fn headings_deeper_than_three_are_ignored_for_the_current_section() {
            let toc = vec![entry(1, "a"), entry(4, "deep")];
            let mut state = TocState::default();
            state.toggle_open(&toc, &ys(&[0.0, 100.0]), 150.0);
            assert_eq!(state.selected, 0);
        }

        #[test]
        fn only_depth_three_is_visible() {
            let state = TocState::default();
            assert_eq!(state.visible(&doc()), vec![0, 1, 2, 4, 5]);
            assert_eq!(MAX_DEPTH, 3);
        }

        #[test]
        fn collapse_hides_descendants_and_expand_restores_them() {
            let mut state = TocState {
                open: true,
                ..TocState::default()
            };
            let toc = doc();
            state.fold(&toc, false);
            assert_eq!(state.visible(&toc), vec![0, 5]);
            state.fold(&toc, true);
            assert_eq!(state.visible(&toc), vec![0, 1, 2, 4, 5]);
            // A leaf ignores collapse.
            state.selected = 5;
            state.fold(&toc, false);
            assert_eq!(state.visible(&toc), vec![0, 1, 2, 4, 5]);
        }

        #[test]
        fn opening_expands_level_one_and_collapses_deeper_parents() {
            let toc = doc();
            let mut state = TocState::default();
            state.toggle_open(&toc, &[], 0.0);
            assert!(state.open);
            assert_eq!(state.visible(&toc), vec![0, 1, 4, 5]);
        }

        #[test]
        fn reopening_selects_the_current_section_and_keeps_folds() {
            let toc = doc();
            let positions = ys(&[0.0, 100.0, 200.0, 250.0, 300.0, 400.0]);
            let mut state = TocState::default();
            state.toggle_open(&toc, &positions, 200.0);
            state.fold(&toc, true); // expand One A
            state.toggle_open(&toc, &positions, 200.0); // close
            state.toggle_open(&toc, &positions, 200.0); // reopen
            assert_eq!(state.selected, 2);
            assert!(state.visible(&toc).contains(&2));
        }

        #[test]
        fn gg_and_g_select_the_first_and_last_rows() {
            let toc = doc();
            let mut state = TocState::default();
            state.select_last(&toc);
            assert_eq!(state.selected, 5);
            state.select_first(&toc);
            assert_eq!(state.selected, 0);
            let now = std::time::Instant::now();
            assert!(!state.gg(now));
            assert!(state.gg(now + std::time::Duration::from_millis(200)));
        }

        #[test]
        fn selection_moves_within_visible_rows_and_stops_at_edges() {
            let mut state = TocState::default();
            let toc = doc();
            state.move_by(&toc, 1);
            assert_eq!(state.selected, 1);
            state.move_by(&toc, 1);
            assert_eq!(state.selected, 2);
            state.move_by(&toc, -5);
            assert_eq!(state.selected, 0);
            state.move_by(&toc, 99);
            assert_eq!(state.selected, 5);
        }

        #[test]
        fn enter_has_no_target_when_no_heading_is_visible() {
            let toc = vec![entry(4, "too deep for the index")];
            let mut state = TocState::default();
            state.toggle_open(&toc, &[], 0.0);
            assert!(state.visible(&toc).is_empty());
            // `selected_entry` is what Enter jumps with: no visible row, no jump.
            assert!(state.selected_entry(&toc).is_none());
            assert!(state.selected_entry(&[]).is_none());
        }

        #[test]
        fn empty_index_fold_is_a_noop_instead_of_a_panic() {
            let mut state = TocState::default();
            state.toggle_open(&[], &[], 0.0);
            state.fold(&[], false);
            state.fold(&[], true);
            assert!(state.selected_entry(&[]).is_none());
        }

        #[test]
        fn remap_follows_heading_ids_across_shifted_indices() {
            let old = doc();
            let mut state = TocState {
                open: true,
                initialized: true,
                ..TocState::default()
            };
            state.collapsed.insert(1); // one-a
            state.selected = 5; // two
            let new = vec![
                entry(1, "zero"),
                entry(1, "one"),
                entry(2, "one-a"),
                entry(3, "one-a-i"),
                entry(4, "too-deep"),
                entry(2, "one-b"),
                entry(1, "two"),
            ];
            state.remap(&old, &new);
            assert_eq!(state.selected, 6, "two moved to index 6");
            assert_eq!(state.visible(&new), vec![0, 1, 2, 5, 6]);
            assert!(state.collapsed.contains(&2), "one-a is still collapsed");
            assert!(!state.collapsed.contains(&1));
        }

        #[test]
        fn remap_drops_removed_folds_and_clamps_a_removed_selection() {
            let old = doc();
            let mut state = TocState {
                open: true,
                initialized: true,
                ..TocState::default()
            };
            state.collapsed.insert(0); // one
            state.selected = 2; // one-a-i, removed below
            let new = vec![entry(1, "two"), entry(1, "three")];
            state.remap(&old, &new);
            assert!(state.collapsed.is_empty());
            assert_eq!(state.selected, 0);
        }

        #[test]
        fn folding_clamps_a_hidden_selection_to_a_visible_row() {
            let toc = doc();
            let mut state = TocState::default();
            state.toggle_open(&toc, &[], 0.0);
            assert_eq!(state.visible(&toc), vec![0, 1, 4, 5]);
            state.selected = 3; // hidden under the collapsed One A (index 1)
            state.fold(&toc, true);
            assert_eq!(state.selected, 1);
            assert!(state.selected_entry(&toc).is_some());
        }
    }
}
