//! Fitting slices into documents (`replace.ts`).

use openrich_model::{ContentMatch, Fragment, MarkSet, Node, NodeType, ResolvedPos, Slice};
use serde_json::{Map, Value};

use crate::step::{ReplaceAroundStep, ReplaceStep, Step};
use crate::structure::insert_point;
use crate::transform::{Transform, TransformError, TransformResult};

fn broken(what: &str) -> TransformError {
    TransformError::Invalid(format!("replace fitting invariant failed: {what}"))
}

/// A step that inserts `slice` at `from..to`, fitting it into the document
/// structure. `None` when nothing would change or no fit exists.
pub fn replace_step(
    doc: &Node,
    from: usize,
    to: usize,
    slice: Slice,
) -> TransformResult<Option<Step>> {
    if from == to && slice.size() == 0 {
        return Ok(None);
    }
    let rfrom = doc.resolve(from)?;
    let rto = doc.resolve(to)?;
    if fits_trivially(&rfrom, &rto, &slice) {
        return Ok(Some(Step::replace(from, to, slice)));
    }
    Fitter::new(rfrom, rto, slice).fit()
}

fn fits_trivially(from: &ResolvedPos, to: &ResolvedPos, slice: &Slice) -> bool {
    slice.open_start == 0
        && slice.open_end == 0
        && from.start(from.depth()) == to.start(to.depth())
        && from.parent().can_replace(
            from.index(from.depth()),
            to.index(to.depth()),
            &slice.content,
            0,
            slice.content.child_count(),
        )
}

struct Fittable {
    slice_depth: usize,
    frontier_depth: usize,
    parent: Option<Node>,
    inject: Option<Fragment>,
    wrap: Option<Vec<NodeType>>,
}

struct FrontierEntry {
    ty: NodeType,
    matched: ContentMatch,
}

struct CloseLevel {
    depth: usize,
    fit: Fragment,
    moved: ResolvedPos,
}

/// Places the content of a slice into the gap between `from` and `to`,
/// opening and closing nodes as needed. See ProseMirror's `Fitter`.
struct Fitter {
    from: ResolvedPos,
    to: ResolvedPos,
    unplaced: Slice,
    frontier: Vec<FrontierEntry>,
    placed: Fragment,
}

fn content_at(fragment: &Fragment, depth: usize) -> Fragment {
    let mut fragment = fragment.clone();
    for _ in 0..depth {
        fragment = fragment
            .first_child()
            .map(|child| child.content().clone())
            .unwrap_or_default();
    }
    fragment
}

fn drop_from_fragment(fragment: &Fragment, depth: usize, count: usize) -> Fragment {
    if depth == 0 {
        return fragment.cut_by_index(count.min(fragment.child_count()), fragment.child_count());
    }
    match fragment.first_child() {
        Some(first) => {
            let inner = drop_from_fragment(first.content(), depth - 1, count);
            fragment.replace_child(0, first.copy(inner))
        }
        None => fragment.clone(),
    }
}

fn add_to_fragment(fragment: &Fragment, depth: usize, content: &Fragment) -> Fragment {
    if depth == 0 {
        return fragment.append(content);
    }
    match fragment.last_child() {
        Some(last) => {
            let inner = add_to_fragment(last.content(), depth - 1, content);
            fragment.replace_child(fragment.child_count() - 1, last.copy(inner))
        }
        None => fragment.clone(),
    }
}

fn close_node_start(node: &Node, open_start: isize, open_end: isize) -> TransformResult<Node> {
    if open_start <= 0 {
        return Ok(node.clone());
    }
    let mut frag = node.content().clone();
    if open_start > 1
        && let Some(first) = frag.first_child()
    {
        let inner_end = if frag.child_count() == 1 {
            open_end - 1
        } else {
            0
        };
        let closed = close_node_start(first, open_start - 1, inner_end)?;
        frag = frag.replace_child(0, closed);
    }
    let content_match = node.node_type().content_match();
    frag = content_match
        .fill_before(&frag, false, 0)
        .ok_or_else(|| broken("close_node_start fill"))?
        .append(&frag);
    if open_end <= 0 {
        let end = content_match
            .match_fragment(&frag, 0, frag.child_count())
            .and_then(|m| m.fill_before(&Fragment::empty(), true, 0))
            .ok_or_else(|| broken("close_node_start end"))?;
        frag = frag.append(&end);
    }
    Ok(node.copy(frag))
}

fn invalid_marks(ty: &NodeType, fragment: &Fragment, start: usize) -> bool {
    (start..fragment.child_count()).any(|i| !ty.allows_marks(fragment.child(i).marks()))
}

fn content_after_fits(
    to: &ResolvedPos,
    depth: usize,
    ty: &NodeType,
    matched: &ContentMatch,
    open: bool,
) -> Option<Fragment> {
    let node = to.node(depth);
    let index = if open {
        to.index_after(depth)
    } else {
        to.index(depth)
    };
    if index == node.child_count() && !ty.compatible_content(node.node_type()) {
        return None;
    }
    let fit = matched.fill_before(node.content(), true, index)?;
    (!invalid_marks(ty, node.content(), index)).then_some(fit)
}

impl Fitter {
    fn new(from: ResolvedPos, to: ResolvedPos, unplaced: Slice) -> Self {
        let mut frontier = Vec::new();
        for depth in 0..=from.depth() {
            let node = from.node(depth);
            frontier.push(FrontierEntry {
                ty: node.node_type().clone(),
                matched: node
                    .content_match_at(from.index_after(depth))
                    .unwrap_or_else(|_| node.node_type().content_match()),
            });
        }
        let mut placed = Fragment::empty();
        for depth in (1..=from.depth()).rev() {
            placed = Fragment::from_node(from.node(depth).copy(placed));
        }
        Self {
            from,
            to,
            unplaced,
            frontier,
            placed,
        }
    }

    fn depth(&self) -> usize {
        self.frontier.len() - 1
    }

    fn fit(mut self) -> TransformResult<Option<Step>> {
        while self.unplaced.size() > 0 {
            match self.find_fittable() {
                Some(fit) => self.place_nodes(fit)?,
                None => {
                    if !self.open_more() {
                        self.drop_node();
                    }
                }
            }
        }
        let move_inline = self.must_move_inline()?;
        let placed_size =
            self.placed.size() as isize - self.depth() as isize - self.from.depth() as isize;
        let target = match move_inline {
            Some(pos) => self.from.doc().resolve(pos)?,
            None => self.to.clone(),
        };
        let Some(to) = self.close(target)? else {
            return Ok(None);
        };
        let mut content = self.placed.clone();
        let mut open_start = self.from.depth();
        let mut open_end = to.depth();
        while open_start > 0 && open_end > 0 && content.child_count() == 1 {
            content = content.child(0).content().clone();
            open_start -= 1;
            open_end -= 1;
        }
        let slice = Slice::new(content, open_start, open_end);
        if let Some(move_to) = move_inline {
            return Ok(Some(Step::ReplaceAround(ReplaceAroundStep {
                from: self.from.pos(),
                to: move_to,
                gap_from: self.to.pos(),
                gap_to: self.to.end(self.to.depth()),
                slice,
                insert: placed_size.max(0) as usize,
                structure: false,
            })));
        }
        if slice.size() > 0 || self.from.pos() != self.to.pos() {
            return Ok(Some(Step::Replace(ReplaceStep {
                from: self.from.pos(),
                to: to.pos(),
                slice,
                structure: false,
            })));
        }
        Ok(None)
    }

    fn find_fittable(&self) -> Option<Fittable> {
        let mut start_depth = self.unplaced.open_start;
        let mut cur = self.unplaced.content.clone();
        let mut open_end = self.unplaced.open_end;
        for d in 0..start_depth {
            let node = cur.first_child()?.clone();
            if cur.child_count() > 1 {
                open_end = 0;
            }
            if node.node_type().spec().isolating && open_end <= d {
                start_depth = d;
                break;
            }
            cur = node.content().clone();
        }
        for pass in 1..=2 {
            let top = if pass == 1 {
                start_depth
            } else {
                self.unplaced.open_start
            };
            for slice_depth in (0..=top).rev() {
                let (fragment, parent) = if slice_depth > 0 {
                    let parent = content_at(&self.unplaced.content, slice_depth - 1)
                        .first_child()?
                        .clone();
                    (parent.content().clone(), Some(parent))
                } else {
                    (self.unplaced.content.clone(), None)
                };
                let first = fragment.first_child();
                for frontier_depth in (0..=self.depth()).rev() {
                    let FrontierEntry { ty, matched } = &self.frontier[frontier_depth];
                    if pass == 1 {
                        let found = match first {
                            Some(first) => {
                                if matched.match_type(first.node_type()).is_some() {
                                    Some(None)
                                } else {
                                    matched
                                        .fill_before(&Fragment::from_node(first.clone()), false, 0)
                                        .map(Some)
                                }
                            }
                            None => parent
                                .as_ref()
                                .filter(|parent| ty.compatible_content(parent.node_type()))
                                .map(|_| None),
                        };
                        if let Some(inject) = found {
                            return Some(Fittable {
                                slice_depth,
                                frontier_depth,
                                parent,
                                inject,
                                wrap: None,
                            });
                        }
                    } else if let Some(first) = first
                        && let Some(wrap) = matched.find_wrapping(first.node_type())
                    {
                        return Some(Fittable {
                            slice_depth,
                            frontier_depth,
                            parent,
                            inject: None,
                            wrap: Some(wrap),
                        });
                    }
                    if let Some(parent) = &parent
                        && matched.match_type(parent.node_type()).is_some()
                    {
                        break;
                    }
                }
            }
        }
        None
    }

    fn open_more(&mut self) -> bool {
        let Slice {
            content,
            open_start,
            open_end,
        } = self.unplaced.clone();
        let inner = content_at(&content, open_start);
        match inner.first_child() {
            Some(first) if !first.is_leaf() => {}
            _ => return false,
        }
        let new_end = if inner.size() + open_start >= content.size() - open_end {
            open_start + 1
        } else {
            0
        };
        self.unplaced = Slice::new(content, open_start + 1, open_end.max(new_end));
        true
    }

    fn drop_node(&mut self) {
        let Slice {
            content,
            open_start,
            open_end,
        } = self.unplaced.clone();
        let inner = content_at(&content, open_start);
        if inner.child_count() <= 1 && open_start > 0 {
            let open_at_end = content.size() - open_start <= open_start + inner.size();
            self.unplaced = Slice::new(
                drop_from_fragment(&content, open_start - 1, 1),
                open_start - 1,
                if open_at_end {
                    open_start - 1
                } else {
                    open_end
                },
            );
        } else {
            self.unplaced = Slice::new(
                drop_from_fragment(&content, open_start, 1),
                open_start,
                open_end,
            );
        }
    }

    fn place_nodes(&mut self, fit: Fittable) -> TransformResult {
        let Fittable {
            slice_depth,
            frontier_depth,
            parent,
            inject,
            wrap,
        } = fit;
        while self.depth() > frontier_depth {
            self.close_frontier_node()?;
        }
        if let Some(wrap) = &wrap {
            for ty in wrap {
                self.open_frontier_node(ty, None, Fragment::empty())?;
            }
        }
        let slice = self.unplaced.clone();
        let fragment = parent
            .as_ref()
            .map_or_else(|| slice.content.clone(), |parent| parent.content().clone());
        let open_start = slice.open_start as isize - slice_depth as isize;
        let mut taken = 0;
        let mut add = Vec::new();
        let ty = self.frontier[frontier_depth].ty.clone();
        let mut matched = self.frontier[frontier_depth].matched.clone();
        if let Some(inject) = &inject {
            add.extend(inject.iter().cloned());
            matched = matched
                .match_fragment(inject, 0, inject.child_count())
                .ok_or_else(|| broken("inject"))?;
        }
        let mut open_end_count = (fragment.size() + slice_depth) as isize
            - (slice.content.size() as isize - slice.open_end as isize);
        while taken < fragment.child_count() {
            let next = fragment.child(taken);
            let Some(matches) = matched.match_type(next.node_type()) else {
                break;
            };
            taken += 1;
            if taken > 1 || open_start == 0 || next.content().size() > 0 {
                matched = matches;
                let marked = next.mark(ty.allowed_marks(next.marks()));
                add.push(close_node_start(
                    &marked,
                    if taken == 1 { open_start } else { 0 },
                    if taken == fragment.child_count() {
                        open_end_count
                    } else {
                        -1
                    },
                )?);
            }
        }
        let to_end = taken == fragment.child_count();
        if !to_end {
            open_end_count = -1;
        }
        self.placed = add_to_fragment(&self.placed, frontier_depth, &Fragment::from_array(add));
        self.frontier[frontier_depth].matched = matched;

        if to_end
            && open_end_count < 0
            && parent
                .as_ref()
                .is_some_and(|parent| *parent.node_type() == self.frontier[self.depth()].ty)
            && self.frontier.len() > 1
        {
            self.close_frontier_node()?;
        }

        let mut cur = fragment.clone();
        for _ in 0..open_end_count.max(0) {
            let Some(node) = cur.last_child().cloned() else {
                break;
            };
            self.frontier.push(FrontierEntry {
                ty: node.node_type().clone(),
                matched: node.content_match_at(node.child_count())?,
            });
            cur = node.content().clone();
        }

        self.unplaced = if !to_end {
            Slice::new(
                drop_from_fragment(&slice.content, slice_depth, taken),
                slice.open_start,
                slice.open_end,
            )
        } else if slice_depth == 0 {
            Slice::empty()
        } else {
            Slice::new(
                drop_from_fragment(&slice.content, slice_depth - 1, 1),
                slice_depth - 1,
                if open_end_count < 0 {
                    slice.open_end
                } else {
                    slice_depth - 1
                },
            )
        };
        Ok(())
    }

    /// The position the fit must extend to when inline content after
    /// `to` has to be pulled into the frontier's textblock.
    fn must_move_inline(&self) -> TransformResult<Option<usize>> {
        if !self.to.parent().is_textblock() {
            return Ok(None);
        }
        let top = &self.frontier[self.depth()];
        if !top.ty.is_textblock()
            || content_after_fits(&self.to, self.to.depth(), &top.ty, &top.matched, false).is_none()
        {
            return Ok(None);
        }
        if self.to.depth() == self.depth()
            && let Some(level) = self.find_close_level(&self.to)?
            && level.depth == self.depth()
        {
            return Ok(None);
        }
        let mut depth = self.to.depth();
        let mut after = self.to.after(depth);
        while depth > 1 {
            depth -= 1;
            if after != self.to.end(depth) {
                break;
            }
            after += 1;
        }
        Ok(Some(after))
    }

    fn find_close_level(&self, to: &ResolvedPos) -> TransformResult<Option<CloseLevel>> {
        'scan: for i in (0..=self.depth().min(to.depth())).rev() {
            let FrontierEntry { ty, matched } = &self.frontier[i];
            let drop_inner = i < to.depth() && to.end(i + 1) == to.pos() + (to.depth() - (i + 1));
            let Some(fit) = content_after_fits(to, i, ty, matched, drop_inner) else {
                continue;
            };
            for d in (0..i).rev() {
                let FrontierEntry { ty, matched } = &self.frontier[d];
                match content_after_fits(to, d, ty, matched, true) {
                    Some(matches) if matches.child_count() == 0 => {}
                    _ => continue 'scan,
                }
            }
            let moved = if drop_inner {
                to.doc().resolve(to.after(i + 1))?
            } else {
                to.clone()
            };
            return Ok(Some(CloseLevel {
                depth: i,
                fit,
                moved,
            }));
        }
        Ok(None)
    }

    fn close(&mut self, to: ResolvedPos) -> TransformResult<Option<ResolvedPos>> {
        let Some(close) = self.find_close_level(&to)? else {
            return Ok(None);
        };
        while self.depth() > close.depth {
            self.close_frontier_node()?;
        }
        if close.fit.child_count() > 0 {
            self.placed = add_to_fragment(&self.placed, close.depth, &close.fit);
        }
        let to = close.moved;
        for d in close.depth + 1..=to.depth() {
            let node = to.node(d);
            let add = node
                .node_type()
                .content_match()
                .fill_before(node.content(), true, to.index(d))
                .ok_or_else(|| broken("close fill"))?;
            let attrs = (**node.attrs()).clone();
            self.open_frontier_node(&node.node_type().clone(), Some(&attrs), add)?;
        }
        Ok(Some(to))
    }

    fn open_frontier_node(
        &mut self,
        ty: &NodeType,
        attrs: Option<&Map<String, Value>>,
        content: Fragment,
    ) -> TransformResult {
        let depth = self.depth();
        let top = &mut self.frontier[depth];
        top.matched = top
            .matched
            .match_type(ty)
            .ok_or_else(|| broken("open frontier"))?;
        let node = ty.create(attrs, content, MarkSet::none())?;
        self.placed = add_to_fragment(&self.placed, depth, &Fragment::from_node(node));
        self.frontier.push(FrontierEntry {
            ty: ty.clone(),
            matched: ty.content_match(),
        });
        Ok(())
    }

    fn close_frontier_node(&mut self) -> TransformResult {
        let open = self
            .frontier
            .pop()
            .ok_or_else(|| broken("empty frontier"))?;
        let add = open
            .matched
            .fill_before(&Fragment::empty(), true, 0)
            .ok_or_else(|| broken("close frontier"))?;
        if add.child_count() > 0 {
            self.placed = add_to_fragment(&self.placed, self.frontier.len(), &add);
        }
        Ok(())
    }
}

/// Depths at which `from..to` spans the whole content of the node.
fn covered_depths(from: &ResolvedPos, to: &ResolvedPos) -> Vec<usize> {
    let mut result = Vec::new();
    for d in (0..=from.depth().min(to.depth())).rev() {
        let start = from.start(d);
        if start + (from.depth() - d) < from.pos()
            || to.end(d) > to.pos() + (to.depth() - d)
            || from.node(d).node_type().spec().isolating
            || to.node(d).node_type().spec().isolating
        {
            break;
        }
        if start == to.start(d)
            || (d == from.depth()
                && d == to.depth()
                && from.parent().inline_content()
                && to.parent().inline_content()
                && d > 0
                && to.start(d - 1) + 1 == start)
        {
            result.push(d);
        }
    }
    result
}

pub(crate) fn replace_range(
    tr: &mut Transform,
    from: usize,
    to: usize,
    slice: Slice,
) -> TransformResult {
    if slice.size() == 0 {
        return delete_range(tr, from, to);
    }
    let rfrom = tr.doc.resolve(from)?;
    let rto = tr.doc.resolve(to)?;
    if fits_trivially(&rfrom, &rto, &slice) {
        tr.step(Step::replace(from, to, slice))?;
        return Ok(());
    }

    let mut target_depths: Vec<isize> = covered_depths(&rfrom, &rto)
        .into_iter()
        .map(|d| d as isize)
        .collect();
    if target_depths.last() == Some(&0) {
        target_depths.pop();
    }
    let mut preferred_target = -(rfrom.depth() as isize + 1);
    target_depths.insert(0, preferred_target);
    let mut pos = rfrom.pos();
    for d in (1..=rfrom.depth()).rev() {
        pos -= 1;
        let ty = rfrom.node(d).node_type();
        if ty.spec().defining || ty.spec().defining_as_context || ty.spec().isolating {
            break;
        }
        if target_depths.contains(&(d as isize)) {
            preferred_target = d as isize;
        } else if rfrom.before(d) == pos {
            target_depths.insert(1, -(d as isize));
        }
    }
    let preferred_target_index = target_depths
        .iter()
        .position(|d| *d == preferred_target)
        .unwrap_or(0);

    let mut left_nodes: Vec<Node> = Vec::new();
    let mut preferred_depth = slice.open_start;
    let mut content = slice.content.clone();
    for i in 0.. {
        let Some(node) = content.first_child().cloned() else {
            break;
        };
        left_nodes.push(node.clone());
        if i == slice.open_start {
            break;
        }
        content = node.content().clone();
    }

    let context_depth = preferred_target.unsigned_abs() - 1;
    for d in (0..preferred_depth).rev() {
        let Some(left) = left_nodes.get(d) else {
            continue;
        };
        let defines = left.node_type().is_defining_for_content();
        if defines && !left.same_markup(rfrom.node(context_depth)) {
            preferred_depth = d;
        } else if defines || !left.node_type().is_textblock() {
            break;
        }
    }

    for j in (0..=slice.open_start).rev() {
        let open_depth = (j + preferred_depth + 1) % (slice.open_start + 1);
        let Some(insert) = left_nodes.get(open_depth) else {
            continue;
        };
        for i in 0..target_depths.len() {
            let mut target_depth =
                target_depths[(i + preferred_target_index) % target_depths.len()];
            let mut expand = true;
            if target_depth < 0 {
                expand = false;
                target_depth = -target_depth;
            }
            let target_depth = target_depth as usize;
            let parent = rfrom.node(target_depth - 1);
            let index = rfrom.index(target_depth - 1);
            if parent.can_replace_with(index, index, insert.node_type(), Some(insert.marks())) {
                let end = if expand { rto.after(target_depth) } else { to };
                let closed = close_fragment(&slice.content, 0, slice.open_start, open_depth, None)?;
                tr.replace(
                    rfrom.before(target_depth),
                    end,
                    Slice::new(closed, open_depth, slice.open_end),
                )?;
                return Ok(());
            }
        }
    }

    let start_steps = tr.steps.len();
    let (mut from, mut to) = (from, to);
    for i in (0..target_depths.len()).rev() {
        tr.replace(from, to, slice.clone())?;
        if tr.steps.len() > start_steps {
            break;
        }
        let depth = target_depths[i];
        if depth < 0 {
            continue;
        }
        from = rfrom.before(depth as usize);
        to = rto.after(depth as usize);
    }
    Ok(())
}

fn close_fragment(
    fragment: &Fragment,
    depth: usize,
    old_open: usize,
    new_open: usize,
    parent: Option<&Node>,
) -> TransformResult<Fragment> {
    let mut fragment = fragment.clone();
    if depth < old_open
        && let Some(first) = fragment.first_child().cloned()
    {
        let inner = close_fragment(first.content(), depth + 1, old_open, new_open, Some(&first))?;
        fragment = fragment.replace_child(0, first.copy(inner));
    }
    if depth > new_open
        && let Some(parent) = parent
    {
        let matched = parent.content_match_at(0)?;
        let start = matched
            .fill_before(&fragment, false, 0)
            .ok_or_else(|| broken("close_fragment start"))?
            .append(&fragment);
        let end = matched
            .match_fragment(&start, 0, start.child_count())
            .and_then(|m| m.fill_before(&Fragment::empty(), true, 0))
            .ok_or_else(|| broken("close_fragment end"))?;
        fragment = start.append(&end);
    }
    Ok(fragment)
}

pub(crate) fn replace_range_with(
    tr: &mut Transform,
    from: usize,
    to: usize,
    node: Node,
) -> TransformResult {
    let (mut from, mut to) = (from, to);
    if !node.is_inline()
        && from == to
        && tr.doc.resolve(from)?.parent().content().size() > 0
        && let Some(point) = insert_point(&tr.doc, from, node.node_type())
    {
        from = point;
        to = point;
    }
    replace_range(tr, from, to, Slice::new(Fragment::from_node(node), 0, 0))
}

pub(crate) fn delete_range(tr: &mut Transform, from: usize, to: usize) -> TransformResult {
    let (mut from, mut to) = (from, to);
    let mut rfrom = tr.doc.resolve(from)?;
    let mut rto = tr.doc.resolve(to)?;

    if rfrom.parent().is_textblock()
        && rto.parent().is_textblock()
        && rfrom.start(rfrom.depth()) != rto.start(rto.depth())
        && rfrom.parent_offset() == 0
        && rto.parent_offset() == 0
    {
        let shared = rfrom.shared_depth(to);
        let isolated = (shared + 1..=rfrom.depth())
            .any(|d| rfrom.node(d).node_type().spec().isolating)
            || (shared + 1..=rto.depth()).any(|d| rto.node(d).node_type().spec().isolating);
        if !isolated {
            let mut d = rfrom.depth();
            while d > 0 && from == rfrom.start(d) {
                from = rfrom.before(d);
                d -= 1;
            }
            let mut d = rto.depth();
            while d > 0 && to == rto.start(d) {
                to = rto.before(d);
                d -= 1;
            }
            rfrom = tr.doc.resolve(from)?;
            rto = tr.doc.resolve(to)?;
        }
    }

    let covered = covered_depths(&rfrom, &rto);
    for (i, depth) in covered.iter().copied().enumerate() {
        let last = i == covered.len() - 1;
        if (last && depth == 0) || rfrom.node(depth).node_type().content_match().valid_end() {
            tr.delete(rfrom.start(depth), rto.end(depth))?;
            return Ok(());
        }
        if depth > 0
            && (last
                || rfrom.node(depth - 1).can_replace(
                    rfrom.index(depth - 1),
                    rto.index_after(depth - 1),
                    &Fragment::empty(),
                    0,
                    0,
                ))
        {
            tr.delete(rfrom.before(depth), rto.after(depth))?;
            return Ok(());
        }
    }
    for d in 1..=rfrom.depth().min(rto.depth()) {
        if from - rfrom.start(d) == rfrom.depth() - d
            && to > rfrom.end(d)
            && rto.end(d) - to != rto.depth() - d
            && rfrom.start(d - 1) == rto.start(d - 1)
            && rfrom.node(d - 1).can_replace(
                rfrom.index(d - 1),
                rto.index(d - 1),
                &Fragment::empty(),
                0,
                0,
            )
        {
            tr.delete(rfrom.before(d), to)?;
            return Ok(());
        }
    }
    tr.delete(from, to)?;
    Ok(())
}
