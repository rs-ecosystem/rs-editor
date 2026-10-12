//! Mark helpers (`mark.ts`).

use openrich_model::{ContentMatch, Fragment, Mark, NodeType, Slice};

use crate::step::{MarkStep, ReplaceStep, Step};
use crate::transform::{MarkMatch, Transform, TransformError, TransformResult};

pub(crate) fn add_mark(tr: &mut Transform, from: usize, to: usize, mark: &Mark) -> TransformResult {
    let mut removed: Vec<MarkStep> = Vec::new();
    let mut added: Vec<MarkStep> = Vec::new();
    let doc = tr.doc.clone();
    doc.nodes_between(from, to, &mut |node, pos, parent, _| {
        if !node.is_inline() {
            return true;
        }
        let marks = node.marks();
        if !mark.is_in_set(marks)
            && parent.is_some_and(|parent| parent.node_type().allows_mark_type(mark.mark_type()))
        {
            let start = pos.max(from);
            let end = (pos + node.node_size()).min(to);
            let new_set = mark.add_to_set(marks);
            for old in marks.iter() {
                if old.is_in_set(&new_set) {
                    continue;
                }
                match removed.last_mut() {
                    Some(removing) if removing.to == start && removing.mark == *old => {
                        removing.to = end
                    }
                    _ => removed.push(MarkStep {
                        from: start,
                        to: end,
                        mark: old.clone(),
                    }),
                }
            }
            match added.last_mut() {
                Some(adding) if adding.to == start => adding.to = end,
                _ => added.push(MarkStep {
                    from: start,
                    to: end,
                    mark: mark.clone(),
                }),
            }
        }
        true
    });
    for step in removed {
        tr.step(Step::RemoveMark(step))?;
    }
    for step in added {
        tr.step(Step::AddMark(step))?;
    }
    Ok(())
}

pub(crate) fn remove_mark(
    tr: &mut Transform,
    from: usize,
    to: usize,
    matching: &MarkMatch,
) -> TransformResult {
    struct Matched {
        style: Mark,
        from: usize,
        to: usize,
        step: usize,
    }
    let mut matched: Vec<Matched> = Vec::new();
    let mut step = 0;
    let doc = tr.doc.clone();
    doc.nodes_between(from, to, &mut |node, pos, _, _| {
        if !node.is_inline() {
            return true;
        }
        step += 1;
        let to_remove: Vec<Mark> = match matching {
            MarkMatch::Type(ty) => node
                .marks()
                .iter()
                .filter(|m| m.mark_type() == ty)
                .cloned()
                .collect(),
            MarkMatch::Mark(mark) if mark.is_in_set(node.marks()) => vec![mark.clone()],
            MarkMatch::Mark(_) => Vec::new(),
            MarkMatch::All => node.marks().to_vec(),
        };
        let end = (pos + node.node_size()).min(to);
        for style in to_remove {
            match matched
                .iter_mut()
                .find(|m| m.step + 1 == step && m.style == style)
            {
                Some(found) => {
                    found.to = end;
                    found.step = step;
                }
                None => matched.push(Matched {
                    style,
                    from: pos.max(from),
                    to: end,
                    step,
                }),
            }
        }
        true
    });
    for m in matched {
        tr.step(Step::RemoveMark(MarkStep {
            from: m.from,
            to: m.to,
            mark: m.style,
        }))?;
    }
    Ok(())
}

/// UTF-16 offsets and lengths of `\r\n`, `\n` and `\r` in `text`.
pub(crate) fn newlines(text: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut offset = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
                found.push((offset, 2));
                offset += 2;
            }
            '\r' | '\n' => {
                found.push((offset, 1));
                offset += 1;
            }
            other => offset += other.len_utf16(),
        }
    }
    found
}

pub(crate) fn clear_incompatible(
    tr: &mut Transform,
    pos: usize,
    parent_type: &NodeType,
    content_match: Option<ContentMatch>,
    clear_newlines: bool,
) -> TransformResult {
    let node = tr
        .doc
        .node_at(pos)
        .ok_or_else(|| TransformError::Invalid(format!("no node at position {pos}")))?;
    let mut matched = content_match.unwrap_or_else(|| parent_type.content_match());
    let mut repl_steps = Vec::new();
    let mut cur = pos + 1;
    for child in node.children() {
        let end = cur + child.node_size();
        match matched.match_type(child.node_type()) {
            None => repl_steps.push(Step::Replace(ReplaceStep {
                from: cur,
                to: end,
                slice: Slice::empty(),
                structure: false,
            })),
            Some(allowed) => {
                matched = allowed;
                for mark in child.marks().iter() {
                    if !parent_type.allows_mark_type(mark.mark_type()) {
                        tr.step(Step::RemoveMark(MarkStep {
                            from: cur,
                            to: end,
                            mark: mark.clone(),
                        }))?;
                    }
                }
                if clear_newlines
                    && let Some(text) = child.text()
                    && !parent_type.is_whitespace_pre()
                {
                    let space = parent_type
                        .schema()
                        .text(" ", parent_type.allowed_marks(child.marks()))?;
                    for (index, len) in newlines(text) {
                        repl_steps.push(Step::Replace(ReplaceStep {
                            from: cur + index,
                            to: cur + index + len,
                            slice: Slice::new(Fragment::from_node(space.clone()), 0, 0),
                            structure: false,
                        }));
                    }
                }
            }
        }
        cur = end;
    }
    if !matched.valid_end() {
        let fill = matched
            .fill_before(&Fragment::empty(), true, 0)
            .ok_or_else(|| {
                TransformError::Invalid(format!("cannot complete {}", parent_type.name()))
            })?;
        tr.replace(cur, cur, Slice::new(fill, 0, 0))?;
    }
    for step in repl_steps.into_iter().rev() {
        tr.step(step)?;
    }
    Ok(())
}
