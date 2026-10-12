//! Slices and the structural replace algorithm from ProseMirror's `replace.ts`.

use std::fmt;

use crate::error::{ModelError, Result};
use crate::fragment::Fragment;
use crate::node::Node;
use crate::resolved::ResolvedPos;

/// A piece of a document: a fragment plus how many levels are open at
/// each side, so it can be fitted into another document.
#[derive(Clone, PartialEq, Eq)]
pub struct Slice {
    pub content: Fragment,
    pub open_start: usize,
    pub open_end: usize,
}

impl fmt::Debug for Slice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}({},{})",
            self.content, self.open_start, self.open_end
        )
    }
}

impl Slice {
    pub fn new(content: Fragment, open_start: usize, open_end: usize) -> Self {
        Self {
            content,
            open_start,
            open_end,
        }
    }

    pub fn empty() -> Self {
        Self::new(Fragment::empty(), 0, 0)
    }

    /// Size the slice adds to a document.
    pub fn size(&self) -> usize {
        self.content.size() - self.open_start - self.open_end
    }

    pub fn insert_at(&self, pos: usize, fragment: &Fragment) -> Option<Slice> {
        let content = insert_into(&self.content, pos + self.open_start, fragment, None)?;
        Some(Slice::new(content, self.open_start, self.open_end))
    }

    pub fn remove_between(&self, from: usize, to: usize) -> Result<Slice> {
        Ok(Slice::new(
            remove_range(&self.content, from + self.open_start, to + self.open_start)?,
            self.open_start,
            self.open_end,
        ))
    }

    /// The fragment with both sides opened as deep as possible.
    pub fn max_open(fragment: Fragment, open_isolating: bool) -> Slice {
        let opens = |mut node: Option<&Node>, last: bool| {
            let mut depth = 0;
            while let Some(current) = node {
                if current.is_leaf() || (!open_isolating && current.node_type().spec().isolating) {
                    break;
                }
                depth += 1;
                node = if last {
                    current.last_child()
                } else {
                    current.first_child()
                };
            }
            depth
        };
        let open_start = opens(fragment.first_child(), false);
        let open_end = opens(fragment.last_child(), true);
        Slice::new(fragment, open_start, open_end)
    }
}

fn remove_range(content: &Fragment, from: usize, to: usize) -> Result<Fragment> {
    let start = content.find_index(from, -1)?;
    let end = content.find_index(to, -1)?;
    let child = content.maybe_child(start.index);
    if start.offset == from || child.is_some_and(Node::is_text) {
        if end.offset != to && !content.child(end.index).is_text() {
            return Err(ModelError::Replace("removing non-flat range".into()));
        }
        return Ok(content.cut(0, from).append(&content.cut_to_end(to)));
    }
    if start.index != end.index {
        return Err(ModelError::Replace("removing non-flat range".into()));
    }
    let child = content.child(start.index);
    let inner = remove_range(
        child.content(),
        from - start.offset - 1,
        to - start.offset - 1,
    )?;
    Ok(content.replace_child(start.index, child.copy(inner)))
}

fn insert_into(
    content: &Fragment,
    dist: usize,
    insert: &Fragment,
    parent: Option<&Node>,
) -> Option<Fragment> {
    let found = content.find_index(dist, -1).ok()?;
    let child = content.maybe_child(found.index);
    if found.offset == dist || child.is_some_and(Node::is_text) {
        if let Some(parent) = parent
            && !parent.can_replace(found.index, found.index, insert, 0, insert.child_count())
        {
            return None;
        }
        return Some(
            content
                .cut(0, dist)
                .append(insert)
                .append(&content.cut_to_end(dist)),
        );
    }
    let child = child?;
    let inner = insert_into(
        child.content(),
        dist - found.offset - 1,
        insert,
        Some(child),
    )?;
    Some(content.replace_child(found.index, child.copy(inner)))
}

pub(crate) fn replace(from: &ResolvedPos, to: &ResolvedPos, slice: &Slice) -> Result<Node> {
    if slice.open_start > from.depth() {
        return Err(ModelError::Replace(
            "inserted content deeper than insertion position".into(),
        ));
    }
    if from.depth() - slice.open_start != to.depth() - slice.open_end {
        return Err(ModelError::Replace("inconsistent open depths".into()));
    }
    replace_outer(from, to, slice, 0)
}

fn replace_outer(
    from: &ResolvedPos,
    to: &ResolvedPos,
    slice: &Slice,
    depth: usize,
) -> Result<Node> {
    let index = from.index(depth);
    let node = from.node(depth);
    if index == to.index(depth) && depth < from.depth() - slice.open_start {
        let inner = replace_outer(from, to, slice, depth + 1)?;
        Ok(node.copy(node.content().replace_child(index, inner)))
    } else if slice.content.size() == 0 {
        close(node, replace_two_way(from, to, depth)?)
    } else if slice.open_start == 0
        && slice.open_end == 0
        && from.depth() == depth
        && to.depth() == depth
    {
        let parent = from.parent();
        let content = parent.content();
        close(
            parent,
            content
                .cut(0, from.parent_offset())
                .append(&slice.content)
                .append(&content.cut_to_end(to.parent_offset())),
        )
    } else {
        let (start, end) = prepare_slice_for_replace(slice, from)?;
        close(node, replace_three_way(from, &start, &end, to, depth)?)
    }
}

fn check_join(main: &Node, sub: &Node) -> Result<()> {
    if sub.node_type().compatible_content(main.node_type()) {
        Ok(())
    } else {
        Err(ModelError::Replace(format!(
            "cannot join {} onto {}",
            sub.type_name(),
            main.type_name()
        )))
    }
}

fn joinable(before: &ResolvedPos, after: &ResolvedPos, depth: usize) -> Result<Node> {
    let node = before.node(depth);
    check_join(node, after.node(depth))?;
    Ok(node.clone())
}

fn add_node(child: Node, target: &mut Vec<Node>) {
    if let Some(last) = target.last_mut()
        && child.is_text()
        && child.same_markup(last)
    {
        let merged = format!(
            "{}{}",
            last.text().unwrap_or(""),
            child.text().unwrap_or("")
        );
        *last = last.with_text(merged);
    } else {
        target.push(child);
    }
}

fn add_range(
    start: Option<&ResolvedPos>,
    end: Option<&ResolvedPos>,
    depth: usize,
    target: &mut Vec<Node>,
) {
    let Some(node) = end.or(start).map(|pos| pos.node(depth)) else {
        return;
    };
    let mut start_index = 0;
    let end_index = end.map_or(node.child_count(), |end| end.index(depth));
    if let Some(start) = start {
        start_index = start.index(depth);
        if start.depth() > depth {
            start_index += 1;
        } else if start.text_offset() > 0 {
            if let Some(after) = start.node_after() {
                add_node(after, target);
            }
            start_index += 1;
        }
    }
    for index in start_index..end_index {
        add_node(node.child(index).clone(), target);
    }
    if let Some(end) = end
        && end.depth() == depth
        && end.text_offset() > 0
        && let Some(before) = end.node_before()
    {
        add_node(before, target);
    }
}

fn close(node: &Node, content: Fragment) -> Result<Node> {
    node.node_type().check_content(&content)?;
    Ok(node.copy(content))
}

fn replace_three_way(
    from: &ResolvedPos,
    start: &ResolvedPos,
    end: &ResolvedPos,
    to: &ResolvedPos,
    depth: usize,
) -> Result<Fragment> {
    let open_start = if from.depth() > depth {
        Some(joinable(from, start, depth + 1)?)
    } else {
        None
    };
    let open_end = if to.depth() > depth {
        Some(joinable(end, to, depth + 1)?)
    } else {
        None
    };
    let mut content = Vec::new();
    add_range(None, Some(from), depth, &mut content);
    match (&open_start, &open_end) {
        (Some(open_start), Some(open_end)) if start.index(depth) == end.index(depth) => {
            check_join(open_start, open_end)?;
            let inner = replace_three_way(from, start, end, to, depth + 1)?;
            add_node(close(open_start, inner)?, &mut content);
        }
        _ => {
            if let Some(open_start) = &open_start {
                add_node(
                    close(open_start, replace_two_way(from, start, depth + 1)?)?,
                    &mut content,
                );
            }
            add_range(Some(start), Some(end), depth, &mut content);
            if let Some(open_end) = &open_end {
                add_node(
                    close(open_end, replace_two_way(end, to, depth + 1)?)?,
                    &mut content,
                );
            }
        }
    }
    add_range(Some(to), None, depth, &mut content);
    Ok(Fragment::new(content))
}

fn replace_two_way(from: &ResolvedPos, to: &ResolvedPos, depth: usize) -> Result<Fragment> {
    let mut content = Vec::new();
    add_range(None, Some(from), depth, &mut content);
    if from.depth() > depth {
        let ty = joinable(from, to, depth + 1)?;
        add_node(
            close(&ty, replace_two_way(from, to, depth + 1)?)?,
            &mut content,
        );
    }
    add_range(Some(to), None, depth, &mut content);
    Ok(Fragment::new(content))
}

fn prepare_slice_for_replace(
    slice: &Slice,
    along: &ResolvedPos,
) -> Result<(ResolvedPos, ResolvedPos)> {
    let extra = along.depth() - slice.open_start;
    let parent = along.node(extra);
    let mut node = parent.copy(slice.content.clone());
    for depth in (0..extra).rev() {
        node = along.node(depth).copy(Fragment::from_node(node));
    }
    let start = node.resolve(slice.open_start + extra)?;
    let end = node.resolve(node.content().size() - slice.open_end - extra)?;
    Ok((start, end))
}
