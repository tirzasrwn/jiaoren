//! Text selection over the search block registry: anchor/focus points in
//! document order, clipboard text extraction, and pointer hit testing against
//! the char maps painted by the block renderers.

use egui::Pos2;

use crate::justify::CharSpan;

/// One end of a selection: a block index (document order, same registry as
/// search) and a char offset in that block's recorded rendered text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SelPoint {
    pub block: usize,
    pub char: usize,
}

/// Live selection state. `anchor` is where the drag started, `focus` where the
/// pointer last landed; a plain click leaves them equal and is cleared on
/// release.
#[derive(Default)]
pub struct SelectionState {
    pub anchor: Option<SelPoint>,
    pub focus: Option<SelPoint>,
    pub dragging: bool,
}

impl SelectionState {
    pub fn clear(&mut self) {
        self.anchor = None;
        self.focus = None;
        self.dragging = false;
    }

    /// Start a selection at `point`.
    pub fn begin(&mut self, point: SelPoint) {
        self.anchor = Some(point);
        self.focus = Some(point);
        self.dragging = true;
    }

    /// Move the focus end while dragging.
    pub fn extend(&mut self, point: SelPoint) {
        if self.anchor.is_some() {
            self.focus = Some(point);
        } else {
            self.begin(point);
        }
    }

    /// End points ordered from the earlier to the later one.
    pub fn normalized(&self) -> Option<(SelPoint, SelPoint)> {
        let (anchor, focus) = (self.anchor?, self.focus?);
        if anchor <= focus {
            Some((anchor, focus))
        } else {
            Some((focus, anchor))
        }
    }

    /// True when the range covers at least one character.
    pub fn is_active(&self) -> bool {
        self.normalized().is_some_and(|(a, b)| a != b)
    }

    /// Char range selected on `block`, clamped to `len`. Empty ranges are
    /// `None`.
    pub fn range_for(&self, block: usize, len: usize) -> Option<(usize, usize)> {
        let (start, end) = self.normalized()?;
        if block < start.block || block > end.block {
            return None;
        }
        let (from, to) = if start.block == end.block {
            (start.char, end.char)
        } else if block == start.block {
            (start.char, len)
        } else if block == end.block {
            (0, end.char)
        } else {
            (0, len)
        };
        let from = from.min(len);
        let to = to.min(len);
        (from < to).then_some((from, to))
    }
}

/// Substring by chars.
fn slice(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

/// Clipboard text for the selection: one block is a substring; multiple blocks
/// are the anchor tail, whole intermediate blocks and the focus head, joined by
/// newlines. Reversed drags are normalized. Empty edge segments are skipped so
/// a selection ending exactly at a block boundary has no spurious blank line.
pub fn selected_text(blocks: &[String], sel: &SelectionState) -> Option<String> {
    let (start, end) = sel.normalized()?;
    if start == end {
        return Some(String::new());
    }
    let mut parts = Vec::new();
    for (index, text) in blocks.iter().enumerate() {
        if index < start.block || index > end.block {
            continue;
        }
        let len = text.chars().count();
        let from = if index == start.block {
            start.char.min(len)
        } else {
            0
        };
        let to = if index == end.block {
            end.char.min(len)
        } else {
            len
        };
        if from < to {
            parts.push(slice(text, from, to));
        }
    }
    Some(parts.join("\n"))
}

/// Nearest char offset in one block's char map. Distance is measured to the
/// nearest span rect; inside a span the x position is interpolated across its
/// chars.
#[cfg_attr(not(test), allow(dead_code))]
pub fn hit_test(map: &[CharSpan], pos: Pos2) -> Option<usize> {
    hit_test_distance(map, pos).map(|(char, _)| char)
}

/// Nearest offset plus rect distance, for choosing between blocks.
pub fn hit_test_distance(map: &[CharSpan], pos: Pos2) -> Option<(usize, f32)> {
    let mut best: Option<(usize, f32)> = None;
    for span in map {
        if span.len == 0 {
            continue;
        }
        let rect = span.rect;
        let dx = (rect.left() - pos.x).max(pos.x - rect.right()).max(0.0);
        let dy = (rect.top() - pos.y).max(pos.y - rect.bottom()).max(0.0);
        let distance = dx * dx + dy * dy;
        if best.is_some_and(|(_, d)| distance >= d) {
            continue;
        }
        let local = if rect.width() > 0.0 {
            (((pos.x - rect.left()) / rect.width()) * span.len as f32).round()
        } else {
            0.0
        };
        let local = (local.max(0.0) as usize).min(span.len);
        best = Some((span.start + local, distance));
    }
    best
}

#[cfg(test)]
mod tests {
    mod text {
        use super::super::{selected_text, SelPoint, SelectionState};

        fn blocks(texts: &[&str]) -> Vec<String> {
            texts.iter().map(|s| (*s).to_string()).collect()
        }

        fn sel(anchor: (usize, usize), focus: (usize, usize)) -> SelectionState {
            SelectionState {
                anchor: Some(SelPoint {
                    block: anchor.0,
                    char: anchor.1,
                }),
                focus: Some(SelPoint {
                    block: focus.0,
                    char: focus.1,
                }),
                dragging: false,
            }
        }

        #[test]
        fn single_block_substring_and_reversed() {
            let b = blocks(&["hello world"]);
            assert_eq!(
                selected_text(&b, &sel((0, 6), (0, 11))).as_deref(),
                Some("world")
            );
            assert_eq!(
                selected_text(&b, &sel((0, 11), (0, 6))).as_deref(),
                Some("world")
            );
        }

        #[test]
        fn multi_block_tail_middle_head() {
            let b = blocks(&["one two", "middle", "three four"]);
            assert_eq!(
                selected_text(&b, &sel((0, 4), (2, 5))).as_deref(),
                Some("two\nmiddle\nthree")
            );
            assert_eq!(
                selected_text(&b, &sel((2, 5), (0, 4))).as_deref(),
                Some("two\nmiddle\nthree")
            );
        }

        #[test]
        fn block_boundary_has_no_blank_line() {
            let b = blocks(&["one", "two"]);
            assert_eq!(
                selected_text(&b, &sel((0, 3), (1, 2))).as_deref(),
                Some("tw")
            );
            assert_eq!(selected_text(&b, &sel((0, 3), (1, 0))).as_deref(), Some(""));
        }

        #[test]
        fn empty_and_equal_points() {
            let b = blocks(&["abc"]);
            let mut s = SelectionState::default();
            assert_eq!(selected_text(&b, &s), None);
            s.begin(SelPoint { block: 0, char: 1 });
            assert_eq!(selected_text(&b, &s).as_deref(), Some(""));
            assert!(!s.is_active());
        }

        #[test]
        fn ranges_clamp_to_block_length() {
            let b = blocks(&["abc"]);
            assert_eq!(
                selected_text(&b, &sel((0, 1), (0, 99))).as_deref(),
                Some("bc")
            );
            let s = sel((0, 1), (0, 2));
            assert_eq!(s.range_for(0, 3), Some((1, 2)));
            assert_eq!(s.range_for(0, 1), None);
            assert_eq!(s.range_for(1, 3), None);
        }
    }

    mod hit {
        use super::super::hit_test;
        use crate::justify::CharSpan;
        use egui::{Pos2, Rect};

        fn span(start: usize, len: usize, x: f32, y: f32, w: f32, h: f32) -> CharSpan {
            CharSpan {
                start,
                len,
                rect: Rect::from_min_size(Pos2::new(x, y), egui::vec2(w, h)),
            }
        }

        fn map() -> Vec<CharSpan> {
            vec![
                span(0, 5, 0.0, 0.0, 50.0, 20.0),
                span(6, 3, 60.0, 0.0, 30.0, 20.0),
                span(10, 4, 0.0, 30.0, 40.0, 20.0),
            ]
        }

        #[test]
        fn before_middle_after_last_char() {
            let m = map();
            assert_eq!(hit_test(&m, Pos2::new(-20.0, 10.0)), Some(0));
            assert_eq!(hit_test(&m, Pos2::new(25.0, 10.0)), Some(3));
            assert_eq!(hit_test(&m, Pos2::new(200.0, 10.0)), Some(9));
        }

        #[test]
        fn another_line_and_gap_between_lines() {
            let m = map();
            assert_eq!(hit_test(&m, Pos2::new(20.0, 40.0)), Some(12));
            // y=25 is closer to the first line.
            assert_eq!(hit_test(&m, Pos2::new(20.0, 25.0)), Some(2));
        }

        #[test]
        fn word_gap_uses_nearest_word() {
            let m = map();
            assert_eq!(hit_test(&m, Pos2::new(55.0, 10.0)), Some(5));
        }

        #[test]
        fn empty_map_has_no_hit() {
            assert_eq!(hit_test(&[], Pos2::new(0.0, 0.0)), None);
        }
    }
}
