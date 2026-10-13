//! Tiptap's core commands, built on the ProseMirror-style commands.

use std::sync::Arc;

use rs_editor_commands as pm;
use rs_editor_model::{Fragment, MarkSet, MarkType, Node, NodeType, Slice};
use rs_editor_state::{
    Command, Dispatch, EditorState, NodeSelection, Selection, TextSelection, ViewContext, command,
};
use rs_editor_transform::{Assoc, Mappable, MarkMatch, can_join, lift_target};
use serde_json::{Map, Value};

use crate::helpers::{
    chain, find_parent_node, get_mark_attributes, get_mark_range, is_mark_active, is_node_active,
};

type View<'a> = Option<&'a dyn ViewContext>;

fn merged(base: Option<&Map<String, Value>>, extra: &Map<String, Value>) -> Map<String, Value> {
    let mut attrs = base.cloned().unwrap_or_default();
    for (key, value) in extra {
        attrs.insert(key.clone(), value.clone());
    }
    attrs
}

/// Whether `mark_type` can be added here: the parent allows it and no
/// present mark excludes it (Tiptap's `canSetMark`).
pub fn can_set_mark(state: &EditorState, mark_type: &MarkType) -> bool {
    let compatible = |marks: &[rs_editor_model::Mark]| {
        mark_type.is_in_set(marks).is_some()
            || !marks.iter().any(|m| m.mark_type().excludes(mark_type))
    };
    let selection = state.selection();
    if let Some(cursor) = selection.cursor() {
        let marks = state
            .stored_marks()
            .cloned()
            .unwrap_or_else(|| cursor.marks());
        return cursor.parent().node_type().allows_mark_type(mark_type) && compatible(&marks);
    }
    let from = selection.from_pos();
    let mut supported = from.depth() == 0
        && state.doc().inline_content()
        && state.doc().node_type().allows_mark_type(mark_type);
    state
        .doc()
        .nodes_between(from.pos(), selection.to(), &mut |node, _, parent, _| {
            if supported {
                return false;
            }
            if node.is_inline() {
                let parent_allows =
                    parent.is_none_or(|p| p.node_type().allows_mark_type(mark_type));
                supported = parent_allows && compatible(node.marks());
            }
            !supported
        });
    supported
}

/// Adds `mark_type` with `attrs` to the selection (or the stored marks),
/// merging with attributes of existing marks of that type.
pub fn set_mark(mark_type: MarkType, attrs: Map<String, Value>) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let selection = state.selection();
            if !can_set_mark(state, &mark_type) {
                return false;
            }
            let Some(dispatch) = dispatch else {
                return true;
            };
            let mut tr = state.tr();
            if selection.empty() {
                let attrs = merged(Some(&get_mark_attributes(state, &mark_type)), &attrs);
                let Ok(mark) = mark_type.create(Some(&attrs)) else {
                    return false;
                };
                tr.add_stored_mark(&mark);
            } else {
                let (from, to) = (selection.from(), selection.to());
                let mut ranges: Vec<(usize, usize, Map<String, Value>)> = Vec::new();
                state.doc().nodes_between(from, to, &mut |node, pos, _, _| {
                    let start = pos.max(from);
                    let end = (pos + node.node_size()).min(to);
                    match mark_type.is_in_set(node.marks()) {
                        Some(existing) => {
                            ranges.push((start, end, merged(Some(existing.attrs()), &attrs)))
                        }
                        None => ranges.push((start, end, attrs.clone())),
                    }
                    true
                });
                for (start, end, attrs) in ranges {
                    let Ok(mark) = mark_type.create(Some(&attrs)) else {
                        return false;
                    };
                    if tr.add_mark(start, end, &mark).is_err() {
                        return false;
                    }
                }
            }
            dispatch(tr);
            true
        },
    )
}

/// Removes `mark_type` from the selection; with `extend_empty_mark_range`
/// a cursor removes the whole mark around it.
pub fn unset_mark(mark_type: MarkType, extend_empty_mark_range: bool) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let Some(dispatch) = dispatch else {
                return true;
            };
            let selection = state.selection();
            let (mut from, mut to) = (selection.from(), selection.to());
            if selection.empty() && extend_empty_mark_range {
                let attrs = selection
                    .from_pos()
                    .marks()
                    .iter()
                    .find(|m| m.mark_type() == &mark_type)
                    .map(|m| (**m.attrs()).clone());
                if let Some((start, end)) =
                    get_mark_range(selection.from_pos(), &mark_type, attrs.as_ref())
                {
                    from = start;
                    to = end;
                }
            }
            let mut tr = state.tr();
            if tr
                .remove_mark(from, to, &MarkMatch::Type(mark_type.clone()))
                .is_err()
            {
                return false;
            }
            tr.remove_stored_mark_type(&mark_type);
            dispatch(tr);
            true
        },
    )
}

/// Tiptap's `toggleMark`: unsets the mark when it covers the selection,
/// sets it otherwise.
pub fn toggle_mark(
    mark_type: MarkType,
    attrs: Map<String, Value>,
    extend_empty_mark_range: bool,
) -> Command {
    Arc::new(move |state: &EditorState, dispatch: Dispatch, view: View| {
        if is_mark_active(state, &mark_type, &attrs) {
            unset_mark(mark_type.clone(), extend_empty_mark_range)(state, dispatch, view)
        } else {
            set_mark(mark_type.clone(), attrs.clone())(state, dispatch, view)
        }
    })
}

/// Removes all marks from the selection.
pub fn unset_all_marks(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let selection = state.selection();
    if selection.empty() {
        return false;
    }
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        if tr
            .remove_mark(selection.from(), selection.to(), &MarkMatch::All)
            .is_err()
        {
            return false;
        }
        dispatch(tr);
    }
    true
}

/// Selects the whole range of `mark_type` around the cursor.
pub fn extend_mark_range(mark_type: MarkType, attrs: Option<Map<String, Value>>) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let selection = state.selection();
            let Some((from, to)) = get_mark_range(selection.from_pos(), &mark_type, attrs.as_ref())
            else {
                return true;
            };
            if selection.from_pos().pos() >= from
                && selection.to() <= to
                && let Some(dispatch) = dispatch
            {
                let mut tr = state.tr();
                match TextSelection::create(state.doc(), from, to) {
                    Ok(range) => tr.set_selection(range),
                    Err(_) => return false,
                };
                dispatch(tr);
            }
            true
        },
    )
}

/// Lifts selected blocks out of wrappers and resets textblocks to the
/// default block type (Tiptap's `clearNodes`).
pub fn clear_nodes(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let Some(dispatch) = dispatch else {
        return true;
    };
    let selection = state.selection();
    let mut tr = state.tr();
    let doc = state.doc().clone();
    doc.nodes_between(selection.from(), selection.to(), &mut |node, pos, _, _| {
        if node.is_text() {
            return true;
        }
        let from = tr.mapping.map(pos, Assoc::After);
        let to = tr.mapping.map(pos + node.node_size(), Assoc::After);
        let (Ok(rfrom), Ok(rto)) = (tr.doc.resolve(from), tr.doc.resolve(to)) else {
            return true;
        };
        let Some(range) = rfrom.block_range(&rto, None) else {
            return true;
        };
        let target = lift_target(&range);
        if node.is_textblock()
            && let Some(default_type) = rfrom
                .parent()
                .content_match_at(rfrom.index(rfrom.depth()))
                .ok()
                .and_then(|m| m.default_type())
        {
            let _ = tr.set_node_markup(range.start(), Some(&default_type), None, None);
        }
        if let Some(target) = target {
            let _ = tr.lift(&range, target);
        }
        true
    });
    dispatch(tr);
    true
}

fn shared_parent_attrs(state: &EditorState) -> Option<Map<String, Value>> {
    let selection = state.selection();
    selection
        .anchor_pos()
        .same_parent(selection.head_pos())
        .then(|| (**selection.anchor_pos().parent().attrs()).clone())
}

/// Turns the selected textblocks into `node_type`, clearing wrappers that
/// prevent it (Tiptap's `setNode`). Attributes of the current block (such
/// as alignment) carry over.
pub fn set_node(node_type: NodeType, attrs: Map<String, Value>) -> Command {
    Arc::new(move |state: &EditorState, dispatch: Dispatch, view: View| {
        if !node_type.is_textblock() {
            return false;
        }
        let attrs = merged(shared_parent_attrs(state).as_ref(), &attrs);
        let set = pm::set_block_type(node_type.clone(), Some(attrs));
        if set(state, None, view) {
            return set(state, dispatch, view);
        }
        chain(vec![command(clear_nodes), set])(state, dispatch, view)
    })
}

/// Sets `node_type`, or `toggle_type` when the selection already has it.
pub fn toggle_node(
    node_type: NodeType,
    toggle_type: NodeType,
    attrs: Map<String, Value>,
) -> Command {
    Arc::new(move |state: &EditorState, dispatch: Dispatch, view: View| {
        let copied = shared_parent_attrs(state).unwrap_or_default();
        if is_node_active(state, &node_type, &attrs) {
            set_node(toggle_type.clone(), copied)(state, dispatch, view)
        } else {
            set_node(node_type.clone(), merged(Some(&copied), &attrs))(state, dispatch, view)
        }
    })
}

/// Lifts the selection out of `node_type` if it is inside one.
pub fn lift_from(node_type: NodeType, attrs: Map<String, Value>) -> Command {
    Arc::new(move |state: &EditorState, dispatch: Dispatch, view: View| {
        is_node_active(state, &node_type, &attrs) && pm::lift(state, dispatch, view)
    })
}

/// Wraps in `node_type`, or lifts out of it when already inside.
pub fn toggle_wrap(node_type: NodeType, attrs: Map<String, Value>) -> Command {
    Arc::new(move |state: &EditorState, dispatch: Dispatch, view: View| {
        if is_node_active(state, &node_type, &attrs) {
            pm::lift(state, dispatch, view)
        } else {
            pm::wrap_in(node_type.clone(), Some(attrs.clone()))(state, dispatch, view)
        }
    })
}

fn is_list(node: &Node) -> bool {
    node.node_type().is_in_group("list")
}

fn join_list(list_type: NodeType, backwards: bool) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let Some(list) = find_parent_node(state.selection().from_pos(), |n| {
                n.node_type() == &list_type
            }) else {
                return true;
            };
            let doc = state.doc();
            let join_at = if backwards {
                let Ok(before) = doc.resolve(list.pos.saturating_sub(1)) else {
                    return true;
                };
                if list.depth > before.depth() + 1 || list.depth == 0 {
                    return true;
                }
                let before_pos = before.before(list.depth);
                let same = doc
                    .node_at(before_pos)
                    .is_some_and(|n| n.node_type() == &list_type);
                (same && can_join(doc, list.pos)).then_some(list.pos)
            } else {
                let Ok(start) = doc.resolve(list.start) else {
                    return true;
                };
                let after = start.after(list.depth);
                let same = doc
                    .node_at(after)
                    .is_some_and(|n| n.node_type() == &list_type);
                (same && can_join(doc, after)).then_some(after)
            };
            if let (Some(pos), Some(dispatch)) = (join_at, dispatch) {
                let mut tr = state.tr();
                if tr.join(pos, 1).is_ok() {
                    dispatch(tr);
                }
            }
            true
        },
    )
}

/// Wraps the selection in a list of `list_type`, lifts it out when it is
/// already that list, or converts between list types (Tiptap's `toggleList`).
pub fn toggle_list(list_type: NodeType, item_type: NodeType, attrs: Map<String, Value>) -> Command {
    Arc::new(move |state: &EditorState, dispatch: Dispatch, view: View| {
        let selection = state.selection();
        let Some(range) = selection.from_pos().block_range(selection.to_pos(), None) else {
            return false;
        };
        let parent_list = find_parent_node(selection.from_pos(), is_list);
        if let Some(parent_list) = &parent_list
            && range.depth >= 1
            && range.depth <= parent_list.depth + 1
        {
            if parent_list.node.node_type() == &list_type {
                return pm::lift_list_item(item_type.clone())(state, dispatch, view);
            }
            if list_type.valid_content(parent_list.node.content()) {
                let pos = parent_list.pos;
                let convert: Command = {
                    let list_type = list_type.clone();
                    Arc::new(
                        move |state: &EditorState, dispatch: Dispatch, _view: View| {
                            if let Some(dispatch) = dispatch {
                                let mut tr = state.tr();
                                if tr
                                    .set_node_markup(pos, Some(&list_type), None, None)
                                    .is_err()
                                {
                                    return false;
                                }
                                dispatch(tr);
                            }
                            true
                        },
                    )
                };
                return chain(vec![
                    convert,
                    join_list(list_type.clone(), true),
                    join_list(list_type.clone(), false),
                ])(state, dispatch, view);
            }
        }
        let wrap = pm::wrap_in_list(list_type.clone(), Some(attrs.clone()));
        let prepare: Command = {
            let wrap = wrap.clone();
            Arc::new(move |state: &EditorState, dispatch: Dispatch, view: View| {
                wrap(state, None, view) || clear_nodes(state, dispatch, view)
            })
        };
        chain(vec![
            prepare,
            wrap,
            join_list(list_type.clone(), true),
            join_list(list_type.clone(), false),
        ])(state, dispatch, view)
    })
}

/// Merges `attrs` into every node of `type_name` (or mark) in the selection.
pub fn update_attributes(type_name: String, attrs: Map<String, Value>) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let schema = state.schema();
            let node_type = schema.node_type(&type_name).ok();
            let mark_type = schema.mark_type(&type_name).ok();
            if node_type.is_none() && mark_type.is_none() {
                return false;
            }
            let Some(dispatch) = dispatch else {
                return true;
            };
            let selection = state.selection();
            let mut tr = state.tr();
            let mut nodes: Vec<(usize, Node)> = Vec::new();
            let mut marks: Vec<(usize, usize, rs_editor_model::Mark)> = Vec::new();
            state
                .doc()
                .nodes_between(selection.from(), selection.to(), &mut |node, pos, _, _| {
                    if node_type.as_ref() == Some(node.node_type()) {
                        nodes.push((pos, node.clone()));
                    }
                    if let Some(mark_type) = &mark_type
                        && let Some(mark) = mark_type.is_in_set(node.marks())
                    {
                        let from = pos.max(selection.from());
                        let to = (pos + node.node_size()).min(selection.to());
                        marks.push((from, to, mark.clone()));
                    }
                    true
                });
            if let Some(node_type) = &node_type {
                let head = selection.head_pos();
                for depth in (1..=head.depth()).rev() {
                    let node = head.node(depth);
                    if node.node_type() == node_type
                        && !nodes.iter().any(|(pos, _)| *pos == head.before(depth))
                    {
                        nodes.push((head.before(depth), node.clone()));
                    }
                }
            }
            for (pos, node) in nodes {
                let attrs = merged(Some(node.attrs()), &attrs);
                if tr.set_node_markup(pos, None, Some(&attrs), None).is_err() {
                    return false;
                }
            }
            for (from, to, mark) in marks {
                let attrs = merged(Some(mark.attrs()), &attrs);
                let Ok(updated) = mark.mark_type().create(Some(&attrs)) else {
                    return false;
                };
                if tr.add_mark(from, to, &updated).is_err() {
                    return false;
                }
            }
            dispatch(tr);
            true
        },
    )
}

/// Removes the inline nodes or wraps blocks so `nodes` replace `from..to`.
/// Text-only content is inserted as text with the current marks.
pub fn insert_content_at(from: usize, to: usize, nodes: Vec<Node>) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            if nodes.is_empty() {
                return false;
            }
            let Some(dispatch) = dispatch else {
                return true;
            };
            let (mut from, mut to) = (from, to);
            let only_text = nodes.iter().all(Node::is_text);
            let only_blocks = nodes.iter().all(Node::is_block);
            let mut tr = state.tr();
            if only_blocks
                && from == to
                && let Ok(resolved) = tr.doc.resolve(from)
            {
                let parent = resolved.parent();
                if parent.is_textblock()
                    && !parent.node_type().spec().code
                    && parent.child_count() == 0
                {
                    from -= 1;
                    to += 1;
                }
            }
            let result = if only_text {
                let text: String = nodes.iter().filter_map(Node::text).collect();
                tr.insert_text_at(&text, from, to).map(|_| ())
            } else {
                let fragment = Fragment::from_array(nodes.clone());
                tr.replace_range(from, to, Slice::new(fragment, 0, 0))
                    .map(|_| ())
            };
            if result.is_err() {
                return false;
            }
            if let Some(end) = insertion_end(&tr) {
                let near = Selection::near(&end, -1);
                tr.set_selection(near);
            }
            dispatch(tr);
            true
        },
    )
}

fn insertion_end(tr: &rs_editor_state::Transaction) -> Option<rs_editor_model::ResolvedPos> {
    let map = tr.mapping.maps().last()?;
    let mut end = None;
    map.for_each(|_, _, _, new_to| {
        end.get_or_insert(new_to);
    });
    tr.doc.resolve(end?).ok()
}

/// Replaces the selection with `nodes`.
pub fn insert_content(nodes: Vec<Node>) -> Command {
    Arc::new(move |state: &EditorState, dispatch: Dispatch, view: View| {
        let selection = state.selection();
        insert_content_at(selection.from(), selection.to(), nodes.clone())(state, dispatch, view)
    })
}

/// Replaces the whole document.
pub fn set_content(doc: Node) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            if doc.node_type().schema() != state.schema() {
                return false;
            }
            if let Some(dispatch) = dispatch {
                let mut tr = state.tr();
                let size = tr.doc.content().size();
                if tr.replace_with(0, size, doc.content().clone()).is_err() {
                    return false;
                }
                let start = Selection::at_start(&tr.doc);
                tr.set_selection(start);
                dispatch(tr);
            }
            true
        },
    )
}

/// Empties the document (keeps one default block).
pub fn clear_content(state: &EditorState, dispatch: Dispatch, view: View) -> bool {
    let Some(empty) =
        state
            .schema()
            .top_node_type()
            .create_and_fill(None, Fragment::empty(), MarkSet::none())
    else {
        return false;
    };
    set_content(empty)(state, dispatch, view)
}

/// Selects text `from..to` (clamped into the document).
pub fn set_text_selection(from: usize, to: usize) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            if let Some(dispatch) = dispatch {
                let size = state.doc().content().size();
                let (Ok(anchor), Ok(head)) = (
                    state.doc().resolve(from.min(size)),
                    state.doc().resolve(to.min(size)),
                ) else {
                    return false;
                };
                let mut tr = state.tr();
                tr.set_selection(TextSelection::between(&anchor, &head, None));
                dispatch(tr);
            }
            true
        },
    )
}

/// Selects the node at `pos`.
pub fn set_node_selection(pos: usize) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let Ok(selection) = NodeSelection::create(state.doc(), pos) else {
                return false;
            };
            if let Some(dispatch) = dispatch {
                let mut tr = state.tr();
                tr.set_selection(selection);
                dispatch(tr);
            }
            true
        },
    )
}

/// Deletes `from..to`.
pub fn delete_range(from: usize, to: usize) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            if let Some(dispatch) = dispatch {
                let mut tr = state.tr();
                if tr.delete_range(from, to).is_err() {
                    return false;
                }
                dispatch(tr);
            }
            true
        },
    )
}

/// Deletes the closest ancestor of `node_type` around the selection.
pub fn delete_node(node_type: NodeType) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let head = state.selection().head_pos();
            for depth in (1..=head.depth()).rev() {
                if head.node(depth).node_type() == &node_type {
                    if let Some(dispatch) = dispatch {
                        let mut tr = state.tr();
                        if tr.delete(head.before(depth), head.after(depth)).is_err() {
                            return false;
                        }
                        dispatch(tr);
                    }
                    return true;
                }
            }
            false
        },
    )
}
