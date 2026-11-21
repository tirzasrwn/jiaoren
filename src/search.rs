//! Search model: find pattern occurrences in the document's block texts, with
//! smartcase, and track the current match. Pure and unit-testable; the app
//! records block texts while drawing and calls into this module from the UI.

/// One match inside one block. Offsets are **character** indices, not bytes, so
/// a later highlight pass can slice the block text by chars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Index into the block list.
    pub block: usize,
    /// First matching character.
    pub start: usize,
    /// One past the last matching character.
    pub end: usize,
}

/// Lowercase `c` when its lower form is a single char; keep it otherwise.
fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(first), None) => first,
        _ => c,
    }
}

/// All non-overlapping matches of `pattern`, left to right and per block in
/// order. Case-sensitive when the pattern contains an uppercase character
/// (smartcase), case-insensitive otherwise. An empty pattern matches nothing.
pub fn find_matches(blocks: &[String], pattern: &str) -> Vec<Hit> {
    if pattern.is_empty() {
        return Vec::new();
    }
    let sensitive = pattern.chars().any(char::is_uppercase);
    let needle: Vec<char> = if sensitive {
        pattern.chars().collect()
    } else {
        pattern.chars().map(fold).collect()
    };
    let mut hits = Vec::new();
    for (block, text) in blocks.iter().enumerate() {
        let hay: Vec<char> = if sensitive {
            text.chars().collect()
        } else {
            text.chars().map(fold).collect()
        };
        let mut i = 0;
        // Never compare past the end; `needle` is non-empty.
        while i + needle.len() <= hay.len() {
            if hay[i..i + needle.len()] == needle[..] {
                hits.push(Hit {
                    block,
                    start: i,
                    end: i + needle.len(),
                });
                // Non-overlapping: continue after the match.
                i += needle.len();
            } else {
                i += 1;
            }
        }
    }
    hits
}

/// Next/previous index in `0..len`, wrapping. `len == 0` always yields 0.
pub fn step(current: usize, len: usize, dir: i32) -> usize {
    if len == 0 {
        return 0;
    }
    match dir.signum() {
        1 => (current + 1) % len,
        -1 => (current + len - 1) % len,
        _ => current.min(len - 1),
    }
}

/// The active search: pattern, every hit, and the current match index.
#[derive(Default, Clone)]
pub struct SearchState {
    /// Confirmed pattern, `None` when no search is active.
    pub pattern: Option<String>,
    /// Index into [`Self::hits`].
    pub current_match: usize,
    /// Matches for `pattern` over the block texts at confirm time.
    pub hits: Vec<Hit>,
}

impl SearchState {
    /// Start a search for `pattern` over `blocks`. Empty patterns clear instead.
    pub fn set_pattern(&mut self, blocks: &[String], pattern: &str) {
        self.hits = find_matches(blocks, pattern);
        self.current_match = 0;
        self.pattern = if pattern.is_empty() {
            None
        } else {
            Some(pattern.to_string())
        };
    }

    /// Drop the pattern and every hit.
    pub fn clear(&mut self) {
        self.pattern = None;
        self.current_match = 0;
        self.hits.clear();
    }

    /// Step to the next match, wrapping.
    pub fn next(&mut self) {
        self.current_match = step(self.current_match, self.hits.len(), 1);
    }

    /// Step to the previous match, wrapping.
    pub fn prev(&mut self) {
        self.current_match = step(self.current_match, self.hits.len(), -1);
    }

    /// Block index of the current match, if any.
    pub fn current_block(&self) -> Option<usize> {
        self.hits.get(self.current_match).map(|h| h.block)
    }

    /// All hit ranges on `block`, in order, plus the index into that slice of
    /// the current match when it belongs to this block.
    pub fn block_highlights(&self, block: usize) -> (Vec<(usize, usize)>, Option<usize>) {
        let mut ranges = Vec::new();
        let mut current = None;
        for (index, hit) in self.hits.iter().enumerate() {
            if hit.block != block {
                continue;
            }
            if index == self.current_match {
                current = Some(ranges.len());
            }
            ranges.push((hit.start, hit.end));
        }
        (ranges, current)
    }

    /// Status line text: `/pattern [i/n]`, 1-based. `None` for no pattern.
    pub fn status_text(&self) -> Option<String> {
        let pattern = self.pattern.as_ref()?;
        if self.hits.is_empty() {
            Some(format!("/{pattern} [0/0]"))
        } else {
            Some(format!(
                "/{pattern} [{}/{}]",
                self.current_match + 1,
                self.hits.len()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    mod matches {
        use super::super::find_matches;
        use super::super::Hit;

        fn blocks(texts: &[&str]) -> Vec<String> {
            texts.iter().map(|s| (*s).to_string()).collect()
        }

        #[test]
        fn lowercase_pattern_is_case_insensitive() {
            let hits = find_matches(&blocks(&["Mermaid and MERMAID"]), "mermaid");
            assert_eq!(hits.len(), 2);
            assert_eq!(
                hits[0],
                Hit {
                    block: 0,
                    start: 0,
                    end: 7
                }
            );
            assert_eq!(
                hits[1],
                Hit {
                    block: 0,
                    start: 12,
                    end: 19
                }
            );
        }

        #[test]
        fn uppercase_pattern_is_case_sensitive() {
            let hits = find_matches(&blocks(&["Mermaid and MERMAID"]), "Mermaid");
            assert_eq!(hits.len(), 1);
            assert_eq!(
                hits[0],
                Hit {
                    block: 0,
                    start: 0,
                    end: 7
                }
            );
            assert!(find_matches(&blocks(&["Mermaid"]), "MERMAID").is_empty());
        }

        #[test]
        fn matches_across_lines_inside_one_block() {
            let hits = find_matches(&blocks(&["first line\nsecond line"]), "line");
            assert_eq!(hits.len(), 2);
            assert_eq!(hits[0].end, 10);
            assert_eq!(hits[1].start, 18);
        }

        #[test]
        fn offsets_are_char_indices_for_unicode() {
            let text = "café 東京 café";
            let hits = find_matches(&blocks(&[text]), "café");
            assert_eq!(hits.len(), 2);
            // "café" is 4 chars, 5 bytes; the CJK block is 3 chars, 9 bytes.
            assert_eq!(
                hits[0],
                Hit {
                    block: 0,
                    start: 0,
                    end: 4
                }
            );
            assert_eq!(
                hits[1],
                Hit {
                    block: 0,
                    start: 8,
                    end: 12
                }
            );
            let cjk = find_matches(&blocks(&[text]), "東京");
            assert_eq!(
                cjk[0],
                Hit {
                    block: 0,
                    start: 5,
                    end: 7
                }
            );
        }

        #[test]
        fn overlapping_candidates_are_not_double_counted() {
            let hits = find_matches(&blocks(&["aaaa"]), "aaa");
            assert_eq!(hits.len(), 1);
            assert_eq!(
                hits[0],
                Hit {
                    block: 0,
                    start: 0,
                    end: 3
                }
            );
        }

        #[test]
        fn empty_pattern_has_no_hits() {
            assert!(find_matches(&blocks(&["anything"]), "").is_empty());
        }

        #[test]
        fn blocks_are_searched_in_order() {
            let hits = find_matches(&blocks(&["zero", "one and one", "two"]), "one");
            assert_eq!(hits.len(), 2);
            assert_eq!(hits[0].block, 1);
            assert_eq!(hits[1].block, 1);
            assert_eq!(hits[1].start, 8);
        }
    }

    mod rendered {
        use super::super::find_matches;
        use crate::justify::search_text;

        #[test]
        fn soft_wrapped_paragraph_matches_its_rendered_line() {
            let block = search_text("tables and mermaid\ndiagrams render as one run");
            assert_eq!(block, "tables and mermaid diagrams render as one run");
            let hits = find_matches(&[block], "mermaid diagrams");
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].start, 11);
        }

        #[test]
        fn emphasis_and_link_markup_match_visible_text_only() {
            let block = search_text("a **bold** word and [label](https://x/y)");
            assert_eq!(block, "a bold word and label");
            assert_eq!(find_matches(std::slice::from_ref(&block), "bold").len(), 1);
            assert_eq!(find_matches(&[block], "label").len(), 1);
            // The Markdown markers themselves are not rendered, so not searchable.
            assert!(find_matches(&[search_text("**bold**")], "**bold**").is_empty());
        }

        #[test]
        fn code_text_is_matched_literally() {
            let code = "let marker = \"**bold**\";\nrun `x`;".to_string();
            assert_eq!(
                find_matches(std::slice::from_ref(&code), "**bold**").len(),
                1
            );
            let hits = find_matches(&[code], "`x`");
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].start, 29);
        }
    }

    mod stepping {
        use super::super::step;

        #[test]
        fn wraps_in_both_directions() {
            assert_eq!(step(0, 3, 1), 1);
            assert_eq!(step(2, 3, 1), 0);
            assert_eq!(step(0, 3, -1), 2);
            assert_eq!(step(2, 3, -1), 1);
            assert_eq!(step(1, 1, 1), 0);
            assert_eq!(step(1, 1, -1), 0);
        }

        #[test]
        fn zero_length_stays_at_zero() {
            assert_eq!(step(0, 0, 1), 0);
            assert_eq!(step(7, 0, -1), 0);
        }
    }

    mod state {
        use super::super::SearchState;

        fn blocks(texts: &[&str]) -> Vec<String> {
            texts.iter().map(|s| (*s).to_string()).collect()
        }

        #[test]
        fn status_is_one_based_and_counts_every_hit() {
            let mut s = SearchState::default();
            s.set_pattern(&blocks(&["mermaid", "mermaid", "x"]), "mermaid");
            assert_eq!(s.status_text().as_deref(), Some("/mermaid [1/2]"));
            s.next();
            assert_eq!(s.status_text().as_deref(), Some("/mermaid [2/2]"));
            s.next();
            assert_eq!(s.status_text().as_deref(), Some("/mermaid [1/2]"));
            s.prev();
            assert_eq!(s.status_text().as_deref(), Some("/mermaid [2/2]"));
            assert_eq!(s.current_block(), Some(1));
        }

        #[test]
        fn empty_pattern_has_no_status() {
            let mut s = SearchState::default();
            s.set_pattern(&blocks(&["anything"]), "");
            assert_eq!(s.status_text(), None);
            assert!(s.hits.is_empty());
            s.next();
            assert_eq!(s.current_match, 0);
        }

        #[test]
        fn no_hits_reports_zero_of_zero() {
            let mut s = SearchState::default();
            s.set_pattern(&blocks(&["anything"]), "missing");
            assert_eq!(s.status_text().as_deref(), Some("/missing [0/0]"));
            assert_eq!(s.current_block(), None);
        }

        #[test]
        fn clear_drops_pattern_and_hits() {
            let mut s = SearchState::default();
            s.set_pattern(&blocks(&["mermaid"]), "mermaid");
            s.clear();
            assert!(s.pattern.is_none());
            assert!(s.hits.is_empty());
            assert_eq!(s.status_text(), None);
        }

        #[test]
        fn block_highlights_are_local_and_mark_the_current_hit() {
            let mut s = SearchState::default();
            s.set_pattern(&blocks(&["mermaid mermaid", "other mermaid"]), "mermaid");
            // Three hits: blocks 0, 0, 1; current starts at 0.
            let (ranges, current) = s.block_highlights(0);
            assert_eq!(ranges, vec![(0, 7), (8, 15)]);
            assert_eq!(current, Some(0));
            let (ranges, current) = s.block_highlights(1);
            assert_eq!(ranges, vec![(6, 13)]);
            assert_eq!(current, None);
            s.next();
            s.next();
            assert_eq!(s.current_block(), Some(1));
            let (_, current) = s.block_highlights(0);
            assert_eq!(current, None);
            let (_, current) = s.block_highlights(1);
            assert_eq!(current, Some(0));
        }
    }
}
