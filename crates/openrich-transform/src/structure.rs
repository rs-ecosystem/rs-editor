//! Structural helpers (`structure.ts`): lift, wrap, split, join and friends.

use openrich_model::{ContentMatch, Fragment, MarkSet, Node, NodeRange, NodeType, Slice};
use serde_json::{Map, Value};

use crate::map::{Assoc, Mappable};
use crate::mark::{clear_incompatible, newlines};
use crate::step::{ReplaceAroundStep, ReplaceStep, Step};
use crate::transform::{NodeTypeWithAttrs, Transform, TransformError, TransformResult};

fn invalid(message: impl Into<String>) -> TransformError {
    TransformError::Invalid(message.into())
}

fn can_cut(node: &Node, start: usize, end: usize) -> bool {
    (start == 0 || node.can_replace(start, node.child_count(), &Fragment::empty(), 0, 0))
        && (end == node.child_count() || node.can_replace(0, end, &Fragment::empty(), 0, 0))
}

/// The depth the content of `range` can be lifted to, never crossing an
/// isolating node.
pub fn lift_target(range: &NodeRange) -> Option<usize> {
    let content = range
        .parent()
        .content()
        .cut_by_index(range.start_index(), range.end_index());
    let mut content_before = 0;
    let mut content_after = 0;
    let mut depth = range.depth;
    loop {
        let node = range.from.node(depth);
        let index = range.from.index(depth) + content_before;
        let end_index = range.to.index_after(depth).saturating_sub(content_after);
        if depth < range.depth
            && node.can_replace(index, end_index, &content, 0, content.child_count())
        {
            return Some(depth);
        }
        if depth == 0 || node.node_type().spec().isolating || !can_cut(node, index, end_index) {
            return None;
        }
        if index > 0 {
            content_before = 1;
        }
        if end_index < node.child_count() {
            content_after = 1;
        }
        depth -= 1;
    }
}

pub(crate) fn lift(tr: &mut Transform, range: &NodeRange, target: usize) -> TransformResult {
    let (from, to, depth) = (&range.from, &range.to, range.depth);
    let gap_start = from.before(depth + 1);
    let gap_end = to.after(depth + 1);
    let mut start = gap_start;
    let mut end = gap_end;

    let mut before = Fragment::empty();
    let mut open_start = 0;
    let mut splitting = false;
    for d in (target + 1..=depth).rev() {
        if splitting || from.index(d) > 0 {
            splitting = true;
            before = Fragment::from_node(from.node(d).copy(before));
            open_start += 1;
        } else {
            start -= 1;
        }
    }
    let mut after = Fragment::empty();
    let mut open_end = 0;
    splitting = false;
    for d in (target + 1..=depth).rev() {
        if splitting || to.after(d + 1) < to.end(d) {
            splitting = true;
            after = Fragment::from_node(to.node(d).copy(after));
            open_end += 1;
        } else {
            end += 1;
        }
    }
    let insert = before.size() - open_start;
    tr.step(Step::ReplaceAround(ReplaceAroundStep {
        from: start,
        to: end,
        gap_from: gap_start,
        gap_to: gap_end,
        slice: Slice::new(before.append(&after), open_start, open_end),
        insert,
        structure: true,
    }))?;
    Ok(())
}

/// Wrappers (outermost first) that place `range` inside a `ty` node, or
/// `None` when no valid wrapping exists.
pub fn find_wrapping(
    range: &NodeRange,
    ty: &NodeType,
    attrs: Option<Map<String, Value>>,
    inner_range: Option<&NodeRange>,
) -> Option<Vec<NodeTypeWithAttrs>> {
    let around = find_wrapping_outside(range, ty)?;
    let inner = find_wrapping_inside(inner_range.unwrap_or(range), ty)?;
    let mut result: Vec<NodeTypeWithAttrs> =
        around.into_iter().map(NodeTypeWithAttrs::new).collect();
    result.push(NodeTypeWithAttrs {
        ty: ty.clone(),
        attrs,
    });
    result.extend(inner.into_iter().map(NodeTypeWithAttrs::new));
    Some(result)
}

fn find_wrapping_outside(range: &NodeRange, ty: &NodeType) -> Option<Vec<NodeType>> {
    let parent = range.parent();
    let around = parent
        .content_match_at(range.start_index())
        .ok()?
        .find_wrapping(ty)?;
    let outer = around.first().unwrap_or(ty);
    parent
        .can_replace_with(range.start_index(), range.end_index(), outer, None)
        .then_some(around)
}

fn find_wrapping_inside(range: &NodeRange, ty: &NodeType) -> Option<Vec<NodeType>> {
    let parent = range.parent();
    let inner = parent.child(range.start_index());
    let inside = ty.content_match().find_wrapping(inner.node_type())?;
    let last = inside.last().unwrap_or(ty);
    let mut matched = Some(last.content_match());
    for index in range.start_index()..range.end_index() {
        matched = matched.and_then(|m| m.match_type(parent.child(index).node_type()));
    }
    matched.filter(ContentMatch::valid_end).map(|_| inside)
}

pub(crate) fn wrap(
    tr: &mut Transform,
    range: &NodeRange,
    wrappers: &[NodeTypeWithAttrs],
) -> TransformResult {
    let mut content = Fragment::empty();
    for wrapper in wrappers.iter().rev() {
        if content.size() > 0 {
            let valid = wrapper
                .ty
                .content_match()
                .match_fragment(&content, 0, content.child_count())
                .is_some_and(|m| m.valid_end());
            if !valid {
                return Err(invalid(
                    "wrapper type does not form valid content of its parent wrapper",
                ));
            }
        }
        content = Fragment::from_node(wrapper.ty.create(
            wrapper.attrs.as_ref(),
            content,
            MarkSet::none(),
        )?);
    }
    let (start, end) = (range.start(), range.end());
    tr.step(Step::ReplaceAround(ReplaceAroundStep {
        from: start,
        to: end,
        gap_from: start,
        gap_to: end,
        slice: Slice::new(content, 0, 0),
        insert: wrappers.len(),
        structure: true,
    }))?;
    Ok(())
}

/// How newlines must change when content moves into `ty`: `Some(true)`
/// turns `\n` into linebreak nodes, `Some(false)` the reverse.
fn convert_newlines(ty: &NodeType) -> Option<bool> {
    let linebreak = ty.schema().linebreak_replacement()?;
    let pre = ty.is_whitespace_pre();
    let supports = ty.content_match().match_type(&linebreak).is_some();
    match (pre, supports) {
        (true, false) => Some(false),
        (false, true) => Some(true),
        _ => None,
    }
}

pub(crate) fn set_block_type(
    tr: &mut Transform,
    from: usize,
    to: usize,
    ty: &NodeType,
    attrs: &dyn Fn(&Node) -> Option<Map<String, Value>>,
) -> TransformResult {
    if !ty.is_textblock() {
        return Err(invalid(
            "type given to set_block_type should be a textblock",
        ));
    }
    let map_from = tr.steps.len();
    let doc = tr.doc.clone();
    let mut error = None;
    doc.nodes_between(from, to, &mut |node, pos, _, _| {
        if error.is_some() {
            return false;
        }
        let attrs_here = attrs(node);
        let same = ty
            .compute_attrs(attrs_here.as_ref())
            .is_ok_and(|computed| node.has_markup(ty, &computed, node.marks()));
        if !node.is_textblock() || same {
            return true;
        }
        let mapped = tr.mapping.slice_from(map_from).map(pos, Assoc::After);
        if !can_change_type(&tr.doc, mapped, ty) {
            return true;
        }
        let result = (|| -> TransformResult {
            let convert = convert_newlines(ty);
            if convert == Some(false) {
                replace_linebreaks(tr, node, pos, map_from)?;
            }
            let at = tr.mapping.slice_from(map_from).map(pos, Assoc::After);
            clear_incompatible(tr, at, ty, None, convert.is_none())?;
            let mapping = tr.mapping.slice_from(map_from);
            let start = mapping.map(pos, Assoc::After);
            let end = mapping.map(pos + node.node_size(), Assoc::After);
            let wrapper =
                ty.create(attrs_here.as_ref(), Fragment::empty(), node.marks().clone())?;
            tr.step(Step::ReplaceAround(ReplaceAroundStep {
                from: start,
                to: end,
                gap_from: start + 1,
                gap_to: end - 1,
                slice: Slice::new(Fragment::from_node(wrapper), 0, 0),
                insert: 1,
                structure: true,
            }))?;
            if convert == Some(true) {
                replace_newlines(tr, node, pos, map_from)?;
            }
            Ok(())
        })();
        if let Err(err) = result {
            error = Some(err);
        }
        false
    });
    error.map_or(Ok(()), Err)
}

fn replace_newlines(
    tr: &mut Transform,
    node: &Node,
    pos: usize,
    map_from: usize,
) -> TransformResult {
    let Some(linebreak) = node.node_type().schema().linebreak_replacement() else {
        return Ok(());
    };
    let mut offset = 0;
    for child in node.children() {
        if let Some(text) = child.text() {
            for (index, len) in newlines(text) {
                let start = tr
                    .mapping
                    .slice_from(map_from)
                    .map(pos + 1 + offset + index, Assoc::After);
                let brk = linebreak.create(None, Fragment::empty(), MarkSet::none())?;
                tr.replace_with(start, start + len, Fragment::from_node(brk))?;
            }
        }
        offset += child.node_size();
    }
    Ok(())
}

fn replace_linebreaks(
    tr: &mut Transform,
    node: &Node,
    pos: usize,
    map_from: usize,
) -> TransformResult {
    let linebreak = node.node_type().schema().linebreak_replacement();
    let mut offset = 0;
    for child in node.children() {
        if Some(child.node_type()) == linebreak.as_ref() {
            let start = tr
                .mapping
                .slice_from(map_from)
                .map(pos + 1 + offset, Assoc::After);
            let newline = node.node_type().schema().text("\n", MarkSet::none())?;
            tr.replace_with(start, start + 1, Fragment::from_node(newline))?;
        }
        offset += child.node_size();
    }
    Ok(())
}

fn can_change_type(doc: &Node, pos: usize, ty: &NodeType) -> bool {
    let Ok(resolved) = doc.resolve(pos) else {
        return false;
    };
    let index = resolved.index(resolved.depth());
    resolved
        .parent()
        .can_replace_with(index, index + 1, ty, None)
}

pub(crate) fn set_node_markup(
    tr: &mut Transform,
    pos: usize,
    ty: Option<&NodeType>,
    attrs: Option<&Map<String, Value>>,
    marks: Option<MarkSet>,
) -> TransformResult {
    let node = tr
        .doc
        .node_at(pos)
        .ok_or_else(|| invalid(format!("no node at position {pos}")))?;
    let ty = ty.cloned().unwrap_or_else(|| node.node_type().clone());
    let new_node = ty.create(
        attrs,
        Fragment::empty(),
        marks.unwrap_or_else(|| node.marks().clone()),
    )?;
    if node.is_leaf() {
        tr.replace_with(pos, pos + node.node_size(), Fragment::from_node(new_node))?;
        return Ok(());
    }
    if !ty.valid_content(node.content()) {
        return Err(invalid(format!(
            "invalid content for node type {}",
            ty.name()
        )));
    }
    tr.step(Step::ReplaceAround(ReplaceAroundStep {
        from: pos,
        to: pos + node.node_size(),
        gap_from: pos + 1,
        gap_to: pos + node.node_size() - 1,
        slice: Slice::new(Fragment::from_node(new_node), 0, 0),
        insert: 1,
        structure: true,
    }))?;
    Ok(())
}

fn create_type(spec: &NodeTypeWithAttrs, content: Fragment) -> Option<Node> {
    spec.ty
        .create(spec.attrs.as_ref(), content, MarkSet::none())
        .ok()
}

/// Whether splitting at `pos`, `depth` levels deep, is allowed.
pub fn can_split(
    doc: &Node,
    pos: usize,
    depth: usize,
    types_after: Option<&[Option<NodeTypeWithAttrs>]>,
) -> bool {
    let Ok(resolved) = doc.resolve(pos) else {
        return false;
    };
    let Some(base) = resolved.depth().checked_sub(depth) else {
        return false;
    };
    let parent = resolved.parent();
    let index = resolved.index(resolved.depth());
    let inner_type = types_after
        .and_then(|types| types.last())
        .and_then(Option::as_ref)
        .map_or_else(|| parent.node_type().clone(), |t| t.ty.clone());
    if parent.node_type().spec().isolating
        || !parent.can_replace(index, parent.child_count(), &Fragment::empty(), 0, 0)
        || !inner_type.valid_content(&parent.content().cut_by_index(index, parent.child_count()))
    {
        return false;
    }
    let mut i = depth as isize - 2;
    for d in (base + 1..resolved.depth()).rev() {
        let node = resolved.node(d);
        let index = resolved.index(d);
        if node.node_type().spec().isolating {
            return false;
        }
        let mut rest = node.content().cut_by_index(index, node.child_count());
        let pick = |n: isize| {
            types_after
                .and_then(|types| usize::try_from(n).ok().and_then(|n| types.get(n)))
                .and_then(Option::as_ref)
        };
        if let Some(override_child) = pick(i + 1) {
            let Some(created) = create_type(override_child, Fragment::empty()) else {
                return false;
            };
            rest = rest.replace_child(0, created);
        }
        let after = pick(i).map_or_else(|| node.node_type().clone(), |t| t.ty.clone());
        if !node.can_replace(index + 1, node.child_count(), &Fragment::empty(), 0, 0)
            || !after.valid_content(&rest)
        {
            return false;
        }
        i -= 1;
    }
    let index = resolved.index_after(base);
    let base_type = types_after
        .and_then(|types| types.first())
        .and_then(Option::as_ref)
        .map_or_else(
            || resolved.node(base + 1).node_type().clone(),
            |t| t.ty.clone(),
        );
    resolved
        .node(base)
        .can_replace_with(index, index, &base_type, None)
}

pub(crate) fn split(
    tr: &mut Transform,
    pos: usize,
    depth: usize,
    types_after: Option<&[Option<NodeTypeWithAttrs>]>,
) -> TransformResult {
    let resolved = tr.doc.resolve(pos)?;
    let mut before = Fragment::empty();
    let mut after = Fragment::empty();
    let base = resolved
        .depth()
        .checked_sub(depth)
        .ok_or_else(|| invalid("split depth exceeds position depth"))?;
    for (i, d) in (base + 1..=resolved.depth()).rev().enumerate() {
        let i = depth - 1 - i;
        before = Fragment::from_node(resolved.node(d).copy(before));
        let type_after = types_after
            .and_then(|types| types.get(i))
            .and_then(Option::as_ref);
        after = Fragment::from_node(match type_after {
            Some(spec) => spec
                .ty
                .create(spec.attrs.as_ref(), after, MarkSet::none())?,
            None => resolved.node(d).copy(after),
        });
    }
    tr.step(Step::Replace(ReplaceStep {
        from: pos,
        to: pos,
        slice: Slice::new(before.append(&after), depth, depth),
        structure: true,
    }))?;
    Ok(())
}

/// Whether the blocks before and after `pos` can be joined.
pub fn can_join(doc: &Node, pos: usize) -> bool {
    let Ok(resolved) = doc.resolve(pos) else {
        return false;
    };
    let index = resolved.index(resolved.depth());
    joinable(
        resolved.node_before().as_ref(),
        resolved.node_after().as_ref(),
    ) && resolved
        .parent()
        .can_replace(index, index + 1, &Fragment::empty(), 0, 0)
}

fn can_append_with_substituted_linebreaks(a: &Node, b: &Node) -> bool {
    let Ok(mut matched) = a.content_match_at(a.child_count()) else {
        return false;
    };
    let schema = a.node_type().schema();
    let linebreak = schema.linebreak_replacement();
    for child in b.children() {
        let ty = if Some(child.node_type()) == linebreak.as_ref() {
            schema.text_type()
        } else {
            child.node_type().clone()
        };
        match matched.match_type(&ty) {
            Some(next) => matched = next,
            None => return false,
        }
        if !a.node_type().allows_marks(child.marks()) {
            return false;
        }
    }
    matched.valid_end()
}

fn joinable(a: Option<&Node>, b: Option<&Node>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => !a.is_leaf() && can_append_with_substituted_linebreaks(a, b),
        _ => false,
    }
}

/// An ancestor boundary of `pos` that can be joined backward (`dir < 0`)
/// or forward.
pub fn join_point(doc: &Node, pos: usize, dir: i8) -> Option<usize> {
    let resolved = doc.resolve(pos).ok()?;
    let mut pos = pos;
    for d in (0..=resolved.depth()).rev() {
        let mut index = resolved.index(d);
        let (before, after) = if d == resolved.depth() {
            (resolved.node_before(), resolved.node_after())
        } else if dir > 0 {
            index += 1;
            (
                Some(resolved.node(d + 1).clone()),
                resolved.node(d).maybe_child(index).cloned(),
            )
        } else {
            (
                index
                    .checked_sub(1)
                    .and_then(|i| resolved.node(d).maybe_child(i).cloned()),
                Some(resolved.node(d + 1).clone()),
            )
        };
        if let Some(before) = &before
            && !before.is_textblock()
            && joinable(Some(before), after.as_ref())
            && resolved
                .node(d)
                .can_replace(index, index + 1, &Fragment::empty(), 0, 0)
        {
            return Some(pos);
        }
        if d == 0 {
            break;
        }
        pos = if dir < 0 {
            resolved.before(d)
        } else {
            resolved.after(d)
        };
    }
    None
}

pub(crate) fn join(tr: &mut Transform, pos: usize, depth: usize) -> TransformResult {
    let before = tr.doc.resolve(pos - depth)?;
    let before_type = before.parent().node_type().clone();
    let convert = if before_type.inline_content() {
        convert_newlines(&before_type)
    } else {
        None
    };
    let map_from = tr.steps.len();
    if convert == Some(false) {
        let after = tr.doc.resolve(pos + depth)?;
        let node = after.parent().clone();
        replace_linebreaks(tr, &node, after.before(after.depth()), map_from)?;
    }
    if before_type.inline_content() {
        let content_match = before
            .parent()
            .content_match_at(before.index(before.depth()))?;
        clear_incompatible(
            tr,
            pos + depth - 1,
            &before_type,
            Some(content_match),
            convert.is_none(),
        )?;
    }
    let mapping = tr.mapping.slice_from(map_from);
    let start = mapping.map(pos - depth, Assoc::After);
    tr.step(Step::Replace(ReplaceStep {
        from: start,
        to: mapping.map(pos + depth, Assoc::Before),
        slice: Slice::empty(),
        structure: true,
    }))?;
    if convert == Some(true) {
        let full = tr.doc.resolve(start)?;
        let node = full.parent().clone();
        let steps = tr.steps.len();
        replace_newlines(tr, &node, full.before(full.depth()), steps)?;
    }
    Ok(())
}

/// A position near `pos` where a `ty` node can be inserted.
pub fn insert_point(doc: &Node, pos: usize, ty: &NodeType) -> Option<usize> {
    let resolved = doc.resolve(pos).ok()?;
    let index = resolved.index(resolved.depth());
    if resolved.parent().can_replace_with(index, index, ty, None) {
        return Some(pos);
    }
    if resolved.parent_offset() == 0 {
        for d in (0..resolved.depth()).rev() {
            let index = resolved.index(d);
            if resolved.node(d).can_replace_with(index, index, ty, None) {
                return Some(resolved.before(d + 1));
            }
            if index > 0 {
                return None;
            }
        }
    }
    if resolved.parent_offset() == resolved.parent().content().size() {
        for d in (0..resolved.depth()).rev() {
            let index = resolved.index_after(d);
            if resolved.node(d).can_replace_with(index, index, ty, None) {
                return Some(resolved.after(d + 1));
            }
            if index < resolved.node(d).child_count() {
                return None;
            }
        }
    }
    None
}

/// A position at or around `pos` where `slice` can be dropped.
pub fn drop_point(doc: &Node, pos: usize, slice: &Slice) -> Option<usize> {
    let resolved = doc.resolve(pos).ok()?;
    if slice.content.size() == 0 {
        return Some(pos);
    }
    let mut content = slice.content.clone();
    for _ in 0..slice.open_start {
        content = content.first_child()?.content().clone();
    }
    let passes = if slice.open_start == 0 && slice.size() > 0 {
        2
    } else {
        1
    };
    for pass in 1..=passes {
        for d in (0..=resolved.depth()).rev() {
            let bias: i8 = if d == resolved.depth() {
                0
            } else if resolved.pos() * 2 <= resolved.start(d + 1) + resolved.end(d + 1) {
                -1
            } else {
                1
            };
            let insert_pos = resolved.index(d) + usize::from(bias > 0);
            let parent = resolved.node(d);
            let fits = if pass == 1 {
                parent.can_replace(insert_pos, insert_pos, &content, 0, content.child_count())
            } else {
                content
                    .first_child()
                    .and_then(|first| {
                        parent
                            .content_match_at(insert_pos)
                            .ok()?
                            .find_wrapping(first.node_type())
                    })
                    .and_then(|wrapping| wrapping.first().cloned())
                    .is_some_and(|outer| {
                        parent.can_replace_with(insert_pos, insert_pos, &outer, None)
                    })
            };
            if fits {
                return Some(match bias {
                    0 => resolved.pos(),
                    b if b < 0 => resolved.before(d + 1),
                    _ => resolved.after(d + 1),
                });
            }
        }
    }
    None
}
