//! The drag handle: a grip in the left margin beside the block under the
//! mouse (or, on touch screens, the block with the caret), as in Notion
//! and Tiptap's DragHandle. Clicking it selects the block; dragging it
//! moves the block, with a line showing where it will land and the view
//! scrolling near its edges.
//!
//! Blocks are the document's top-level blocks: a list moves as a whole.

use accesskit::{Action, NodeId, Role};
use rs_editor_layout::{Rect, TopBlock};
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

/// A block being dragged by its grip.
#[derive(Debug, Clone, Copy)]
struct BlockDrag {
    /// The block: position before it and its size.
    pos: usize,
    size: usize,
    start: (f32, f32),
    pointer: (f32, f32),
    moved: bool,
    /// Where the block would land: a position between top-level blocks.
    drop: Option<usize>,
}

#[derive(Debug, Default)]
pub(crate) struct BlockHandle {
    pub enabled: bool,
    appear: Appear,
    /// The block the grip belongs to (position before it).
    block: Option<usize>,
    /// The grip, in view coordinates.
    grip: skia_safe::Rect,
    /// The mouse over the view, for the block under it.
    pointer: Option<(f32, f32)>,
    /// The mouse is over the grip.
    hovered: bool,
    drag: Option<BlockDrag>,
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

    /// The mouse left the view.
    pub fn pointer_leave(&mut self) {
        if self.blocks.pointer.take().is_some() {
            self.blocks.hovered = false;
            self.request_redraw();
        }
    }

    /// The top-level block the grip should show for, if any.
    fn handle_block(&mut self) -> Option<TopBlock> {
        let layout = self.layout();
        if let Some(drag) = self.blocks.drag {
            return layout.tops.get(layout.top_at_pos(drag.pos)?);
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
            return layout.tops.get(layout.top_at_pos(head)?);
        }
        let (x, y) = self.blocks.pointer?;
        if x < 0.0 || x > self.width || y < 0.0 || y > self.visible_height() {
            return None;
        }
        let (_, doc_y) = self.to_document(x, y);
        let block = layout.tops.get(layout.top_at_y(doc_y)?)?;
        // Below the last block is not beside it.
        (doc_y <= block.bottom() + self.theme().spacing.block_gap).then_some(block)
    }

    /// Where the grip goes for `block`: in the left margin, centered on the
    /// block's first line (view coordinates).
    fn grip_rect(&mut self, block: &TopBlock) -> skia_safe::Rect {
        let layout = self.layout();
        let first_line = layout
            .texts
            .iter()
            .find(|t| t.pos > block.pos && t.pos < block.end())
            .and_then(|t| {
                let line = t.layout.lines().first()?;
                Some(t.rect.y + line.top + line.height / 2.0)
            });
        let center = first_line.unwrap_or(block.y + block.height.min(GRIP_HEIGHT) / 2.0);
        let center = self.to_view(Rect::new(0.0, center, 0.0, 0.0)).y;
        let x = (self.padding.left - GRIP_WIDTH - 2.0).max(0.0);
        skia_safe::Rect::from_xywh(x, center - GRIP_HEIGHT / 2.0, GRIP_WIDTH, GRIP_HEIGHT)
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
        self.blocks.block = block.map(|b| b.pos);
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
    }

    /// Whether `(x, y)` is on the grip, reaching `slack` further up, down
    /// and left (never into the text, where taps place the caret).
    fn on_grip(&self, x: f32, y: f32, slack: f32) -> bool {
        let grip = self.blocks.grip;
        let area = skia_safe::Rect::from_ltrb(
            grip.left - slack,
            grip.top - slack,
            (grip.right + slack).min(self.padding.left).max(grip.right),
            grip.bottom + slack,
        );
        self.blocks.appear.target() && area.contains(skia_safe::Point::new(x, y))
    }

    /// Starts a press on the grip: the block is selected, and moving makes
    /// it a drag.
    fn start_block_drag(&mut self, x: f32, y: f32) -> bool {
        let Some(pos) = self.blocks.block else {
            return false;
        };
        let doc = self.editor.doc().clone();
        let Some(node) = doc.node_at(pos) else {
            return false;
        };
        self.finish_composition();
        self.set_focused(true);
        if let Ok(selection) = NodeSelection::create(&doc, pos) {
            let mut tr = self.editor.state().tr();
            tr.set_selection(selection);
            self.dispatch(tr);
        }
        self.blocks.drag = Some(BlockDrag {
            pos,
            size: node.node_size(),
            start: (x, y),
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
            drag.drop = self.drop_target(&drag, y);
        }
        self.blocks.drag = Some(drag);
        self.request_redraw();
    }

    /// The boundary between top-level blocks nearest to view `y`, unless
    /// dropping there would leave the block where it is.
    fn drop_target(&mut self, drag: &BlockDrag, y: f32) -> Option<usize> {
        let (_, doc_y) = self.to_document(0.0, y);
        let layout = self.layout();
        let tops: Vec<TopBlock> = layout.tops.iter().collect();
        let index = tops
            .iter()
            .take_while(|b| b.y + b.height / 2.0 < doc_y)
            .count();
        let boundary = match tops.get(index) {
            Some(block) => block.pos,
            None => tops.last()?.end(),
        };
        (boundary != drag.pos && boundary != drag.pos + drag.size).then_some(boundary)
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
            self.move_block(drag.pos, drag.size, drop);
        }
        self.request_redraw();
    }

    /// Moves the top-level block at `pos` (`size` long) to boundary `to`,
    /// as one undo step, and selects it there.
    fn move_block(&mut self, pos: usize, size: usize, to: usize) -> bool {
        let doc = self.editor.doc().clone();
        let Some(node) = doc.node_at(pos) else {
            return false;
        };
        let mut tr = self.editor.state().tr();
        let target = if to > pos { to - size } else { to };
        if tr.delete(pos, pos + size).is_err()
            || tr.insert(target, Fragment::from_node(node)).is_err()
        {
            return false;
        }
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

    /// Paints the grip and, during a drag, the dimmed block and the line
    /// where it will land (view coordinates).
    pub(crate) fn paint_block_handle(&mut self, canvas: &Canvas) {
        let theme = self.theme().clone();
        let palette = &theme.palette;
        if let Some(drag) = self.blocks.drag.filter(|d| d.moved) {
            let layout = self.layout();
            let left = self.padding.left;
            let right = self.width - self.padding.right;
            if let Some(block) = layout.top_at_pos(drag.pos).and_then(|i| layout.tops.get(i)) {
                let rect = self.to_view(Rect::new(0.0, block.y, 0.0, block.height));
                let mut veil = Paint::default();
                veil.set_color(skia_safe::Color::new(
                    palette.background.with_alpha(0.6).argb(),
                ));
                canvas.draw_rect(
                    skia_safe::Rect::from_ltrb(
                        left - 4.0,
                        rect.y - 2.0,
                        right + 4.0,
                        rect.bottom() + 2.0,
                    ),
                    &veil,
                );
            }
            if let Some(drop) = drag.drop {
                let gap = theme.spacing.block_gap / 2.0;
                let y = match layout.top_at_pos(drop).and_then(|i| layout.tops.get(i)) {
                    Some(block) if block.pos == drop => block.y - gap,
                    _ => layout.tops.iter().last().map_or(0.0, |b| b.bottom() + gap),
                };
                let y = self.to_view(Rect::new(0.0, y, 0.0, 0.0)).y;
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

#[cfg(test)]
impl BlockHandle {
    pub(crate) fn grip_center(&self) -> (f32, f32) {
        (self.grip.center_x(), self.grip.center_y())
    }

    pub(crate) fn block(&self) -> Option<usize> {
        self.block
    }
}
