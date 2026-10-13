//! State queries (Tiptap's helpers) and command composition.

use std::sync::Arc;

use openrich_model::{Mark, MarkType, Node, NodeType, ResolvedPos};
use openrich_state::{Command, Dispatch, EditorState, Transaction, ViewContext};
use serde_json::{Map, Value};

/// `attrs` is a subset of `actual` (loose, like Tiptap's `objectIncludes`).
pub fn attrs_include(actual: &Map<String, Value>, attrs: &Map<String, Value>) -> bool {
    attrs
        .iter()
        .all(|(key, value)| actual.get(key) == Some(value))
}

/// Whether `mark_type` (with `attrs`) covers the whole selection, or is
/// stored/active at the cursor.
pub fn is_mark_active(
    state: &EditorState,
    mark_type: &MarkType,
    attrs: &Map<String, Value>,
) -> bool {
    let selection = state.selection();
    if selection.empty() {
        let marks = state
            .stored_marks()
            .cloned()
            .unwrap_or_else(|| selection.from_pos().marks());
        return marks
            .iter()
            .any(|m| m.mark_type() == mark_type && attrs_include(m.attrs(), attrs));
    }
    let (from, to) = (selection.from(), selection.to());
    let mut selection_range = 0;
    let mut ranges: Vec<(Mark, usize)> = Vec::new();
    state.doc().nodes_between(from, to, &mut |node, pos, _, _| {
        if !node.is_text() && node.marks().is_empty() {
            return true;
        }
        let range = (pos + node.node_size()).min(to) - pos.max(from);
        selection_range += range;
        ranges.extend(node.marks().iter().map(|m| (m.clone(), range)));
        true
    });
    if selection_range == 0 {
        return false;
    }
    let matched: usize = ranges
        .iter()
        .filter(|(m, _)| m.mark_type() == mark_type && attrs_include(m.attrs(), attrs))
        .map(|(_, range)| range)
        .sum();
    let excluded: usize = ranges
        .iter()
        .filter(|(m, _)| m.mark_type() != mark_type && m.mark_type().excludes(mark_type))
        .map(|(_, range)| range)
        .sum();
    let range = if matched > 0 {
        matched + excluded
    } else {
        matched
    };
    range >= selection_range
}

/// Whether nodes of `node_type` (with `attrs`) cover the selection.
pub fn is_node_active(
    state: &EditorState,
    node_type: &NodeType,
    attrs: &Map<String, Value>,
) -> bool {
    let selection = state.selection();
    let (from, to) = (selection.from(), selection.to());
    let mut matched = 0;
    let mut any = false;
    state.doc().nodes_between(from, to, &mut |node, pos, _, _| {
        if node.is_text() {
            return true;
        }
        if node.node_type() == node_type && attrs_include(node.attrs(), attrs) {
            any = true;
            matched += (pos + node.node_size()).min(to) - pos.max(from);
        }
        true
    });
    if selection.empty() {
        // `nodes_between` skips the node a collapsed cursor sits in when it
        // is at the node's very start, so also check the ancestors.
        let head = selection.head_pos();
        return any
            || (0..=head.depth()).rev().any(|d| {
                head.node(d).node_type() == node_type && attrs_include(head.node(d).attrs(), attrs)
            });
    }
    matched >= to - from
}

/// The range around `pos` covered by `mark_type` (optionally with `attrs`).
pub fn get_mark_range(
    pos: &ResolvedPos,
    mark_type: &MarkType,
    attrs: Option<&Map<String, Value>>,
) -> Option<(usize, usize)> {
    let parent = pos.parent();
    let has = |node: &Node| node.marks().iter().any(|m| m.mark_type() == mark_type);
    let mut start = parent.child_after(pos.parent_offset()).ok()?;
    if !start.node.as_ref().is_some_and(has) {
        start = parent.child_before(pos.parent_offset()).ok()?;
    }
    let node = start.node.filter(has)?;
    let mark = match attrs {
        Some(attrs) => node
            .marks()
            .iter()
            .find(|m| m.mark_type() == mark_type && attrs_include(m.attrs(), attrs))?
            .clone(),
        None => mark_type.is_in_set(node.marks())?.clone(),
    };
    let in_set = |node: &Node| mark.is_in_set(node.marks());
    let mut start_index = start.index;
    let mut start_pos = pos.start(pos.depth()) + start.offset;
    let mut end_index = start_index + 1;
    let mut end_pos = start_pos + node.node_size();
    while start_index > 0 && in_set(parent.child(start_index - 1)) {
        start_index -= 1;
        start_pos -= parent.child(start_index).node_size();
    }
    while end_index < parent.child_count() && in_set(parent.child(end_index)) {
        end_pos += parent.child(end_index).node_size();
        end_index += 1;
    }
    Some((start_pos, end_pos))
}

/// Attributes of the first mark of `mark_type` in the selection (or at the cursor).
pub fn get_mark_attributes(state: &EditorState, mark_type: &MarkType) -> Map<String, Value> {
    let selection = state.selection();
    let mut marks: Vec<Mark> = Vec::new();
    if selection.empty() {
        let at = state
            .stored_marks()
            .cloned()
            .unwrap_or_else(|| selection.from_pos().marks());
        marks.extend(at.iter().cloned());
    } else {
        state
            .doc()
            .nodes_between(selection.from(), selection.to(), &mut |node, _, _, _| {
                marks.extend(node.marks().iter().cloned());
                true
            });
    }
    marks
        .into_iter()
        .find(|m| m.mark_type() == mark_type)
        .map(|m| (**m.attrs()).clone())
        .unwrap_or_default()
}

/// Attributes of the innermost node of `node_type` in the selection.
pub fn get_node_attributes(state: &EditorState, node_type: &NodeType) -> Map<String, Value> {
    let selection = state.selection();
    let mut found: Option<Node> = None;
    state
        .doc()
        .nodes_between(selection.from(), selection.to(), &mut |node, _, _, _| {
            if node.node_type() == node_type {
                found = Some(node.clone());
            }
            true
        });
    if found.is_none() {
        let head = selection.head_pos();
        found = (0..=head.depth())
            .rev()
            .map(|d| head.node(d))
            .find(|node| node.node_type() == node_type)
            .cloned();
    }
    found
        .map(|node| (**node.attrs()).clone())
        .unwrap_or_default()
}

/// A parent of a position, as found by [`find_parent_node`].
#[derive(Debug, Clone)]
pub struct ParentNode {
    /// Position before the node.
    pub pos: usize,
    /// Start of the node's content.
    pub start: usize,
    pub depth: usize,
    pub node: Node,
}

/// The closest ancestor of `pos` (excluding the document) matching `predicate`.
pub fn find_parent_node(
    pos: &ResolvedPos,
    predicate: impl Fn(&Node) -> bool,
) -> Option<ParentNode> {
    (1..=pos.depth()).rev().find_map(|depth| {
        let node = pos.node(depth);
        predicate(node).then(|| ParentNode {
            pos: pos.before(depth),
            start: pos.start(depth),
            depth,
            node: node.clone(),
        })
    })
}

/// Appends the steps, selection, stored marks and metadata of `sub`
/// (made against `target.doc`) to `target`.
fn merge_transaction(target: &mut Transaction, sub: Transaction) -> bool {
    for step in sub.steps.iter().cloned() {
        if target.step(step).is_err() {
            return false;
        }
    }
    if sub.selection_set() || sub.doc_changed() {
        let bookmark = sub.selection().bookmark();
        let selection = bookmark.resolve(&target.doc);
        target.set_selection(selection);
    }
    if sub.stored_marks_set() {
        target.set_stored_marks(sub.stored_marks().cloned());
    }
    if sub.scrolled_into_view() {
        target.scroll_into_view();
    }
    target.copy_meta_from(&sub);
    true
}

/// Runs `commands` in order as one transaction, like Tiptap's
/// `chain()...run()`: each sees the state left by the previous ones, all
/// changes are dispatched together, and the result is `true` only if every
/// command applied. A dry run (`dispatch = None`) applies the commands to a
/// scratch state in the same way, so a command that depends on an earlier
/// one (`clearNodes` then `wrapInList`) is judged as it would run.
pub fn chain(commands: Vec<Command>) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, view: Option<&dyn ViewContext>| {
            let Some(dispatch) = dispatch else {
                if let [command] = commands.as_slice() {
                    return command(state, None, view);
                }
                let mut current = state.clone();
                let mut all = true;
                for cmd in &commands {
                    let mut produced: Option<Transaction> = None;
                    all &= cmd(&current, Some(&mut |tr| produced = Some(tr)), view);
                    if let Some(next) = produced.and_then(|tr| current.apply(tr).ok()) {
                        current = next;
                    }
                }
                return all;
            };
            let mut current = state.clone();
            let mut combined = state.tr();
            let mut any = false;
            let mut all = true;
            for cmd in &commands {
                let mut produced: Option<Transaction> = None;
                let ok = cmd(&current, Some(&mut |tr| produced = Some(tr)), view);
                all &= ok;
                if let Some(tr) = produced {
                    let Ok(next) = current.apply(tr.clone()) else {
                        all = false;
                        continue;
                    };
                    if !merge_transaction(&mut combined, tr) {
                        all = false;
                        continue;
                    }
                    current = next;
                    any = true;
                }
            }
            if any {
                dispatch(combined);
            }
            all
        },
    )
}

/// Runs `commands` until one applies (Tiptap's `first`).
pub fn first(commands: Vec<Command>) -> Command {
    openrich_state::chain_commands(commands)
}
