//! The drag handle: a grip in the left margin beside the block under the
//! mouse (or, on touch screens, the block with the caret), as in Notion
//! and Tiptap's DragHandle. Clicking it selects the block; dragging it
//! moves the block, with a line showing where it will land and the view
//! scrolling near its edges.
//!
//! A block is a list item when the pointer (or caret) is in a list, else
//! a top-level block. List items land between items of lists that take
//! them, at any depth (moving the pointer right picks the deeper of two
//! lists ending together), or between top-level blocks as a list of their
//! own.

use accesskit::{Action, NodeId, Role};
use rs_editor_layout::{DocumentLayout, Rect};
use rs_editor_model::Fragment;
use rs_editor_state::{NodeSelection, Selection, TextSelection};
use rs_editor_ui::{Appear, Icon};
use skia_safe::{Canvas, Contains, Paint, RRect};

use crate::a11y::{OverlayNodes, overlay_ids, overlay_node};
use crate::{CursorIcon, EditorView, Effect, HapticKind, TouchPhase};

const GRIP_WIDTH: f32 = 20.0;
const GRIP_HEIGHT: f32 = 24.0;
const ICON_SIZE: f32 = 16.0;
/// How far the pointer moves before a press on the grip becomes a drag.
const DRAG_SLOP: f32 = 4.0;
/// Extra reach of the grip for a finger.
const TOUCH_SLACK: f32 = 10.0;
/// Pointers this close to the top or bottom edge scroll the view.
const EDGE: f32 = 40.0;

/// What a grip moves: a top-level block or a list item.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Unit {
    /// Position before the node and its size.
    pos: usize,
    size: usize,
    /// Its box, in document coordinates.
    rect: Rect,
    item: bool,
}

impl Unit {
    fn end(&self) -> usize {
        self.pos + self.size
    }
}

/// Where a dragged block would land.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drop {
    /// A position between siblings.
    pos: usize,
    /// The line showing it (document coordinates).
    y: f32,
    left: f32,
    /// Between top-level blocks: a list item lands in a list of its own.
    top_level: bool,
}

/// A block being dragged by its grip.
#[derive(Debug, Clone, Copy)]
struct BlockDrag {
    unit: Unit,
    start: (f32, f32),
    /// The scroll offset when the drag started (the ghost follows the
    /// pointer across scrolling).
    start_scroll: f32,
    pointer: (f32, f32),
    moved: bool,
    drop: Option<Drop>,
}

/// The drop line as shown: it slides to each new place.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Line {
    y: f32,
    left: f32,
    at_ms: u64,
}

/// How quickly the drop line catches up (milliseconds to cover ~63%).
const LINE_EASE_MS: f32 = 60.0;
/// A new drop place must be this much nearer than the current one to
/// take over, so the line does not flicker between two near places.
const HYSTERESIS: f32 = 6.0;

#[derive(Debug, Default)]
pub(crate) struct BlockHandle {
    pub enabled: bool,
    appear: Appear,
    /// The block the grip belongs to.
    block: Option<Unit>,
    /// The grip, in view coordinates.
    grip: skia_safe::Rect,
    /// The mouse over the view, for the block under it.
    pointer: Option<(f32, f32)>,
    /// The mouse is over the grip.
    hovered: bool,
    drag: Option<BlockDrag>,
    line: Option<Line>,
}

impl BlockHandle {
    pub(crate) fn new() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

impl EditorView {
    /// Shows a grip beside blocks to select and move them (on by default).
    pub fn set_drag_handle(&mut self, enabled: bool) {
        self.blocks.enabled = enabled;
        if !enabled {
            self.blocks.appear.jump(false);
            self.blocks.drag = None;
        }
        self.request_redraw();
    }

    /// Whether the drag handle is (becoming) visible.
    pub fn drag_handle_shown(&self) -> bool {
        self.blocks.appear.target()
    }

    /// Where the grip is while it shows (view coordinates), for platforms
    /// that must keep their own edge gestures (Android's Back) off it.
    pub fn drag_handle_rect(&self) -> Option<Rect> {
        let grip = self.blocks.grip;
        self.blocks.appear.target().then(|| {
            Rect::new(
                grip.left - TOUCH_SLACK,
                grip.top - TOUCH_SLACK,
                grip.width() + TOUCH_SLACK * 2.0,
                grip.height() + TOUCH_SLACK * 2.0,
            )
        })
    }

    /// Whether a block is being dragged.
    pub fn is_dragging_block(&self) -> bool {
        self.blocks.drag.is_some_and(|d| d.moved)
    }

    /// No mouse is over the view (a finger is in use).
    pub(crate) fn forget_pointer(&mut self) {
        self.blocks.pointer = None;
        self.blocks.hovered = false;
    }

    /// The mouse left the view.
    pub fn pointer_leave(&mut self) {
        if self.blocks.pointer.take().is_some() {
            self.blocks.hovered = false;
            self.request_redraw();
        }
    }

    /// The block the grip should show for, if any.
    fn handle_block(&mut self) -> Option<Unit> {
        let layout = self.layout();
        if let Some(drag) = self.blocks.drag {
            return Some(drag.unit);
        }
        if !self.blocks.enabled
            || !self.editor.is_editable()
            || self.drag.is_some()
            || self.composition.is_some()
            || self.link_editing()
            || self.slash_menu_shown()
        {
            return None;
        }
        if self.touch.active {
            if !self.focused || self.touch_gesture_active() {
                return None;
            }
            let head = self.editor.selection().head();
            let item = innermost_item(&layout, |b| {
                b.pos < head && head < b.pos + b.node.node_size()
            });
            return item.or_else(|| top_unit(&layout, layout.top_at_pos(head)?));
        }
        let (x, y) = self.blocks.pointer?;
        if x < 0.0 || x > self.width || y < 0.0 || y > self.visible_height() {
            return None;
        }
        let (_, doc_y) = self.to_document(x, y);
        let item = innermost_item(&layout, |b| b.rect.y <= doc_y && doc_y < b.rect.bottom());
        if item.is_some() {
            return item;
        }
        let block = top_unit(&layout, layout.top_at_y(doc_y)?)?;
        // Below the last block is not beside it.
        (doc_y <= block.rect.bottom() + self.theme().spacing.block_gap).then_some(block)
    }

    /// Where the grip goes for `block`: left of it (of a list item's
    /// marker), centered on its first line (view coordinates).
    fn grip_rect(&mut self, block: &Unit) -> skia_safe::Rect {
        let layout = self.layout();
        let first_line = layout
            .texts
            .iter()
            .find(|t| t.pos > block.pos && t.pos < block.end())
            .and_then(|t| {
                let line = t.layout.lines().first()?;
                Some(t.rect.y + line.top + line.height / 2.0)
            });
        let center = first_line.unwrap_or(block.rect.y + block.rect.height.min(GRIP_HEIGHT) / 2.0);
        let at = self.to_view(Rect::new(block.rect.x, center, 0.0, 0.0));
        let x = (at.x - GRIP_WIDTH - 2.0).max(0.0);
        skia_safe::Rect::from_xywh(x, at.y - GRIP_HEIGHT / 2.0, GRIP_WIDTH, GRIP_HEIGHT)
    }

    /// Brings the grip in line with the pointer, the caret and a drag.
    pub(crate) fn update_block_handle(&mut self) {
        self.tick_block_drag();
        let block = self.handle_block();
        let wanted = block.is_some_and(|b| {
            let grip = self.grip_rect(&b);
            self.blocks.grip = grip;
            grip.bottom() > 0.0 && grip.top < self.visible_height()
        });
        self.blocks.block = block;
        if !wanted {
            self.blocks.hovered = false;
        }
        if wanted != self.blocks.appear.target() {
            self.blocks.appear.set(wanted, self.clock.now_ms());
            self.request_redraw();
        }
    }

    pub(crate) fn block_handle_animating(&self) -> bool {
        self.blocks.appear.animating(self.clock.now_ms())
            || self
                .blocks
                .drag
                .is_some_and(|d| d.moved && self.edge_speed(d.pointer.1) != 0.0)
            || self.drop_line_moving()
    }

    /// Whether `(x, y)` is on the grip, reaching `slack` further up, down
    /// and left (barely right: the text starts there, and taps on it place
    /// the caret).
    fn on_grip(&self, x: f32, y: f32, slack: f32) -> bool {
        let grip = self.blocks.grip;
        let area = skia_safe::Rect::from_ltrb(
            grip.left - slack,
            grip.top - slack,
            grip.right + slack.min(2.0),
            grip.bottom + slack,
        );
        self.blocks.appear.target() && area.contains(skia_safe::Point::new(x, y))
    }

    /// Starts a press on the grip: the block is selected, and moving makes
    /// it a drag.
    fn start_block_drag(&mut self, x: f32, y: f32) -> bool {
        let Some(unit) = self.blocks.block else {
            return false;
        };
        let doc = self.editor.doc().clone();
        self.finish_composition();
        self.set_focused(true);
        if let Ok(selection) = NodeSelection::create(&doc, unit.pos) {
            let mut tr = self.editor.state().tr();
            tr.set_selection(selection);
            self.dispatch(tr);
        }
        self.blocks.drag = Some(BlockDrag {
            unit,
            start: (x, y),
            start_scroll: self.scroll_y,
            pointer: (x, y),
            moved: false,
            drop: None,
        });
        self.request_redraw();
        true
    }

    /// The pointer moved during a press on the grip.
    fn move_block_drag(&mut self, x: f32, y: f32) {
        let Some(mut drag) = self.blocks.drag else {
            return;
        };
        drag.pointer = (x, y);
        drag.moved |= (x - drag.start.0).hypot(y - drag.start.1) > DRAG_SLOP;
        if drag.moved {
            drag.drop = self.drop_target(&drag.unit, x, y, drag.drop);
        }
        self.blocks.drag = Some(drag);
        self.request_redraw();
    }

    /// The place nearest to the pointer at view `(x, y)` where `unit` can
    /// land, unless that would leave it where it is.
    fn drop_target(&mut self, unit: &Unit, x: f32, y: f32, current: Option<Drop>) -> Option<Drop> {
        let (doc_x, doc_y) = self.to_document(x, y);
        let layout = self.layout();
        let doc = self.editor.doc().clone();
        let spacing = self.theme().spacing;
        let mut candidates: Vec<Drop> = Vec::new();
        // Between top-level blocks.
        let tops: Vec<_> = layout.tops.iter().collect();
        for (index, block) in tops.iter().enumerate() {
            // Midway between the blocks (headings keep more space).
            let y = match index.checked_sub(1).and_then(|i| tops.get(i)) {
                Some(previous) => (previous.bottom() + block.y) / 2.0,
                None => block.y,
            };
            candidates.push(Drop {
                pos: block.pos,
                y,
                left: 0.0,
                top_level: true,
            });
        }
        if let Some(last) = tops.last() {
            candidates.push(Drop {
                pos: last.end(),
                y: last.bottom() + spacing.block_gap / 2.0,
                left: 0.0,
                top_level: true,
            });
        }
        // Between the items of lists that take this item.
        if unit.item {
            let kind = doc.node_at(unit.pos).map(|n| n.type_name().to_owned());
            for item in layout
                .blocks
                .iter()
                .filter(|b| Some(b.node.type_name()) == kind.as_deref())
            {
                let gap = spacing.list_item_gap / 2.0;
                candidates.push(Drop {
                    pos: item.pos,
                    y: item.rect.y - gap,
                    left: item.rect.x,
                    top_level: false,
                });
                let end = item.pos + item.node.node_size();
                let last = doc.resolve(end).is_ok_and(|at| at.end(at.depth()) == end);
                if last {
                    candidates.push(Drop {
                        pos: end,
                        y: item.rect.bottom() + gap,
                        left: item.rect.x,
                        top_level: false,
                    });
                }
            }
        }
        candidates.retain(|c| c.pos < unit.pos || c.pos > unit.end());
        let nearest = candidates
            .iter()
            .map(|c| (c.y - doc_y).abs())
            .fold(f32::INFINITY, f32::min);
        // Of places about as near, the deepest the pointer reaches; where
        // a list and the top level meet (a list's first or last item), the
        // list keeps its item.
        let close = candidates
            .iter()
            .filter(|c| (c.y - doc_y).abs() <= nearest + 6.0);
        let reached = close
            .clone()
            .filter(|c| c.left <= doc_x + 8.0)
            .max_by(|a, b| {
                a.left
                    .total_cmp(&b.left)
                    .then(b.top_level.cmp(&a.top_level))
            });
        let best = reached
            .or_else(|| {
                close.min_by(|a, b| {
                    a.left
                        .total_cmp(&b.left)
                        .then(a.top_level.cmp(&b.top_level))
                })
            })
            .copied()?;
        // The place shown stays until another is clearly nearer (a place
        // at the same height, a different depth, takes over at once).
        let kept = current.and_then(|current| {
            candidates
                .iter()
                .find(|c| c.pos == current.pos && c.top_level == current.top_level)
                .filter(|c| {
                    (c.y - best.y).abs() > 1.0
                        && (c.y - doc_y).abs() <= (best.y - doc_y).abs() + HYSTERESIS
                })
                .copied()
        });
        Some(kept.unwrap_or(best))
    }

    /// Scroll speed (pixels per frame) for a pointer at view `y` during a
    /// drag: faster the closer it is to an edge.
    fn edge_speed(&self, y: f32) -> f32 {
        let visible = self.visible_height();
        if y < EDGE {
            -((EDGE - y) / EDGE).min(1.5) * 12.0
        } else if y > visible - EDGE {
            ((y - (visible - EDGE)) / EDGE).min(1.5) * 12.0
        } else {
            0.0
        }
    }

    /// Scrolls while a drag holds the pointer near an edge.
    fn tick_block_drag(&mut self) {
        let Some(drag) = self.blocks.drag.filter(|d| d.moved) else {
            return;
        };
        let speed = self.edge_speed(drag.pointer.1);
        if speed != 0.0 {
            let before = self.scroll_y;
            self.scroll_to_inner(self.scroll_y + speed);
            if self.scroll_y != before {
                self.move_block_drag(drag.pointer.0, drag.pointer.1);
            }
        }
    }

    /// The press on the grip ended: a drag drops the block, a click keeps
    /// it selected.
    fn end_block_drag(&mut self) {
        let Some(drag) = self.blocks.drag.take() else {
            return;
        };
        if let (true, Some(drop)) = (drag.moved, drag.drop) {
            self.move_block(&drag.unit, &drop);
        }
        self.request_redraw();
    }

    /// Moves `unit` to `drop` as one undo step and selects it there. A
    /// list item dropped between top-level blocks goes in a list like the
    /// one it left; a list left empty goes away.
    fn move_block(&mut self, unit: &Unit, drop: &Drop) -> bool {
        let doc = self.editor.doc().clone();
        let Some(node) = doc.node_at(unit.pos) else {
            return false;
        };
        let Ok(at) = doc.resolve(unit.pos) else {
            return false;
        };
        let parent = at.parent().clone();
        // The removed range: the item, or its list when it was the only one.
        let (from, to) = if unit.item && parent.child_count() == 1 {
            let list = at.before(at.depth());
            (list, list + parent.node_size())
        } else {
            (unit.pos, unit.end())
        };
        let content = if unit.item && drop.top_level {
            match parent.node_type().create(
                Some(parent.attrs()),
                Fragment::from_node(node),
                Default::default(),
            ) {
                Ok(list) => list,
                Err(_) => return false,
            }
        } else {
            node
        };
        let mut tr = self.editor.state().tr();
        let target = if drop.pos > from {
            drop.pos - (to - from)
        } else {
            drop.pos
        };
        if tr.delete(from, to).is_err() || tr.insert(target, Fragment::from_node(content)).is_err()
        {
            return false;
        }
        // Select the moved block itself, inside a list made for it.
        let target = if unit.item && drop.top_level {
            target + 1
        } else {
            target
        };
        let selection = match NodeSelection::create(&tr.doc, target) {
            Ok(selection) => selection,
            Err(_) => match tr.doc.resolve(target + 1) {
                Ok(inside) => Selection::Text(TextSelection::new(inside.clone(), inside)),
                Err(_) => return false,
            },
        };
        tr.set_selection(selection);
        self.dispatch(tr)
    }

    /// The grip as a button for screen readers: activating it selects the
    /// block.
    pub(crate) fn grip_a11y(&self, out: &mut OverlayNodes) {
        if !self.blocks.appear.target() || self.blocks.drag.is_some() {
            return;
        }
        let id = NodeId(overlay_ids::GRIP);
        let mut node = overlay_node(Role::Button, self.blocks.grip, self.strings.move_block);
        node.add_action(Action::Click);
        out.nodes.push((id, node));
        out.roots.push(id);
    }

    /// A screen reader activated the grip: the block is selected.
    pub(crate) fn grip_click(&mut self) -> bool {
        let grip = self.blocks.grip;
        if !self.blocks.appear.target() || !self.start_block_drag(grip.center_x(), grip.center_y())
        {
            return false;
        }
        self.end_block_drag();
        true
    }

    /// Escape during a drag puts the block back.
    pub(crate) fn cancel_block_drag(&mut self) -> bool {
        if self.blocks.drag.take().is_some_and(|d| d.moved) {
            self.request_redraw();
            return true;
        }
        false
    }

    /// A press on the grip; true when it took the press.
    pub(crate) fn block_pointer_down(&mut self, x: f32, y: f32) -> bool {
        self.blocks.pointer = Some((x, y));
        self.on_grip(x, y, 0.0) && self.start_block_drag(x, y)
    }

    /// Hovering (which block shows the grip) and dragging; `Some` when the
    /// move belongs to the grip.
    pub(crate) fn block_pointer_move(&mut self, x: f32, y: f32) -> Option<bool> {
        if self.blocks.drag.is_some() {
            self.move_block_drag(x, y);
            self.push_effect(Effect::Cursor(CursorIcon::Grabbing));
            return Some(true);
        }
        let before = (self.blocks.block, self.blocks.hovered);
        self.blocks.pointer = Some((x, y));
        self.update_block_handle();
        self.blocks.hovered = self.on_grip(x, y, 0.0);
        if before != (self.blocks.block, self.blocks.hovered) {
            self.request_redraw();
        }
        if !self.blocks.hovered {
            // The text under the pointer decides the cursor.
            return None;
        }
        self.push_effect(Effect::Cursor(CursorIcon::Grab));
        Some(true)
    }

    pub(crate) fn block_pointer_up(&mut self) -> bool {
        if self.blocks.drag.is_none() {
            return false;
        }
        self.end_block_drag();
        true
    }

    /// A finger on the grip; true when the grip took the touch.
    pub(crate) fn block_touch(&mut self, phase: TouchPhase, x: f32, y: f32) -> bool {
        match phase {
            TouchPhase::Down => {
                if !self.on_grip(x, y, TOUCH_SLACK) || !self.start_block_drag(x, y) {
                    return false;
                }
                self.hide_menu();
                self.push_effect(Effect::Haptic(HapticKind::LongPress));
                true
            }
            TouchPhase::Move => {
                if self.blocks.drag.is_none() {
                    return false;
                }
                self.move_block_drag(x, y);
                true
            }
            TouchPhase::Up => {
                if self.blocks.drag.is_none() {
                    return false;
                }
                self.end_block_drag();
                true
            }
            TouchPhase::Cancel => {
                let had = self.blocks.drag.take().is_some();
                if had {
                    self.request_redraw();
                }
                had
            }
        }
    }

    /// Moves the drop line shown toward `target` (it slides rather than
    /// jumps between places).
    fn ease_drop_line(&mut self, target: Option<Drop>) {
        let now = self.clock.now_ms();
        self.blocks.line = target.map(|target| match self.blocks.line {
            Some(line) => {
                let elapsed = now.saturating_sub(line.at_ms) as f32;
                let step = 1.0 - (-elapsed / LINE_EASE_MS).exp();
                let ease = |from: f32, to: f32| {
                    let next = from + (to - from) * step;
                    if (to - next).abs() < 0.5 { to } else { next }
                };
                Line {
                    y: ease(line.y, target.y),
                    left: ease(line.left, target.left),
                    at_ms: now,
                }
            }
            None => Line {
                y: target.y,
                left: target.left,
                at_ms: now,
            },
        });
    }

    /// The drop line is still sliding to its place.
    fn drop_line_moving(&self) -> bool {
        match (self.blocks.line, self.blocks.drag.and_then(|d| d.drop)) {
            (Some(line), Some(drop)) => line.y != drop.y || line.left != drop.left,
            _ => false,
        }
    }

    /// A lifted copy of the dragged block under the pointer, on a card
    /// with a shadow: what moves.
    fn paint_block_ghost(
        &mut self,
        canvas: &Canvas,
        drag: &BlockDrag,
        theme: &rs_editor_theme::Theme,
    ) {
        let layout = self.layout();
        let source = drag.unit.rect;
        let dy = (drag.pointer.1 - drag.start.1) + (self.scroll_y - drag.start_scroll);
        let right = self.content_width();
        let area = skia_safe::Rect::from_ltrb(
            source.x - 6.0,
            source.y - 4.0,
            right + 6.0,
            source.bottom() + 4.0,
        );
        canvas.save();
        canvas.translate((self.padding.left, self.padding.top - self.scroll_y + dy));
        rs_editor_ui::paint_panel(canvas, area, theme, 1.0);
        canvas.clip_rect(area.with_inset((1.0, 1.0)), None, true);
        rs_editor_layout::render::paint_document(canvas, &layout, theme, source);
        canvas.restore();
    }

    /// Paints the grip and, during a drag, the dimmed block and the line
    /// where it will land (view coordinates).
    pub(crate) fn paint_block_handle(&mut self, canvas: &Canvas) {
        let theme = self.theme().clone();
        let palette = &theme.palette;
        let drag = self.blocks.drag.filter(|d| d.moved);
        self.ease_drop_line(drag.and_then(|d| d.drop));
        if let Some(drag) = drag {
            let right = self.width - self.padding.right;
            let rect = self.to_view(drag.unit.rect);
            let mut veil = Paint::default();
            veil.set_color(skia_safe::Color::new(
                palette.background.with_alpha(0.6).argb(),
            ));
            canvas.draw_rect(
                skia_safe::Rect::from_ltrb(
                    rect.x - 4.0,
                    rect.y - 2.0,
                    right + 4.0,
                    rect.bottom() + 2.0,
                ),
                &veil,
            );
            // The ghost under the pointer, then the line over it.
            self.paint_block_ghost(canvas, &drag, &theme);
            if let Some(line) = self.blocks.line {
                let at = self.to_view(Rect::new(line.left, line.y, 0.0, 0.0));
                let (left, y) = (at.x, at.y);
                let mut line = Paint::default();
                line.set_anti_alias(true);
                line.set_color(skia_safe::Color::new(palette.primary.argb()));
                canvas.draw_rrect(
                    RRect::new_rect_xy(
                        skia_safe::Rect::from_ltrb(left, y - 1.0, right, y + 1.0),
                        1.0,
                        1.0,
                    ),
                    &line,
                );
                canvas.draw_circle((left, y), 3.0, &line);
            }
        }
        let progress = self.blocks.appear.value(self.clock.now_ms());
        if progress <= 0.0 {
            return;
        }
        let grip = self.blocks.grip;
        let active = self.blocks.hovered || self.blocks.drag.is_some();
        if active {
            let mut fill = Paint::default();
            fill.set_anti_alias(true);
            fill.set_color(skia_safe::Color::new(
                palette.accent.with_alpha(progress).argb(),
            ));
            canvas.draw_rrect(
                RRect::new_rect_xy(grip, theme.radius.sm, theme.radius.sm),
                &fill,
            );
        }
        let color = if active {
            palette.accent_foreground
        } else {
            palette.muted_foreground
        };
        Icon::GripVertical.draw(
            canvas,
            grip.center_x() - ICON_SIZE / 2.0,
            grip.center_y() - ICON_SIZE / 2.0,
            ICON_SIZE,
            skia_safe::Color::new(color.with_alpha(progress).argb()),
        );
    }
}

/// The deepest list item whose box `pick` accepts.
fn innermost_item(
    layout: &DocumentLayout,
    pick: impl Fn(&rs_editor_layout::BlockBox) -> bool,
) -> Option<Unit> {
    layout
        .blocks
        .iter()
        .filter(|b| matches!(b.node.type_name(), "listItem" | "taskItem") && pick(b))
        .max_by_key(|b| b.depth)
        .map(|b| Unit {
            pos: b.pos,
            size: b.node.node_size(),
            rect: b.rect,
            item: true,
        })
}

/// The top-level block at index `index` of the layout's tops.
fn top_unit(layout: &DocumentLayout, index: usize) -> Option<Unit> {
    let block = layout.tops.get(index)?;
    Some(Unit {
        pos: block.pos,
        size: block.size,
        rect: Rect::new(0.0, block.y, layout.width, block.height),
        item: false,
    })
}

#[cfg(test)]
impl BlockHandle {
    pub(crate) fn grip_center(&self) -> (f32, f32) {
        (self.grip.center_x(), self.grip.center_y())
    }

    pub(crate) fn block(&self) -> Option<usize> {
        self.block.map(|b| b.pos)
    }
}

#[cfg(test)]
impl EditorView {
    /// The drop line as shown and where it is going (document y).
    pub(crate) fn drop_line(&self) -> Option<(f32, f32)> {
        let line = self.blocks.line?;
        let drop = self.blocks.drag?.drop?;
        Some((line.y, drop.y))
    }
}
