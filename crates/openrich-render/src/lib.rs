//! Renderer-independent geometry and scene contract for OpenRich.
//!
//! Concrete backends such as Skia will live in separate crates and consume
//! this data. This crate must not depend on a windowing toolkit.

use openrich_document::{Affinity, Block, Document, NodeId, Position, Selection};
use openrich_layout::{LayoutBackend, ParagraphLayout, Point, Size, TextHit, TextIndexError};

/// Logical pixel rectangle shared with layout backends.
pub use openrich_layout::Rect;

/// Draws a layout produced by the backend into a host-owned target.
pub trait Renderer: LayoutBackend {
    type Target;

    fn draw(&self, layout: &Self::Layout, target: &Self::Target, origin: Point);
}

/// A positioned text node in a complete document layout.
pub struct DocumentEntry<L> {
    pub node_id: NodeId,
    /// Index of the top-level document block containing this entry.
    pub top_level_index: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub depth: usize,
    pub marker: Option<String>,
    pub kind: DocumentEntryKind,
    pub layout: Option<L>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentEntryKind {
    Paragraph,
    Heading,
    HorizontalRule,
}

/// Vertical layout for all text-bearing nodes, including nested lists and quotes.
pub struct DocumentLayout<L> {
    pub entries: Vec<DocumentEntry<L>>,
    pub size: Size,
}

impl<L: ParagraphLayout> DocumentLayout<L> {
    pub fn new<B>(
        document: &Document,
        backend: &B,
        width: f32,
        font_size: f32,
    ) -> Result<Self, B::Error>
    where
        B: LayoutBackend<Layout = L>,
        L: ParagraphLayout,
    {
        let mut builder = DocumentLayoutBuilder {
            backend,
            entries: Vec::new(),
            width,
            font_size,
            y: 0.0,
            max_width: 0.0,
        };
        for (top_level_index, block) in document.blocks.iter().enumerate() {
            builder.layout_block(block, top_level_index, 0, 0.0, None)?;
        }
        Ok(Self {
            entries: builder.entries,
            size: Size {
                width: builder.max_width.min(width),
                height: builder.y,
            },
        })
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<DocumentHit> {
        self.entries.iter().find_map(|entry| {
            let layout = entry.layout.as_ref()?;
            if y < entry.y || y > entry.y + entry.height || x < entry.x || x > entry.x + entry.width
            {
                return None;
            }
            let hit = layout.hit_test(x - entry.x, y - entry.y)?;
            Some(DocumentHit {
                node_id: entry.node_id.clone(),
                top_level_index: entry.top_level_index,
                hit,
            })
        })
    }

    /// Pointer drags can leave the glyph bounds or pass through block spacing.
    /// Snap to the nearest editable top-level text block, then let the backend
    /// resolve its visual line, bidi run, and grapheme boundary.
    pub fn hit_test_nearest(&self, x: f32, y: f32) -> Option<DocumentHit> {
        let entry = self
            .entries
            .iter()
            .filter(|entry| entry.depth == 0 && entry.layout.is_some())
            .min_by(|a, b| {
                let distance = |entry: &DocumentEntry<L>| {
                    (entry.y - y).max(0.0).max(y - entry.y - entry.height)
                };
                distance(a).total_cmp(&distance(b))
            })?;
        let layout = entry.layout.as_ref()?;
        let hit = layout.hit_test(
            x - entry.x,
            (y - entry.y).clamp(0.0, (entry.height - 0.01).max(0.0)),
        )?;
        Some(DocumentHit {
            node_id: entry.node_id.clone(),
            top_level_index: entry.top_level_index,
            hit,
        })
    }

    pub fn selection_rects(&self, selection: &Selection) -> Result<Vec<Rect>, TextIndexError> {
        let (start, end) = ordered_positions(selection);
        let mut output = Vec::new();
        for entry in &self.entries {
            let Some(layout) = &entry.layout else {
                continue;
            };
            let index = entry.top_level_index;
            if index < start.block_index || index > end.block_index {
                continue;
            }
            let from = if index == start.block_index {
                start.offset
            } else {
                0
            };
            let to = if index == end.block_index {
                end.offset
            } else {
                layout.text_len()
            };
            for rect in layout.selection_rects(from..to)? {
                output.push(Rect {
                    x: entry.x + rect.x,
                    y: entry.y + rect.y,
                    width: rect.width,
                    height: rect.height,
                });
            }
        }
        Ok(output)
    }

    pub fn caret_rect(&self, position: &Position) -> Result<Option<Rect>, TextIndexError> {
        let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.top_level_index == position.block_index && entry.layout.is_some())
        else {
            return Ok(None);
        };
        Ok(entry
            .layout
            .as_ref()
            .unwrap()
            .caret_rect(position.offset)?
            .map(|rect| Rect {
                x: entry.x + rect.x,
                y: entry.y + rect.y,
                width: rect.width,
                height: rect.height,
            }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentHit {
    pub node_id: NodeId,
    pub top_level_index: usize,
    pub hit: TextHit,
}

struct DocumentLayoutBuilder<'a, B: LayoutBackend> {
    backend: &'a B,
    entries: Vec<DocumentEntry<B::Layout>>,
    width: f32,
    font_size: f32,
    y: f32,
    max_width: f32,
}

impl<B: LayoutBackend> DocumentLayoutBuilder<'_, B>
where
    B::Layout: ParagraphLayout,
{
    fn layout_block(
        &mut self,
        block: &Block,
        top_level_index: usize,
        depth: usize,
        x: f32,
        marker: Option<String>,
    ) -> Result<(), B::Error> {
        let indent = self.font_size * 1.5 * depth as f32;
        match block {
            Block::Formatted { id, .. }
            | Block::Paragraph { id, .. }
            | Block::Heading { id, .. } => {
                let (indent, marker) = if let Block::Formatted { format, .. } = block {
                    use openrich_document::ListStyle;
                    let marker = match format.list {
                        ListStyle::None => None,
                        ListStyle::Bullet => Some("•".to_owned()),
                        ListStyle::Checkbox => {
                            Some(if format.checked { "☑" } else { "☐" }.to_owned())
                        }
                        ListStyle::Ordered => {
                            let count = self
                                .entries
                                .iter()
                                .rev()
                                .take_while(|entry| {
                                    entry.marker.as_ref().is_some_and(|m| m.ends_with('.'))
                                })
                                .count();
                            Some(format!("{}.", count + 1))
                        }
                    };
                    (
                        indent
                            + if marker.is_some() {
                                self.font_size * 2.0
                            } else {
                                0.0
                            },
                        marker,
                    )
                } else {
                    (indent, marker)
                };
                let available_width = (self.width - x - indent).max(1.0);
                let layout = self
                    .backend
                    .layout_block(block, available_width, self.font_size)?;
                let size = layout.size();
                let height = size.height.max(self.font_size * 1.25);
                self.entries.push(DocumentEntry {
                    node_id: id.clone(),
                    top_level_index,
                    x: x + indent,
                    y: self.y,
                    width: available_width,
                    height,
                    depth,
                    marker,
                    kind: if matches!(block, Block::Heading { .. }) {
                        DocumentEntryKind::Heading
                    } else {
                        DocumentEntryKind::Paragraph
                    },
                    layout: Some(layout),
                });
                self.max_width = self.max_width.max(x + indent + size.width);
                self.y += height + self.font_size * 0.5;
            }
            Block::BulletList { items, .. } | Block::OrderedList { items, .. } => {
                let ordered = matches!(block, Block::OrderedList { .. });
                let start = match block {
                    Block::OrderedList { start, .. } => *start,
                    _ => 1,
                };
                for (item_index, item) in items.iter().enumerate() {
                    for (child_index, child) in item.blocks.iter().enumerate() {
                        let marker = (child_index == 0).then(|| {
                            if ordered {
                                format!("{}. ", start.saturating_add(item_index as u32))
                            } else {
                                "• ".to_owned()
                            }
                        });
                        self.layout_block(child, top_level_index, depth + 1, x, marker)?;
                    }
                }
            }
            Block::Blockquote { blocks, .. } => {
                for child in blocks {
                    self.layout_block(child, top_level_index, depth + 1, x, None)?;
                }
            }
            Block::HorizontalRule { id } => {
                let height = self.font_size * 1.25;
                self.entries.push(DocumentEntry {
                    node_id: id.clone(),
                    top_level_index,
                    x: x + indent,
                    y: self.y,
                    width: (self.width - x - indent).max(1.0),
                    height,
                    depth,
                    marker: None,
                    kind: DocumentEntryKind::HorizontalRule,
                    layout: None,
                });
                self.max_width = self.width;
                self.y += height + self.font_size * 0.5;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DocumentHitPosition {
    pub block_index: usize,
    pub offset: usize,
    pub affinity: Affinity,
}

impl DocumentHit {
    pub fn position(&self) -> DocumentHitPosition {
        DocumentHitPosition {
            block_index: self.top_level_index,
            offset: self.hit.grapheme_offset,
            affinity: self.hit.affinity,
        }
    }
}

fn ordered_positions(selection: &Selection) -> (Position, Position) {
    let a = &selection.anchor;
    let b = &selection.focus;
    if (a.block_index, a.offset) <= (b.block_index, b.offset) {
        (a.clone(), b.clone())
    } else {
        (b.clone(), a.clone())
    }
}

/// One operation in a renderer-neutral display list.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    Clear {
        color: Color,
    },
    FillRect {
        rect: Rect,
        color: Color,
    },
    DrawText {
        text: String,
        origin_x: f32,
        baseline_y: f32,
        color: Color,
        size: f32,
    },
}

/// RGBA color with channels normalized to 0.0–1.0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

/// A frame of renderer-neutral commands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayList {
    pub commands: Vec<DrawCommand>,
}
