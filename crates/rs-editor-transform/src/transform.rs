use rs_editor_model::{
    ContentMatch, Fragment, Mark, MarkSet, MarkType, ModelError, Node, NodeRange, NodeType, Slice,
};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::map::{Assoc, Mappable, Mapping};
use crate::step::{AttrStep, DocAttrStep, NodeMarkStep, Step, StepError};
use crate::{mark, replace, structure};

/// Why a transform operation failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransformError {
    #[error("step failed: {0}")]
    Step(#[from] StepError),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error("{0}")]
    Invalid(String),
}

pub type TransformResult<T = ()> = Result<T, TransformError>;

/// Which marks [`Transform::remove_mark`] removes.
#[derive(Debug, Clone)]
pub enum MarkMatch {
    /// Exactly this mark (type and attributes).
    Mark(Mark),
    /// Every mark of this type.
    Type(MarkType),
    /// All marks.
    All,
}

/// A wrapper type, with optional attributes, for [`Transform::wrap`] and
/// [`Transform::split`].
#[derive(Debug, Clone)]
pub struct NodeTypeWithAttrs {
    pub ty: NodeType,
    pub attrs: Option<Map<String, Value>>,
}

impl NodeTypeWithAttrs {
    pub fn new(ty: NodeType) -> Self {
        Self { ty, attrs: None }
    }
}

/// Accumulates steps against a document. Every helper either applies
/// valid steps or returns an error and leaves already applied steps intact.
#[derive(Debug, Clone)]
pub struct Transform {
    pub doc: Node,
    pub steps: Vec<Step>,
    /// Document before each step.
    pub docs: Vec<Node>,
    pub mapping: Mapping,
}

impl Transform {
    pub fn new(doc: Node) -> Self {
        Self {
            doc,
            steps: Vec::new(),
            docs: Vec::new(),
            mapping: Mapping::new(),
        }
    }

    /// The document before any step.
    pub fn before(&self) -> &Node {
        self.docs.first().unwrap_or(&self.doc)
    }

    pub fn doc_changed(&self) -> bool {
        !self.steps.is_empty()
    }

    /// Applies `step`, failing if it does not apply.
    pub fn step(&mut self, step: Step) -> TransformResult<&mut Self> {
        let doc = step.apply(&self.doc)?;
        self.add_step(step, doc);
        Ok(self)
    }

    /// Applies `step` if it applies; returns whether it did.
    pub fn maybe_step(&mut self, step: Step) -> bool {
        match step.apply(&self.doc) {
            Ok(doc) => {
                self.add_step(step, doc);
                true
            }
            Err(_) => false,
        }
    }

    pub(crate) fn add_step(&mut self, step: Step, doc: Node) {
        self.docs.push(std::mem::replace(&mut self.doc, doc));
        self.mapping.append_map(step.get_map(), None);
        self.steps.push(step);
    }

    /// The range of the final document touched by this transform.
    pub fn changed_range(&self) -> Option<(usize, usize)> {
        let mut range: Option<(usize, usize)> = None;
        for (index, map) in self.mapping.maps().iter().enumerate() {
            if index > 0
                && let Some((from, to)) = range
            {
                range = Some((map.map(from, Assoc::After), map.map(to, Assoc::Before)));
            }
            map.for_each(|_, _, from_b, to_b| {
                range = Some(match range {
                    Some((from, to)) => (from.min(from_b), to.max(to_b)),
                    None => (from_b, to_b),
                });
            });
        }
        range
    }

    /// Replaces `from..to` with `slice`, fitting it into the document.
    pub fn replace(&mut self, from: usize, to: usize, slice: Slice) -> TransformResult<&mut Self> {
        if let Some(step) = replace::replace_step(&self.doc, from, to, slice)? {
            self.step(step)?;
        }
        Ok(self)
    }

    pub fn replace_with(
        &mut self,
        from: usize,
        to: usize,
        content: Fragment,
    ) -> TransformResult<&mut Self> {
        self.replace(from, to, Slice::new(content, 0, 0))
    }

    pub fn delete(&mut self, from: usize, to: usize) -> TransformResult<&mut Self> {
        self.replace(from, to, Slice::empty())
    }

    pub fn insert(&mut self, pos: usize, content: Fragment) -> TransformResult<&mut Self> {
        self.replace_with(pos, pos, content)
    }

    /// Like [`replace`](Self::replace), but expands to cover parent nodes
    /// when the slice's open sides suggest it (used for paste).
    pub fn replace_range(
        &mut self,
        from: usize,
        to: usize,
        slice: Slice,
    ) -> TransformResult<&mut Self> {
        replace::replace_range(self, from, to, slice)?;
        Ok(self)
    }

    pub fn replace_range_with(
        &mut self,
        from: usize,
        to: usize,
        node: Node,
    ) -> TransformResult<&mut Self> {
        replace::replace_range_with(self, from, to, node)?;
        Ok(self)
    }

    /// Deletes `from..to`, widening to whole nodes when that is cleaner.
    pub fn delete_range(&mut self, from: usize, to: usize) -> TransformResult<&mut Self> {
        replace::delete_range(self, from, to)?;
        Ok(self)
    }

    pub fn lift(&mut self, range: &NodeRange, target: usize) -> TransformResult<&mut Self> {
        structure::lift(self, range, target)?;
        Ok(self)
    }

    pub fn join(&mut self, pos: usize, depth: usize) -> TransformResult<&mut Self> {
        structure::join(self, pos, depth)?;
        Ok(self)
    }

    pub fn wrap(
        &mut self,
        range: &NodeRange,
        wrappers: &[NodeTypeWithAttrs],
    ) -> TransformResult<&mut Self> {
        structure::wrap(self, range, wrappers)?;
        Ok(self)
    }

    /// Turns every textblock in `from..to` into `ty`.
    pub fn set_block_type(
        &mut self,
        from: usize,
        to: usize,
        ty: &NodeType,
        attrs: Option<&Map<String, Value>>,
    ) -> TransformResult<&mut Self> {
        structure::set_block_type(self, from, to, ty, &|_| attrs.cloned())?;
        Ok(self)
    }

    /// [`set_block_type`](Self::set_block_type) with attributes derived from each old node.
    pub fn set_block_type_with(
        &mut self,
        from: usize,
        to: usize,
        ty: &NodeType,
        attrs: &dyn Fn(&Node) -> Option<Map<String, Value>>,
    ) -> TransformResult<&mut Self> {
        structure::set_block_type(self, from, to, ty, attrs)?;
        Ok(self)
    }

    pub fn set_node_markup(
        &mut self,
        pos: usize,
        ty: Option<&NodeType>,
        attrs: Option<&Map<String, Value>>,
        marks: Option<MarkSet>,
    ) -> TransformResult<&mut Self> {
        structure::set_node_markup(self, pos, ty, attrs, marks)?;
        Ok(self)
    }

    pub fn set_node_attribute(
        &mut self,
        pos: usize,
        attr: &str,
        value: Value,
    ) -> TransformResult<&mut Self> {
        self.step(Step::Attr(AttrStep {
            pos,
            attr: attr.to_owned(),
            value,
        }))
    }

    pub fn set_doc_attribute(&mut self, attr: &str, value: Value) -> TransformResult<&mut Self> {
        self.step(Step::DocAttr(DocAttrStep {
            attr: attr.to_owned(),
            value,
        }))
    }

    pub fn add_node_mark(&mut self, pos: usize, mark: Mark) -> TransformResult<&mut Self> {
        self.step(Step::AddNodeMark(NodeMarkStep { pos, mark }))
    }

    pub fn remove_node_mark(
        &mut self,
        pos: usize,
        matching: &MarkMatch,
    ) -> TransformResult<&mut Self> {
        let node = self
            .doc
            .node_at(pos)
            .ok_or_else(|| TransformError::Invalid(format!("no node at position {pos}")))?;
        let marks: Vec<Mark> = node
            .marks()
            .iter()
            .filter(|mark| match matching {
                MarkMatch::Mark(wanted) => *mark == wanted,
                MarkMatch::Type(ty) => mark.mark_type() == ty,
                MarkMatch::All => true,
            })
            .cloned()
            .collect();
        for mark in marks.into_iter().rev() {
            self.step(Step::RemoveNodeMark(NodeMarkStep { pos, mark }))?;
        }
        Ok(self)
    }

    /// Splits the node at `pos` `depth` levels up, optionally changing the
    /// types after the split (innermost last).
    pub fn split(
        &mut self,
        pos: usize,
        depth: usize,
        types_after: Option<&[Option<NodeTypeWithAttrs>]>,
    ) -> TransformResult<&mut Self> {
        structure::split(self, pos, depth, types_after)?;
        Ok(self)
    }

    pub fn add_mark(&mut self, from: usize, to: usize, mark: &Mark) -> TransformResult<&mut Self> {
        mark::add_mark(self, from, to, mark)?;
        Ok(self)
    }

    pub fn remove_mark(
        &mut self,
        from: usize,
        to: usize,
        matching: &MarkMatch,
    ) -> TransformResult<&mut Self> {
        mark::remove_mark(self, from, to, matching)?;
        Ok(self)
    }

    /// Removes content and marks of the node at `pos` that `parent_type` does not allow.
    pub fn clear_incompatible(
        &mut self,
        pos: usize,
        parent_type: &NodeType,
        content_match: Option<ContentMatch>,
    ) -> TransformResult<&mut Self> {
        mark::clear_incompatible(self, pos, parent_type, content_match, true)?;
        Ok(self)
    }
}
