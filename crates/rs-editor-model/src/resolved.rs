use std::fmt;

use crate::error::{ModelError, Result};
use crate::mark::MarkSet;
use crate::node::Node;
use crate::text::utf16_to_byte;

#[derive(Clone)]
struct PathEntry {
    node: Node,
    index: usize,
    /// Absolute position of the start of the child at `index`.
    offset: usize,
}

/// A position resolved against a document, giving access to its ancestors.
#[derive(Clone)]
pub struct ResolvedPos {
    pos: usize,
    path: Vec<PathEntry>,
    parent_offset: usize,
}

impl fmt::Debug for ResolvedPos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        for depth in 1..=self.depth() {
            if !out.is_empty() {
                out.push('/');
            }
            out.push_str(&format!(
                "{}_{}",
                self.node(depth).type_name(),
                self.index(depth - 1)
            ));
        }
        write!(f, "{out}:{}", self.parent_offset)
    }
}

impl ResolvedPos {
    pub(crate) fn resolve(doc: &Node, pos: usize) -> Result<Self> {
        let size = doc.content().size();
        if pos > size {
            return Err(ModelError::PositionOutOfRange { pos, size });
        }
        let mut path = Vec::new();
        let mut start = 0;
        let mut parent_offset = pos;
        let mut node = doc.clone();
        loop {
            let found = node.content().find_index(parent_offset, -1)?;
            let rem = parent_offset - found.offset;
            path.push(PathEntry {
                node: node.clone(),
                index: found.index,
                offset: start + found.offset,
            });
            if rem == 0 {
                break;
            }
            let child = node.child(found.index).clone();
            if let Some(text) = child.text() {
                if utf16_to_byte(text, rem).is_none() {
                    return Err(ModelError::PositionInsideCharacter(pos));
                }
                break;
            }
            parent_offset = rem - 1;
            start += found.offset + 1;
            node = child;
        }
        Ok(Self {
            pos,
            path,
            parent_offset,
        })
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Number of ancestors; 0 when the position is directly in the document.
    pub fn depth(&self) -> usize {
        self.path.len() - 1
    }

    /// Offset into the parent node's content.
    pub fn parent_offset(&self) -> usize {
        self.parent_offset
    }

    pub fn parent(&self) -> &Node {
        self.node(self.depth())
    }

    pub fn doc(&self) -> &Node {
        self.node(0)
    }

    /// The ancestor at `depth` (0 is the document).
    pub fn node(&self, depth: usize) -> &Node {
        &self.path[depth].node
    }

    /// Index into the ancestor at `depth`.
    pub fn index(&self, depth: usize) -> usize {
        self.path[depth].index
    }

    /// Index pointing after this position in the ancestor at `depth`.
    pub fn index_after(&self, depth: usize) -> usize {
        self.index(depth) + usize::from(!(depth == self.depth() && self.text_offset() == 0))
    }

    /// Start of the ancestor's content at `depth`.
    pub fn start(&self, depth: usize) -> usize {
        if depth == 0 {
            0
        } else {
            self.path[depth - 1].offset + 1
        }
    }

    /// End of the ancestor's content at `depth`.
    pub fn end(&self, depth: usize) -> usize {
        self.start(depth) + self.node(depth).content().size()
    }

    /// Position before the ancestor at `depth` (`depth > 0`).
    pub fn before(&self, depth: usize) -> usize {
        debug_assert!(depth > 0, "there is no position before the top-level node");
        if depth == self.depth() + 1 {
            self.pos
        } else {
            self.path[depth - 1].offset
        }
    }

    /// Position after the ancestor at `depth` (`depth > 0`).
    pub fn after(&self, depth: usize) -> usize {
        debug_assert!(depth > 0, "there is no position after the top-level node");
        if depth == self.depth() + 1 {
            self.pos
        } else {
            self.path[depth - 1].offset + self.path[depth].node.node_size()
        }
    }

    /// Offset into a text node when the position points inside one.
    pub fn text_offset(&self) -> usize {
        self.pos - self.path[self.path.len() - 1].offset
    }

    pub fn node_after(&self) -> Option<Node> {
        let parent = self.parent();
        let index = self.index(self.depth());
        if index == parent.child_count() {
            return None;
        }
        let child = parent.child(index);
        let offset = self.text_offset();
        Some(if offset > 0 {
            child.cut(offset, child.text_len())
        } else {
            child.clone()
        })
    }

    pub fn node_before(&self) -> Option<Node> {
        let index = self.index(self.depth());
        let offset = self.text_offset();
        if offset > 0 {
            return Some(self.parent().child(index).cut(0, offset));
        }
        (index > 0).then(|| self.parent().child(index - 1).clone())
    }

    /// Position of child `index` in the ancestor at `depth`.
    pub fn pos_at_index(&self, index: usize, depth: usize) -> usize {
        let node = self.node(depth);
        let mut pos = self.start(depth);
        for i in 0..index {
            pos += node.child(i).node_size();
        }
        pos
    }

    /// Marks that text typed at this position would get.
    pub fn marks(&self) -> MarkSet {
        let parent = self.parent();
        let index = self.index(self.depth());
        if parent.content().size() == 0 {
            return MarkSet::none();
        }
        if self.text_offset() > 0 {
            return parent.child(index).marks().clone();
        }
        let mut main = index.checked_sub(1).and_then(|i| parent.maybe_child(i));
        let mut other = parent.maybe_child(index);
        if main.is_none() {
            std::mem::swap(&mut main, &mut other);
        }
        let Some(main) = main else {
            return MarkSet::none();
        };
        let mut marks = main.marks().clone();
        let mut i = 0;
        while i < marks.len() {
            let mark = marks[i].clone();
            if !mark.mark_type().spec().inclusive
                && other.is_none_or(|other| !mark.is_in_set(other.marks()))
            {
                marks = mark.remove_from_set(&marks);
            } else {
                i += 1;
            }
        }
        marks
    }

    /// Marks to keep when deleting from here to `end`.
    pub fn marks_across(&self, end: &ResolvedPos) -> Option<MarkSet> {
        let after = self.parent().maybe_child(self.index(self.depth()))?;
        if !after.is_inline() {
            return None;
        }
        let next = end.parent().maybe_child(end.index(end.depth()));
        let mut marks = after.marks().clone();
        let mut i = 0;
        while i < marks.len() {
            let mark = marks[i].clone();
            if !mark.mark_type().spec().inclusive
                && next.is_none_or(|next| !mark.is_in_set(next.marks()))
            {
                marks = mark.remove_from_set(&marks);
            } else {
                i += 1;
            }
        }
        Some(marks)
    }

    /// Depth of the deepest ancestor that contains both this and `pos`.
    pub fn shared_depth(&self, pos: usize) -> usize {
        (1..=self.depth())
            .rev()
            .find(|depth| self.start(*depth) <= pos && self.end(*depth) >= pos)
            .unwrap_or(0)
    }

    /// The block range around this position and `other`.
    pub fn block_range(
        &self,
        other: &ResolvedPos,
        pred: Option<&dyn Fn(&Node) -> bool>,
    ) -> Option<NodeRange> {
        if other.pos < self.pos {
            return other.block_range(self, pred);
        }
        let skip = usize::from(self.parent().inline_content() || self.pos == other.pos);
        let top = self.depth().checked_sub(skip)?;
        (0..=top)
            .rev()
            .find(|depth| {
                other.pos <= self.end(*depth) && pred.is_none_or(|pred| pred(self.node(*depth)))
            })
            .map(|depth| NodeRange::new(self.clone(), other.clone(), depth))
    }

    pub fn same_parent(&self, other: &ResolvedPos) -> bool {
        self.pos - self.parent_offset == other.pos - other.parent_offset
    }

    pub fn max<'a>(&'a self, other: &'a ResolvedPos) -> &'a ResolvedPos {
        if other.pos > self.pos { other } else { self }
    }

    pub fn min<'a>(&'a self, other: &'a ResolvedPos) -> &'a ResolvedPos {
        if other.pos < self.pos { other } else { self }
    }
}

/// A flat range of sibling nodes, as used by lift and wrap.
#[derive(Debug, Clone)]
pub struct NodeRange {
    pub from: ResolvedPos,
    pub to: ResolvedPos,
    pub depth: usize,
}

impl NodeRange {
    pub fn new(from: ResolvedPos, to: ResolvedPos, depth: usize) -> Self {
        Self { from, to, depth }
    }

    pub fn start(&self) -> usize {
        self.from.before(self.depth + 1)
    }

    pub fn end(&self) -> usize {
        self.to.after(self.depth + 1)
    }

    pub fn parent(&self) -> &Node {
        self.from.node(self.depth)
    }

    pub fn start_index(&self) -> usize {
        self.from.index(self.depth)
    }

    pub fn end_index(&self) -> usize {
        self.to.index_after(self.depth)
    }
}
