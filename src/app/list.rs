//! List rendering for the `<mdlist>` html hook: one justified, hanging-indented
//! row per item, with the marker painted in the gutter.

use std::collections::HashMap;
use std::rc::Rc;

use crate::doc::{ListKind, ListSpec};
use crate::justify::{self, Marker};

use super::searchui::SearchUi;
use super::table::table_pad;

/// Shared, interior-mutable state backing the `<mdlist>` render path. Holds the
/// parsed specs and the reading column recorded once per frame.
pub(crate) struct ListUi {
    specs: HashMap<String, ListSpec>,
    /// Reading column in effect this frame; `ui.available_width()` inside the
    /// html hook is not that column.
    column: f32,
    /// Visible text per item, built once per list id and shared per frame.
    texts: HashMap<String, Rc<[Rc<str>]>>,
}

impl ListUi {
    pub(crate) fn new(specs: HashMap<String, ListSpec>) -> Self {
        ListUi {
            specs,
            column: 0.0,
            texts: HashMap::new(),
        }
    }

    /// Record the document's reading column once per frame, before the viewer
    /// runs.
    pub(crate) fn begin_frame(&mut self, column: f32) {
        self.column = column;
    }

    pub(crate) fn reset(&mut self, specs: HashMap<String, ListSpec>) {
        self.specs = specs;
        self.texts.clear();
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, id: &str, search: &mut SearchUi) {
        // Borrow the spec for the frame; the text cache is a separate field.
        let Some(spec) = self.specs.get(id) else {
            return; // unknown id: draw nothing, never panic
        };
        let column = self.column;
        let space = justify::space_width(ui);
        let pad = table_pad(ui.text_style_height(&egui::TextStyle::Body));
        let texts = item_texts(&mut self.texts, id, spec);
        // A bare `add_space` would advance horizontally in the hook's wrapping
        // layout; the vertical wrapper makes the padding real space.
        ui.vertical(|ui| {
            // Half the normal item gap reads as one block, not loose items.
            ui.spacing_mut().item_spacing.y *= 0.5;
            ui.add_space(pad);
            for (index, item) in spec.items.iter().enumerate() {
                // One searchable block per item, at the row it starts on.
                let text = texts.get(index).map_or_else(|| Rc::from(""), Rc::clone);
                let block = search.record(ui, text);
                let (ranges, current) = search.highlights(block);
                let highlights = justify::MatchHighlight {
                    ranges: &ranges,
                    current,
                    selection: search.selection_range(block),
                };
                let marker = match item.kind {
                    ListKind::Bullet if item.depth % 2 == 1 => Marker::Hollow,
                    ListKind::Bullet => Marker::Bullet,
                    ListKind::Ordered(n, delim) => Marker::Number(n, delim),
                    ListKind::Task { done, ordered } => Marker::Task(done, ordered),
                };
                let runs = justify::parse_inline(&item.text);
                let indent = justify::list_indent(item.depth, space);
                let painted = justify::draw_indented(ui, &runs, column, indent, marker, highlights);
                search.set_height(block, painted.height);
                search.set_rect(block, painted.rect);
                search.set_char_map(block, painted.chars);
                for rect in painted.interactives {
                    search.add_control(rect, egui::CursorIcon::PointingHand);
                }
            }
            ui.add_space(pad);
        });
    }
}

/// Visible text of every item in one list, built once and shared per frame.
fn item_texts<'a>(
    cache: &'a mut HashMap<String, Rc<[Rc<str>]>>,
    id: &str,
    spec: &ListSpec,
) -> &'a [Rc<str>] {
    if !cache.contains_key(id) {
        let texts: Vec<Rc<str>> = spec
            .items
            .iter()
            .map(|item| Rc::from(crate::justify::search_text(&item.text)))
            .collect();
        cache.insert(id.to_string(), Rc::from(texts));
    }
    cache.get(id).map_or(&[][..], |texts| &texts[..])
}

#[cfg(test)]
mod tests {
    use super::item_texts;
    use crate::doc::{ListItem, ListKind, ListSpec};
    use std::collections::HashMap;

    #[test]
    fn item_texts_are_built_once_and_reused() {
        let item = |text: &str| ListItem {
            depth: 0,
            kind: ListKind::Bullet,
            text: text.to_string(),
        };
        let spec = ListSpec {
            items: vec![item("**one**"), item("`two`")],
        };
        let mut cache = HashMap::new();
        let first = item_texts(&mut cache, "list", &spec);
        assert_eq!(&*first[0], "one");
        assert_eq!(&*first[1], "two");
        // A different spec under the same id is ignored: the cache wins.
        let changed = ListSpec {
            items: vec![item("changed")],
        };
        let again = item_texts(&mut cache, "list", &changed);
        assert_eq!(again.len(), 2);
        assert_eq!(&*again[0], "one");
    }
}
