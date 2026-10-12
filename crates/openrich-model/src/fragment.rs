use std::fmt;
use std::sync::Arc;

use crate::error::{ModelError, Result};
use crate::node::{Node, NodeVisitor};

/// An immutable sequence of child nodes with a cached size.
#[derive(Clone)]
pub struct Fragment {
    nodes: Arc<[Node]>,
    size: usize,
}

impl Default for Fragment {
    fn default() -> Self {
        Self::empty()
    }
}

impl PartialEq for Fragment {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.nodes, &other.nodes)
            || (self.size == other.size
                && self.nodes.len() == other.nodes.len()
                && self.nodes == other.nodes)
    }
}

impl Eq for Fragment {}

impl fmt::Debug for Fragment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{self}>")
    }
}

impl fmt::Display for Fragment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, node) in self.nodes.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{node}")?;
        }
        Ok(())
    }
}

/// Result of [`Fragment::find_index`]: the child index and its start offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexOffset {
    pub index: usize,
    pub offset: usize,
}

impl Fragment {
    pub fn empty() -> Self {
        Self {
            nodes: Arc::from([]),
            size: 0,
        }
    }

    /// Wraps `nodes` as they are, without joining adjacent text.
    pub fn new(nodes: Vec<Node>) -> Self {
        let size = nodes.iter().map(Node::node_size).sum();
        Self {
            nodes: Arc::from(nodes),
            size,
        }
    }

    pub fn from_node(node: Node) -> Self {
        let size = node.node_size();
        Self {
            nodes: Arc::from([node]),
            size,
        }
    }

    /// Builds a fragment, joining adjacent text nodes with the same marks.
    pub fn from_array(nodes: Vec<Node>) -> Self {
        let mut joined: Vec<Node> = Vec::with_capacity(nodes.len());
        for node in nodes {
            if let Some(last) = joined.last_mut()
                && node.is_text()
                && last.same_markup(&node)
            {
                let merged = format!("{}{}", last.text().unwrap_or(""), node.text().unwrap_or(""));
                *last = last.with_text(merged);
            } else {
                joined.push(node);
            }
        }
        Self::new(joined)
    }

    /// True when both fragments share the same storage.
    pub fn ptr_eq(&self, other: &Fragment) -> bool {
        Arc::ptr_eq(&self.nodes, &other.nodes)
    }

    /// Content size in positions.
    pub fn size(&self) -> usize {
        self.size
    }

    pub fn child_count(&self) -> usize {
        self.nodes.len()
    }

    /// Panics when `index` is out of range, like slice indexing.
    pub fn child(&self, index: usize) -> &Node {
        &self.nodes[index]
    }

    pub fn maybe_child(&self, index: usize) -> Option<&Node> {
        self.nodes.get(index)
    }

    pub fn first_child(&self) -> Option<&Node> {
        self.nodes.first()
    }

    pub fn last_child(&self) -> Option<&Node> {
        self.nodes.last()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Node> {
        self.nodes.iter()
    }

    pub fn as_slice(&self) -> &[Node] {
        &self.nodes
    }

    /// Calls `f(node, pos, parent, index)` for every node overlapping
    /// `from..to`, descending while `f` returns `true`.
    pub fn nodes_between(
        &self,
        from: usize,
        to: usize,
        f: &mut NodeVisitor<'_>,
        node_start: usize,
        parent: Option<&Node>,
    ) {
        let mut pos = 0;
        for (index, child) in self.nodes.iter().enumerate() {
            if pos >= to {
                break;
            }
            let end = pos + child.node_size();
            if end > from && f(child, node_start + pos, parent, index) && child.content().size() > 0
            {
                let start = pos + 1;
                child.content().nodes_between(
                    from.saturating_sub(start),
                    child.content().size().min(to.saturating_sub(start)),
                    f,
                    node_start + start,
                    Some(child),
                );
            }
            pos = end;
        }
    }

    /// Plain text between two positions. `block_separator` goes between
    /// textblocks; `leaf_text` overrides the text of non-text leaves.
    pub fn text_between(
        &self,
        from: usize,
        to: usize,
        block_separator: Option<&str>,
        leaf_text: Option<&str>,
    ) -> String {
        let mut text = String::new();
        let mut first = true;
        self.nodes_between(
            from,
            to,
            &mut |node, pos, _, _| {
                let node_text: String = if let Some(value) = node.text() {
                    let start = from.max(pos) - pos;
                    let end = (to - pos).min(node.text_len());
                    crate::text::slice_utf16(value, start, end)
                        .unwrap_or("")
                        .to_owned()
                } else if !node.is_leaf() {
                    String::new()
                } else if let Some(leaf) = leaf_text {
                    leaf.to_owned()
                } else {
                    node.node_type()
                        .spec()
                        .leaf_text
                        .clone()
                        .unwrap_or_default()
                };
                if node.is_block()
                    && ((node.is_leaf() && !node_text.is_empty()) || node.is_textblock())
                    && let Some(separator) = block_separator
                {
                    if first {
                        first = false;
                    } else {
                        text.push_str(separator);
                    }
                }
                text.push_str(&node_text);
                true
            },
            0,
            None,
        );
        text
    }

    /// Concatenates two fragments, joining text at the seam when possible.
    pub fn append(&self, other: &Fragment) -> Fragment {
        if other.size == 0 {
            return self.clone();
        }
        if self.size == 0 {
            return other.clone();
        }
        let mut content = self.nodes.to_vec();
        let mut rest = other.nodes.iter();
        if let (Some(last), Some(first)) = (self.last_child(), other.first_child())
            && last.is_text()
            && last.same_markup(first)
        {
            let merged = format!(
                "{}{}",
                last.text().unwrap_or(""),
                first.text().unwrap_or("")
            );
            let index = content.len() - 1;
            content[index] = last.with_text(merged);
            rest.next();
        }
        content.extend(rest.cloned());
        Fragment {
            nodes: Arc::from(content),
            size: self.size + other.size,
        }
    }

    /// The part of this fragment between two positions.
    pub fn cut(&self, from: usize, to: usize) -> Fragment {
        if from == 0 && to == self.size {
            return self.clone();
        }
        let mut result = Vec::new();
        if to > from {
            let mut pos = 0;
            for child in self.nodes.iter() {
                if pos >= to {
                    break;
                }
                let end = pos + child.node_size();
                if end > from {
                    let child = if pos < from || end > to {
                        if child.is_text() {
                            child.cut_text(from.saturating_sub(pos), child.text_len().min(to - pos))
                        } else {
                            child.cut(
                                from.saturating_sub(pos + 1),
                                child.content().size().min(to.saturating_sub(pos + 1)),
                            )
                        }
                    } else {
                        child.clone()
                    };
                    result.push(child);
                }
                pos = end;
            }
        }
        Fragment::new(result)
    }

    pub fn cut_to_end(&self, from: usize) -> Fragment {
        self.cut(from, self.size)
    }

    pub fn cut_by_index(&self, from: usize, to: usize) -> Fragment {
        if from == to {
            return Fragment::empty();
        }
        if from == 0 && to == self.nodes.len() {
            return self.clone();
        }
        Fragment::new(self.nodes[from..to].to_vec())
    }

    pub fn replace_child(&self, index: usize, node: Node) -> Fragment {
        let current = &self.nodes[index];
        if current.ptr_eq(&node) {
            return self.clone();
        }
        let size = self.size + node.node_size() - current.node_size();
        let mut copy = self.nodes.to_vec();
        copy[index] = node;
        Fragment {
            nodes: Arc::from(copy),
            size,
        }
    }

    pub fn add_to_start(&self, node: Node) -> Fragment {
        let size = self.size + node.node_size();
        let mut copy = Vec::with_capacity(self.nodes.len() + 1);
        copy.push(node);
        copy.extend(self.nodes.iter().cloned());
        Fragment {
            nodes: Arc::from(copy),
            size,
        }
    }

    pub fn add_to_end(&self, node: Node) -> Fragment {
        let size = self.size + node.node_size();
        let mut copy = self.nodes.to_vec();
        copy.push(node);
        Fragment {
            nodes: Arc::from(copy),
            size,
        }
    }

    /// The child index at `pos` and that child's start offset. With
    /// `round > 0`, a position inside a child resolves to the next one.
    pub fn find_index(&self, pos: usize, round: i8) -> Result<IndexOffset> {
        if pos == 0 {
            return Ok(IndexOffset {
                index: 0,
                offset: 0,
            });
        }
        if pos == self.size {
            return Ok(IndexOffset {
                index: self.nodes.len(),
                offset: pos,
            });
        }
        if pos > self.size {
            return Err(ModelError::PositionOutOfRange {
                pos,
                size: self.size,
            });
        }
        let mut current = 0;
        for (index, child) in self.nodes.iter().enumerate() {
            let end = current + child.node_size();
            if end >= pos {
                if end == pos || round > 0 {
                    return Ok(IndexOffset {
                        index: index + 1,
                        offset: end,
                    });
                }
                return Ok(IndexOffset {
                    index,
                    offset: current,
                });
            }
            current = end;
        }
        Err(ModelError::PositionOutOfRange {
            pos,
            size: self.size,
        })
    }

    /// First position where the two fragments differ, if any.
    pub fn find_diff_start(&self, other: &Fragment, pos: usize) -> Option<usize> {
        let mut pos = pos;
        let mut index = 0;
        loop {
            if index == self.child_count() || index == other.child_count() {
                return (self.child_count() != other.child_count()).then_some(pos);
            }
            let (a, b) = (self.child(index), other.child(index));
            if a.ptr_eq(b) {
                pos += a.node_size();
                index += 1;
                continue;
            }
            if !a.same_markup(b) {
                return Some(pos);
            }
            if let (Some(ta), Some(tb)) = (a.text(), b.text()) {
                if ta != tb {
                    let common = ta
                        .chars()
                        .zip(tb.chars())
                        .take_while(|(x, y)| x == y)
                        .map(|(x, _)| x.len_utf16())
                        .sum::<usize>();
                    return Some(pos + common);
                }
            } else if (a.content().size() > 0 || b.content().size() > 0)
                && let Some(inner) = a.content().find_diff_start(b.content(), pos + 1)
            {
                return Some(inner);
            }
            pos += a.node_size();
            index += 1;
        }
    }

    /// Last positions (in `self` and `other`) where the fragments differ.
    pub fn find_diff_end(
        &self,
        other: &Fragment,
        pos_a: usize,
        pos_b: usize,
    ) -> Option<(usize, usize)> {
        let (mut ia, mut ib) = (self.child_count(), other.child_count());
        let (mut pos_a, mut pos_b) = (pos_a, pos_b);
        loop {
            if ia == 0 || ib == 0 {
                return (ia != ib).then_some((pos_a, pos_b));
            }
            ia -= 1;
            ib -= 1;
            let (a, b) = (self.child(ia), other.child(ib));
            let size = a.node_size();
            if a.ptr_eq(b) {
                pos_a -= size;
                pos_b -= size;
                continue;
            }
            if !a.same_markup(b) {
                return Some((pos_a, pos_b));
            }
            if let (Some(ta), Some(tb)) = (a.text(), b.text()) {
                if ta != tb {
                    let common = ta
                        .chars()
                        .rev()
                        .zip(tb.chars().rev())
                        .take_while(|(x, y)| x == y)
                        .map(|(x, _)| x.len_utf16())
                        .sum::<usize>();
                    return Some((pos_a - common, pos_b - common));
                }
            } else if (a.content().size() > 0 || b.content().size() > 0)
                && let Some(inner) = a.content().find_diff_end(b.content(), pos_a - 1, pos_b - 1)
            {
                return Some(inner);
            }
            pos_a -= size;
            pos_b -= size;
        }
    }
}

impl<'a> IntoIterator for &'a Fragment {
    type Item = &'a Node;
    type IntoIter = std::slice::Iter<'a, Node>;

    fn into_iter(self) -> Self::IntoIter {
        self.nodes.iter()
    }
}
