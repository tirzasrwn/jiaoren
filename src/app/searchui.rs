//! Shared search state for the render hook and the app: the persistent
//! [`SearchState`] plus the block texts and content positions recorded while
//! the document is drawn, and the `/` command line buffer.

use crate::justify::CharSpan;
use crate::search::SearchState;

use super::selection::{hit_test_distance, SelPoint, SelectionState};

/// Search state backing the `/` command line, the status line and `n`/`N`.
pub(crate) struct SearchUi {
    /// Confirmed pattern and its hits.
    pub(crate) state: SearchState,
    /// Live text selection over the same block registry.
    pub(crate) selection: SelectionState,
    /// `Some(buffer)` while the command line is open.
    pub(crate) input: Option<String>,
    /// Block texts in draw order, rebuilt every frame. Keys run before the
    /// frame is drawn, so they see the previous frame's list.
    texts: Vec<String>,
    /// Screen-space top of each block, recorded while drawing.
    tops: Vec<f32>,
    /// Content-space top of each block, computed after the layout.
    ys: Vec<f32>,
    /// Screen-space height recorded while drawing this frame.
    heights: Vec<f32>,
    /// Content-space block heights from the last completed frame.
    hs: Vec<f32>,
    /// Char maps (fragment rects in screen space) from the last completed frame.
    maps: Vec<Vec<CharSpan>>,
    /// Link/copy-control rects from the last completed frame. Presses here must
    /// not start a text selection.
    controls: Vec<egui::Rect>,
}

impl SearchUi {
    pub(crate) fn new() -> Self {
        SearchUi {
            state: SearchState::default(),
            selection: SelectionState::default(),
            input: None,
            texts: Vec::new(),
            tops: Vec::new(),
            ys: Vec::new(),
            heights: Vec::new(),
            hs: Vec::new(),
            maps: Vec::new(),
            controls: Vec::new(),
        }
    }

    /// Drop last frame's blocks; called before the viewer draws this one.
    pub(crate) fn begin_frame(&mut self) {
        self.texts.clear();
        self.tops.clear();
        self.heights.clear();
        self.maps.clear();
        self.controls.clear();
    }

    /// Record one searchable block at the ui's current cursor position. Returns
    /// its block index for highlight lookup.
    pub(crate) fn record(&mut self, ui: &egui::Ui, text: String) -> usize {
        let index = self.texts.len();
        self.texts.push(text);
        self.tops.push(ui.cursor().min.y);
        self.heights.push(0.0);
        self.maps.push(Vec::new());
        index
    }

    /// Store a block's painted char map for pointer hit testing.
    pub(crate) fn set_char_map(&mut self, index: usize, chars: Vec<CharSpan>) {
        if let Some(slot) = self.maps.get_mut(index) {
            *slot = chars;
        }
    }

    /// Register a link/control rect that owns the press instead of selection.
    pub(crate) fn add_control(&mut self, rect: egui::Rect) {
        self.controls.push(rect);
    }

    /// True when `pos` is on a link or copy button painted last frame.
    pub(crate) fn pointer_on_control(&self, pos: egui::Pos2) -> bool {
        self.controls
            .iter()
            .any(|rect| rect.expand(1.0).contains(pos))
    }

    /// Selected char range in `block`, if any.
    pub(crate) fn selection_range(&self, block: usize) -> Option<(usize, usize)> {
        let len = self.texts.get(block)?.chars().count();
        self.selection.range_for(block, len)
    }

    /// Nearest block/char under `pos` from the last frame's char maps.
    pub(crate) fn hit(&self, pos: egui::Pos2) -> Option<SelPoint> {
        self.hit_distance(pos).map(|(point, _)| point)
    }

    /// Like [`Self::hit`], but only when the pointer is close to painted text;
    /// an empty margin press must fall through to document panning/clearing.
    pub(crate) fn hit_near(&self, pos: egui::Pos2) -> Option<SelPoint> {
        const SLOP: f32 = 6.0;
        self.hit_distance(pos)
            .filter(|(_, distance)| *distance <= SLOP * SLOP)
            .map(|(point, _)| point)
    }

    fn hit_distance(&self, pos: egui::Pos2) -> Option<(SelPoint, f32)> {
        let mut best: Option<(SelPoint, f32)> = None;
        for (block, map) in self.maps.iter().enumerate() {
            if let Some((char, distance)) = hit_test_distance(map, pos) {
                if best.is_none_or(|(_, d)| distance < d) {
                    best = Some((SelPoint { block, char }, distance));
                }
            }
        }
        best
    }

    /// Set a block height measured by its painter (code blocks, table cells).
    pub(crate) fn set_height(&mut self, index: usize, height: f32) {
        if let Some(slot) = self.heights.get_mut(index) {
            *slot = height.max(0.0);
        }
    }

    /// Convert this frame's screen positions into content positions:
    /// `offset + top - viewport_top`. Called once after the scroll area.
    pub(crate) fn finish_frame(&mut self, offset_y: f32, viewport_top: f32) {
        self.ys.clear();
        self.ys
            .extend(self.tops.iter().map(|top| offset_y + top - viewport_top));
        self.hs.clear();
        self.hs.extend_from_slice(&self.heights);
    }

    /// Block texts of the last completed frame, in document order.
    pub(crate) fn texts(&self) -> &[String] {
        &self.texts
    }

    /// Content y of block `index` in the last completed frame.
    pub(crate) fn block_y(&self, index: usize) -> Option<f32> {
        self.ys.get(index).copied()
    }

    /// Height of block `index` in the last completed frame.
    pub(crate) fn block_height(&self, index: usize) -> Option<f32> {
        self.hs.get(index).copied()
    }

    /// Block-local highlight ranges and the current-hit index, if the current
    /// hit belongs to `block`.
    pub(crate) fn highlights(&self, block: usize) -> (Vec<(usize, usize)>, Option<usize>) {
        self.state.block_highlights(block)
    }

    /// Open the command line with an empty buffer.
    pub(crate) fn open(&mut self) {
        self.input = Some(String::new());
    }

    /// Close the command line, discarding the buffer.
    pub(crate) fn cancel(&mut self) {
        self.input = None;
    }

    /// Whether the command line owns the keyboard.
    pub(crate) fn is_open(&self) -> bool {
        self.input.is_some()
    }

    /// Take the confirmed pattern and close the command line.
    pub(crate) fn take_input(&mut self) -> Option<String> {
        self.input.take()
    }
}

#[cfg(test)]
mod tests {
    mod positions {
        use super::super::SearchUi;

        #[test]
        fn content_y_is_offset_plus_top_minus_viewport_top() {
            let mut s = SearchUi::new();
            s.texts = vec!["first".to_string(), "second".to_string()];
            s.tops = vec![100.0, 250.0];
            s.heights = vec![32.0, 10.0];
            s.finish_frame(40.0, 20.0);
            assert_eq!(s.block_y(0), Some(120.0));
            assert_eq!(s.block_y(1), Some(270.0));
            assert_eq!(s.block_y(2), None);
            assert_eq!(s.block_height(0), Some(32.0));
            assert_eq!(s.block_height(1), Some(10.0));
            assert_eq!(s.block_height(2), None);
        }

        #[test]
        fn open_cancel_and_take_input() {
            let mut s = SearchUi::new();
            assert!(!s.is_open());
            s.open();
            assert!(s.is_open());
            s.cancel();
            assert!(!s.is_open());

            s.open();
            s.input = Some("mermaid".to_string());
            assert_eq!(s.take_input().as_deref(), Some("mermaid"));
            assert!(!s.is_open());
        }
    }
}
