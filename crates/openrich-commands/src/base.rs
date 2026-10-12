//! Basic editing commands (`commands.ts`).

use std::sync::Arc;

use openrich_model::{
    ContentMatch, Fragment, MarkSet, MarkType, Node, NodeType, ResolvedPos, Slice,
};
use openrich_state::{
    AllSelection, Command, Dispatch, EditorState, NodeSelection, Selection, TextDirection,
    TextSelection, Transaction, ViewContext, reborrow,
};
use openrich_transform::{
    Assoc, Mappable, MarkMatch, NodeTypeWithAttrs, ReplaceAroundStep, Step, can_join, can_split,
    find_wrapping, join_point, lift_target, replace_step,
};
use serde_json::{Map, Value};

type View<'a> = Option<&'a dyn ViewContext>;

fn send(dispatch: Dispatch, mut tr: Transaction) {
    if let Some(dispatch) = dispatch {
        tr.scroll_into_view();
        dispatch(tr);
    }
}

/// Deletes a non-empty selection.
pub fn delete_selection(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    if state.selection().empty() {
        return false;
    }
    if dispatch.is_some() {
        let mut tr = state.tr();
        if tr.delete_selection().is_err() {
            return false;
        }
        send(dispatch, tr);
    }
    true
}

fn at_block_start(state: &EditorState, view: View) -> Option<ResolvedPos> {
    let cursor = state.selection().cursor()?;
    let at_start = match view {
        Some(view) => view.end_of_textblock(TextDirection::Backward, state),
        None => cursor.parent_offset() == 0,
    };
    at_start.then(|| cursor.clone())
}

fn at_block_end(state: &EditorState, view: View) -> Option<ResolvedPos> {
    let cursor = state.selection().cursor()?;
    let at_end = match view {
        Some(view) => view.end_of_textblock(TextDirection::Forward, state),
        None => cursor.parent_offset() == cursor.parent().content().size(),
    };
    at_end.then(|| cursor.clone())
}

fn find_cut_before(pos: &ResolvedPos) -> Option<ResolvedPos> {
    if pos.parent().node_type().spec().isolating {
        return None;
    }
    for i in (0..pos.depth()).rev() {
        if pos.index(i) > 0 {
            return pos.doc().resolve(pos.before(i + 1)).ok();
        }
        if pos.node(i).node_type().spec().isolating {
            break;
        }
    }
    None
}

fn find_cut_after(pos: &ResolvedPos) -> Option<ResolvedPos> {
    if pos.parent().node_type().spec().isolating {
        return None;
    }
    for i in (0..pos.depth()).rev() {
        let parent = pos.node(i);
        if pos.index(i) + 1 < parent.child_count() {
            return pos.doc().resolve(pos.after(i + 1)).ok();
        }
        if parent.node_type().spec().isolating {
            break;
        }
    }
    None
}

/// Whether descending along `side` (`start` = first children) reaches a
/// textblock; with `only`, every step must have exactly one child.
fn textblock_at(node: &Node, start: bool, only: bool) -> bool {
    let mut scan = Some(node.clone());
    while let Some(node) = scan {
        if node.is_textblock() {
            return true;
        }
        if only && node.child_count() != 1 {
            return false;
        }
        scan = if start {
            node.first_child().cloned()
        } else {
            node.last_child().cloned()
        };
    }
    false
}

fn lift_cursor_block(state: &EditorState, dispatch: Dispatch, cursor: &ResolvedPos) -> bool {
    let Some(range) = cursor.block_range(cursor, None) else {
        return false;
    };
    let Some(target) = lift_target(&range) else {
        return false;
    };
    if dispatch.is_some() {
        let mut tr = state.tr();
        if tr.lift(&range, target).is_err() {
            return false;
        }
        send(dispatch, tr);
    }
    true
}

/// At the start of a textblock: joins it with the block before, lifts it
/// out of its parent, or deletes an atom before it.
pub fn join_backward(state: &EditorState, mut dispatch: Dispatch, view: View) -> bool {
    let Some(cursor) = at_block_start(state, view) else {
        return false;
    };
    let Some(cut) = find_cut_before(&cursor) else {
        return lift_cursor_block(state, dispatch, &cursor);
    };
    let Some(before) = cut.node_before() else {
        return false;
    };
    if delete_barrier(state, &cut, reborrow(&mut dispatch), -1) {
        return true;
    }
    if cursor.parent().content().size() == 0
        && (textblock_at(&before, false, false) || NodeSelection::is_selectable(&before))
    {
        let mut depth = cursor.depth();
        loop {
            if let Ok(Some(Step::Replace(del))) = replace_step(
                state.doc(),
                cursor.before(depth),
                cursor.after(depth),
                Slice::empty(),
            ) && del.slice.size() < del.to - del.from
            {
                if dispatch.is_some() {
                    let mut tr = state.tr();
                    if tr.step(Step::Replace(del)).is_err() {
                        return false;
                    }
                    let selection = if textblock_at(&before, false, false) {
                        let at = tr.mapping.map(cut.pos(), Assoc::Before);
                        tr.doc
                            .resolve(at)
                            .ok()
                            .and_then(|pos| Selection::find_from(&pos, -1, false))
                    } else {
                        NodeSelection::create(&tr.doc, cut.pos() - before.node_size()).ok()
                    };
                    if let Some(selection) = selection {
                        tr.set_selection(selection);
                    }
                    send(dispatch, tr);
                }
                return true;
            }
            if depth == 1 || cursor.node(depth - 1).child_count() > 1 {
                break;
            }
            depth -= 1;
        }
    }
    if before.is_atom() && cut.depth() + 1 == cursor.depth() {
        if dispatch.is_some() {
            let mut tr = state.tr();
            if tr
                .delete(cut.pos() - before.node_size(), cut.pos())
                .is_err()
            {
                return false;
            }
            send(dispatch, tr);
        }
        return true;
    }
    false
}

/// At the start of a textblock, joins its text into the closest textblock before it.
pub fn join_textblock_backward(state: &EditorState, dispatch: Dispatch, view: View) -> bool {
    let Some(cursor) = at_block_start(state, view) else {
        return false;
    };
    find_cut_before(&cursor).is_some_and(|cut| join_textblocks_around(state, &cut, dispatch))
}

/// At the end of a textblock, joins the closest textblock after it into it.
pub fn join_textblock_forward(state: &EditorState, dispatch: Dispatch, view: View) -> bool {
    let Some(cursor) = at_block_end(state, view) else {
        return false;
    };
    find_cut_after(&cursor).is_some_and(|cut| join_textblocks_around(state, &cut, dispatch))
}

fn join_textblocks_around(state: &EditorState, cut: &ResolvedPos, dispatch: Dispatch) -> bool {
    let (Some(before), Some(after)) = (cut.node_before(), cut.node_after()) else {
        return false;
    };
    let mut before_text = before;
    let mut before_pos = cut.pos() - 1;
    while !before_text.is_textblock() {
        if before_text.node_type().spec().isolating {
            return false;
        }
        let Some(child) = before_text.last_child().cloned() else {
            return false;
        };
        before_text = child;
        before_pos -= 1;
    }
    let mut after_text = after;
    let mut after_pos = cut.pos() + 1;
    while !after_text.is_textblock() {
        if after_text.node_type().spec().isolating {
            return false;
        }
        let Some(child) = after_text.first_child().cloned() else {
            return false;
        };
        after_text = child;
        after_pos += 1;
    }
    let step = match replace_step(state.doc(), before_pos, after_pos, Slice::empty()) {
        Ok(Some(step)) => step,
        _ => return false,
    };
    match &step {
        Step::Replace(replace)
            if replace.from != before_pos || replace.slice.size() >= after_pos - before_pos =>
        {
            return false;
        }
        Step::ReplaceAround(around) if around.from != before_pos => return false,
        _ => {}
    }
    if dispatch.is_some() {
        let mut tr = state.tr();
        if tr.step(step).is_err() {
            return false;
        }
        if let Ok(selection) = TextSelection::cursor_at(&tr.doc, before_pos) {
            tr.set_selection(selection);
        }
        send(dispatch, tr);
    }
    true
}

/// Selects the node before an empty selection at the start of a textblock.
pub fn select_node_backward(state: &EditorState, dispatch: Dispatch, view: View) -> bool {
    let selection = state.selection();
    if !selection.empty() {
        return false;
    }
    let head = selection.head_pos();
    let cut = if head.parent().is_textblock() {
        let at_start = match view {
            Some(view) => view.end_of_textblock(TextDirection::Backward, state),
            None => head.parent_offset() == 0,
        };
        if !at_start {
            return false;
        }
        find_cut_before(head)
    } else {
        Some(head.clone())
    };
    let Some(cut) = cut else {
        return false;
    };
    let Some(node) = cut.node_before() else {
        return false;
    };
    if !NodeSelection::is_selectable(&node) {
        return false;
    }
    if dispatch.is_some() {
        let mut tr = state.tr();
        match NodeSelection::create(state.doc(), cut.pos() - node.node_size()) {
            Ok(selection) => tr.set_selection(selection),
            Err(_) => return false,
        };
        send(dispatch, tr);
    }
    true
}

/// At the end of a textblock: joins the block after into it, or deletes
/// an atom after it.
pub fn join_forward(state: &EditorState, mut dispatch: Dispatch, view: View) -> bool {
    let Some(cursor) = at_block_end(state, view) else {
        return false;
    };
    let Some(cut) = find_cut_after(&cursor) else {
        return false;
    };
    let Some(after) = cut.node_after() else {
        return false;
    };
    if delete_barrier(state, &cut, reborrow(&mut dispatch), 1) {
        return true;
    }
    if cursor.parent().content().size() == 0
        && (textblock_at(&after, true, false) || NodeSelection::is_selectable(&after))
        && let Ok(Some(Step::Replace(del))) = replace_step(
            state.doc(),
            cursor.before(cursor.depth()),
            cursor.after(cursor.depth()),
            Slice::empty(),
        )
        && del.slice.size() < del.to - del.from
    {
        if dispatch.is_some() {
            let mut tr = state.tr();
            if tr.step(Step::Replace(del)).is_err() {
                return false;
            }
            let at = tr.mapping.map(cut.pos(), Assoc::After);
            let selection = if textblock_at(&after, true, false) {
                tr.doc
                    .resolve(at)
                    .ok()
                    .and_then(|pos| Selection::find_from(&pos, 1, false))
            } else {
                NodeSelection::create(&tr.doc, at).ok()
            };
            if let Some(selection) = selection {
                tr.set_selection(selection);
            }
            send(dispatch, tr);
        }
        return true;
    }
    if after.is_atom() && cut.depth() + 1 == cursor.depth() {
        if dispatch.is_some() {
            let mut tr = state.tr();
            if tr.delete(cut.pos(), cut.pos() + after.node_size()).is_err() {
                return false;
            }
            send(dispatch, tr);
        }
        return true;
    }
    false
}

/// Selects the node after an empty selection at the end of a textblock.
pub fn select_node_forward(state: &EditorState, dispatch: Dispatch, view: View) -> bool {
    let selection = state.selection();
    if !selection.empty() {
        return false;
    }
    let head = selection.head_pos();
    let cut = if head.parent().is_textblock() {
        let at_end = match view {
            Some(view) => view.end_of_textblock(TextDirection::Forward, state),
            None => head.parent_offset() == head.parent().content().size(),
        };
        if !at_end {
            return false;
        }
        find_cut_after(head)
    } else {
        Some(head.clone())
    };
    let Some(cut) = cut else {
        return false;
    };
    let Some(node) = cut.node_after() else {
        return false;
    };
    if !NodeSelection::is_selectable(&node) {
        return false;
    }
    if dispatch.is_some() {
        let mut tr = state.tr();
        match NodeSelection::create(state.doc(), cut.pos()) {
            Ok(selection) => tr.set_selection(selection),
            Err(_) => return false,
        };
        send(dispatch, tr);
    }
    true
}

/// Joins the selected block (or the closest joinable ancestor) with the sibling above.
pub fn join_up(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let selection = state.selection();
    let node_selection = selection.as_node();
    let point = match node_selection {
        Some(node) => {
            if node.node().is_textblock() || !can_join(state.doc(), selection.from()) {
                return false;
            }
            selection.from()
        }
        None => match join_point(state.doc(), selection.from(), -1) {
            Some(point) => point,
            None => return false,
        },
    };
    if dispatch.is_some() {
        let mut tr = state.tr();
        if tr.join(point, 1).is_err() {
            return false;
        }
        if node_selection.is_some()
            && let Some(before) = state
                .doc()
                .resolve(point)
                .ok()
                .and_then(|pos| pos.node_before())
            && let Ok(selection) = NodeSelection::create(&tr.doc, point - before.node_size())
        {
            tr.set_selection(selection);
        }
        send(dispatch, tr);
    }
    true
}

/// Joins the selected block (or the closest joinable ancestor) with the sibling below.
pub fn join_down(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let selection = state.selection();
    let point = match selection.as_node() {
        Some(node) => {
            if node.node().is_textblock() || !can_join(state.doc(), selection.to()) {
                return false;
            }
            selection.to()
        }
        None => match join_point(state.doc(), selection.to(), 1) {
            Some(point) => point,
            None => return false,
        },
    };
    if dispatch.is_some() {
        let mut tr = state.tr();
        if tr.join(point, 1).is_err() {
            return false;
        }
        send(dispatch, tr);
    }
    true
}

/// Lifts the selected block(s) out of their parent.
pub fn lift(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let selection = state.selection();
    let Some(range) = selection.from_pos().block_range(selection.to_pos(), None) else {
        return false;
    };
    let Some(target) = lift_target(&range) else {
        return false;
    };
    if dispatch.is_some() {
        let mut tr = state.tr();
        if tr.lift(&range, target).is_err() {
            return false;
        }
        send(dispatch, tr);
    }
    true
}

/// Inserts a newline in a code block.
pub fn newline_in_code(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let selection = state.selection();
    let head = selection.head_pos();
    if !head.parent().node_type().spec().code || !head.same_parent(selection.anchor_pos()) {
        return false;
    }
    if dispatch.is_some() {
        let mut tr = state.tr();
        if tr.insert_text("\n").is_err() {
            return false;
        }
        send(dispatch, tr);
    }
    true
}

fn default_block_at(content_match: &ContentMatch) -> Option<NodeType> {
    (0..content_match.edge_count())
        .filter_map(|i| content_match.edge(i))
        .map(|(ty, _)| ty)
        .find(|ty| ty.is_textblock() && !ty.has_required_attrs())
}

/// Leaves a code block, creating a default block after it.
pub fn exit_code(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let selection = state.selection();
    let head = selection.head_pos();
    if !head.parent().node_type().spec().code
        || !head.same_parent(selection.anchor_pos())
        || head.depth() == 0
    {
        return false;
    }
    let above = head.node(head.depth() - 1);
    let after = head.index_after(head.depth() - 1);
    let Some(ty) = above
        .content_match_at(after)
        .ok()
        .and_then(|m| default_block_at(&m))
    else {
        return false;
    };
    if !above.can_replace_with(after, after, &ty, None) {
        return false;
    }
    if dispatch.is_some() {
        let pos = head.after(head.depth());
        let Some(block) = ty.create_and_fill(None, Fragment::empty(), MarkSet::none()) else {
            return false;
        };
        let mut tr = state.tr();
        if tr
            .replace_with(pos, pos, Fragment::from_node(block))
            .is_err()
        {
            return false;
        }
        if let Ok(resolved) = tr.doc.resolve(pos) {
            let near = Selection::near(&resolved, 1);
            tr.set_selection(near);
        }
        send(dispatch, tr);
    }
    true
}

/// With a selected block node, creates an empty paragraph before or after it.
pub fn create_paragraph_near(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let selection = state.selection();
    let (from, to) = (selection.from_pos(), selection.to_pos());
    if selection.is_all() || from.parent().inline_content() || to.parent().inline_content() {
        return false;
    }
    let Some(ty) = to
        .parent()
        .content_match_at(to.index_after(to.depth()))
        .ok()
        .and_then(|m| default_block_at(&m))
    else {
        return false;
    };
    if dispatch.is_some() {
        let side = if from.parent_offset() == 0 && to.index(to.depth()) < to.parent().child_count()
        {
            from.pos()
        } else {
            to.pos()
        };
        let Some(block) = ty.create_and_fill(None, Fragment::empty(), MarkSet::none()) else {
            return false;
        };
        let mut tr = state.tr();
        if tr.insert(side, Fragment::from_node(block)).is_err() {
            return false;
        }
        if let Ok(selection) = TextSelection::cursor_at(&tr.doc, side + 1) {
            tr.set_selection(selection);
        }
        send(dispatch, tr);
    }
    true
}

/// In an empty block: splits its parent or lifts it out.
pub fn lift_empty_block(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let Some(cursor) = state.selection().cursor().cloned() else {
        return false;
    };
    if cursor.parent().content().size() > 0 {
        return false;
    }
    if cursor.depth() > 1 && cursor.after(cursor.depth()) != cursor.end(cursor.depth() - 1) {
        let before = cursor.before(cursor.depth());
        if can_split(state.doc(), before, 1, None) {
            if dispatch.is_some() {
                let mut tr = state.tr();
                if tr.split(before, 1, None).is_err() {
                    return false;
                }
                send(dispatch, tr);
            }
            return true;
        }
    }
    lift_cursor_block(state, dispatch, &cursor)
}

/// How [`split_block_as`] picks the type of the block after the split.
pub type SplitNode = dyn Fn(&Node, bool, &ResolvedPos) -> Option<NodeTypeWithAttrs> + Send + Sync;

/// Splits the parent block of the selection, deleting a range selection
/// first. `split_node(parent, at_end, from)` may choose the new block type.
pub fn split_block_as(split_node: Option<Arc<SplitNode>>) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            split_block_inner(state, dispatch, split_node.as_deref())
        },
    )
}

fn split_block_inner(
    state: &EditorState,
    dispatch: Dispatch,
    split_node: Option<&SplitNode>,
) -> bool {
    let selection = state.selection();
    let (from, to) = (selection.from_pos().clone(), selection.to_pos().clone());
    if let Some(node) = selection.as_node()
        && node.node().is_block()
    {
        if from.parent_offset() == 0 || !can_split(state.doc(), from.pos(), 1, None) {
            return false;
        }
        if dispatch.is_some() {
            let mut tr = state.tr();
            if tr.split(from.pos(), 1, None).is_err() {
                return false;
            }
            send(dispatch, tr);
        }
        return true;
    }
    if from.depth() == 0 {
        return false;
    }
    let mut types: Vec<Option<NodeTypeWithAttrs>> = Vec::new();
    let mut d = from.depth();
    let (split_depth, default_type, at_end, at_start) = loop {
        let node = from.node(d);
        if node.is_block() {
            let at_end = from.end(d) == from.pos() + (from.depth() - d);
            let at_start = from.start(d) + (from.depth() - d) == from.pos();
            let default_type = from
                .node(d - 1)
                .content_match_at(from.index_after(d - 1))
                .ok()
                .and_then(|m| default_block_at(&m));
            let split_type = split_node.and_then(|f| f(to.parent(), at_end, &from));
            let chosen = split_type.or_else(|| match (&default_type, at_end) {
                (Some(ty), true) => Some(NodeTypeWithAttrs::new(ty.clone())),
                _ => None,
            });
            types.insert(0, chosen);
            break (d, default_type, at_end, at_start);
        }
        if d == 1 {
            return false;
        }
        types.insert(0, None);
        d -= 1;
    };
    let mut tr = state.tr();
    if (selection.is_text() || selection.is_all()) && tr.delete_selection().is_err() {
        return false;
    }
    let split_pos = tr.mapping.map(from.pos(), Assoc::After);
    let mut can = can_split(&tr.doc, split_pos, types.len(), Some(&types));
    if !can {
        types[0] = default_type.clone().map(NodeTypeWithAttrs::new);
        can = can_split(&tr.doc, split_pos, types.len(), Some(&types));
    }
    if !can {
        return false;
    }
    if tr.split(split_pos, types.len(), Some(&types)).is_err() {
        return false;
    }
    if !at_end
        && at_start
        && let Some(default_type) = &default_type
        && from.node(split_depth).node_type() != default_type
    {
        let first = tr.mapping.map(from.before(split_depth), Assoc::After);
        if let Ok(resolved_first) = tr.doc.resolve(first) {
            let index = resolved_first.index(resolved_first.depth());
            if from
                .node(split_depth - 1)
                .can_replace_with(index, index + 1, default_type, None)
            {
                let _ = tr.set_node_markup(first, Some(default_type), None, None);
            }
        }
    }
    send(dispatch, tr);
    true
}

/// Splits the parent block of the selection (Enter).
pub fn split_block(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    split_block_inner(state, dispatch, None)
}

/// [`split_block`], keeping the active marks for the new block.
pub fn split_block_keep_marks(state: &EditorState, dispatch: Dispatch, view: View) -> bool {
    match dispatch {
        Some(dispatch) => {
            let marks = keep_marks(state);
            split_block(
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
        None => split_block(state, None, view),
    }
}

/// The marks a split should carry over, as in ProseMirror's `KeepMarks` commands.
pub(crate) fn keep_marks(state: &EditorState) -> Option<MarkSet> {
    state.stored_marks().cloned().or_else(|| {
        let selection = state.selection();
        (selection.to_pos().parent_offset() > 0).then(|| selection.from_pos().marks())
    })
}

/// Selects the parent node of the selection.
pub fn select_parent_node(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let selection = state.selection();
    let same = selection.from_pos().shared_depth(selection.to());
    if same == 0 {
        return false;
    }
    let pos = selection.from_pos().before(same);
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        match NodeSelection::create(state.doc(), pos) {
            Ok(selection) => tr.set_selection(selection),
            Err(_) => return false,
        };
        dispatch(tr);
    }
    true
}

/// Selects the whole document.
pub fn select_all(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.set_selection(Selection::All(AllSelection::new(state.doc())));
        dispatch(tr);
    }
    true
}

fn join_maybe_clear(state: &EditorState, pos: &ResolvedPos, dispatch: Dispatch) -> bool {
    let (Some(before), Some(after)) = (pos.node_before(), pos.node_after()) else {
        return false;
    };
    let index = pos.index(pos.depth());
    if !before.node_type().compatible_content(after.node_type()) {
        return false;
    }
    if before.content().size() == 0
        && index > 0
        && pos
            .parent()
            .can_replace(index - 1, index, &Fragment::empty(), 0, 0)
    {
        if dispatch.is_some() {
            let mut tr = state.tr();
            if tr
                .delete(pos.pos() - before.node_size(), pos.pos())
                .is_err()
            {
                return false;
            }
            send(dispatch, tr);
        }
        return true;
    }
    if !pos
        .parent()
        .can_replace(index, index + 1, &Fragment::empty(), 0, 0)
        || !(after.is_textblock() || can_join(state.doc(), pos.pos()))
    {
        return false;
    }
    if dispatch.is_some() {
        let mut tr = state.tr();
        if tr.join(pos.pos(), 1).is_err() {
            return false;
        }
        send(dispatch, tr);
    }
    true
}

fn delete_barrier(state: &EditorState, cut: &ResolvedPos, mut dispatch: Dispatch, dir: i8) -> bool {
    let (Some(before), Some(after)) = (cut.node_before(), cut.node_after()) else {
        return false;
    };
    let isolated = before.node_type().spec().isolating || after.node_type().spec().isolating;
    if !isolated && join_maybe_clear(state, cut, reborrow(&mut dispatch)) {
        return true;
    }
    let index = cut.index(cut.depth());
    let can_del_after = !isolated
        && cut
            .parent()
            .can_replace(index, index + 1, &Fragment::empty(), 0, 0);
    if can_del_after
        && let Ok(content_match) = before.content_match_at(before.child_count())
        && let Some(conn) = content_match.find_wrapping(after.node_type())
        && content_match
            .match_type(conn.first().unwrap_or(after.node_type()))
            .is_some_and(|m| m.valid_end())
    {
        if dispatch.is_some() {
            let end = cut.pos() + after.node_size();
            let mut wrap = Fragment::empty();
            for ty in conn.iter().rev() {
                match ty.create(None, wrap, MarkSet::none()) {
                    Ok(node) => wrap = Fragment::from_node(node),
                    Err(_) => return false,
                }
            }
            wrap = Fragment::from_node(before.copy(wrap));
            let mut tr = state.tr();
            let step = Step::ReplaceAround(ReplaceAroundStep {
                from: cut.pos() - 1,
                to: end,
                gap_from: cut.pos(),
                gap_to: end,
                slice: Slice::new(wrap, 1, 0),
                insert: conn.len(),
                structure: true,
            });
            if tr.step(step).is_err() {
                return false;
            }
            if let Ok(join_at) = tr.doc.resolve(end + 2 * conn.len())
                && join_at
                    .node_after()
                    .is_some_and(|n| n.node_type() == before.node_type())
                && can_join(&tr.doc, join_at.pos())
            {
                let _ = tr.join(join_at.pos(), 1);
            }
            send(dispatch, tr);
        }
        return true;
    }
    let sel_after = if after.node_type().spec().isolating || (dir > 0 && isolated) {
        None
    } else {
        Selection::find_from(cut, 1, false)
    };
    if let Some(sel_after) = &sel_after
        && let Some(range) = sel_after.from_pos().block_range(sel_after.to_pos(), None)
        && let Some(target) = lift_target(&range)
        && target >= cut.depth()
    {
        if dispatch.is_some() {
            let mut tr = state.tr();
            if tr.lift(&range, target).is_err() {
                return false;
            }
            send(dispatch, tr);
        }
        return true;
    }
    if can_del_after && textblock_at(&after, true, true) && textblock_at(&before, false, false) {
        let mut at = before.clone();
        let mut wrap = vec![at.clone()];
        while !at.is_textblock() {
            let Some(last) = at.last_child().cloned() else {
                return false;
            };
            at = last;
            wrap.push(at.clone());
        }
        let mut after_text = after.clone();
        let mut after_depth = 1;
        while !after_text.is_textblock() {
            let Some(first) = after_text.first_child().cloned() else {
                return false;
            };
            after_text = first;
            after_depth += 1;
        }
        if at.can_replace(
            at.child_count(),
            at.child_count(),
            after_text.content(),
            0,
            after_text.child_count(),
        ) {
            if dispatch.is_some() {
                let mut end = Fragment::empty();
                for node in wrap.iter().rev() {
                    end = Fragment::from_node(node.copy(end));
                }
                let mut tr = state.tr();
                let step = Step::ReplaceAround(ReplaceAroundStep {
                    from: cut.pos() - wrap.len(),
                    to: cut.pos() + after.node_size(),
                    gap_from: cut.pos() + after_depth,
                    gap_to: cut.pos() + after.node_size() - after_depth,
                    slice: Slice::new(end, wrap.len(), 0),
                    insert: 0,
                    structure: true,
                });
                if tr.step(step).is_err() {
                    return false;
                }
                send(dispatch, tr);
            }
            return true;
        }
    }
    false
}

fn select_textblock_side(state: &EditorState, dispatch: Dispatch, start: bool) -> bool {
    let selection = state.selection();
    let pos = if start {
        selection.from_pos()
    } else {
        selection.to_pos()
    };
    let mut depth = pos.depth();
    while pos.node(depth).is_inline() {
        if depth == 0 {
            return false;
        }
        depth -= 1;
    }
    if !pos.node(depth).is_textblock() {
        return false;
    }
    if let Some(dispatch) = dispatch {
        let target = if start {
            pos.start(depth)
        } else {
            pos.end(depth)
        };
        let mut tr = state.tr();
        match TextSelection::cursor_at(state.doc(), target) {
            Ok(selection) => tr.set_selection(selection),
            Err(_) => return false,
        };
        dispatch(tr);
    }
    true
}

/// Moves the cursor to the start of the current textblock.
pub fn select_textblock_start(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    select_textblock_side(state, dispatch, true)
}

/// Moves the cursor to the end of the current textblock.
pub fn select_textblock_end(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    select_textblock_side(state, dispatch, false)
}

/// Wraps the selected blocks in a node of `ty`.
pub fn wrap_in(ty: NodeType, attrs: Option<Map<String, Value>>) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let selection = state.selection();
            let Some(range) = selection.from_pos().block_range(selection.to_pos(), None) else {
                return false;
            };
            let Some(wrapping) = find_wrapping(&range, &ty, attrs.clone(), None) else {
                return false;
            };
            if dispatch.is_some() {
                let mut tr = state.tr();
                if tr.wrap(&range, &wrapping).is_err() {
                    return false;
                }
                send(dispatch, tr);
            }
            true
        },
    )
}

/// Whether [`set_block_type`] with these arguments would change anything.
pub fn can_set_block_type(
    state: &EditorState,
    ty: &NodeType,
    attrs: Option<&Map<String, Value>>,
) -> bool {
    let selection = state.selection();
    let Ok(computed) = ty.compute_attrs(attrs) else {
        return false;
    };
    let mut applicable = false;
    state
        .doc()
        .nodes_between(selection.from(), selection.to(), &mut |node, pos, _, _| {
            if applicable {
                return false;
            }
            if !node.is_textblock() || node.has_markup(ty, &computed, node.marks()) {
                return true;
            }
            if node.node_type() == ty {
                applicable = true;
            } else if let Ok(resolved) = state.doc().resolve(pos) {
                let index = resolved.index(resolved.depth());
                applicable = resolved
                    .parent()
                    .can_replace_with(index, index + 1, ty, None);
            }
            true
        });
    applicable
}

/// Turns the selected textblocks into `ty` (paragraph, heading, code block).
pub fn set_block_type(ty: NodeType, attrs: Option<Map<String, Value>>) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            if !can_set_block_type(state, &ty, attrs.as_ref()) {
                return false;
            }
            if dispatch.is_some() {
                let selection = state.selection();
                let mut tr = state.tr();
                if tr
                    .set_block_type(selection.from(), selection.to(), &ty, attrs.as_ref())
                    .is_err()
                {
                    return false;
                }
                send(dispatch, tr);
            }
            true
        },
    )
}

fn mark_applies(
    doc: &Node,
    from: &ResolvedPos,
    to: &ResolvedPos,
    ty: &MarkType,
    enter_atoms: bool,
) -> bool {
    let mut can = from.depth() == 0 && doc.inline_content() && doc.node_type().allows_mark_type(ty);
    doc.nodes_between(from.pos(), to.pos(), &mut |node, pos, _, _| {
        if can
            || (!enter_atoms
                && node.is_atom()
                && node.is_inline()
                && pos >= from.pos()
                && pos + node.node_size() <= to.pos())
        {
            return false;
        }
        can = node.inline_content() && node.node_type().allows_mark_type(ty);
        true
    });
    can
}

/// Options for [`toggle_mark`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToggleMarkOptions {
    /// Remove the mark when any part of the selection has it (default);
    /// otherwise add it unless every part has it.
    pub remove_when_present: bool,
    /// Apply inside inline atoms with content.
    pub enter_inline_atoms: bool,
    /// Also mark leading and trailing whitespace of the selection.
    pub include_whitespace: bool,
}

impl Default for ToggleMarkOptions {
    fn default() -> Self {
        Self {
            remove_when_present: true,
            enter_inline_atoms: true,
            include_whitespace: false,
        }
    }
}

fn leading_space(text: &str) -> usize {
    text.chars()
        .take_while(|ch| ch.is_whitespace())
        .map(char::len_utf16)
        .sum()
}

fn trailing_space(text: &str) -> usize {
    text.chars()
        .rev()
        .take_while(|ch| ch.is_whitespace())
        .map(char::len_utf16)
        .sum()
}

/// Toggles `ty` on the selection, or in the stored marks for a cursor.
pub fn toggle_mark(
    ty: MarkType,
    attrs: Option<Map<String, Value>>,
    options: ToggleMarkOptions,
) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            toggle_mark_inner(state, dispatch, &ty, attrs.as_ref(), options)
        },
    )
}

fn toggle_mark_inner(
    state: &EditorState,
    dispatch: Dispatch,
    ty: &MarkType,
    attrs: Option<&Map<String, Value>>,
    options: ToggleMarkOptions,
) -> bool {
    let selection = state.selection();
    let cursor = selection.cursor().cloned();
    if (selection.empty() && cursor.is_none())
        || !mark_applies(
            state.doc(),
            selection.from_pos(),
            selection.to_pos(),
            ty,
            options.enter_inline_atoms,
        )
    {
        return false;
    }
    let Some(dispatch) = dispatch else {
        return true;
    };
    let Ok(mark) = ty.create(attrs) else {
        return false;
    };
    let mut tr = state.tr();
    if let Some(cursor) = cursor {
        let current = state
            .stored_marks()
            .cloned()
            .unwrap_or_else(|| cursor.marks());
        if ty.is_in_set(&current).is_some() {
            tr.remove_stored_mark_type(ty);
        } else {
            tr.add_stored_mark(&mark);
        }
        dispatch(tr);
        return true;
    }
    let (from, to) = (selection.from_pos().clone(), selection.to_pos().clone());
    let ranges = if options.enter_inline_atoms {
        vec![(from.clone(), to.clone())]
    } else {
        remove_inline_atoms(&from, &to)
    };
    let add = if options.remove_when_present {
        !ranges
            .iter()
            .any(|(f, t)| state.doc().range_has_mark(f.pos(), t.pos(), ty))
    } else {
        !ranges.iter().all(|(f, t)| {
            let mut missing = false;
            state
                .doc()
                .nodes_between(f.pos(), t.pos(), &mut |node, pos, parent, _| {
                    if missing {
                        return false;
                    }
                    let blank = node.is_text()
                        && node
                            .text_between(
                                f.pos().saturating_sub(pos),
                                node.node_size().min(t.pos() - pos),
                                None,
                                None,
                            )
                            .chars()
                            .all(char::is_whitespace);
                    missing = ty.is_in_set(node.marks()).is_none()
                        && parent.is_some_and(|p| p.node_type().allows_mark_type(ty))
                        && !blank;
                    true
                });
            !missing
        })
    };
    for (rfrom, rto) in &ranges {
        if !add {
            if tr
                .remove_mark(rfrom.pos(), rto.pos(), &MarkMatch::Type(ty.clone()))
                .is_err()
            {
                return false;
            }
            continue;
        }
        let (mut start, mut end) = (rfrom.pos(), rto.pos());
        let drop_space = !options.include_whitespace;
        let space_start = match rfrom.node_after() {
            Some(node) if drop_space => node.text().map_or(0, leading_space),
            _ => 0,
        };
        let space_end = match rto.node_before() {
            Some(node) if drop_space => node.text().map_or(0, trailing_space),
            _ => 0,
        };
        if start + space_start < end {
            start += space_start;
            end -= space_end;
        }
        if tr.add_mark(start, end, &mark).is_err() {
            return false;
        }
    }
    tr.scroll_into_view();
    dispatch(tr);
    true
}

fn remove_inline_atoms(from: &ResolvedPos, to: &ResolvedPos) -> Vec<(ResolvedPos, ResolvedPos)> {
    let doc = from.doc().clone();
    let mut result = Vec::new();
    let mut start = from.clone();
    doc.nodes_between(from.pos(), to.pos(), &mut |node, pos, _, _| {
        if node.is_atom()
            && node.content().size() > 0
            && node.is_inline()
            && pos >= start.pos()
            && pos + node.node_size() <= to.pos()
        {
            if pos + 1 > start.pos()
                && let Ok(end) = doc.resolve(pos + 1)
            {
                result.push((start.clone(), end));
            }
            if let Ok(next) = doc.resolve(pos + 1 + node.content().size()) {
                start = next;
            }
            return false;
        }
        true
    });
    if start.pos() < to.pos() {
        result.push((start, to.clone()));
    }
    result
}

/// Decides whether two adjacent nodes of the same type should be joined.
pub type JoinPredicate = Arc<dyn Fn(&Node, &Node) -> bool + Send + Sync>;

/// Wraps a command so that, after a generic change, adjacent nodes the
/// predicate accepts (e.g. two lists of the same type) are joined.
pub fn auto_join(command: Command, is_joinable: JoinPredicate) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, view: View| match dispatch {
            Some(dispatch) => command(
                state,
                Some(&mut |mut tr: Transaction| {
                    join_adjacent(&mut tr, is_joinable.as_ref());
                    dispatch(tr);
                }),
                view,
            ),
            None => command(state, None, view),
        },
    )
}

fn join_adjacent(tr: &mut Transaction, is_joinable: &dyn Fn(&Node, &Node) -> bool) {
    if !tr.is_generic() {
        return;
    }
    let mut ranges: Vec<usize> = Vec::new();
    for map in tr.mapping.maps() {
        for range in &mut ranges {
            *range = map.map(*range, Assoc::After);
        }
        map.for_each(|_, _, from, to| {
            ranges.push(from);
            ranges.push(to);
        });
    }
    let mut joinable: Vec<usize> = Vec::new();
    for pair in ranges.chunks(2) {
        let (from, to) = (pair[0], pair[1]);
        let Ok(rfrom) = tr.doc.resolve(from) else {
            continue;
        };
        let depth = rfrom.shared_depth(to);
        let parent = rfrom.node(depth).clone();
        let mut index = rfrom.index_after(depth);
        let mut pos = if depth < rfrom.depth() {
            rfrom.after(depth + 1)
        } else {
            rfrom.pos()
        };
        while pos <= to {
            let Some(after) = parent.maybe_child(index) else {
                break;
            };
            if index > 0 && !joinable.contains(&pos) {
                let before = parent.child(index - 1);
                if before.node_type() == after.node_type() && is_joinable(before, after) {
                    joinable.push(pos);
                }
            }
            pos += after.node_size();
            index += 1;
        }
    }
    joinable.sort_unstable();
    for pos in joinable.into_iter().rev() {
        if can_join(&tr.doc, pos) {
            let _ = tr.join(pos, 1);
        }
    }
}
