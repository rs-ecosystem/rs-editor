//! The bubble menu: formatting buttons (and a link button, which opens the
//! address field) over a text selection made with a mouse or trackpad, as
//! in Tiptap's BubbleMenu. Touch selections get the platform's own
//! selection menu instead (see `touch.rs`), which is what users of phones
//! expect.

use accesskit::{Action, NodeId, Role, Toggled};
use rs_editor_state::Selection;
use rs_editor_ui::{Appear, Icon, Toolbar, ToolbarButton, ToolbarEntry};
use serde_json::{Map, Value};
use skia_safe::Canvas;

use crate::a11y::{OverlayNodes, overlay_ids, overlay_node};
use crate::{CursorIcon, EditorView, Effect};

/// Space between the selection and the menu.
const GAP: f32 = 8.0;
/// Space kept from the view's edges.
const MARGIN: f32 = 8.0;

/// A button: its icon, the command it runs, and the mark it shows active.
struct Item {
    icon: Icon,
    command: &'static str,
    mark: &'static str,
}

const ITEMS: &[Item] = &[
    Item {
        icon: Icon::Bold,
        command: "toggleBold",
        mark: "bold",
    },
    Item {
        icon: Icon::Italic,
        command: "toggleItalic",
        mark: "italic",
    },
    Item {
        icon: Icon::Underline,
        command: "toggleUnderline",
        mark: "underline",
    },
    Item {
        icon: Icon::Strikethrough,
        command: "toggleStrike",
        mark: "strike",
    },
    Item {
        icon: Icon::Code,
        command: "toggleCode",
        mark: "code",
    },
    Item {
        icon: Icon::Highlighter,
        command: "toggleHighlight",
        mark: "highlight",
    },
    Item {
        icon: Icon::Link,
        command: LINK,
        mark: "link",
    },
];

/// The link button opens the address field instead of running a command.
const LINK: &str = "setLink";

#[derive(Debug, Default)]
pub(crate) struct Bubble {
    pub enabled: bool,
    appear: Appear,
    pub(crate) toolbar: Toolbar,
    /// The editor revision the buttons' states were computed at.
    revision: Option<u64>,
    /// Closed with Escape: stays closed while the selection stays.
    dismissed: Option<(usize, usize)>,
}

impl Bubble {
    pub(crate) fn new() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

impl EditorView {
    /// Shows a formatting menu over text selected with a mouse or trackpad
    /// (on by default).
    pub fn set_bubble_menu(&mut self, enabled: bool) {
        self.bubble.enabled = enabled;
        if !enabled {
            self.bubble.appear.jump(false);
        }
        self.request_redraw();
    }

    /// Whether the bubble menu is (becoming) visible.
    pub fn bubble_menu_shown(&self) -> bool {
        self.bubble.appear.target()
    }

    fn bubble_wanted(&self) -> bool {
        let selection = self.editor.selection();
        self.bubble.enabled
            && self.focused
            && self.editor.is_editable()
            && !self.touch.finger_used
            && self.drag.is_none()
            && self.composition.is_none()
            && !self.link_editing()
            && matches!(selection, Selection::Text(_) | Selection::All(_))
            && !selection.empty()
            && self.bubble.dismissed != Some((selection.from(), selection.to()))
    }

    /// Escape over the menu closes it until the selection changes.
    pub(crate) fn bubble_escape(&mut self) -> bool {
        if !self.bubble.appear.target() {
            return false;
        }
        let selection = self.editor.selection();
        self.bubble.dismissed = Some((selection.from(), selection.to()));
        self.update_bubble();
        true
    }

    /// Brings the menu in line with the selection: shows or hides it,
    /// refreshes its buttons and places it.
    pub(crate) fn update_bubble(&mut self) {
        let now = self.clock.now_ms();
        let mut wanted = self.bubble_wanted();
        if wanted {
            let selection = self.editor.selection();
            let (from, to) = (selection.from(), selection.to());
            let rects: Vec<_> = self
                .layout()
                .selection_rects(from, to)
                .into_iter()
                .map(|r| self.to_view(r))
                .collect();
            let top = rects.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
            let bottom = rects
                .iter()
                .map(|r| r.bottom())
                .fold(f32::NEG_INFINITY, f32::max);
            let visible = self.visible_height();
            if rects.is_empty() || bottom < 0.0 || top > visible {
                wanted = false;
            } else {
                self.refresh_bubble_buttons();
                let (width, height) = self.bubble.toolbar.size();
                // Over the selection's first line; under its last when
                // there is no room above (the first line scrolled away).
                let mut y = top - GAP - height;
                let above = y >= MARGIN;
                if !above {
                    y = (bottom + GAP).min(visible - height - MARGIN);
                }
                let line = |r: &&rs_editor_layout::Rect| {
                    if above {
                        r.y < top + 1.0
                    } else {
                        r.bottom() > bottom - 1.0
                    }
                };
                let left = rects
                    .iter()
                    .filter(line)
                    .map(|r| r.x)
                    .fold(f32::INFINITY, f32::min);
                let right = rects
                    .iter()
                    .filter(line)
                    .map(|r| r.right())
                    .fold(f32::NEG_INFINITY, f32::max);
                let x = ((left + right) / 2.0 - width / 2.0)
                    .clamp(MARGIN, (self.width - width - MARGIN).max(MARGIN));
                self.bubble.toolbar.origin = (x, y);
            }
        }
        if wanted != self.bubble.appear.target() {
            self.bubble.appear.set(wanted, now);
            if !wanted {
                self.bubble.toolbar.hovered = None;
                self.bubble.toolbar.pressed = None;
            }
            self.request_redraw();
        }
    }

    fn refresh_bubble_buttons(&mut self) {
        let revision = self.revision();
        if self.bubble.revision == Some(revision) && !self.bubble.toolbar.entries.is_empty() {
            return;
        }
        self.bubble.revision = Some(revision);
        let no_attrs = Map::new();
        let entries = ITEMS
            .iter()
            .map(|item| {
                ToolbarEntry::Button(ToolbarButton {
                    icon: item.icon,
                    active: self.editor.is_active(item.mark, &no_attrs),
                    enabled: item.command == LINK || self.editor.can(item.command, &Value::Null),
                })
            })
            .collect();
        self.bubble.toolbar.entries = entries;
    }

    pub(crate) fn bubble_animating(&self) -> bool {
        self.bubble.appear.animating(self.clock.now_ms())
    }

    /// Paints the menu (view coordinates, logical pixels).
    pub(crate) fn paint_bubble(&self, canvas: &Canvas) {
        let progress = self.bubble.appear.value(self.clock.now_ms());
        if progress > 0.0 {
            self.bubble.toolbar.paint(canvas, self.theme(), progress);
        }
    }

    /// The menu as a toolbar of toggle buttons for screen readers.
    pub(crate) fn bubble_a11y(&self, out: &mut OverlayNodes) {
        if !self.bubble.appear.target() {
            return;
        }
        let toolbar = &self.bubble.toolbar;
        let mut children = Vec::new();
        for (index, (entry, rect)) in toolbar
            .entries
            .iter()
            .zip(toolbar.entry_rects())
            .enumerate()
        {
            let ToolbarEntry::Button(button) = entry else {
                continue;
            };
            let id = NodeId(overlay_ids::BUBBLE_BUTTON + index as u64);
            let label = self.strings.marks.get(index).copied().unwrap_or_default();
            let mut node = overlay_node(Role::Button, rect, label);
            node.set_toggled(Toggled::from(button.active));
            if button.enabled {
                node.add_action(Action::Click);
            } else {
                node.set_disabled();
            }
            out.nodes.push((id, node));
            children.push(id);
        }
        let id = NodeId(overlay_ids::BUBBLE_BAR);
        let mut bar = overlay_node(Role::Toolbar, toolbar.rect(), self.strings.formatting);
        bar.set_children(children);
        out.nodes.push((id, bar));
        out.roots.push(id);
    }

    /// A screen reader pressed button `index`.
    pub(crate) fn bubble_click(&mut self, index: usize) -> bool {
        if !self.bubble.appear.target() {
            return false;
        }
        let Some(item) = ITEMS.get(index) else {
            return false;
        };
        if item.command == LINK {
            self.edit_link()
        } else {
            self.run(item.command, &Value::Null).unwrap_or(false)
        }
    }

    /// Whether the menu takes pointer events at `(x, y)`.
    fn bubble_at(&self, x: f32, y: f32) -> bool {
        self.bubble.appear.target() && self.bubble.toolbar.contains(x, y)
    }

    /// A press on the menu: holds a button, and keeps the selection.
    pub(crate) fn bubble_pointer_down(&mut self, x: f32, y: f32) -> bool {
        if !self.bubble_at(x, y) {
            return false;
        }
        self.bubble.toolbar.pressed = self.bubble.toolbar.button_at(x, y);
        self.request_redraw();
        true
    }

    /// Hovering the menu; `Some` when it took the move.
    pub(crate) fn bubble_pointer_move(&mut self, x: f32, y: f32) -> Option<bool> {
        if self.bubble.toolbar.pressed.is_none() && !self.bubble_at(x, y) {
            if self.bubble.toolbar.hovered.take().is_some() {
                self.request_redraw();
            }
            return None;
        }
        let hovered = self.bubble.toolbar.button_at(x, y);
        if hovered != self.bubble.toolbar.hovered {
            self.bubble.toolbar.hovered = hovered;
            self.request_redraw();
        }
        self.push_effect(Effect::Cursor(if hovered.is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        }));
        Some(true)
    }

    /// The release ends a press on the menu, running the button's command
    /// when still on it.
    pub(crate) fn bubble_pointer_up(&mut self, x: f32, y: f32) -> bool {
        let Some(pressed) = self.bubble.toolbar.pressed.take() else {
            return false;
        };
        if self.bubble.toolbar.button_at(x, y) == Some(pressed)
            && let Some(item) = ITEMS.get(pressed)
        {
            if item.command == LINK {
                self.edit_link();
            } else {
                let _ = self.run(item.command, &Value::Null);
            }
        }
        self.request_redraw();
        true
    }
}

#[cfg(test)]
impl Bubble {
    pub(crate) fn toolbar_rect(&self) -> rs_editor_layout::Rect {
        let r = self.toolbar.rect();
        rs_editor_layout::Rect::new(r.left, r.top, r.width(), r.height())
    }

    pub(crate) fn button_center(&self, index: usize) -> (f32, f32) {
        let r = self.toolbar.entry_rects()[index];
        (r.center_x(), r.center_y())
    }

    pub(crate) fn button_active(&self, index: usize) -> bool {
        matches!(self.toolbar.entries[index], ToolbarEntry::Button(b) if b.active)
    }
}
