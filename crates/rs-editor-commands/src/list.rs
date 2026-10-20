//! List commands (`schema-list.ts`): wrap, split, lift and sink list items.

use std::sync::Arc;

use rs_editor_model::{Fragment, MarkSet, NodeRange, NodeType, Slice};
use rs_editor_state::{Command, Dispatch, EditorState, Selection, Transaction, ViewContext};
use rs_editor_transform::{
    Assoc, Mappable, NodeTypeWithAttrs, ReplaceAroundStep, Step, can_join, can_split,
    find_wrapping, lift_target,
};
use serde_json::{Map, Value};

use crate::base::keep_marks;

type View<'a> = Option<&'a dyn ViewContext>;

fn send(dispatch: &mut dyn FnMut(Transaction), mut tr: Transaction) {
    tr.scroll_into_view();
    dispatch(tr);
}

/// Wraps the selected blocks in a list of `list_type`, joining with a list
/// directly above when the selection starts a list item.
pub fn wrap_in_list(list_type: NodeType, attrs: Option<Map<String, Value>>) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let selection = state.selection();
            let Some(range) = selection.from_pos().block_range(selection.to_pos(), None) else {
                return false;
            };
            match dispatch {
                Some(dispatch) => {
                    let mut tr = state.tr();
                    if !wrap_range_in_list(Some(&mut tr), range, &list_type, attrs.clone()) {
                        return false;
                    }
                    send(dispatch, tr);
                    true
                }
                None => wrap_range_in_list(None, range, &list_type, attrs.clone()),
            }
        },
    )
}

/// Wraps `range` in a list; with `tr == None` only checks whether it can.
pub fn wrap_range_in_list(
    tr: Option<&mut Transaction>,
    range: NodeRange,
    list_type: &NodeType,
    attrs: Option<Map<String, Value>>,
) -> bool {
    let mut do_join = false;
    let mut range = range;
    let mut outer_range = range.clone();
    let doc = range.from.doc().clone();
    if range.depth >= 2
        && range
            .from
            .node(range.depth - 1)
            .node_type()
            .compatible_content(list_type)
        && range.start_index() == 0
    {
        if range.from.index(range.depth - 1) == 0 {
            return false;
        }
        let Ok(insert) = doc.resolve(range.start() - 2) else {
            return false;
        };
        outer_range = NodeRange::new(insert.clone(), insert, range.depth);
        if range.end_index() < range.parent().child_count() {
            let Ok(end) = doc.resolve(range.to.end(range.depth)) else {
                return false;
            };
            range = NodeRange::new(range.from.clone(), end, range.depth);
        }
        do_join = true;
    }
    let Some(wrap) = find_wrapping(&outer_range, list_type, attrs, Some(&range)) else {
        return false;
    };
    match tr {
        Some(tr) => do_wrap_in_list(tr, &range, &wrap, do_join, list_type),
        None => true,
    }
}

fn do_wrap_in_list(
    tr: &mut Transaction,
    range: &NodeRange,
    wrappers: &[NodeTypeWithAttrs],
    join_before: bool,
    list_type: &NodeType,
) -> bool {
    let mut content = Fragment::empty();
    for wrapper in wrappers.iter().rev() {
        match wrapper
            .ty
            .create(wrapper.attrs.as_ref(), content, MarkSet::none())
        {
            Ok(node) => content = Fragment::from_node(node),
            Err(_) => return false,
        }
    }
    let shift = if join_before { 2 } else { 0 };
    let step = Step::ReplaceAround(ReplaceAroundStep {
        from: range.start() - shift,
        to: range.end(),
        gap_from: range.start(),
        gap_to: range.end(),
        slice: Slice::new(content, 0, 0),
        insert: wrappers.len(),
        structure: true,
    });
    if tr.step(step).is_err() {
        return false;
    }
    let found = wrappers
        .iter()
        .rposition(|wrapper| wrapper.ty == *list_type)
        .map_or(0, |i| i + 1);
    let split_depth = wrappers.len() - found;
    let mut split_pos = range.start() + wrappers.len() - shift;
    let parent = range.parent().clone();
    for (offset, i) in (range.start_index()..range.end_index()).enumerate() {
        if offset > 0 && can_split(&tr.doc, split_pos, split_depth, None) {
            if tr.split(split_pos, split_depth, None).is_err() {
                return false;
            }
            split_pos += 2 * split_depth;
        }
        split_pos += parent.child(i).node_size();
    }
    true
}

/// Splits a list item at the cursor (Enter in a list). In an empty nested
/// item, splits the parent item instead.
pub fn split_list_item(item_type: NodeType, item_attrs: Option<Map<String, Value>>) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            split_list_item_inner(state, dispatch, &item_type, item_attrs.as_ref())
        },
    )
}

fn split_list_item_inner(
    state: &EditorState,
    dispatch: Dispatch,
    item_type: &NodeType,
    item_attrs: Option<&Map<String, Value>>,
) -> bool {
    let selection = state.selection();
    let (from, to) = (selection.from_pos().clone(), selection.to_pos().clone());
    if selection
        .as_node()
        .is_some_and(|node| node.node().is_block())
        || from.depth() < 2
        || !from.same_parent(&to)
    {
        return false;
    }
    let depth = from.depth();
    let grand_parent = from.node(depth - 1);
    if grand_parent.node_type() != item_type {
        return false;
    }
    if from.parent().content().size() == 0
        && from.node(depth - 1).child_count() == from.index_after(depth - 1)
    {
        // An empty last block of an item: split the outer item of a nested
        // list, or let the next command lift the block out.
        if depth <= 3
            || from.node(depth - 3).node_type() != item_type
            || from.index(depth - 2) != from.node(depth - 2).child_count() - 1
        {
            return false;
        }
        if let Some(dispatch) = dispatch {
            let mut wrap = Fragment::empty();
            let depth_before = if from.index(depth - 1) > 0 {
                1
            } else if from.index(depth - 2) > 0 {
                2
            } else {
                3
            };
            for d in (depth - 3..=depth - depth_before).rev() {
                wrap = Fragment::from_node(from.node(d).copy(wrap));
            }
            let depth_after = if from.index_after(depth - 1) < from.node(depth - 2).child_count() {
                1
            } else if from.index_after(depth - 2) < from.node(depth - 3).child_count() {
                2
            } else {
                3
            };
            let Some(item) = item_type.create_and_fill(None, Fragment::empty(), MarkSet::none())
            else {
                return false;
            };
            wrap = wrap.append(&Fragment::from_node(item));
            let start = from.before(depth - (depth_before - 1));
            let mut tr = state.tr();
            if tr
                .replace(
                    start,
                    from.after(depth - depth_after),
                    Slice::new(wrap, 4 - depth_before, 0),
                )
                .is_err()
            {
                return false;
            }
            let mut sel = None;
            let size = tr.doc.content().size();
            tr.doc.nodes_between(start, size, &mut |node, pos, _, _| {
                if sel.is_some() {
                    return false;
                }
                if node.is_textblock() && node.content().size() == 0 {
                    sel = Some(pos + 1);
                }
                true
            });
            if let Some(sel) = sel
                && let Ok(resolved) = tr.doc.resolve(sel)
            {
                let near = Selection::near(&resolved, 1);
                tr.set_selection(near);
            }
            send(dispatch, tr);
        }
        return true;
    }
    let next_type = if to.pos() == from.end(depth) {
        grand_parent
            .content_match_at(0)
            .ok()
            .and_then(|m| m.default_type())
    } else {
        None
    };
    let mut tr = state.tr();
    if tr.delete(from.pos(), to.pos()).is_err() {
        return false;
    }
    let types = next_type.map(|next| {
        vec![
            item_attrs.map(|attrs| NodeTypeWithAttrs {
                ty: item_type.clone(),
                attrs: Some(attrs.clone()),
            }),
            Some(NodeTypeWithAttrs::new(next)),
        ]
    });
    if !can_split(&tr.doc, from.pos(), 2, types.as_deref()) {
        return false;
    }
    if let Some(dispatch) = dispatch {
        if tr.split(from.pos(), 2, types.as_deref()).is_err() {
            return false;
        }
        send(dispatch, tr);
    }
    true
}

/// [`split_list_item`], keeping the active marks in the new item.
pub fn split_list_item_keep_marks(
    item_type: NodeType,
    item_attrs: Option<Map<String, Value>>,
) -> Command {
    let split = split_list_item(item_type, item_attrs);
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, view: View| match dispatch {
            Some(dispatch) => {
                let marks = keep_marks(state);
                split(
                    state,
                    Some(&mut |mut tr: Transaction| {
                        if let Some(marks) = &marks {
                            tr.ensure_marks(marks.clone());
                        }
                        dispatch(tr);
                    }),
                    view,
                )
            }
            None => split(state, None, view),
        },
    )
}

fn item_range(state: &EditorState, item_type: &NodeType) -> Option<NodeRange> {
    let selection = state.selection();
    let is_item_parent = |node: &rs_editor_model::Node| {
        node.first_child()
            .is_some_and(|c| c.node_type() == item_type)
    };
    selection
        .from_pos()
        .block_range(selection.to_pos(), Some(&is_item_parent))
}

/// Lifts the selected list items out one level (Shift-Tab).
pub fn lift_list_item(item_type: NodeType) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let Some(range) = item_range(state, &item_type) else {
                return false;
            };
            let Some(dispatch) = dispatch else {
                return true;
            };
            if range.depth >= 1 && range.from.node(range.depth - 1).node_type() == &item_type {
                lift_to_outer_list(state, dispatch, &item_type, range)
            } else {
                lift_out_of_list(state, dispatch, range)
            }
        },
    )
}

fn lift_to_outer_list(
    state: &EditorState,
    dispatch: &mut dyn FnMut(Transaction),
    item_type: &NodeType,
    range: NodeRange,
) -> bool {
    let mut tr = state.tr();
    let mut range = range;
    let end = range.end();
    let end_of_list = range.to.end(range.depth);
    if end < end_of_list {
        // Siblings after the lifted items become children of the last item.
        let Ok(item) = item_type.create(
            None,
            Fragment::from_node(range.parent().copy(Fragment::empty())),
            MarkSet::none(),
        ) else {
            return false;
        };
        let step = Step::ReplaceAround(ReplaceAroundStep {
            from: end - 1,
            to: end_of_list,
            gap_from: end,
            gap_to: end_of_list,
            slice: Slice::new(Fragment::from_node(item), 1, 0),
            insert: 1,
            structure: true,
        });
        if tr.step(step).is_err() {
            return false;
        }
        let (Ok(from), Ok(to)) = (
            tr.doc.resolve(range.from.pos()),
            tr.doc.resolve(end_of_list),
        ) else {
            return false;
        };
        range = NodeRange::new(from, to, range.depth);
    }
    let Some(target) = lift_target(&range) else {
        return false;
    };
    if tr.lift(&range, target).is_err() {
        return false;
    }
    let after = tr.mapping.map(end, Assoc::Before) - 1;
    if can_join(&tr.doc, after)
        && let Ok(resolved) = tr.doc.resolve(after)
        && let (Some(before), Some(next)) = (resolved.node_before(), resolved.node_after())
        && before.node_type() == next.node_type()
    {
        let _ = tr.join(after, 1);
    }
    send(dispatch, tr);
    true
}

fn lift_out_of_list(
    state: &EditorState,
    dispatch: &mut dyn FnMut(Transaction),
    range: NodeRange,
) -> bool {
    let mut tr = state.tr();
    let list = range.parent().clone();
    let mut pos = range.end();
    for i in (range.start_index() + 1..range.end_index()).rev() {
        pos -= list.child(i).node_size();
        if tr.delete(pos - 1, pos + 1).is_err() {
            return false;
        }
    }
    let Ok(start) = tr.doc.resolve(range.start()) else {
        return false;
    };
    let Some(item) = start.node_after() else {
        return false;
    };
    if tr.mapping.map(range.end(), Assoc::After) != range.start() + item.node_size() {
        return false;
    }
    let at_start = range.start_index() == 0;
    let at_end = range.end_index() == list.child_count();
    let parent = start.node(start.depth() - 1);
    let index_before = start.index(start.depth() - 1);
    let tail = if at_end {
        Fragment::empty()
    } else {
        Fragment::from_node(list.clone())
    };
    let content = item.content().append(&tail);
    if !parent.can_replace(
        index_before + usize::from(!at_start),
        index_before + 1,
        &content,
        0,
        content.child_count(),
    ) {
        return false;
    }
    let start_pos = start.pos();
    let end_pos = start_pos + item.node_size();
    let empty_list = || Fragment::from_node(list.copy(Fragment::empty()));
    let slice_content = (if at_start {
        Fragment::empty()
    } else {
        empty_list()
    })
    .append(
        &(if at_end {
            Fragment::empty()
        } else {
            empty_list()
        }),
    );
    let step = Step::ReplaceAround(ReplaceAroundStep {
        from: start_pos - usize::from(at_start),
        to: end_pos + usize::from(at_end),
        gap_from: start_pos + 1,
        gap_to: end_pos - 1,
        slice: Slice::new(slice_content, usize::from(!at_start), usize::from(!at_end)),
        insert: usize::from(!at_start),
        structure: true,
    });
    if tr.step(step).is_err() {
        return false;
    }
    send(dispatch, tr);
    true
}

/// Sinks the selected list items into the previous item (Tab).
pub fn sink_list_item(item_type: NodeType) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let Some(range) = item_range(state, &item_type) else {
                return false;
            };
            let start_index = range.start_index();
            if start_index == 0 {
                return false;
            }
            let parent = range.parent().clone();
            let node_before = parent.child(start_index - 1);
            if node_before.node_type() != &item_type {
                return false;
            }
            if let Some(dispatch) = dispatch {
                let nested_before = node_before
                    .last_child()
                    .is_some_and(|last| last.node_type() == parent.node_type());
                let inner = if nested_before {
                    match item_type.create(None, Fragment::empty(), MarkSet::none()) {
                        Ok(item) => Fragment::from_node(item),
                        Err(_) => return false,
                    }
                } else {
                    Fragment::empty()
                };
                let Ok(list) = parent.node_type().create(None, inner, MarkSet::none()) else {
                    return false;
                };
                let Ok(item) = item_type.create(None, Fragment::from_node(list), MarkSet::none())
                else {
                    return false;
                };
                let open = if nested_before { 3 } else { 1 };
                let slice = Slice::new(Fragment::from_node(item), open, 0);
                let (before, after) = (range.start(), range.end());
                let mut tr = state.tr();
                let step = Step::ReplaceAround(ReplaceAroundStep {
                    from: before - open,
                    to: after,
                    gap_from: before,
                    gap_to: after,
                    slice,
                    insert: 1,
                    structure: true,
                });
                if tr.step(step).is_err() {
                    return false;
                }
                send(dispatch, tr);
            }
            true
        },
    )
}
