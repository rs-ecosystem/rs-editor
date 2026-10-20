//! Mouse and pen input: click, shift-click, double/triple click, drag
//! selection, checkboxes and links.

use rs_editor_extensions::starter_kit::toggle_task_item;
use rs_editor_layout::{Affinity, Decoration, Hit};
use rs_editor_state::Selection;

use crate::{CursorIcon, EditorView, Effect, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

/// A pointer event in view coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerEvent {
    pub x: f32,
    pub y: f32,
    pub button: PointerButton,
    pub modifiers: Modifiers,
    /// 1 for a click, 2 for a double click, 3 for a triple click.
    pub click_count: u32,
}

impl PointerEvent {
    pub fn at(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            button: PointerButton::Primary,
            modifiers: Modifiers::default(),
            click_count: 1,
        }
    }
}

/// An active drag selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Drag {
    anchor: usize,
    /// Word or block range to keep selected around the anchor.
    unit: Option<(usize, usize)>,
}

impl EditorView {
    pub(crate) fn hit_at(&mut self, x: f32, y: f32) -> Option<Hit> {
        let (dx, dy) = self.to_document(x, y);
        self.layout().hit(dx, dy)
    }

    /// The task item whose checkbox is at `x, y`, counting `slack` around
    /// the box (a finger needs more than a mouse).
    pub(crate) fn checkbox_at(&mut self, x: f32, y: f32) -> Option<usize> {
        self.checkbox_near(x, y, 4.0)
    }

    pub(crate) fn checkbox_near(&mut self, x: f32, y: f32, slack: f32) -> Option<usize> {
        let (dx, dy) = self.to_document(x, y);
        self.layout()
            .decorations
            .iter()
            .find_map(|decoration| match decoration {
                Decoration::Checkbox { rect, pos, .. } => {
                    let target = rs_editor_layout::Rect::new(
                        rect.x - slack,
                        rect.y - slack,
                        rect.width + slack * 2.0,
                        rect.height + slack * 2.0,
                    );
                    target.contains(dx, dy).then_some(*pos)
                }
                _ => None,
            })
    }

    pub(crate) fn link_at(&mut self, pos: usize) -> Option<String> {
        let doc = self.editor.doc().clone();
        let resolved = doc.resolve(pos).ok()?;
        let node = resolved.node_after().or_else(|| resolved.node_before())?;
        node.marks()
            .iter()
            .find(|m| m.mark_type().name() == "link")
            .and_then(|m| m.attr("href"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    }

    pub(crate) fn word_range(&mut self, pos: usize) -> Option<(usize, usize)> {
        let layout = self.layout().clone();
        let text = layout.text_at(pos)?;
        let range = text.layout.word_at(pos - text.pos);
        Some((text.pos + range.start, text.pos + range.end))
    }

    pub(crate) fn block_range(&mut self, pos: usize) -> Option<(usize, usize)> {
        let layout = self.layout().clone();
        let text = layout.text_at(pos)?;
        Some((text.pos, text.end()))
    }

    fn select_range(&mut self, anchor: usize, head: usize, affinity: Affinity) {
        self.select_text(anchor, head, affinity);
    }

    pub fn pointer_down(&mut self, event: PointerEvent) -> bool {
        self.end_touch_mode();
        self.touch.finger_used = false;
        if event.button != PointerButton::Primary {
            return false;
        }
        if self.link_pointer_down(event.x, event.y, event.click_count, event.modifiers.shift)
            || self.slash_pointer_down(event.x, event.y)
            || self.bubble_pointer_down(event.x, event.y)
            || self.block_pointer_down(event.x, event.y)
        {
            return true;
        }
        self.stop_scrolling();
        self.set_focused(true);
        self.finish_composition();
        self.goal_x = None;
        if self.editor.is_editable()
            && let Some(pos) = self.checkbox_at(event.x, event.y)
        {
            let toggle = toggle_task_item(pos);
            return self.run_command(&toggle);
        }
        let Some(hit) = self.hit_at(event.x, event.y) else {
            return false;
        };
        match hit {
            Hit::Node { pos } => {
                self.drag = None;
                self.retry_image_at(pos);
                self.select_node(pos)
            }
            Hit::Text { pos, affinity } => {
                let mod_click = if self.platform() == rs_editor_commands::KeyPlatform::Apple {
                    event.modifiers.meta
                } else {
                    event.modifiers.ctrl
                };
                if mod_click && let Some(href) = self.link_at(pos) {
                    if let Some(href) = crate::link::safe_url(&href) {
                        self.push_effect(Effect::OpenUrl(href));
                    }
                    return true;
                }
                match event.click_count {
                    2 => {
                        let (from, to) = self.word_range(pos).unwrap_or((pos, pos));
                        self.drag = Some(Drag {
                            anchor: from,
                            unit: Some((from, to)),
                        });
                        self.select_range(from, to, Affinity::Downstream);
                    }
                    n if n >= 3 => {
                        let (from, to) = self.block_range(pos).unwrap_or((pos, pos));
                        self.drag = Some(Drag {
                            anchor: from,
                            unit: Some((from, to)),
                        });
                        self.select_range(from, to, Affinity::Downstream);
                    }
                    _ if event.modifiers.shift => {
                        let anchor = self.editor.selection().anchor();
                        self.drag = Some(Drag { anchor, unit: None });
                        self.select_range(anchor, pos, affinity);
                    }
                    _ => {
                        self.drag = Some(Drag {
                            anchor: pos,
                            unit: None,
                        });
                        self.select_range(pos, pos, affinity);
                    }
                }
                true
            }
        }
    }

    pub fn pointer_move(&mut self, event: PointerEvent) -> bool {
        self.touch.finger_used = false;
        if self.drag.is_none()
            && let Some(handled) = self
                .block_pointer_move(event.x, event.y)
                .or_else(|| self.link_pointer_move(event.x, event.y))
                .or_else(|| self.slash_pointer_move(event.x, event.y))
                .or_else(|| self.bubble_pointer_move(event.x, event.y))
        {
            return handled;
        }
        let Some(drag) = self.drag else {
            let cursor = if self.checkbox_at(event.x, event.y).is_some() {
                CursorIcon::Pointer
            } else {
                CursorIcon::Text
            };
            self.push_effect(Effect::Cursor(cursor));
            return false;
        };
        // Dragging past the edges scrolls.
        if event.y < 0.0 {
            self.scroll_by(event.y.max(-40.0));
        } else if event.y > self.height {
            self.scroll_by((event.y - self.height).min(40.0));
        }
        let Some(Hit::Text { pos, affinity }) =
            self.hit_at(event.x, event.y.clamp(0.0, self.height))
        else {
            return false;
        };
        let (anchor, head) = match drag.unit {
            Some((from, to)) if pos < from => (to, pos),
            Some((from, to)) if pos > to => (from, pos),
            Some((from, to)) => (from, to),
            None => (drag.anchor, pos),
        };
        let current = self.editor.selection();
        if current.anchor() == anchor && current.head() == head {
            return false;
        }
        self.select_range(anchor, head, affinity);
        true
    }

    pub fn pointer_up(&mut self, event: PointerEvent) -> bool {
        if self.block_pointer_up()
            || self.link_pointer_up(event.x, event.y)
            || self.slash_pointer_up(event.x, event.y)
            || self.bubble_pointer_up(event.x, event.y)
        {
            return true;
        }
        let dragged = self.drag.take().is_some();
        if dragged {
            // A finished drag selection may bring up the bubble menu.
            self.request_redraw();
        }
        dragged
    }

    /// A mouse wheel or trackpad scroll of `dy` with the pointer at
    /// `(x, y)`: scrolls a menu under the pointer, else the document.
    pub fn wheel(&mut self, x: f32, y: f32, dy: f32, smooth: bool) {
        if self.slash_wheel(x, y, dy) {
            return;
        }
        if smooth {
            self.scroll_smooth_by(dy);
        } else {
            self.scroll_by(dy);
        }
    }

    /// The selection as text positions, for hosts that render handles.
    pub fn selection_range(&self) -> (usize, usize) {
        let selection = self.editor.selection();
        (selection.from(), selection.to())
    }

    /// Whether the selection is a selected node (image, rule).
    pub fn has_node_selection(&self) -> bool {
        matches!(self.editor.selection(), Selection::Node(_))
    }
}
