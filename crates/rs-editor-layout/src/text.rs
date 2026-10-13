//! One laid-out textblock: SkParagraph plus geometry queries in UTF-16
//! offsets that equal the block's content positions. Emoji drawn by the
//! host are one placeholder each in the paragraph, so paragraph offsets
//! are mapped to and from content offsets around them.

use std::fmt;
use std::ops::Range;

use rs_editor_text::{Affinity, TextIndexMap};
use skia_safe::textlayout::{
    Affinity as SkAffinity, Paragraph, RectHeightStyle, RectWidthStyle, TextAlign, TextDirection,
};
use skia_safe::{Canvas, Point, SamplingOptions};

use crate::emoji::SharedEmoji;
use crate::geometry::Rect;

/// A grapheme of the text (an emoji) shown as one placeholder in the
/// paragraph: `len` UTF-16 units at content offset `doc` are paragraph
/// offset `para`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Squeeze {
    pub doc: usize,
    pub len: usize,
    pub para: usize,
}

/// What each placeholder of the paragraph stands for, in order: an inline
/// node (`None`) or an emoji drawn from an image.
pub(crate) type Placeholders = Vec<Option<String>>;

/// A caret position: a vertical bar at `x` from `y` to `y + height`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caret {
    pub x: f32,
    pub y: f32,
    pub height: f32,
}

/// One visual line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    /// First offset of the line.
    pub start: usize,
    /// End offset, excluding a trailing hard break.
    pub end: usize,
    /// End offset including the hard break; the next line starts here.
    pub end_with_break: usize,
    pub top: f32,
    pub height: f32,
    pub baseline: f32,
    pub left: f32,
    pub width: f32,
}

/// A caret position the arrow keys can move to, ordered by `x` on a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretStop {
    pub x: f32,
    pub offset: usize,
    pub affinity: Affinity,
}

/// A laid-out textblock. Coordinates are relative to its top-left corner.
pub struct TextLayout {
    paragraph: Paragraph,
    text: String,
    index: TextIndexMap,
    lines: Vec<Line>,
    width: f32,
    align: TextAlign,
    /// Caret extent for an empty block: (top, height).
    empty_caret: (f32, f32),
    squeezes: Vec<Squeeze>,
    placeholders: Placeholders,
    emoji: Option<SharedEmoji>,
}

impl fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextLayout")
            .field("text", &self.text)
            .field("width", &self.width)
            .field("height", &self.height())
            .field("lines", &self.lines.len())
            .finish()
    }
}

impl TextLayout {
    /// Wraps an already laid out paragraph built from `text`.
    pub(crate) fn new(
        paragraph: Paragraph,
        text: String,
        width: f32,
        align: TextAlign,
        line_height: f32,
        caret_height: f32,
    ) -> Self {
        Self::with_emoji(
            paragraph,
            text,
            width,
            align,
            line_height,
            caret_height,
            Vec::new(),
            Vec::new(),
            None,
        )
    }

    /// Like [`TextLayout::new`], for a paragraph where the graphemes in
    /// `squeezes` are placeholders painted with images from `emoji`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn with_emoji(
        paragraph: Paragraph,
        text: String,
        width: f32,
        align: TextAlign,
        line_height: f32,
        caret_height: f32,
        squeezes: Vec<Squeeze>,
        placeholders: Placeholders,
        emoji: Option<SharedEmoji>,
    ) -> Self {
        let to_doc = |para: usize| doc_offset(&squeezes, para);
        let mut lines: Vec<Line> = paragraph
            .get_line_metrics()
            .iter()
            .map(|m| {
                let top = (m.baseline - m.ascent) as f32;
                Line {
                    start: to_doc(m.start_index),
                    end: to_doc(if m.hard_break {
                        m.end_index.min(m.end_including_newline)
                    } else {
                        m.end_index
                    }),
                    end_with_break: to_doc(m.end_including_newline),
                    top,
                    height: m.height as f32,
                    baseline: m.baseline as f32,
                    left: m.left as f32,
                    width: m.width as f32,
                }
            })
            .collect();
        let index = TextIndexMap::from_text(&text);
        if lines.is_empty() {
            lines.push(Line {
                start: 0,
                end: 0,
                end_with_break: 0,
                top: 0.0,
                height: line_height,
                baseline: line_height * 0.75,
                left: 0.0,
                width: 0.0,
            });
        }
        // A trailing hard break ends with an empty line Skia may omit.
        let len = index.utf16_len();
        if let Some(last) = lines.last().copied()
            && last.end_with_break == len
            && text.ends_with('\n')
            && last.start < len
        {
            lines.push(Line {
                start: len,
                end: len,
                end_with_break: len,
                top: last.top + last.height,
                height: last.height,
                baseline: last.baseline + last.height,
                left: 0.0,
                width: 0.0,
            });
        }
        let empty_top = (line_height - caret_height) / 2.0;
        Self {
            paragraph,
            text,
            index,
            lines,
            width,
            align,
            empty_caret: (empty_top.max(0.0), caret_height),
            squeezes,
            placeholders,
            emoji,
        }
    }

    /// The paragraph offset of content offset `doc` (inside a squeezed
    /// grapheme: its end).
    fn to_para(&self, doc: usize) -> usize {
        let mut shift = 0;
        for s in &self.squeezes {
            if doc <= s.doc {
                break;
            }
            if doc < s.doc + s.len {
                return s.para + 1;
            }
            shift += s.len - 1;
        }
        doc - shift
    }

    fn para_range(&self, range: Range<usize>) -> Range<usize> {
        self.to_para(range.start)..self.to_para(range.end)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Length in UTF-16 units (= the block's content size).
    pub fn len(&self) -> usize {
        self.index.utf16_len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn index(&self) -> &TextIndexMap {
        &self.index
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.lines
            .last()
            .map_or(0.0, |line| line.top + line.height)
            .max(self.paragraph.height())
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    pub fn paint(&self, canvas: &Canvas, x: f32, y: f32) {
        self.paragraph.paint(canvas, Point::new(x, y));
        let Some(emoji) = &self.emoji else {
            return;
        };
        let emoji = emoji.borrow();
        let rects = self.paragraph.get_rects_for_placeholders();
        for (placeholder, rect) in self.placeholders.iter().zip(rects.iter()) {
            let Some(image) = placeholder.as_ref().and_then(|g| emoji.images.get(g)) else {
                continue;
            };
            let rect = rect.rect.with_offset((x, y));
            canvas.draw_image_rect_with_sampling_options(
                image,
                None,
                rect,
                SamplingOptions::from(skia_safe::CubicResampler::mitchell()),
                &skia_safe::Paint::default(),
            );
        }
    }

    /// The line that shows `offset` with `affinity` (an upstream offset at
    /// a soft wrap belongs to the end of the earlier line).
    pub fn line_index(&self, offset: usize, affinity: Affinity) -> usize {
        let offset = offset.min(self.len());
        for (i, line) in self.lines.iter().enumerate() {
            let next_start = self.lines.get(i + 1).map_or(usize::MAX, |next| next.start);
            let wrap_end = affinity == Affinity::Upstream
                && offset == line.end
                && offset > line.start
                && line.end == next_start;
            if wrap_end || (offset >= line.start && offset < next_start) {
                return i;
            }
        }
        self.lines.len() - 1
    }

    fn edge_rect(&self, range: Range<usize>) -> Option<(skia_safe::Rect, TextDirection)> {
        self.paragraph
            .get_rects_for_range(
                self.para_range(range),
                RectHeightStyle::Tight,
                RectWidthStyle::Tight,
            )
            .into_iter()
            .map(|b| (b.rect, b.direct))
            .next()
    }

    fn aligned_empty_x(&self, line: &Line) -> f32 {
        match self.align {
            TextAlign::Center => self.width / 2.0,
            TextAlign::Right | TextAlign::End => self.width,
            _ => line.left,
        }
    }

    fn is_break(&self, offset: usize) -> bool {
        // Hard breaks are single UTF-16 units, so a char lookup suffices.
        let mut units = 0;
        for ch in self.text.chars() {
            if units == offset {
                return ch == '\n';
            }
            units += ch.len_utf16();
            if units > offset {
                return false;
            }
        }
        false
    }

    /// The caret at `offset`.
    pub fn caret(&self, offset: usize, affinity: Affinity) -> Caret {
        let offset = self.index.snap(offset, affinity);
        let line = self.lines[self.line_index(offset, affinity)];
        let fallback = |x: f32| {
            let (top, height) = self.empty_caret;
            Caret {
                x,
                y: line.top + top.min(line.height - height).max(0.0),
                height,
            }
        };
        if self.is_empty() || line.start == line.end {
            return fallback(self.aligned_empty_x(&line));
        }
        let at_line_end = offset >= line.end;
        let use_before = offset > line.start && (affinity == Affinity::Upstream || at_line_end);
        let edge = if use_before {
            let prev = self.index.prev_boundary(offset).unwrap_or(0);
            self.edge_rect(prev..offset).map(|(rect, dir)| {
                (
                    if dir == TextDirection::LTR {
                        rect.right
                    } else {
                        rect.left
                    },
                    rect,
                )
            })
        } else {
            let next = self.index.next_boundary(offset).unwrap_or(offset);
            if self.is_break(offset) {
                None
            } else {
                self.edge_rect(offset..next).map(|(rect, dir)| {
                    (
                        if dir == TextDirection::LTR {
                            rect.left
                        } else {
                            rect.right
                        },
                        rect,
                    )
                })
            }
        };
        match edge {
            Some((x, rect)) => {
                let height = (rect.bottom - rect.top).max(1.0);
                Caret {
                    x,
                    y: rect.top,
                    height,
                }
            }
            None => {
                // Before a hard break: the end of the previous glyph.
                let prev = self
                    .index
                    .prev_boundary(offset)
                    .filter(|p| *p >= line.start);
                match prev.and_then(|p| self.edge_rect(p..offset)) {
                    Some((rect, dir)) => Caret {
                        x: if dir == TextDirection::LTR {
                            rect.right
                        } else {
                            rect.left
                        },
                        y: rect.top,
                        height: (rect.bottom - rect.top).max(1.0),
                    },
                    None => fallback(self.aligned_empty_x(&line)),
                }
            }
        }
    }

    fn glyph_hit(&self, x: f32, y: f32) -> (usize, Affinity) {
        let hit = self
            .paragraph
            .get_glyph_position_at_coordinate(Point::new(x, y));
        let affinity = match hit.affinity {
            SkAffinity::Upstream => Affinity::Upstream,
            SkAffinity::Downstream => Affinity::Downstream,
        };
        let offset =
            doc_offset(&self.squeezes, usize::try_from(hit.position).unwrap_or(0)).min(self.len());
        (self.index.snap(offset, affinity), affinity)
    }

    /// The offset closest to a point. Inside a grapheme drawn with several
    /// glyphs (e.g. Devanagari conjuncts), the grapheme's midpoint decides.
    pub fn offset_at(&self, x: f32, y: f32) -> (usize, Affinity) {
        if self.is_empty() {
            return (0, Affinity::Downstream);
        }
        let line_index = self
            .lines
            .iter()
            .position(|line| y < line.top + line.height)
            .unwrap_or(self.lines.len() - 1);
        let line = self.lines[line_index];
        let y = (line.top + line.height / 2.0).clamp(0.0, self.height());
        let (offset, affinity) = self.glyph_hit(x, y);
        let candidates = [self.index.prev_boundary(offset), Some(offset)];
        for start in candidates.into_iter().flatten() {
            let Some(end) = self.index.next_boundary(start) else {
                continue;
            };
            if end > line.end_with_break.max(line.end) || start < line.start {
                continue;
            }
            if let Some((rect, dir)) = self.edge_rect(start..end)
                && x >= rect.left
                && x <= rect.right
            {
                let before = x < rect.center_x();
                let ltr = dir == TextDirection::LTR;
                return if before == ltr {
                    (start, Affinity::Downstream)
                } else {
                    (end, Affinity::Upstream)
                };
            }
        }
        // Past the end of a soft-wrapped line, stay on that line.
        if offset == line.end && line_index + 1 < self.lines.len() {
            return (offset, Affinity::Upstream);
        }
        (offset, affinity)
    }

    /// Highlight rectangles for `range`, covering the full line height.
    pub fn selection_rects(&self, range: Range<usize>) -> Vec<Rect> {
        if range.is_empty() {
            return Vec::new();
        }
        self.paragraph
            .get_rects_for_range(
                self.para_range(range),
                RectHeightStyle::Max,
                RectWidthStyle::Tight,
            )
            .into_iter()
            .map(|b| Rect::new(b.rect.left, b.rect.top, b.rect.width(), b.rect.height()))
            .filter(|r| r.width > 0.0)
            .collect()
    }

    /// Caret positions of line `line_index`, left to right.
    pub fn caret_stops(&self, line_index: usize) -> Vec<CaretStop> {
        let Some(line) = self.lines.get(line_index).copied() else {
            return Vec::new();
        };
        if line.start == line.end {
            return vec![CaretStop {
                x: self.aligned_empty_x(&line),
                offset: line.start,
                affinity: Affinity::Downstream,
            }];
        }
        let mut stops: Vec<CaretStop> = Vec::new();
        let mut offset = line.start;
        while offset < line.end {
            let Some(next) = self.index.next_boundary(offset) else {
                break;
            };
            if let Some((rect, dir)) = self.edge_rect(offset..next) {
                let (start_x, end_x) = if dir == TextDirection::LTR {
                    (rect.left, rect.right)
                } else {
                    (rect.right, rect.left)
                };
                let next_affinity = if next == line.end {
                    Affinity::Upstream
                } else {
                    Affinity::Downstream
                };
                stops.push(CaretStop {
                    x: start_x,
                    offset,
                    affinity: Affinity::Downstream,
                });
                stops.push(CaretStop {
                    x: end_x,
                    offset: next,
                    affinity: next_affinity,
                });
            }
            offset = next;
        }
        stops.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.offset.cmp(&b.offset)));
        // Two glyph edges at the same x are one stop; keep the logical order.
        stops.dedup_by(|b, a| (a.x - b.x).abs() < 0.5 && a.offset == b.offset);
        stops.dedup_by(|b, a| (a.x - b.x).abs() < 0.5);
        stops
    }

    /// The word (or run of other characters) around `offset`.
    pub fn word_at(&self, offset: usize) -> Range<usize> {
        rs_editor_text::word_at(&self.text, offset)
    }
}

/// The content offset of paragraph offset `para`.
fn doc_offset(squeezes: &[Squeeze], para: usize) -> usize {
    let mut shift = 0;
    for s in squeezes {
        if para <= s.para {
            break;
        }
        shift += s.len - 1;
    }
    para + shift
}
