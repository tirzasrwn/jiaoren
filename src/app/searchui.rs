//! Shared search state for the render hook and the app: the persistent
//! [`SearchState`] plus the block texts and content positions recorded while
//! the document is drawn, and the `/` command line buffer.

use std::collections::HashMap;
use std::rc::Rc;

use crate::justify::CharSpan;
use crate::search::SearchState;

use super::selection::{hit_test_distance, SelPoint, SelectionState};

/// Search state backing the `/` command line, the status line and `n`/`N`.
pub(crate) struct SearchUi {
    /// Confirmed pattern and its hits.
    pub(crate) state: SearchState,
    /// Live text selection over the same block registry.
    pub(crate) selection: SelectionState,
    /// `Some(query)` while the command line is open.
    pub(crate) input: Option<String>,
    /// State to restore when `/` input is cancelled with Esc/Ctrl+C.
    backup: Option<SearchState>,
    /// Block under the viewport when `/` opened; live matches start here so a
    /// search never jumps backwards to the top of the document.
    origin: Option<usize>,
    /// Visible text per placeholder id, shared with every frame. The renderers
    /// build it once (markdown is stripped inline-wise, which is expensive) and
    /// `texts` then only clones the `Rc`.
    cache: HashMap<String, Rc<str>>,
    /// Block texts in draw order, rebuilt every frame. Keys run before the
    /// frame is drawn, so they see the previous frame's list.
    texts: Vec<Rc<str>>,
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
    /// Painted block rects from the last completed frame, for the text cursor.
    rects: Vec<egui::Rect>,
    /// Link/copy-control rects and their intended cursor from the last
    /// completed frame. Presses here must not start a text selection. Sorted by
    /// top in [`Self::finish_frame`]: a block that overlays its button (table,
    /// code) appends it after its cells, so draw order is not top order.
    controls: Vec<(egui::Rect, egui::CursorIcon)>,
    /// Heading anchors painted this frame: slug → block index.
    anchors: std::collections::HashMap<String, usize>,
    /// Heading id → block index, for the index overlay jump.
    heading_blocks: std::collections::HashMap<String, usize>,
    /// Recompute hits from the next frame's block texts, after a reload kept
    /// the committed pattern.
    recompute_after_frame: bool,
}

impl SearchUi {
    pub(crate) fn new() -> Self {
        SearchUi {
            state: SearchState::default(),
            selection: SelectionState::default(),
            input: None,
            backup: None,
            origin: None,
            cache: HashMap::new(),
            texts: Vec::new(),
            tops: Vec::new(),
            ys: Vec::new(),
            heights: Vec::new(),
            hs: Vec::new(),
            maps: Vec::new(),
            rects: Vec::new(),
            controls: Vec::new(),
            anchors: std::collections::HashMap::new(),
            heading_blocks: std::collections::HashMap::new(),
            recompute_after_frame: false,
        }
    }

    /// Drop last frame's blocks; called before the viewer draws this one.
    pub(crate) fn begin_frame(&mut self) {
        self.texts.clear();
        self.tops.clear();
        self.heights.clear();
        self.maps.clear();
        self.rects.clear();
        self.controls.clear();
        self.anchors.clear();
        self.heading_blocks.clear();
    }

    /// Drop per-document state: hits, selection, cached block texts and the
    /// previous frame's screen geometry. The cache keys are placeholder ids, so
    /// a reload must not keep them.
    pub(crate) fn reset(&mut self) {
        self.state.clear();
        self.selection.clear();
        self.cache.clear();
        self.texts.clear();
        self.tops.clear();
        self.ys.clear();
        self.heights.clear();
        self.hs.clear();
        self.maps.clear();
        self.rects.clear();
        self.controls.clear();
        self.anchors.clear();
        self.heading_blocks.clear();
        self.origin = None;
        self.recompute_after_frame = false;
    }

    /// Drop per-document state but keep the committed search pattern. Hits are
    /// recomputed from the next frame's freshly recorded block texts, so a live
    /// reload does not silently clear the search.
    pub(crate) fn reset_keeping_pattern(&mut self) {
        let pattern = self.state.pattern.take();
        self.reset();
        self.recompute_after_frame = pattern.is_some();
        self.state.pattern = pattern;
    }

    /// Visible text for a placeholder id, built once and shared by every frame.
    /// `id` is the content hash from `doc`, so identical blocks share one entry.
    pub(crate) fn cached_text(&mut self, id: &str, build: impl FnOnce() -> String) -> Rc<str> {
        if let Some(text) = self.cache.get(id) {
            return Rc::clone(text);
        }
        let text: Rc<str> = Rc::from(build());
        self.cache.insert(id.to_string(), Rc::clone(&text));
        text
    }

    /// Record one searchable block at the ui's current cursor position. Returns
    /// its block index for highlight lookup. Accepting anything that converts
    /// into an `Rc<str>` lets renderers pass a cached text without copying it.
    pub(crate) fn record(&mut self, ui: &egui::Ui, text: impl Into<Rc<str>>) -> usize {
        let index = self.texts.len();
        self.texts.push(text.into());
        self.tops.push(ui.cursor().min.y);
        self.heights.push(0.0);
        self.maps.push(Vec::new());
        self.rects.push(egui::Rect::ZERO);
        index
    }

    /// Store a block's painted char map for pointer hit testing.
    pub(crate) fn set_char_map(&mut self, index: usize, chars: Vec<CharSpan>) {
        if let Some(slot) = self.maps.get_mut(index) {
            *slot = chars;
        }
    }

    /// Store a block's painted rect for cursor hover tests.
    pub(crate) fn set_rect(&mut self, index: usize, rect: egui::Rect) {
        if let Some(slot) = self.rects.get_mut(index) {
            *slot = rect;
        }
    }

    /// Index just past the last block whose recorded top is at or above `y`.
    /// `tops` is written in document order, so it is sorted and searchable.
    fn blocks_above(&self, y: f32) -> usize {
        self.tops.partition_point(|&top| top <= y)
    }

    /// True when `pos` is over painted text (any block rect). Only blocks that
    /// start above `pos` can contain it, so the prefix is enough.
    pub(crate) fn pointer_over_text(&self, pos: egui::Pos2) -> bool {
        let above = self.blocks_above(pos.y).min(self.rects.len());
        self.rects[..above].iter().any(|rect| rect.contains(pos))
    }

    /// Record a heading id's block so the index overlay can jump to it.
    pub(crate) fn record_heading(&mut self, id: &str, block: usize) {
        self.heading_blocks.insert(id.to_string(), block);
    }

    /// Block for a heading id painted last frame.
    pub(crate) fn heading_block(&self, id: &str) -> Option<usize> {
        self.heading_blocks.get(id).copied()
    }

    /// Record a heading's anchor slug so `#slug` links can jump to it.
    pub(crate) fn record_anchor(&mut self, slug: &str, block: usize) {
        self.anchors.entry(slug.to_string()).or_insert(block);
    }

    /// Block for an anchor slug painted last frame.
    pub(crate) fn anchor_block(&self, slug: &str) -> Option<usize> {
        self.anchors.get(slug).copied()
    }

    /// Register a link/control rect that owns the press instead of selection.
    pub(crate) fn add_control(&mut self, rect: egui::Rect, cursor: egui::CursorIcon) {
        self.controls.push((rect, cursor));
    }

    /// True when `pos` is on a link or copy button painted last frame. Controls
    /// are sorted by top in [`Self::finish_frame`], so the prefix is searchable.
    pub(crate) fn pointer_on_control(&self, pos: egui::Pos2) -> bool {
        let above = self
            .controls
            .partition_point(|(rect, _)| rect.top() <= pos.y + 1.0)
            .min(self.controls.len());
        self.controls[..above]
            .iter()
            .any(|(rect, _)| rect.expand(1.0).contains(pos))
    }

    /// The cursor requested by a control under `pos`, if any.
    pub(crate) fn control_cursor(&self, pos: egui::Pos2) -> Option<egui::CursorIcon> {
        let above = self
            .controls
            .partition_point(|(rect, _)| rect.top() <= pos.y + 1.0)
            .min(self.controls.len());
        self.controls[..above]
            .iter()
            .rev()
            .find(|(rect, _)| rect.expand(1.0).contains(pos))
            .map(|(_, cursor)| *cursor)
    }

    /// Selected char range in `block`, if any.
    pub(crate) fn selection_range(&self, block: usize) -> Option<(usize, usize)> {
        if !self.selection.is_active() {
            return None; // no selection: skip counting every block's chars
        }
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

    /// Screen-space vertical band of a recorded block, from the draw that
    /// produced its char map. The painted rect can be taller than the recorded
    /// height (frames, padded rows), so take the larger bottom.
    fn band(&self, block: usize) -> (f32, f32) {
        let top = self.tops.get(block).copied().unwrap_or(0.0);
        let mut bottom = top + self.heights.get(block).copied().unwrap_or(0.0);
        if let Some(rect) = self.rects.get(block) {
            if rect.width() > 0.0 && rect.height() > 0.0 {
                bottom = bottom.max(rect.bottom());
            }
        }
        (top, bottom)
    }

    /// Nearest char: walk blocks outward from the one under `pos.y`, stopping a
    /// direction once its vertical gap alone is farther than the best distance
    /// found. Bands are in document order, so the walk keeps nearest-point
    /// semantics while skipping the span scan for every other block.
    fn hit_distance(&self, pos: egui::Pos2) -> Option<(SelPoint, f32)> {
        let len = self.maps.len();
        if len == 0 {
            return None;
        }
        let split = self.blocks_above(pos.y);
        let mut best: Option<(SelPoint, f32)> = None;
        let consider = |block: usize, best: &mut Option<(SelPoint, f32)>| -> bool {
            let (top, bottom) = self.band(block);
            let dy = (top - pos.y).max(pos.y - bottom).max(0.0);
            if best.is_some_and(|(_, d)| dy * dy >= d) {
                return false;
            }
            if let Some((char, distance)) = hit_test_distance(&self.maps[block], pos) {
                if best.is_none_or(|(_, d)| distance < d) {
                    *best = Some((SelPoint { block, char }, distance));
                }
            }
            true
        };
        let mut block = split.min(len);
        while block > 0 {
            block -= 1;
            if !consider(block, &mut best) {
                break;
            }
        }
        for block in split..len {
            if !consider(block, &mut best) {
                break;
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
        // The control queries binary-search by top, but draw order is not top
        // order: a table's copy button is appended after its cells while sitting
        // at the table's top. Sort once here, before the next frame's queries.
        self.controls
            .sort_by(|a, b| a.0.top().total_cmp(&b.0.top()));
        self.ys.clear();
        self.ys
            .extend(self.tops.iter().map(|top| offset_y + top - viewport_top));
        self.hs.clear();
        self.hs.extend_from_slice(&self.heights);
        if std::mem::take(&mut self.recompute_after_frame) {
            if let Some(pattern) = self.state.pattern.clone() {
                // Reset to the first hit; the reader can step from there.
                self.state
                    .set_pattern(self.texts.iter().map(|text| &**text), &pattern);
            }
        }
    }

    /// Block texts of the last completed frame, in document order.
    pub(crate) fn texts(&self) -> &[Rc<str>] {
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

    /// Block under content offset `y`, or the next one when `y` falls in a gap.
    /// `None` past the last block (the search then wraps).
    pub(crate) fn block_at(&self, y: f32) -> Option<usize> {
        self.ys
            .iter()
            .zip(&self.hs)
            .position(|(&top, &height)| top + height.max(0.0) > y)
    }

    /// Block index at the next (`dir > 0`) or previous block start relative to
    /// content offset `from`. One-point slack keeps the block the viewport is
    /// already showing from counting as the next one.
    pub(crate) fn block_after(&self, from: f32, dir: isize) -> Option<usize> {
        if dir > 0 {
            self.ys.iter().position(|&y| y > from + 0.5)
        } else {
            self.ys.iter().rposition(|&y| y < from - 0.5)
        }
    }

    /// Block-local highlight ranges and the current-hit index, if the current
    /// hit belongs to `block`.
    pub(crate) fn highlights(&self, block: usize) -> (Vec<(usize, usize)>, Option<usize>) {
        // Nothing to paint without a pattern, and one highlight at a time while
        // a selection is painted (the search fills would sit on top of it and
        // read as a second colour).
        if self.selection.is_active() || self.state.pattern.is_none() {
            return (Vec::new(), None);
        }
        self.state.block_highlights(block)
    }

    /// Open the command line with an empty query, remembering the committed
    /// search so Esc/Ctrl+C can restore it.
    pub(crate) fn open(&mut self) {
        self.open_from(None);
    }

    /// [`Self::open`] with the block under the viewport, so the first match is
    /// searched forward from where the reader is.
    pub(crate) fn open_from(&mut self, origin: Option<usize>) {
        self.backup = Some(self.state.clone());
        self.input = Some(String::new());
        self.state.clear();
        self.origin = origin;
    }

    /// Close the command line and restore the pre-`/` search state. Returns the
    /// restored current match's block, so the app can re-centre the viewport.
    pub(crate) fn cancel(&mut self) -> Option<usize> {
        if let Some(state) = self.backup.take() {
            self.state = state;
        }
        self.input = None;
        self.origin = None;
        self.state.current_block()
    }

    /// Keep the live search and leave input mode.
    pub(crate) fn commit(&mut self) {
        self.input = None;
        self.backup = None;
        self.origin = None;
    }

    /// Current query text while input mode is open.
    pub(crate) fn query(&self) -> &str {
        self.input.as_deref().unwrap_or_default()
    }

    /// Append raw typed/pasted text and recompute hits live. Test-only: the app
    /// applies a frame's edits in one batch, then calls [`Self::recompute_live`].
    #[cfg(test)]
    pub(crate) fn type_text(&mut self, text: &str) {
        if let Some(query) = self.input.as_mut() {
            query.push_str(text);
        }
        self.live_update();
    }

    /// Delete the last char (unicode-aware) and recompute live. Test-only.
    #[cfg(test)]
    pub(crate) fn backspace(&mut self) {
        if let Some(query) = self.input.as_mut() {
            query.pop();
        }
        self.live_update();
    }

    /// Recompute hits after applying a frame's edits. One pass per frame, no
    /// per-event block cloning.
    pub(crate) fn recompute_live(&mut self) {
        self.live_update();
    }

    /// Recompute the live match set from the current query. An empty query
    /// clears hits but stays in input mode. The block texts are borrowed, so an
    /// edit copies no document text.
    fn live_update(&mut self) {
        let pattern = self.query().to_string();
        self.state
            .set_pattern(self.texts.iter().map(|text| &**text), &pattern);
        if let Some(origin) = self.origin {
            self.state.start_at(origin);
        }
    }

    /// Whether the command line owns the keyboard.
    pub(crate) fn is_open(&self) -> bool {
        self.input.is_some()
    }
}

#[cfg(test)]
mod tests {
    mod positions {
        use super::super::SearchUi;
        use std::rc::Rc;

        fn rc(texts: &[&str]) -> Vec<Rc<str>> {
            texts.iter().map(|text| Rc::from(*text)).collect()
        }

        #[test]
        fn content_y_is_offset_plus_top_minus_viewport_top() {
            let mut s = SearchUi::new();
            s.texts = rc(&["first", "second"]);
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
        fn block_at_finds_the_block_under_the_viewport_top() {
            let mut s = SearchUi::new();
            s.ys = vec![100.0, 200.0, 300.0];
            s.hs = vec![50.0, 50.0, 50.0];
            assert_eq!(s.block_at(100.0), Some(0));
            assert_eq!(s.block_at(149.0), Some(0));
            assert_eq!(s.block_at(150.0), Some(1));
            assert_eq!(s.block_at(310.0), Some(2));
            assert_eq!(s.block_at(400.0), None);
        }

        #[test]
        fn open_from_searches_forward_from_the_viewport() {
            let mut s = SearchUi::new();
            s.texts = rc(&["alpha", "beta", "alpha"]);
            s.ys = vec![100.0, 200.0, 300.0];
            s.hs = vec![50.0, 50.0, 50.0];
            s.open_from(Some(1));
            s.type_text("alpha");
            // The hit in block 0 is above the reader; block 2 is the next one.
            assert_eq!(s.state.current_block(), Some(2));
            // Past the last match, the search wraps to the first one.
            s.open_from(Some(2));
            s.type_text("beta");
            assert_eq!(s.state.current_block(), Some(1));
            // Without an origin (a direct open) the first match still wins.
            s.open_from(None);
            s.type_text("alpha");
            assert_eq!(s.state.current_block(), Some(0));
        }

        #[test]
        fn block_after_steps_over_blocks_in_content_order() {
            let mut s = SearchUi::new();
            s.ys = vec![100.0, 200.0, 300.0];
            assert_eq!(s.block_after(100.0, 1), Some(1));
            assert_eq!(s.block_after(200.0, 1), Some(2));
            assert_eq!(s.block_after(300.0, 1), None);
            assert_eq!(s.block_after(300.0, -1), Some(1));
            assert_eq!(s.block_after(100.0, -1), None);
            assert_eq!(s.block_after(150.0, -1), Some(0));
            assert_eq!(s.block_after(150.0, 1), Some(1));
        }

        #[test]
        fn anchors_live_for_one_frame() {
            let mut s = SearchUi::new();
            s.record_anchor("intro", 4);
            assert_eq!(s.anchor_block("intro"), Some(4));
            assert_eq!(s.anchor_block("missing"), None);
            s.begin_frame();
            assert_eq!(s.anchor_block("intro"), None);
        }

        #[test]
        fn live_input_updates_backspace_and_cancel_restore() {
            let mut s = SearchUi::new();
            s.texts = rc(&["abc one", "abc two"]);
            // Previous committed search to restore on cancel.
            s.state.set_pattern(s.texts.iter().map(|t| &**t), "two");
            assert_eq!(s.state.pattern.as_deref(), Some("two"));
            s.open();
            assert!(s.is_open());
            assert_eq!(s.query(), "");
            assert!(s.state.pattern.is_none(), "opening clears live state");

            s.type_text("abc");
            assert_eq!(s.query(), "abc");
            assert_eq!(s.state.hits.len(), 2);
            assert_eq!(s.state.status_text().as_deref(), Some("/abc [1/2]"));
            s.backspace();
            assert_eq!(s.query(), "ab");
            assert_eq!(s.state.hits.len(), 2);

            let restored = s.cancel();
            assert!(!s.is_open());
            assert_eq!(s.state.pattern.as_deref(), Some("two"));
            assert_eq!(s.state.hits.len(), 1);
            // Restored current block is returned so the app can re-centre.
            assert_eq!(restored, Some(1));
            assert_eq!(s.state.current_block(), Some(1));
        }

        #[test]
        fn enter_commit_keeps_live_search_and_empty_query_clears_hits() {
            let mut s = SearchUi::new();
            s.texts = rc(&["hello world"]);
            s.open();
            s.type_text("world");
            assert_eq!(s.state.status_text().as_deref(), Some("/world [1/1]"));
            s.commit();
            assert!(!s.is_open());
            assert_eq!(s.state.pattern.as_deref(), Some("world"));

            s.open();
            s.type_text("x");
            s.backspace();
            assert_eq!(s.query(), "");
            assert!(s.state.hits.is_empty());
            assert!(s.is_open(), "empty query stays in input mode");
            s.commit();
            assert_eq!(s.state.pattern, None);
        }

        #[test]
        fn reload_keeps_pattern_and_recomputes_from_new_texts() {
            let mut s = SearchUi::new();
            s.texts = rc(&["old needle", "plain"]);
            s.state.set_pattern(s.texts.iter().map(|t| &**t), "needle");
            assert_eq!(s.state.current_block(), Some(0));
            s.reset_keeping_pattern();
            assert_eq!(s.state.pattern.as_deref(), Some("needle"));
            assert!(s.state.hits.is_empty(), "stale hits are dropped");
            assert!(!s.is_open());
            s.texts = rc(&["intro", "new needle here"]);
            s.tops = vec![0.0, 40.0];
            s.heights = vec![10.0, 10.0];
            s.finish_frame(0.0, 0.0);
            assert_eq!(s.state.pattern.as_deref(), Some("needle"));
            assert_eq!(s.state.hits.len(), 1);
            assert_eq!(s.state.current_block(), Some(1));
        }

        #[test]
        fn full_reset_still_clears_the_pattern() {
            let mut s = SearchUi::new();
            s.texts = rc(&["needle"]);
            s.state.set_pattern(s.texts.iter().map(|t| &**t), "needle");
            s.reset();
            assert!(s.state.pattern.is_none());
            assert!(s.state.hits.is_empty());
        }

        #[test]
        fn cancel_without_prior_search_has_no_jump_target() {
            let mut s = SearchUi::new();
            s.texts = rc(&["abc"]);
            s.open();
            s.type_text("abc");
            assert_eq!(s.cancel(), None);
            assert!(s.state.pattern.is_none());
        }

        #[test]
        fn backspace_is_one_unicode_char() {
            let mut s = SearchUi::new();
            s.texts = rc(&["é東"]);
            s.open();
            s.type_text("é東");
            assert_eq!(s.query(), "é東");
            s.backspace();
            assert_eq!(s.query(), "é");
            s.backspace();
            assert_eq!(s.query(), "");
            s.backspace();
            assert_eq!(s.query(), "");
        }
    }

    mod cache {
        use super::super::SearchUi;
        use std::rc::Rc;

        #[test]
        fn cached_text_is_built_once_and_shared() {
            let mut s = SearchUi::new();
            let first = s.cached_text("id", || "hello".to_string());
            let second = s.cached_text("id", || panic!("must not rebuild"));
            assert_eq!(&*second, "hello");
            assert!(Rc::ptr_eq(&first, &second));
        }

        #[test]
        fn reset_drops_the_text_cache() {
            let mut s = SearchUi::new();
            let _ = s.cached_text("id", || "hello".to_string());
            s.reset();
            let rebuilt = s.cached_text("id", || "again".to_string());
            assert_eq!(&*rebuilt, "again");
        }
    }

    mod hit {
        use super::super::SearchUi;
        use crate::justify::CharSpan;
        use egui::{Pos2, Rect};
        use std::rc::Rc;

        fn span(start: usize, len: usize, x: f32, y: f32, w: f32, h: f32) -> CharSpan {
            CharSpan {
                start,
                len,
                rect: Rect::from_min_size(Pos2::new(x, y), egui::vec2(w, h)),
            }
        }

        fn add(s: &mut SearchUi, top: f32, height: f32, text: &str, chars: Vec<CharSpan>) {
            s.texts.push(Rc::from(text));
            s.tops.push(top);
            s.heights.push(height);
            s.rects.push(Rect::from_min_size(
                Pos2::new(0.0, top),
                egui::vec2(100.0, height),
            ));
            s.maps.push(chars);
        }

        fn two_blocks() -> SearchUi {
            let mut s = SearchUi::new();
            add(
                &mut s,
                0.0,
                20.0,
                "hello",
                vec![span(0, 5, 0.0, 0.0, 50.0, 20.0)],
            );
            add(
                &mut s,
                40.0,
                20.0,
                "world",
                vec![span(0, 5, 0.0, 40.0, 50.0, 20.0)],
            );
            s
        }

        #[test]
        fn picks_the_block_under_the_pointer() {
            let s = two_blocks();
            assert_eq!(s.hit(Pos2::new(25.0, 10.0)).map(|p| p.block), Some(0));
            assert_eq!(s.hit(Pos2::new(25.0, 50.0)).map(|p| p.block), Some(1));
            assert_eq!(s.hit(Pos2::new(25.0, 10.0)).map(|p| p.char), Some(3));
        }

        #[test]
        fn gap_prefers_the_nearer_block_and_edges_stay_reachable() {
            let s = two_blocks();
            // y=35 is 15 px from block 0 and 5 px from block 1.
            assert_eq!(s.hit(Pos2::new(10.0, 35.0)).map(|p| p.block), Some(1));
            // Above the first and below the last still land on the nearest one.
            assert_eq!(s.hit(Pos2::new(10.0, -50.0)).map(|p| p.block), Some(0));
            assert_eq!(s.hit(Pos2::new(10.0, 500.0)).map(|p| p.block), Some(1));
        }

        #[test]
        fn nearest_point_semantics_survive_the_y_prefilter() {
            let mut s = SearchUi::new();
            // A tall empty block between two text rows must not hide the text:
            // the nearest char is still in one of the rows.
            add(
                &mut s,
                0.0,
                20.0,
                "top",
                vec![span(0, 3, 0.0, 0.0, 30.0, 20.0)],
            );
            add(&mut s, 200.0, 20.0, "", Vec::new());
            add(
                &mut s,
                400.0,
                20.0,
                "bottom",
                vec![span(0, 6, 0.0, 400.0, 60.0, 20.0)],
            );
            // Inside the empty block's band, the nearest painted char is above.
            assert_eq!(s.hit(Pos2::new(5.0, 210.0)).map(|p| p.block), Some(0));
            // Closer to the bottom row it flips.
            assert_eq!(s.hit(Pos2::new(5.0, 390.0)).map(|p| p.block), Some(2));
        }

        #[test]
        fn pointer_over_text_uses_the_same_bands() {
            let s = two_blocks();
            assert!(s.pointer_over_text(Pos2::new(25.0, 10.0)));
            assert!(!s.pointer_over_text(Pos2::new(25.0, 30.0)));
        }

        #[test]
        fn controls_are_found_from_the_sorted_prefix() {
            let mut s = SearchUi::new();
            let rect = |y: f32| Rect::from_min_size(Pos2::new(0.0, y), egui::vec2(14.0, 14.0));
            s.add_control(rect(10.0), egui::CursorIcon::Default);
            s.add_control(rect(50.0), egui::CursorIcon::PointingHand);
            assert!(s.pointer_on_control(Pos2::new(5.0, 52.0)));
            assert!(!s.pointer_on_control(Pos2::new(5.0, 30.0)));
            assert_eq!(
                s.control_cursor(Pos2::new(5.0, 52.0)),
                Some(egui::CursorIcon::PointingHand)
            );
            assert_eq!(s.control_cursor(Pos2::new(5.0, 30.0)), None);
        }

        #[test]
        fn idle_selection_range_does_not_touch_block_texts() {
            let s = SearchUi::new();
            assert_eq!(s.selection_range(0), None);
        }
    }
}
