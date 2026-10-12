//! Atomic document changes (`step.ts`, `replace_step.ts`, `mark_step.ts`,
//! `attr_step.ts`).

use openrich_model::{Fragment, Mark, Node, Schema, Slice};
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::map::{Assoc, Mappable, StepMap};

/// A step could not be applied to the given document.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct StepError(pub String);

impl StepError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl From<openrich_model::ModelError> for StepError {
    fn from(error: openrich_model::ModelError) -> Self {
        Self(error.to_string())
    }
}

/// Replaces `from..to` with a slice. `structure` steps may only move
/// node boundaries, never overwrite content.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplaceStep {
    pub from: usize,
    pub to: usize,
    pub slice: Slice,
    pub structure: bool,
}

/// Replaces `from..to` with a slice, keeping `gap_from..gap_to` and
/// inserting it into the slice at `insert`. Used to wrap and unwrap.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplaceAroundStep {
    pub from: usize,
    pub to: usize,
    pub gap_from: usize,
    pub gap_to: usize,
    pub slice: Slice,
    pub insert: usize,
    pub structure: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarkStep {
    pub from: usize,
    pub to: usize,
    pub mark: Mark,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeMarkStep {
    pub pos: usize,
    pub mark: Mark,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttrStep {
    pub pos: usize,
    pub attr: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocAttrStep {
    pub attr: String,
    pub value: Value,
}

/// An atomic, invertible, mappable document change.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Replace(ReplaceStep),
    ReplaceAround(ReplaceAroundStep),
    AddMark(MarkStep),
    RemoveMark(MarkStep),
    AddNodeMark(NodeMarkStep),
    RemoveNodeMark(NodeMarkStep),
    Attr(AttrStep),
    DocAttr(DocAttrStep),
}

fn from_replace(doc: &Node, from: usize, to: usize, slice: &Slice) -> Result<Node, StepError> {
    doc.replace(from, to, slice).map_err(StepError::from)
}

/// Whether `from..to` contains anything besides node boundaries.
fn content_between(doc: &Node, from: usize, to: usize) -> Result<bool, StepError> {
    let resolved = doc.resolve(from)?;
    let mut dist = to.saturating_sub(from);
    let mut depth = resolved.depth();
    while dist > 0 && depth > 0 && resolved.index_after(depth) == resolved.node(depth).child_count()
    {
        depth -= 1;
        dist -= 1;
    }
    if dist > 0 {
        let mut next = resolved
            .node(depth)
            .maybe_child(resolved.index_after(depth))
            .cloned();
        while dist > 0 {
            match next {
                Some(node) if !node.is_leaf() => next = node.first_child().cloned(),
                _ => return Ok(true),
            }
            dist -= 1;
        }
    }
    Ok(false)
}

fn map_fragment(fragment: &Fragment, f: &dyn Fn(&Node, &Node) -> Node, parent: &Node) -> Fragment {
    let mut mapped = Vec::with_capacity(fragment.child_count());
    for child in fragment {
        let mut child = child.clone();
        if child.content().size() > 0 {
            let inner = map_fragment(child.content(), f, &child);
            child = child.copy(inner);
        }
        if child.is_inline() {
            child = f(&child, parent);
        }
        mapped.push(child);
    }
    Fragment::from_array(mapped)
}

/// Replaces the node at `pos` with a copy carrying new attrs or marks,
/// keeping its content (an open slice preserves the children).
fn replace_node_markup(
    doc: &Node,
    pos: usize,
    attrs: &Map<String, Value>,
    marks: openrich_model::MarkSet,
) -> Result<Node, StepError> {
    let node = doc
        .node_at(pos)
        .ok_or_else(|| StepError::new("no node at the step's position"))?;
    let updated = node
        .node_type()
        .create(Some(attrs), Fragment::empty(), marks)?;
    let open = usize::from(!node.is_leaf());
    from_replace(
        doc,
        pos,
        pos + 1,
        &Slice::new(Fragment::from_node(updated), 0, open),
    )
}

impl Step {
    pub fn replace(from: usize, to: usize, slice: Slice) -> Self {
        Step::Replace(ReplaceStep {
            from,
            to,
            slice,
            structure: false,
        })
    }

    /// Applies the step, returning the new document.
    pub fn apply(&self, doc: &Node) -> Result<Node, StepError> {
        match self {
            Step::Replace(step) => {
                if step.structure && content_between(doc, step.from, step.to)? {
                    return Err(StepError::new("structure replace would overwrite content"));
                }
                from_replace(doc, step.from, step.to, &step.slice)
            }
            Step::ReplaceAround(step) => {
                if step.structure
                    && (content_between(doc, step.from, step.gap_from)?
                        || content_between(doc, step.gap_to, step.to)?)
                {
                    return Err(StepError::new(
                        "structure gap-replace would overwrite content",
                    ));
                }
                let gap = doc.slice(step.gap_from, step.gap_to, false)?;
                if gap.open_start > 0 || gap.open_end > 0 {
                    return Err(StepError::new("gap is not a flat range"));
                }
                let inserted = step
                    .slice
                    .insert_at(step.insert, &gap.content)
                    .ok_or_else(|| StepError::new("content does not fit in gap"))?;
                from_replace(doc, step.from, step.to, &inserted)
            }
            Step::AddMark(step) => {
                let old = doc.slice(step.from, step.to, false)?;
                let resolved = doc.resolve(step.from)?;
                let parent = resolved.node(resolved.shared_depth(step.to)).clone();
                let mark = &step.mark;
                let content = map_fragment(
                    &old.content,
                    &|node, parent| {
                        if !node.is_atom() || !parent.node_type().allows_mark_type(mark.mark_type())
                        {
                            node.clone()
                        } else {
                            node.mark(mark.add_to_set(node.marks()))
                        }
                    },
                    &parent,
                );
                from_replace(
                    doc,
                    step.from,
                    step.to,
                    &Slice::new(content, old.open_start, old.open_end),
                )
            }
            Step::RemoveMark(step) => {
                let old = doc.slice(step.from, step.to, false)?;
                let mark = &step.mark;
                let content = map_fragment(
                    &old.content,
                    &|node, _| node.mark(mark.remove_from_set(node.marks())),
                    doc,
                );
                from_replace(
                    doc,
                    step.from,
                    step.to,
                    &Slice::new(content, old.open_start, old.open_end),
                )
            }
            Step::AddNodeMark(step) => {
                let node = doc
                    .node_at(step.pos)
                    .ok_or_else(|| StepError::new("no node at mark step's position"))?;
                replace_node_markup(
                    doc,
                    step.pos,
                    node.attrs(),
                    step.mark.add_to_set(node.marks()),
                )
            }
            Step::RemoveNodeMark(step) => {
                let node = doc
                    .node_at(step.pos)
                    .ok_or_else(|| StepError::new("no node at mark step's position"))?;
                replace_node_markup(
                    doc,
                    step.pos,
                    node.attrs(),
                    step.mark.remove_from_set(node.marks()),
                )
            }
            Step::Attr(step) => {
                let node = doc
                    .node_at(step.pos)
                    .ok_or_else(|| StepError::new("no node at attribute step's position"))?;
                let mut attrs = (**node.attrs()).clone();
                attrs.insert(step.attr.clone(), step.value.clone());
                replace_node_markup(doc, step.pos, &attrs, node.marks().clone())
            }
            Step::DocAttr(step) => {
                let mut attrs = (**doc.attrs()).clone();
                attrs.insert(step.attr.clone(), step.value.clone());
                Ok(doc.node_type().create(
                    Some(&attrs),
                    doc.content().clone(),
                    doc.marks().clone(),
                )?)
            }
        }
    }

    /// The position changes this step makes.
    pub fn get_map(&self) -> StepMap {
        match self {
            Step::Replace(step) => {
                StepMap::new(vec![step.from, step.to - step.from, step.slice.size()])
            }
            Step::ReplaceAround(step) => StepMap::new(vec![
                step.from,
                step.gap_from - step.from,
                step.insert,
                step.gap_to,
                step.to - step.gap_to,
                step.slice.size() - step.insert,
            ]),
            _ => StepMap::empty(),
        }
    }

    /// The step that undoes this one. `doc` is the document before this step.
    pub fn invert(&self, doc: &Node) -> Result<Step, StepError> {
        Ok(match self {
            Step::Replace(step) => Step::Replace(ReplaceStep {
                from: step.from,
                to: step.from + step.slice.size(),
                slice: doc.slice(step.from, step.to, false)?,
                structure: false,
            }),
            Step::ReplaceAround(step) => {
                let gap = step.gap_to - step.gap_from;
                let to = step.from + step.slice.size() + gap;
                let natural = Step::ReplaceAround(ReplaceAroundStep {
                    from: step.from,
                    to,
                    gap_from: step.from + step.insert,
                    gap_to: step.from + step.insert + gap,
                    slice: doc
                        .slice(step.from, step.to, false)?
                        .remove_between(step.gap_from - step.from, step.gap_to - step.from)?,
                    insert: step.gap_from - step.from,
                    structure: step.structure,
                });
                // ProseMirror's gap inverse can be unapplicable: lifting a
                // non-first child out of a list item reinserts it into an
                // empty item that must start with a paragraph. A plain
                // replace of the whole range always restores the original.
                let after = self.apply(doc)?;
                if natural.apply(&after).is_ok() {
                    natural
                } else {
                    Step::Replace(ReplaceStep {
                        from: step.from,
                        to,
                        slice: doc.slice(step.from, step.to, false)?,
                        structure: false,
                    })
                }
            }
            Step::AddMark(step) => Step::RemoveMark(step.clone()),
            Step::RemoveMark(step) => Step::AddMark(step.clone()),
            Step::AddNodeMark(step) => {
                if let Some(node) = doc.node_at(step.pos) {
                    let new_set = step.mark.add_to_set(node.marks());
                    if new_set.len() == node.marks().len() {
                        let replaced = node.marks().iter().find(|mark| !mark.is_in_set(&new_set));
                        return Ok(Step::AddNodeMark(NodeMarkStep {
                            pos: step.pos,
                            mark: replaced.unwrap_or(&step.mark).clone(),
                        }));
                    }
                }
                Step::RemoveNodeMark(step.clone())
            }
            Step::RemoveNodeMark(step) => match doc.node_at(step.pos) {
                Some(node) if step.mark.is_in_set(node.marks()) => Step::AddNodeMark(step.clone()),
                _ => self.clone(),
            },
            Step::Attr(step) => {
                let node = doc
                    .node_at(step.pos)
                    .ok_or_else(|| StepError::new("no node at attribute step's position"))?;
                Step::Attr(AttrStep {
                    pos: step.pos,
                    attr: step.attr.clone(),
                    value: node.attr(&step.attr).cloned().unwrap_or(Value::Null),
                })
            }
            Step::DocAttr(step) => Step::DocAttr(DocAttrStep {
                attr: step.attr.clone(),
                value: doc.attr(&step.attr).cloned().unwrap_or(Value::Null),
            }),
        })
    }

    /// This step rebased over `mapping`, or `None` if it was deleted.
    pub fn map(&self, mapping: &dyn Mappable) -> Option<Step> {
        match self {
            Step::Replace(step) => {
                let from = mapping.map_result(step.from, Assoc::After);
                let to = mapping.map_result(step.to, Assoc::Before);
                if from.deleted_across() && to.deleted_across() {
                    return None;
                }
                Some(Step::Replace(ReplaceStep {
                    from: from.pos,
                    to: from.pos.max(to.pos),
                    slice: step.slice.clone(),
                    structure: step.structure,
                }))
            }
            Step::ReplaceAround(step) => {
                let from = mapping.map_result(step.from, Assoc::After);
                let to = mapping.map_result(step.to, Assoc::Before);
                let gap_from = if step.from == step.gap_from {
                    from.pos
                } else {
                    mapping.map(step.gap_from, Assoc::Before)
                };
                let gap_to = if step.to == step.gap_to {
                    to.pos
                } else {
                    mapping.map(step.gap_to, Assoc::After)
                };
                if (from.deleted_across() && to.deleted_across())
                    || gap_from < from.pos
                    || gap_to > to.pos
                {
                    return None;
                }
                Some(Step::ReplaceAround(ReplaceAroundStep {
                    from: from.pos,
                    to: to.pos,
                    gap_from,
                    gap_to,
                    slice: step.slice.clone(),
                    insert: step.insert,
                    structure: step.structure,
                }))
            }
            Step::AddMark(step) | Step::RemoveMark(step) => {
                let from = mapping.map_result(step.from, Assoc::After);
                let to = mapping.map_result(step.to, Assoc::Before);
                if (from.deleted() && to.deleted()) || from.pos >= to.pos {
                    return None;
                }
                let mapped = MarkStep {
                    from: from.pos,
                    to: to.pos,
                    mark: step.mark.clone(),
                };
                Some(if matches!(self, Step::AddMark(_)) {
                    Step::AddMark(mapped)
                } else {
                    Step::RemoveMark(mapped)
                })
            }
            Step::AddNodeMark(step) | Step::RemoveNodeMark(step) => {
                let pos = mapping.map_result(step.pos, Assoc::After);
                if pos.deleted_after() {
                    return None;
                }
                let mapped = NodeMarkStep {
                    pos: pos.pos,
                    mark: step.mark.clone(),
                };
                Some(if matches!(self, Step::AddNodeMark(_)) {
                    Step::AddNodeMark(mapped)
                } else {
                    Step::RemoveNodeMark(mapped)
                })
            }
            Step::Attr(step) => {
                let pos = mapping.map_result(step.pos, Assoc::After);
                (!pos.deleted_after()).then(|| {
                    Step::Attr(AttrStep {
                        pos: pos.pos,
                        attr: step.attr.clone(),
                        value: step.value.clone(),
                    })
                })
            }
            Step::DocAttr(_) => Some(self.clone()),
        }
    }

    /// Combines this step with `other` (applied right after it), if possible.
    pub fn merge(&self, other: &Step) -> Option<Step> {
        match (self, other) {
            (Step::Replace(a), Step::Replace(b)) => {
                if a.structure || b.structure {
                    return None;
                }
                let empty = a.slice.size() + b.slice.size() == 0;
                if a.from + a.slice.size() == b.from
                    && a.slice.open_end == 0
                    && b.slice.open_start == 0
                {
                    let slice = if empty {
                        Slice::empty()
                    } else {
                        Slice::new(
                            a.slice.content.append(&b.slice.content),
                            a.slice.open_start,
                            b.slice.open_end,
                        )
                    };
                    Some(Step::Replace(ReplaceStep {
                        from: a.from,
                        to: a.to + (b.to - b.from),
                        slice,
                        structure: false,
                    }))
                } else if b.to == a.from && a.slice.open_start == 0 && b.slice.open_end == 0 {
                    let slice = if empty {
                        Slice::empty()
                    } else {
                        Slice::new(
                            b.slice.content.append(&a.slice.content),
                            b.slice.open_start,
                            a.slice.open_end,
                        )
                    };
                    Some(Step::Replace(ReplaceStep {
                        from: b.from,
                        to: a.to,
                        slice,
                        structure: false,
                    }))
                } else {
                    None
                }
            }
            (Step::AddMark(a), Step::AddMark(b)) | (Step::RemoveMark(a), Step::RemoveMark(b))
                if a.mark == b.mark && a.from <= b.to && a.to >= b.from =>
            {
                let merged = MarkStep {
                    from: a.from.min(b.from),
                    to: a.to.max(b.to),
                    mark: a.mark.clone(),
                };
                Some(if matches!(self, Step::AddMark(_)) {
                    Step::AddMark(merged)
                } else {
                    Step::RemoveMark(merged)
                })
            }
            _ => None,
        }
    }

    /// Serializes the step in ProseMirror's JSON format.
    pub fn to_json(&self) -> Value {
        let with_slice = |mut object: Map<String, Value>, slice: &Slice, structure: bool| {
            if slice.size() > 0 {
                object.insert("slice".into(), slice.to_json());
            }
            if structure {
                object.insert("structure".into(), json!(true));
            }
            Value::Object(object)
        };
        match self {
            Step::Replace(step) => {
                let object = json!({"stepType": "replace", "from": step.from, "to": step.to});
                with_slice(
                    object.as_object().cloned().unwrap_or_default(),
                    &step.slice,
                    step.structure,
                )
            }
            Step::ReplaceAround(step) => {
                let object = json!({
                    "stepType": "replaceAround", "from": step.from, "to": step.to,
                    "gapFrom": step.gap_from, "gapTo": step.gap_to, "insert": step.insert,
                });
                with_slice(
                    object.as_object().cloned().unwrap_or_default(),
                    &step.slice,
                    step.structure,
                )
            }
            Step::AddMark(step) | Step::RemoveMark(step) => json!({
                "stepType": if matches!(self, Step::AddMark(_)) { "addMark" } else { "removeMark" },
                "mark": step.mark.to_json(), "from": step.from, "to": step.to,
            }),
            Step::AddNodeMark(step) | Step::RemoveNodeMark(step) => json!({
                "stepType": if matches!(self, Step::AddNodeMark(_)) { "addNodeMark" } else { "removeNodeMark" },
                "pos": step.pos, "mark": step.mark.to_json(),
            }),
            Step::Attr(step) => {
                json!({"stepType": "attr", "pos": step.pos, "attr": step.attr, "value": step.value})
            }
            Step::DocAttr(step) => {
                json!({"stepType": "docAttr", "attr": step.attr, "value": step.value})
            }
        }
    }

    pub fn from_json(schema: &Schema, value: &Value) -> Result<Step, StepError> {
        let object = value
            .as_object()
            .ok_or_else(|| StepError::new("step must be an object"))?;
        let num = |key: &str| -> Result<usize, StepError> {
            object
                .get(key)
                .and_then(Value::as_u64)
                .map(|n| n as usize)
                .ok_or_else(|| StepError::new(format!("step needs a numeric `{key}`")))
        };
        let text = |key: &str| -> Result<String, StepError> {
            object
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| StepError::new(format!("step needs a string `{key}`")))
        };
        let slice = || schema.slice_from_json(object.get("slice").unwrap_or(&Value::Null));
        let mark = || schema.mark_from_json(object.get("mark").unwrap_or(&Value::Null));
        let structure = object
            .get("structure")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let value_of = || object.get("value").cloned().unwrap_or(Value::Null);
        Ok(match text("stepType")?.as_str() {
            "replace" => Step::Replace(ReplaceStep {
                from: num("from")?,
                to: num("to")?,
                slice: slice()?,
                structure,
            }),
            "replaceAround" => Step::ReplaceAround(ReplaceAroundStep {
                from: num("from")?,
                to: num("to")?,
                gap_from: num("gapFrom")?,
                gap_to: num("gapTo")?,
                slice: slice()?,
                insert: num("insert")?,
                structure,
            }),
            "addMark" => Step::AddMark(MarkStep {
                from: num("from")?,
                to: num("to")?,
                mark: mark()?,
            }),
            "removeMark" => Step::RemoveMark(MarkStep {
                from: num("from")?,
                to: num("to")?,
                mark: mark()?,
            }),
            "addNodeMark" => Step::AddNodeMark(NodeMarkStep {
                pos: num("pos")?,
                mark: mark()?,
            }),
            "removeNodeMark" => Step::RemoveNodeMark(NodeMarkStep {
                pos: num("pos")?,
                mark: mark()?,
            }),
            "attr" => Step::Attr(AttrStep {
                pos: num("pos")?,
                attr: text("attr")?,
                value: value_of(),
            }),
            "docAttr" => Step::DocAttr(DocAttrStep {
                attr: text("attr")?,
                value: value_of(),
            }),
            other => return Err(StepError::new(format!("unknown step type `{other}`"))),
        })
    }
}
