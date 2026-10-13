use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use crate::content::ContentMatch;
use crate::error::{ModelError, Result};
use crate::fragment::Fragment;
use crate::mark::{Mark, MarkSet};
use crate::replace::{self, Slice};
use crate::resolved::ResolvedPos;
use crate::schema::{Attrs, MarkType, NodeType};
use crate::text::{slice_utf16, utf16_len};

struct NodeInner {
    ty: NodeType,
    attrs: Attrs,
    content: Fragment,
    marks: MarkSet,
    text: Option<Arc<str>>,
    /// UTF-16 length of `text`.
    text_len: usize,
}

/// Callback for [`Node::nodes_between`]: `(node, pos, parent, index)`.
/// Return `false` to skip the node's children.
pub type NodeVisitor<'a> = dyn FnMut(&Node, usize, Option<&Node>, usize) -> bool + 'a;

/// An immutable document node. Cloning shares the underlying data, so
/// unchanged subtrees are shared between document versions.
#[derive(Clone)]
pub struct Node(Arc<NodeInner>);

/// A child found by [`Node::child_after`] or [`Node::child_before`].
#[derive(Debug, Clone)]
pub struct ChildInfo {
    pub node: Option<Node>,
    pub index: usize,
    pub offset: usize,
}

impl Node {
    pub(crate) fn new(ty: NodeType, attrs: Attrs, content: Fragment, marks: MarkSet) -> Self {
        Self(Arc::new(NodeInner {
            ty,
            attrs,
            content,
            marks,
            text: None,
            text_len: 0,
        }))
    }

    pub(crate) fn new_text(ty: NodeType, text: Arc<str>, text_len: usize, marks: MarkSet) -> Self {
        let attrs = ty.compute_attrs(None).unwrap_or_default();
        Self(Arc::new(NodeInner {
            ty,
            attrs,
            content: Fragment::empty(),
            marks,
            text: Some(text),
            text_len,
        }))
    }

    pub fn ptr_eq(&self, other: &Node) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Identity of this node's shared data. Unchanged subtrees keep their
    /// identity across document versions, so caches can key on it (while
    /// holding a clone, so the address cannot be reused).
    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    pub fn node_type(&self) -> &NodeType {
        &self.0.ty
    }

    pub fn type_name(&self) -> &str {
        self.0.ty.name()
    }

    pub fn attrs(&self) -> &Attrs {
        &self.0.attrs
    }

    pub fn attr(&self, name: &str) -> Option<&Value> {
        self.0.attrs.get(name)
    }

    pub fn content(&self) -> &Fragment {
        &self.0.content
    }

    pub fn marks(&self) -> &MarkSet {
        &self.0.marks
    }

    /// The text of a text node.
    pub fn text(&self) -> Option<&str> {
        self.0.text.as_deref()
    }

    /// UTF-16 length of a text node's text; zero for other nodes.
    pub fn text_len(&self) -> usize {
        self.0.text_len
    }

    pub fn is_text(&self) -> bool {
        self.0.text.is_some()
    }

    pub fn is_block(&self) -> bool {
        self.0.ty.is_block()
    }

    pub fn is_inline(&self) -> bool {
        self.0.ty.is_inline()
    }

    pub fn is_textblock(&self) -> bool {
        self.0.ty.is_textblock()
    }

    pub fn inline_content(&self) -> bool {
        self.0.ty.inline_content()
    }

    pub fn is_leaf(&self) -> bool {
        self.0.ty.is_leaf()
    }

    pub fn is_atom(&self) -> bool {
        self.0.ty.is_atom()
    }

    /// Size in positions: text length, 1 for leaves, content + 2 otherwise.
    pub fn node_size(&self) -> usize {
        if self.is_text() {
            self.0.text_len
        } else if self.is_leaf() {
            1
        } else {
            2 + self.0.content.size()
        }
    }

    pub fn child_count(&self) -> usize {
        self.0.content.child_count()
    }

    pub fn child(&self, index: usize) -> &Node {
        self.0.content.child(index)
    }

    pub fn maybe_child(&self, index: usize) -> Option<&Node> {
        self.0.content.maybe_child(index)
    }

    pub fn first_child(&self) -> Option<&Node> {
        self.0.content.first_child()
    }

    pub fn last_child(&self) -> Option<&Node> {
        self.0.content.last_child()
    }

    pub fn children(&self) -> std::slice::Iter<'_, Node> {
        self.0.content.iter()
    }

    /// Calls `f(node, pos, parent, index)` for descendants overlapping
    /// `from..to` (relative to this node's content).
    pub fn nodes_between(&self, from: usize, to: usize, f: &mut NodeVisitor<'_>) {
        self.0.content.nodes_between(from, to, f, 0, Some(self));
    }

    pub fn descendants(&self, f: &mut NodeVisitor<'_>) {
        self.nodes_between(0, self.0.content.size(), f);
    }

    pub fn text_content(&self) -> String {
        match self.text() {
            Some(text) => text.to_owned(),
            None => self.text_between(0, self.0.content.size(), None, None),
        }
    }

    pub fn text_between(
        &self,
        from: usize,
        to: usize,
        block_separator: Option<&str>,
        leaf_text: Option<&str>,
    ) -> String {
        match self.text() {
            Some(text) => slice_utf16(text, from, to.min(self.0.text_len))
                .unwrap_or("")
                .to_owned(),
            None => self
                .0
                .content
                .text_between(from, to, block_separator, leaf_text),
        }
    }

    pub fn same_markup(&self, other: &Node) -> bool {
        self.has_markup(&other.0.ty, &other.0.attrs, &other.0.marks)
    }

    pub fn has_markup(&self, ty: &NodeType, attrs: &Attrs, marks: &MarkSet) -> bool {
        self.0.ty == *ty
            && (Arc::ptr_eq(&self.0.attrs, attrs) || self.0.attrs == *attrs)
            && self.0.marks.same_set(marks)
    }

    /// Same node with different content.
    pub fn copy(&self, content: Fragment) -> Node {
        if content.ptr_eq(&self.0.content) {
            return self.clone();
        }
        Node::new(
            self.0.ty.clone(),
            self.0.attrs.clone(),
            content,
            self.0.marks.clone(),
        )
    }

    /// Same node with different marks.
    pub fn mark(&self, marks: MarkSet) -> Node {
        if marks.same_set(&self.0.marks) {
            return self.clone();
        }
        Node(Arc::new(NodeInner {
            ty: self.0.ty.clone(),
            attrs: self.0.attrs.clone(),
            content: self.0.content.clone(),
            marks,
            text: self.0.text.clone(),
            text_len: self.0.text_len,
        }))
    }

    /// Same node with different attributes (not validated).
    pub fn with_attrs(&self, attrs: Attrs) -> Node {
        Node(Arc::new(NodeInner {
            ty: self.0.ty.clone(),
            attrs,
            content: self.0.content.clone(),
            marks: self.0.marks.clone(),
            text: self.0.text.clone(),
            text_len: self.0.text_len,
        }))
    }

    /// A text node with the same marks and new text.
    pub fn with_text(&self, text: impl Into<Arc<str>>) -> Node {
        let text: Arc<str> = text.into();
        if self.text() == Some(&*text) {
            return self.clone();
        }
        let len = utf16_len(&text);
        Node::new_text(self.0.ty.clone(), text, len, self.0.marks.clone())
    }

    /// Cuts a text node by UTF-16 offsets, clamping to character boundaries.
    pub(crate) fn cut_text(&self, from: usize, to: usize) -> Node {
        let text = self.text().unwrap_or("");
        if from == 0 && to == self.0.text_len {
            return self.clone();
        }
        let mut start = from;
        while start > 0 && slice_utf16(text, 0, start).is_none() {
            start -= 1;
        }
        let mut end = to;
        while end < self.0.text_len && slice_utf16(text, 0, end).is_none() {
            end += 1;
        }
        self.with_text(slice_utf16(text, start, end).unwrap_or(""))
    }

    /// Copy containing only the content between two positions.
    pub fn cut(&self, from: usize, to: usize) -> Node {
        if self.is_text() {
            return self.cut_text(from, to);
        }
        if from == 0 && to == self.0.content.size() {
            return self.clone();
        }
        self.copy(self.0.content.cut(from, to))
    }

    /// The slice of this node's content between two positions.
    pub fn slice(&self, from: usize, to: usize, include_parents: bool) -> Result<Slice> {
        if from == to {
            return Ok(Slice::empty());
        }
        let rfrom = self.resolve(from)?;
        let rto = self.resolve(to)?;
        let depth = if include_parents {
            0
        } else {
            rfrom.shared_depth(to)
        };
        let start = rfrom.start(depth);
        let node = rfrom.node(depth);
        let content = node.content().cut(rfrom.pos() - start, rto.pos() - start);
        Ok(Slice::new(
            content,
            rfrom.depth() - depth,
            rto.depth() - depth,
        ))
    }

    /// Replaces `from..to` with `slice`, returning the new node.
    pub fn replace(&self, from: usize, to: usize, slice: &Slice) -> Result<Node> {
        replace::replace(&self.resolve(from)?, &self.resolve(to)?, slice)
    }

    /// The node directly after `pos`, at any depth.
    pub fn node_at(&self, pos: usize) -> Option<Node> {
        let mut node = self.clone();
        let mut pos = pos;
        loop {
            let found = node.content().find_index(pos, -1).ok()?;
            let child = node.maybe_child(found.index)?.clone();
            if found.offset == pos || child.is_text() {
                return Some(child);
            }
            pos -= found.offset + 1;
            node = child;
        }
    }

    pub fn child_after(&self, pos: usize) -> Result<ChildInfo> {
        let found = self.0.content.find_index(pos, -1)?;
        Ok(ChildInfo {
            node: self.0.content.maybe_child(found.index).cloned(),
            index: found.index,
            offset: found.offset,
        })
    }

    pub fn child_before(&self, pos: usize) -> Result<ChildInfo> {
        if pos == 0 {
            return Ok(ChildInfo {
                node: None,
                index: 0,
                offset: 0,
            });
        }
        let found = self.0.content.find_index(pos, -1)?;
        if found.offset < pos {
            return Ok(ChildInfo {
                node: Some(self.0.content.child(found.index).clone()),
                index: found.index,
                offset: found.offset,
            });
        }
        let node = self.0.content.child(found.index - 1).clone();
        let offset = found.offset - node.node_size();
        Ok(ChildInfo {
            node: Some(node),
            index: found.index - 1,
            offset,
        })
    }

    pub fn resolve(&self, pos: usize) -> Result<ResolvedPos> {
        ResolvedPos::resolve(self, pos)
    }

    pub fn range_has_mark(&self, from: usize, to: usize, ty: &MarkType) -> bool {
        let mut found = false;
        if to > from {
            self.nodes_between(from, to, &mut |node, _, _, _| {
                if ty.is_in_set(node.marks()).is_some() {
                    found = true;
                }
                !found
            });
        }
        found
    }

    pub fn content_match_at(&self, index: usize) -> Result<ContentMatch> {
        self.0
            .ty
            .content_match()
            .match_fragment(&self.0.content, 0, index)
            .ok_or_else(|| ModelError::InvalidContent(self.type_name().to_owned()))
    }

    /// Whether replacing children `from..to` with `replacement[start..end]`
    /// keeps the content valid.
    pub fn can_replace(
        &self,
        from: usize,
        to: usize,
        replacement: &Fragment,
        start: usize,
        end: usize,
    ) -> bool {
        let Ok(at) = self.content_match_at(from) else {
            return false;
        };
        let Some(one) = at.match_fragment(replacement, start, end) else {
            return false;
        };
        let Some(two) = one.match_fragment(&self.0.content, to, self.child_count()) else {
            return false;
        };
        two.valid_end()
            && (start..end).all(|index| self.0.ty.allows_marks(replacement.child(index).marks()))
    }

    pub fn can_replace_with(
        &self,
        from: usize,
        to: usize,
        ty: &NodeType,
        marks: Option<&[Mark]>,
    ) -> bool {
        if let Some(marks) = marks
            && !self.0.ty.allows_marks(marks)
        {
            return false;
        }
        let Ok(at) = self.content_match_at(from) else {
            return false;
        };
        at.match_type(ty)
            .and_then(|start| start.match_fragment(&self.0.content, to, self.child_count()))
            .is_some_and(|end| end.valid_end())
    }

    pub fn can_append(&self, other: &Node) -> bool {
        if other.content().size() > 0 {
            self.can_replace(
                self.child_count(),
                self.child_count(),
                other.content(),
                0,
                other.child_count(),
            )
        } else {
            self.0.ty.compatible_content(other.node_type())
        }
    }

    /// Validates content, attributes and marks of this node and all descendants.
    pub fn check(&self) -> Result<()> {
        if !self.is_text() {
            self.0.ty.check_content(&self.0.content)?;
        }
        self.0.ty.check_attrs(&self.0.attrs)?;
        let mut normalized = MarkSet::none();
        for mark in self.0.marks.iter() {
            normalized = mark.add_to_set(&normalized);
        }
        if !normalized.same_set(&self.0.marks) {
            return Err(ModelError::InvalidMarks(self.type_name().to_owned()));
        }
        self.0.content.iter().try_for_each(Node::check)
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
            || (self.same_markup(other)
                && self.0.text == other.0.text
                && self.0.content == other.0.content)
    }
}

impl Eq for Node {}

fn wrap_marks(marks: &[Mark], inner: String) -> String {
    marks.iter().rev().fold(inner, |acc, mark| {
        format!("{}({acc})", mark.mark_type().name())
    })
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = match self.text() {
            Some(text) => format!("{text:?}"),
            None if self.0.content.size() > 0 => {
                format!("{}({})", self.type_name(), self.0.content)
            }
            None => self.type_name().to_owned(),
        };
        f.write_str(&wrap_marks(&self.0.marks, inner))
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
