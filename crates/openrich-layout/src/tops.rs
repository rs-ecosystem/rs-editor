//! Places of the top-level blocks, kept in chunks so that changing one
//! block rebuilds one chunk instead of the whole list, and so that a
//! layout snapshot shares unchanged chunks with the previous one.

use std::ops::Range;
use std::rc::Rc;

/// Blocks per chunk.
pub(crate) const CHUNK: usize = 64;

/// What placing a block needs to know.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Slot {
    pub size: usize,
    /// Space above the block.
    pub gap: f32,
    pub height: f32,
    pub measured: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Chunk {
    slots: Rc<[Slot]>,
    size: usize,
    extent: f32,
}

impl Chunk {
    pub(crate) fn new(slots: Vec<Slot>) -> Self {
        let size = slots.iter().map(|s| s.size).sum();
        let extent = slots.iter().map(|s| s.gap + s.height).sum();
        Self {
            slots: Rc::from(slots),
            size,
            extent,
        }
    }
}

/// A top-level block: where it is, and whether its height is measured or
/// still an estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TopBlock {
    /// Position before the block.
    pub pos: usize,
    /// The block's node size.
    pub size: usize,
    pub y: f32,
    pub height: f32,
    pub measured: bool,
}

impl TopBlock {
    pub fn end(&self) -> usize {
        self.pos + self.size
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// Every top-level block's place in the document.
#[derive(Debug, Clone)]
pub struct Tops {
    chunks: Rc<[Chunk]>,
    /// Position and y where each chunk starts (before its first gap).
    starts: Rc<[(usize, f32)]>,
    len: usize,
    height: f32,
}

impl Tops {
    pub(crate) fn new(chunks: &[Chunk]) -> Self {
        let mut starts = Vec::with_capacity(chunks.len());
        let (mut pos, mut y, mut len) = (0, 0.0, 0);
        for chunk in chunks {
            starts.push((pos, y));
            pos += chunk.size;
            y += chunk.extent;
            len += chunk.slots.len();
        }
        Self {
            chunks: Rc::from(chunks),
            starts: Rc::from(starts),
            len,
            height: y,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bottom of the last block.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// Blocks of chunk `k` with their places.
    fn chunk_blocks(&self, k: usize) -> impl Iterator<Item = (usize, TopBlock)> + '_ {
        let (mut pos, mut y) = self.starts[k];
        self.chunks[k]
            .slots
            .iter()
            .enumerate()
            .map(move |(j, slot)| {
                let top = TopBlock {
                    pos,
                    size: slot.size,
                    y: y + slot.gap,
                    height: slot.height,
                    measured: slot.measured,
                };
                pos += slot.size;
                y += slot.gap + slot.height;
                (k * CHUNK + j, top)
            })
    }

    pub fn get(&self, index: usize) -> Option<TopBlock> {
        if index >= self.len {
            return None;
        }
        self.chunk_blocks(index / CHUNK)
            .nth(index % CHUNK)
            .map(|(_, top)| top)
    }

    pub fn iter(&self) -> impl Iterator<Item = TopBlock> + '_ {
        (0..self.chunks.len()).flat_map(move |k| self.chunk_blocks(k).map(|(_, top)| top))
    }

    /// The block containing `pos` (the last one at the document's end).
    pub fn at_pos(&self, pos: usize) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let k = self
            .starts
            .partition_point(|&(start, _)| start <= pos)
            .saturating_sub(1);
        Some(
            self.chunk_blocks(k)
                .find(|(_, top)| pos < top.end())
                .map_or(self.len - 1, |(index, _)| index),
        )
    }

    /// The first block reaching down to `y` or below (the last one past
    /// the end).
    pub fn at_y(&self, y: f32) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let k = self
            .starts
            .partition_point(|&(_, start)| start < y)
            .saturating_sub(1);
        Some(
            (k..self.chunks.len())
                .flat_map(|k| self.chunk_blocks(k))
                .find(|(_, top)| top.bottom() >= y)
                .map_or(self.len - 1, |(index, _)| index),
        )
    }

    /// Blocks intersecting `from..=to`.
    pub fn range_y(&self, from: f32, to: f32) -> Range<usize> {
        let Some(start) = self.at_y(from) else {
            return 0..0;
        };
        let k = start / CHUNK;
        let end = (k..self.chunks.len())
            .flat_map(|k| self.chunk_blocks(k))
            .skip(start % CHUNK)
            .find(|(_, top)| top.y > to)
            .map_or(self.len, |(index, _)| index);
        start..end.max(start)
    }
}
