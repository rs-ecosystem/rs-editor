//! Text-index and geometry contracts shared by layout backends.
//!
//! The editor stores caret offsets in extended grapheme clusters. Text engines
//! may expose offsets in another unit, so adapters must convert explicitly.

use openrich_document::{Affinity, Block};
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

/// Maps user-visible grapheme offsets to UTF-16 code-unit offsets.
///
/// Skia Paragraph and many platform text APIs use UTF-16 positions. This map
/// keeps that boundary out of the document and editor model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextIndexMap {
    grapheme_to_utf16: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextHit {
    pub grapheme_offset: usize,
    pub affinity: Affinity,
}

/// Geometry and hit-testing exposed by one laid-out paragraph.
pub trait ParagraphLayout {
    fn size(&self) -> Size;
    fn line_count(&self) -> usize;
    fn hit_test(&self, x: f32, y: f32) -> Option<TextHit>;
    fn selection_rects(
        &self,
        grapheme_range: std::ops::Range<usize>,
    ) -> Result<Vec<Rect>, TextIndexError>;

    fn caret_rect(&self, grapheme_offset: usize) -> Result<Option<Rect>, TextIndexError> {
        let length = self.text_len();
        if grapheme_offset > length {
            return Err(TextIndexError::GraphemeRangeOutOfBounds {
                start: grapheme_offset,
                end: grapheme_offset,
                length,
            });
        }
        if grapheme_offset < length {
            let Some(rect) = self
                .selection_rects(grapheme_offset..grapheme_offset + 1)?
                .first()
                .copied()
            else {
                return Ok(None);
            };
            return Ok(Some(Rect {
                x: rect.x,
                y: rect.y,
                width: 1.0,
                height: rect.height,
            }));
        }
        if grapheme_offset > 0 {
            let Some(rect) = self
                .selection_rects(grapheme_offset - 1..grapheme_offset)?
                .last()
                .copied()
            else {
                return Ok(None);
            };
            return Ok(Some(Rect {
                x: rect.x + rect.width,
                y: rect.y,
                width: 1.0,
                height: rect.height,
            }));
        }
        let size = self.size();
        Ok(Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: size.height,
        }))
    }

    fn text_len(&self) -> usize;
}

/// Creates paragraph layouts from document blocks.
pub trait LayoutBackend {
    type Layout: ParagraphLayout;
    type Error;

    fn layout_block(
        &self,
        block: &Block,
        width: f32,
        font_size: f32,
    ) -> Result<Self::Layout, Self::Error>;
}

impl TextIndexMap {
    pub fn from_text(text: &str) -> Self {
        let mut grapheme_to_utf16 = Vec::new();
        let mut utf16_offset = 0;
        grapheme_to_utf16.push(utf16_offset);
        for grapheme in text.graphemes(true) {
            utf16_offset += grapheme.encode_utf16().count();
            grapheme_to_utf16.push(utf16_offset);
        }
        Self { grapheme_to_utf16 }
    }

    /// Builds a map from the concatenated inline text of a paragraph or heading.
    pub fn from_block(block: &Block) -> Option<Self> {
        let content = block.inline_content()?;
        let text = content
            .iter()
            .map(|inline| match inline {
                openrich_document::Inline::Text { text, .. } => text.as_str(),
            })
            .fold(String::new(), |mut combined, part| {
                combined.push_str(part);
                combined
            });
        Some(Self::from_text(&text))
    }

    pub fn grapheme_len(&self) -> usize {
        self.grapheme_to_utf16.len() - 1
    }

    pub fn utf16_len(&self) -> usize {
        *self.grapheme_to_utf16.last().unwrap_or(&0)
    }

    pub fn grapheme_boundaries_in_utf16(&self) -> &[usize] {
        &self.grapheme_to_utf16
    }

    pub fn to_utf16(&self, grapheme_offset: usize) -> Option<usize> {
        self.grapheme_to_utf16.get(grapheme_offset).copied()
    }

    /// Maps a UTF-16 offset to a grapheme boundary.
    ///
    /// If the supplied offset falls inside a grapheme (including a surrogate
    /// pair), affinity chooses the preceding or following boundary.
    pub fn from_utf16(&self, utf16_offset: usize, affinity: Affinity) -> Option<usize> {
        if utf16_offset > self.utf16_len() {
            return None;
        }
        match self.grapheme_to_utf16.binary_search(&utf16_offset) {
            Ok(grapheme_offset) => Some(grapheme_offset),
            Err(next_boundary) => match affinity {
                Affinity::Upstream => Some(next_boundary.saturating_sub(1)),
                Affinity::Downstream => Some(next_boundary),
            },
        }
    }

    pub fn range_to_utf16(
        &self,
        grapheme_range: std::ops::Range<usize>,
    ) -> Result<std::ops::Range<usize>, TextIndexError> {
        if grapheme_range.start > grapheme_range.end || grapheme_range.end > self.grapheme_len() {
            return Err(TextIndexError::GraphemeRangeOutOfBounds {
                start: grapheme_range.start,
                end: grapheme_range.end,
                length: self.grapheme_len(),
            });
        }
        Ok(
            self.grapheme_to_utf16[grapheme_range.start]
                ..self.grapheme_to_utf16[grapheme_range.end],
        )
    }

    /// Maps a UTF-16 range to whole grapheme clusters.
    pub fn range_from_utf16(
        &self,
        utf16_range: std::ops::Range<usize>,
    ) -> Result<std::ops::Range<usize>, TextIndexError> {
        if utf16_range.start > utf16_range.end || utf16_range.end > self.utf16_len() {
            return Err(TextIndexError::Utf16RangeOutOfBounds {
                start: utf16_range.start,
                end: utf16_range.end,
                length: self.utf16_len(),
            });
        }
        if utf16_range.is_empty() {
            let caret = self
                .from_utf16(utf16_range.start, Affinity::Downstream)
                .ok_or(TextIndexError::OffsetOutOfBounds(utf16_range.start))?;
            return Ok(caret..caret);
        }
        let start = self
            .from_utf16(utf16_range.start, Affinity::Upstream)
            .ok_or(TextIndexError::OffsetOutOfBounds(utf16_range.start))?;
        let end = self
            .from_utf16(utf16_range.end, Affinity::Downstream)
            .ok_or(TextIndexError::OffsetOutOfBounds(utf16_range.end))?;
        Ok(start..end)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TextIndexError {
    #[error("grapheme range {start}..{end} exceeds text length {length}")]
    GraphemeRangeOutOfBounds {
        start: usize,
        end: usize,
        length: usize,
    },
    #[error("UTF-16 range {start}..{end} exceeds text length {length}")]
    Utf16RangeOutOfBounds {
        start: usize,
        end: usize,
        length: usize,
    },
    #[error("UTF-16 offset {0} is outside the text")]
    OffsetOutOfBounds(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_graphemes_to_utf16_and_snaps_inside_clusters_by_affinity() {
        let map = TextIndexMap::from_text("A🙂e\u{301}👩‍💻");

        assert_eq!(map.grapheme_len(), 4);
        assert_eq!(map.utf16_len(), 10);
        assert_eq!(map.grapheme_boundaries_in_utf16(), &[0, 1, 3, 5, 10]);
        assert_eq!(map.to_utf16(2), Some(3));
        assert_eq!(map.from_utf16(2, Affinity::Upstream), Some(1));
        assert_eq!(map.from_utf16(2, Affinity::Downstream), Some(2));
        assert_eq!(map.from_utf16(4, Affinity::Upstream), Some(2));
        assert_eq!(map.from_utf16(4, Affinity::Downstream), Some(3));
        assert_eq!(map.from_utf16(11, Affinity::Downstream), None);
    }

    #[test]
    fn converts_ranges_to_whole_grapheme_clusters() {
        let map = TextIndexMap::from_text("A🙂e\u{301}👩‍💻");

        assert_eq!(map.range_to_utf16(1..3).unwrap(), 1..5);
        assert_eq!(map.range_from_utf16(4..5).unwrap(), 2..3);
        assert_eq!(map.range_from_utf16(2..2).unwrap(), 2..2);
        assert!(matches!(
            map.range_to_utf16(0..5),
            Err(TextIndexError::GraphemeRangeOutOfBounds { .. })
        ));
    }
}
