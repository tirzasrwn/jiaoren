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
    pub(crate) fn toggle_open(&mut self, toc: &[TocEntry]) {
        self.open = !self.open;
        if self.open {
            self.last_g = None;
            if !self.initialized {
                self.initialized = true;
                self.selected = 0;
                // Minimise every parent so the first open is a top-level outline.
                self.collapsed = (0..toc.len())
                    .filter(|&i| toc[i].level <= MAX_DEPTH && self.has_children(toc, i))
                    .collect();
            } else {
                // Reopening keeps the last selection and folds; reveal it if a
                // collapsed ancestor hides it.
                self.reveal_selected(toc);
            }
            self.scroll_to_selected = true;
        }
    }

    /// Drop index state when a different document is loaded.
    pub(crate) fn reset(&mut self) {
        *self = TocState::default();
    }

    fn reveal_selected(&mut self, toc: &[TocEntry]) {
        let mut level = toc.get(self.selected).map_or(MAX_DEPTH + 1, |e| e.level);
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
            return;
        }
        if self.has_children(toc, self.selected) {
            self.collapsed.insert(self.selected);
        }
    }

    fn has_children(&self, toc: &[TocEntry], index: usize) -> bool {
        let level = toc[index].level;
        toc.iter()
            .skip(index + 1)
            .take_while(|entry| entry.level > level)
            .any(|entry| entry.level <= MAX_DEPTH)
    }

    /// The heading the selection points at.
    pub(crate) fn selected_entry<'a>(&self, toc: &'a [TocEntry]) -> Option<&'a TocEntry> {
        toc.get(self.selected)
    }
}

/// Paint the overlay. Keyboard-only; the list scrolls to the selected row.
pub(crate) fn draw(ctx: &egui::Context, toc: &[TocEntry], state: &mut TocState, theme: Theme) {
    let screen = ctx.content_rect();
    let palette = theme::palette(theme);
    let base = egui::Color32::from(palette.base);
    egui::Area::new(egui::Id::new("jiaoren-toc"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.left_top())
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
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(screen.width() - 28.0);
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
                            for &index in &visible {
                                let entry = &toc[index];
                                let marker = if state.has_children(toc, index) {
                                    if state.collapsed.contains(&index) {
                                        "▸"
                                    } else {
                                        "▾"
                                    }
                                } else {
                                    " "
                                };
                                let indent = "  ".repeat((entry.level - 1) as usize);
                                let plain = crate::justify::search_text(&entry.text);
                                let text = format!("{indent}{marker} {plain}");
                                let response = ui.selectable_label(
                                    index == state.selected,
                                    egui::RichText::new(text).monospace(),
                                );
                                // No keyboard focus: the index is painted, and a
                                // focus ring on the first row looks like a glitch.
                                response.surrender_focus();
                                if index == state.selected && state.scroll_to_selected {
                                    response.scroll_to_me(Some(egui::Align::Center));
                                    state.scroll_to_selected = false;
                                }
                            }
                            ui.add_space(8.0);
                        });
                });
        });
    // Same plain bottom-left chip as the search command line.
    egui::Area::new(egui::Id::new("jiaoren-toc-help"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(8.0, -8.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, |ui| {
                    ui.add(egui::Label::new(
                        egui::RichText::new(
                            "tab/esc close · j/k · h/l · enter jump · gg/G · t theme",
                        )
                        .monospace()
                        .color(ui.visuals().text_color()),
                    ));
                });
        });
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
        fn opening_minimises_every_parent() {
            let toc = doc();
            let mut state = TocState::default();
            state.toggle_open(&toc);
            assert!(state.open);
            assert_eq!(state.visible(&toc), vec![0, 5]);
        }

        #[test]
        fn reopening_keeps_selection_and_folds() {
            let toc = doc();
            let mut state = TocState::default();
            state.toggle_open(&toc);
            state.selected = 1; // One A is hidden by the initial minimise
            state.fold(&toc, true); // expand One
            state.selected = 2; // One A I
            state.toggle_open(&toc); // close
            state.toggle_open(&toc); // reopen
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
    }
}
