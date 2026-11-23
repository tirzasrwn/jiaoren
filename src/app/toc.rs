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
}

impl TocState {
    pub(crate) fn toggle_open(&mut self) {
        self.open = !self.open;
        if self.open {
            self.selected = 0;
            self.collapsed.clear();
        }
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
pub(crate) fn draw(ctx: &egui::Context, toc: &[TocEntry], state: &TocState, theme: Theme) {
    let screen = ctx.content_rect();
    let palette = theme::palette(theme);
    let base =
        egui::Color32::from_rgba_unmultiplied(palette.base.0, palette.base.1, palette.base.2, 246);
    egui::Area::new(egui::Id::new("jiaoren-toc"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.left_top())
        .interactable(false)
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
                    let hint = egui::RichText::new(
                        "Tab/Esc close · j/k move · h/l fold · Enter/Space jump",
                    )
                    .monospace()
                    .color(ui.visuals().weak_text_color());
                    ui.label(hint);
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .max_height(screen.height() - 70.0)
                        .show(ui, |ui| {
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
                                let text = format!("{indent}{marker} {}", entry.text);
                                let response = ui.selectable_label(
                                    index == state.selected,
                                    egui::RichText::new(text).monospace(),
                                );
                                if index == state.selected {
                                    response.scroll_to_me(Some(egui::Align::Center));
                                }
                            }
                        });
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
                selected: 0,
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
