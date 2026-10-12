//! Text segmentation in UTF-16 units, the unit of OpenRich positions.
//!
//! The same code runs on every platform, so caret movement, deletion and
//! word selection agree everywhere instead of depending on each platform's
//! segmentation tables.

use std::ops::Range;

use openrich_model::Node;
use unicode_segmentation::UnicodeSegmentation;

/// Which side of a boundary a position belongs to when it falls inside a
/// grapheme or at a line wrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Affinity {
    /// Belongs to the preceding character (end of the previous line).
    Upstream,
    /// Belongs to the following character (start of the next line).
    #[default]
    Downstream,
}

/// The text of a textblock with the same offsets as its content
/// positions: text as is, every inline leaf as one U+FFFC.
pub fn textblock_text(node: &Node) -> String {
    let mut text = String::new();
    for child in node.children() {
        match child.text() {
            Some(value) => text.push_str(value),
            None => {
                // Inline nodes with content (rare) still take nodeSize positions.
                for _ in 0..child.node_size() {
                    text.push('\u{fffc}');
                }
            }
        }
    }
    text
}

/// Grapheme cluster boundaries of a text, as UTF-16 offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextIndexMap {
    /// Ascending, starting at 0 and ending at the UTF-16 length.
    boundaries: Vec<usize>,
}

impl TextIndexMap {
    pub fn from_text(text: &str) -> Self {
        let mut boundaries = Vec::with_capacity(text.len() + 1);
        let mut offset = 0;
        boundaries.push(0);
        for grapheme in text.graphemes(true) {
            offset += grapheme.encode_utf16().count();
            boundaries.push(offset);
        }
        Self { boundaries }
    }

    pub fn from_textblock(node: &Node) -> Self {
        Self::from_text(&textblock_text(node))
    }

    pub fn grapheme_len(&self) -> usize {
        self.boundaries.len() - 1
    }

    pub fn utf16_len(&self) -> usize {
        *self.boundaries.last().unwrap_or(&0)
    }

    pub fn boundaries(&self) -> &[usize] {
        &self.boundaries
    }

    pub fn is_boundary(&self, offset: usize) -> bool {
        self.boundaries.binary_search(&offset).is_ok()
    }

    /// The closest boundary at or around `offset`, by affinity.
    pub fn snap(&self, offset: usize, affinity: Affinity) -> usize {
        let offset = offset.min(self.utf16_len());
        match self.boundaries.binary_search(&offset) {
            Ok(_) => offset,
            Err(next) => match affinity {
                Affinity::Upstream => self.boundaries[next - 1],
                Affinity::Downstream => self.boundaries[next],
            },
        }
    }

    /// The boundary before `offset` (one grapheme back).
    pub fn prev_boundary(&self, offset: usize) -> Option<usize> {
        let index = match self.boundaries.binary_search(&offset) {
            Ok(index) => index,
            Err(next) => next,
        };
        index.checked_sub(1).map(|i| self.boundaries[i])
    }

    /// The boundary after `offset` (one grapheme forward).
    pub fn next_boundary(&self, offset: usize) -> Option<usize> {
        let index = match self.boundaries.binary_search(&offset) {
            Ok(index) => index + 1,
            Err(next) => next,
        };
        self.boundaries.get(index).copied()
    }

    /// UTF-16 offset of grapheme index `grapheme`.
    pub fn to_utf16(&self, grapheme: usize) -> Option<usize> {
        self.boundaries.get(grapheme).copied()
    }

    /// Grapheme index at a UTF-16 offset, snapping inside clusters.
    pub fn from_utf16(&self, offset: usize, affinity: Affinity) -> Option<usize> {
        if offset > self.utf16_len() {
            return None;
        }
        Some(match self.boundaries.binary_search(&offset) {
            Ok(index) => index,
            Err(next) => match affinity {
                Affinity::Upstream => next - 1,
                Affinity::Downstream => next,
            },
        })
    }

    /// Expands a UTF-16 range to whole grapheme clusters.
    pub fn expand(&self, range: Range<usize>) -> Range<usize> {
        if range.is_empty() {
            let caret = self.snap(range.start, Affinity::Downstream);
            return caret..caret;
        }
        self.snap(range.start, Affinity::Upstream)..self.snap(range.end, Affinity::Downstream)
    }
}

fn is_word(segment: &str) -> bool {
    segment.chars().any(char::is_alphanumeric)
}

/// UTF-16 ranges of the word-boundary segments of `text`.
fn segments(text: &str) -> Vec<(Range<usize>, bool)> {
    let mut result = Vec::new();
    let mut offset = 0;
    for segment in text.split_word_bounds() {
        let len = segment.encode_utf16().count();
        result.push((offset..offset + len, is_word(segment)));
        offset += len;
    }
    result
}

/// The word (or run of spaces or punctuation) around `offset`, for
/// double-click selection. At a boundary between a word and something
/// else, the word wins.
pub fn word_at(text: &str, offset: usize) -> Range<usize> {
    let segments = segments(text);
    let containing: Vec<&(Range<usize>, bool)> = segments
        .iter()
        .filter(|(range, _)| range.start <= offset && offset <= range.end)
        .collect();
    containing
        .iter()
        .find(|(range, word)| *word && range.contains(&offset))
        .or_else(|| containing.iter().find(|(_, word)| *word))
        .or_else(|| containing.iter().find(|(range, _)| range.contains(&offset)))
        .or(containing.first())
        .map_or(offset..offset, |(range, _)| range.clone())
}

/// Where Ctrl/Alt+Left moves: the start of the word before `offset`.
pub fn prev_word_start(text: &str, offset: usize) -> usize {
    segments(text)
        .iter()
        .rev()
        .find(|(range, word)| *word && range.start < offset)
        .map_or(0, |(range, _)| range.start)
}

/// Where Ctrl/Alt+Right moves: the end of the word after `offset`.
pub fn next_word_end(text: &str, offset: usize) -> usize {
    let len = text.encode_utf16().count();
    segments(text)
        .iter()
        .find(|(range, word)| *word && range.end > offset)
        .map_or(len, |(range, _)| range.end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrich_model::testing::{br, doc, img, p};
    use openrich_model::v;

    #[test]
    fn maps_graphemes_to_utf16_and_snaps_inside_clusters() {
        let map = TextIndexMap::from_text("A🙂e\u{301}👩‍💻");
        assert_eq!(map.grapheme_len(), 4);
        assert_eq!(map.utf16_len(), 10);
        assert_eq!(map.boundaries(), &[0, 1, 3, 5, 10]);
        assert_eq!(map.snap(2, Affinity::Upstream), 1);
        assert_eq!(map.snap(2, Affinity::Downstream), 3);
        assert_eq!(map.prev_boundary(10), Some(5));
        assert_eq!(map.prev_boundary(7), Some(5));
        assert_eq!(map.next_boundary(5), Some(10));
        assert_eq!(map.next_boundary(10), None);
        assert_eq!(map.from_utf16(4, Affinity::Upstream), Some(2));
        assert_eq!(map.from_utf16(4, Affinity::Downstream), Some(3));
        assert_eq!(map.from_utf16(11, Affinity::Downstream), None);
        assert_eq!(map.expand(4..6), 3..10);
    }

    #[test]
    fn textblock_offsets_match_document_positions() {
        let d = doc(v![p(v!["a", img(), "b🙂", br(), "c"])]).node;
        let paragraph = d.child(0);
        let text = textblock_text(paragraph);
        assert_eq!(text.encode_utf16().count(), paragraph.content().size());
        assert_eq!(text, "a\u{fffc}b🙂\u{fffc}c");
    }

    #[test]
    fn words_for_selection_and_movement() {
        let text = "Halo, dunia! 👋 ok";
        assert_eq!(word_at(text, 2), 0..4);
        assert_eq!(word_at(text, 4), 0..4, "the word wins at its end");
        assert_eq!(word_at(text, 6), 6..11);
        assert_eq!(next_word_end(text, 0), 4);
        assert_eq!(next_word_end(text, 4), 11);
        assert_eq!(prev_word_start(text, 11), 6);
        assert_eq!(prev_word_start(text, 6), 0);
        assert_eq!(next_word_end(text, 11), text.encode_utf16().count());
        let thai = "สวัสดีครับ";
        assert!(word_at(thai, 1).len() < thai.encode_utf16().count() + 1);
    }
}
