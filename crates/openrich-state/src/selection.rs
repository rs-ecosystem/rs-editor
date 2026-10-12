//! Selections (`selection.ts`): text ranges, selected nodes and the whole
//! document, plus bookmarks that survive document changes.

use std::fmt;

use openrich_model::{Fragment, ModelError, Node, ResolvedPos, Slice};
use openrich_transform::{Assoc, Mappable, Step, TransformResult};
use serde_json::{Value, json};

use crate::transaction::Transaction;

/// Resolves `pos`, clamping it into the document and off surrogate halves.
/// Mapped positions are always valid in practice; this keeps the selection
/// layer panic-free if a host hands in a stale offset.
pub(crate) fn resolve_clamped(doc: &Node, pos: usize) -> ResolvedPos {
    let mut pos = pos.min(doc.content().size());
    loop {
        match doc.resolve(pos) {
            Ok(resolved) => return resolved,
            Err(_) if pos > 0 => pos -= 1,
            Err(_) => unreachable!("position 0 always resolves"),
        }
    }
}

/// A cursor or range in inline content.
#[derive(Debug, Clone)]
pub struct TextSelection {
    anchor: ResolvedPos,
    head: ResolvedPos,
}

/// A single selected node (an image, a horizontal rule, a whole block).
#[derive(Debug, Clone)]
pub struct NodeSelection {
    anchor: ResolvedPos,
    head: ResolvedPos,
    node: Node,
}

/// The whole document.
#[derive(Debug, Clone)]
pub struct AllSelection {
    anchor: ResolvedPos,
    head: ResolvedPos,
}

/// The editor selection. Always a single range.
#[derive(Clone)]
pub enum Selection {
    Text(TextSelection),
    Node(NodeSelection),
    All(AllSelection),
}

impl fmt::Debug for Selection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Selection::Text(_) => write!(f, "TextSelection({}, {})", self.anchor(), self.head()),
            Selection::Node(_) => write!(f, "NodeSelection({})", self.anchor()),
            Selection::All(_) => write!(f, "AllSelection"),
        }
    }
}

impl PartialEq for Selection {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Selection::Text(a), Selection::Text(b)) => {
                a.anchor.pos() == b.anchor.pos() && a.head.pos() == b.head.pos()
            }
            (Selection::Node(a), Selection::Node(b)) => a.anchor.pos() == b.anchor.pos(),
            (Selection::All(_), Selection::All(_)) => true,
            _ => false,
        }
    }
}

impl TextSelection {
    /// Both endpoints should point into inline content.
    pub fn new(anchor: ResolvedPos, head: ResolvedPos) -> Self {
        Self { anchor, head }
    }

    pub fn create(doc: &Node, anchor: usize, head: usize) -> Result<Selection, ModelError> {
        let anchor = doc.resolve(anchor)?;
        let head = doc.resolve(head)?;
        Ok(Selection::Text(TextSelection::new(anchor, head)))
    }

    pub fn cursor_at(doc: &Node, pos: usize) -> Result<Selection, ModelError> {
        Self::create(doc, pos, pos)
    }

    /// A text selection between two positions, moved into inline content
    /// when needed. `bias` picks the search direction for collapsed input.
    pub fn between(anchor: &ResolvedPos, head: &ResolvedPos, bias: Option<i8>) -> Selection {
        let d_pos = anchor.pos() as isize - head.pos() as isize;
        let bias = match bias {
            Some(bias) if d_pos == 0 => bias,
            _ => {
                if d_pos >= 0 {
                    1
                } else {
                    -1
                }
            }
        };
        let mut head = head.clone();
        if !head.parent().inline_content() {
            match Selection::find_from(&head, bias, true)
                .or_else(|| Selection::find_from(&head, -bias, true))
            {
                Some(found) => head = found.head_pos().clone(),
                None => return Selection::near(&head, bias),
            }
        }
        let mut anchor = anchor.clone();
        if !anchor.parent().inline_content() {
            if d_pos == 0 {
                anchor = head.clone();
            } else {
                let found = Selection::find_from(&anchor, -bias, true)
                    .or_else(|| Selection::find_from(&anchor, bias, true));
                if let Some(found) = found {
                    anchor = found.anchor_pos().clone();
                }
                if (anchor.pos() < head.pos()) != (d_pos < 0) {
                    anchor = head.clone();
                }
            }
        }
        Selection::Text(TextSelection::new(anchor, head))
    }
}

impl NodeSelection {
    /// Selects the node after `pos`, if there is one.
    pub fn new(pos: ResolvedPos) -> Option<Self> {
        let node = pos.node_after()?;
        let end = pos.doc().resolve(pos.pos() + node.node_size()).ok()?;
        Some(Self {
            anchor: pos,
            head: end,
            node,
        })
    }

    pub fn create(doc: &Node, from: usize) -> Result<Selection, ModelError> {
        let pos = doc.resolve(from)?;
        Self::new(pos)
            .map(Selection::Node)
            .ok_or(ModelError::PositionOutOfRange {
                pos: from,
                size: doc.content().size(),
            })
    }

    pub fn node(&self) -> &Node {
        &self.node
    }

    /// Whether the node can be selected as a whole.
    pub fn is_selectable(node: &Node) -> bool {
        !node.is_text() && node.node_type().spec().selectable != Some(false)
    }
}

impl AllSelection {
    pub fn new(doc: &Node) -> Self {
        Self {
            anchor: resolve_clamped(doc, 0),
            head: resolve_clamped(doc, doc.content().size()),
        }
    }
}

/// Finds a selection inside `node` starting at `pos`, scanning in `dir`.
fn find_selection_in(
    doc: &Node,
    node: &Node,
    pos: usize,
    index: usize,
    dir: i8,
    text_only: bool,
) -> Option<Selection> {
    if node.inline_content() {
        return TextSelection::cursor_at(doc, pos).ok();
    }
    let mut pos = pos as isize;
    let mut i = index as isize - if dir > 0 { 0 } else { 1 };
    while (dir > 0 && i < node.child_count() as isize) || (dir < 0 && i >= 0) {
        let child = node.child(i as usize);
        if !child.is_atom() {
            let start = if dir < 0 { child.child_count() } else { 0 };
            if let Some(inner) = find_selection_in(
                doc,
                child,
                (pos + dir as isize) as usize,
                start,
                dir,
                text_only,
            ) {
                return Some(inner);
            }
        } else if !text_only && NodeSelection::is_selectable(child) {
            let at = pos
                - if dir < 0 {
                    child.node_size() as isize
                } else {
                    0
                };
            return NodeSelection::create(doc, at as usize).ok();
        }
        pos += child.node_size() as isize * dir as isize;
        i += dir as isize;
    }
    None
}

/// Moves the selection to the end of the content inserted by the last step.
fn selection_to_insertion_end(tr: &mut Transaction, start_len: usize, bias: i8) {
    let Some(last) = tr.steps.len().checked_sub(1) else {
        return;
    };
    if last < start_len || !matches!(tr.steps[last], Step::Replace(_) | Step::ReplaceAround(_)) {
        return;
    }
    let mut end = None;
    tr.mapping.maps()[last].for_each(|_, _, _, new_to| {
        end.get_or_insert(new_to);
    });
    if let Some(end) = end {
        let near = Selection::near(&resolve_clamped(&tr.doc, end), bias);
        tr.set_selection(near);
    }
}

impl Selection {
    pub fn anchor_pos(&self) -> &ResolvedPos {
        match self {
            Selection::Text(s) => &s.anchor,
            Selection::Node(s) => &s.anchor,
            Selection::All(s) => &s.anchor,
        }
    }

    pub fn head_pos(&self) -> &ResolvedPos {
        match self {
            Selection::Text(s) => &s.head,
            Selection::Node(s) => &s.head,
            Selection::All(s) => &s.head,
        }
    }

    pub fn anchor(&self) -> usize {
        self.anchor_pos().pos()
    }

    pub fn head(&self) -> usize {
        self.head_pos().pos()
    }

    /// The lower end, resolved.
    pub fn from_pos(&self) -> &ResolvedPos {
        self.anchor_pos().min(self.head_pos())
    }

    /// The upper end, resolved.
    pub fn to_pos(&self) -> &ResolvedPos {
        self.anchor_pos().max(self.head_pos())
    }

    pub fn from(&self) -> usize {
        self.from_pos().pos()
    }

    pub fn to(&self) -> usize {
        self.to_pos().pos()
    }

    pub fn empty(&self) -> bool {
        self.from() == self.to()
    }

    /// Node selections are drawn by the selected node, not as a range.
    pub fn visible(&self) -> bool {
        !matches!(self, Selection::Node(_))
    }

    /// The collapsed text cursor, if this is one.
    pub fn cursor(&self) -> Option<&ResolvedPos> {
        match self {
            Selection::Text(s) if s.anchor.pos() == s.head.pos() => Some(&s.head),
            _ => None,
        }
    }

    pub fn as_node(&self) -> Option<&NodeSelection> {
        match self {
            Selection::Node(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(self, Selection::Text(_))
    }

    pub fn is_all(&self) -> bool {
        matches!(self, Selection::All(_))
    }

    /// This selection after a document change.
    pub fn map(&self, doc: &Node, mapping: &dyn Mappable) -> Selection {
        match self {
            Selection::Text(s) => {
                let head = resolve_clamped(doc, mapping.map(s.head.pos(), Assoc::After));
                if !head.parent().inline_content() {
                    return Selection::near(&head, 1);
                }
                let anchor = resolve_clamped(doc, mapping.map(s.anchor.pos(), Assoc::After));
                let anchor = if anchor.parent().inline_content() {
                    anchor
                } else {
                    head.clone()
                };
                Selection::Text(TextSelection::new(anchor, head))
            }
            Selection::Node(s) => {
                let result = mapping.map_result(s.anchor.pos(), Assoc::After);
                let pos = resolve_clamped(doc, result.pos);
                if result.deleted() {
                    return Selection::near(&pos, 1);
                }
                match NodeSelection::new(pos.clone()) {
                    Some(selection) => Selection::Node(selection),
                    None => Selection::near(&pos, 1),
                }
            }
            Selection::All(_) => Selection::All(AllSelection::new(doc)),
        }
    }

    /// The selected content as a slice.
    pub fn content(&self) -> Slice {
        match self {
            Selection::Node(s) => Slice::new(Fragment::from_node(s.node.clone()), 0, 0),
            _ => self
                .from_pos()
                .doc()
                .slice(self.from(), self.to(), true)
                .unwrap_or_else(|_| Slice::empty()),
        }
    }

    /// Replaces the selection with `content` (deletes it for `None`).
    pub fn replace(&self, tr: &mut Transaction, content: Option<Slice>) -> TransformResult {
        if let Selection::All(_) = self
            && content.is_none()
        {
            let size = tr.doc.content().size();
            tr.delete(0, size)?;
            let at_start = Selection::at_start(&tr.doc);
            if at_start != tr.selection() {
                tr.set_selection(at_start);
            }
            return Ok(());
        }
        let deleting = content.is_none();
        let content = content.unwrap_or_else(Slice::empty);
        let mut last_node = content.content.last_child().cloned();
        let mut last_parent: Option<Node> = None;
        for _ in 0..content.open_end {
            last_parent = last_node.clone();
            last_node = last_node.and_then(|node| node.last_child().cloned());
        }
        let map_from = tr.steps.len();
        let mapping = tr.mapping.slice_from(map_from);
        let from = mapping.map(self.from(), Assoc::After);
        let to = mapping.map(self.to(), Assoc::After);
        tr.replace_range(from, to, content)?;
        let inline_end = match &last_node {
            Some(node) => node.is_inline(),
            None => last_parent.as_ref().is_some_and(Node::is_textblock),
        };
        selection_to_insertion_end(tr, map_from, if inline_end { -1 } else { 1 });
        if deleting
            && let Selection::Text(_) = self
            && let Some(marks) = self.from_pos().marks_across(self.to_pos())
        {
            tr.ensure_marks(marks);
        }
        Ok(())
    }

    /// Replaces the selection with a single node.
    pub fn replace_with(&self, tr: &mut Transaction, node: Node) -> TransformResult {
        let map_from = tr.steps.len();
        let mapping = tr.mapping.slice_from(map_from);
        let from = mapping.map(self.from(), Assoc::After);
        let to = mapping.map(self.to(), Assoc::After);
        let inline = node.is_inline();
        tr.replace_range_with(from, to, node)?;
        selection_to_insertion_end(tr, map_from, if inline { -1 } else { 1 });
        Ok(())
    }

    /// The first valid selection at or after (`dir > 0`) or before `pos`.
    pub fn find_from(pos: &ResolvedPos, dir: i8, text_only: bool) -> Option<Selection> {
        let doc = pos.doc();
        let inner = if pos.parent().inline_content() {
            Some(Selection::Text(TextSelection::new(
                pos.clone(),
                pos.clone(),
            )))
        } else {
            find_selection_in(
                doc,
                pos.parent(),
                pos.pos(),
                pos.index(pos.depth()),
                dir,
                text_only,
            )
        };
        if inner.is_some() {
            return inner;
        }
        for depth in (0..pos.depth()).rev() {
            let found = if dir < 0 {
                find_selection_in(
                    doc,
                    pos.node(depth),
                    pos.before(depth + 1),
                    pos.index(depth),
                    dir,
                    text_only,
                )
            } else {
                find_selection_in(
                    doc,
                    pos.node(depth),
                    pos.after(depth + 1),
                    pos.index(depth) + 1,
                    dir,
                    text_only,
                )
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// The nearest valid selection, searching in `bias` direction first.
    pub fn near(pos: &ResolvedPos, bias: i8) -> Selection {
        Self::find_from(pos, bias, false)
            .or_else(|| Self::find_from(pos, -bias, false))
            .unwrap_or_else(|| Selection::All(AllSelection::new(pos.doc())))
    }

    pub fn at_start(doc: &Node) -> Selection {
        find_selection_in(doc, doc, 0, 0, 1, false)
            .unwrap_or_else(|| Selection::All(AllSelection::new(doc)))
    }

    pub fn at_end(doc: &Node) -> Selection {
        find_selection_in(doc, doc, doc.content().size(), doc.child_count(), -1, false)
            .unwrap_or_else(|| Selection::All(AllSelection::new(doc)))
    }

    pub fn to_json(&self) -> Value {
        match self {
            Selection::Text(_) => {
                json!({"type": "text", "anchor": self.anchor(), "head": self.head()})
            }
            Selection::Node(_) => json!({"type": "node", "anchor": self.anchor()}),
            Selection::All(_) => json!({"type": "all"}),
        }
    }

    pub fn from_json(doc: &Node, value: &Value) -> Result<Selection, ModelError> {
        let invalid = || ModelError::Json("invalid selection JSON".into());
        let pos = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_u64)
                .map(|n| n as usize)
                .ok_or_else(invalid)
        };
        match value.get("type").and_then(Value::as_str) {
            Some("text") => TextSelection::create(doc, pos("anchor")?, pos("head")?),
            Some("node") => NodeSelection::create(doc, pos("anchor")?),
            Some("all") => Ok(Selection::All(AllSelection::new(doc))),
            _ => Err(invalid()),
        }
    }

    /// A lightweight, mappable description of this selection.
    pub fn bookmark(&self) -> SelectionBookmark {
        match self {
            Selection::Node(_) => SelectionBookmark::Node {
                anchor: self.anchor(),
            },
            Selection::All(_) => SelectionBookmark::All,
            Selection::Text(_) => SelectionBookmark::Text {
                anchor: self.anchor(),
                head: self.head(),
            },
        }
    }
}

/// A selection reduced to positions, so it can be mapped through changes
/// (e.g. by the undo history) and resolved again later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionBookmark {
    Text { anchor: usize, head: usize },
    Node { anchor: usize },
    All,
}

impl SelectionBookmark {
    pub fn map(&self, mapping: &dyn Mappable) -> SelectionBookmark {
        match *self {
            SelectionBookmark::Text { anchor, head } => SelectionBookmark::Text {
                anchor: mapping.map(anchor, Assoc::After),
                head: mapping.map(head, Assoc::After),
            },
            SelectionBookmark::Node { anchor } => {
                let result = mapping.map_result(anchor, Assoc::After);
                if result.deleted() {
                    SelectionBookmark::Text {
                        anchor: result.pos,
                        head: result.pos,
                    }
                } else {
                    SelectionBookmark::Node { anchor: result.pos }
                }
            }
            SelectionBookmark::All => SelectionBookmark::All,
        }
    }

    pub fn resolve(&self, doc: &Node) -> Selection {
        match *self {
            SelectionBookmark::Text { anchor, head } => TextSelection::between(
                &resolve_clamped(doc, anchor),
                &resolve_clamped(doc, head),
                None,
            ),
            SelectionBookmark::Node { anchor } => {
                let pos = resolve_clamped(doc, anchor);
                match pos.node_after() {
                    Some(node) if NodeSelection::is_selectable(&node) => {
                        NodeSelection::new(pos.clone())
                            .map_or_else(|| Selection::near(&pos, 1), Selection::Node)
                    }
                    _ => Selection::near(&pos, 1),
                }
            }
            SelectionBookmark::All => Selection::All(AllSelection::new(doc)),
        }
    }
}
