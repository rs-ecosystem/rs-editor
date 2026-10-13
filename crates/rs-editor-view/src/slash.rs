//! The slash menu: typing `/` at the start of a block or after a space
//! opens a list of blocks to turn the line into, filtered by what follows
//! the slash (`/hea` → headings), like Notion and Tiptap's slash command
//! template.
//!
//! The menu is derived from the document, as Tiptap's Suggestion plugin
//! does: it shows while the text between a qualifying `/` and the caret
//! is a query, so typing, deleting, undo and IME input all keep it right
//! without tracking keystrokes.

use rs_editor_layout::{Affinity, UiText};
use rs_editor_state::Selection;
use rs_editor_ui::{Appear, Icon, Menu, MenuItem};
use serde_json::{Value, json};
use skia_safe::Canvas;

use accesskit::{Action, NodeId, Role};

use crate::a11y::{OverlayNodes, overlay_ids, overlay_node};
use crate::strings::Strings;
use crate::{CursorIcon, EditorView, Effect, KeyEvent, TouchPhase};

/// Space between the line and the menu.
const GAP: f32 = 4.0;
/// Space kept from the view's edges.
const MARGIN: f32 = 8.0;
const WIDTH: f32 = 256.0;
/// Room for every row (ten of 32 pixels, and the panel's padding).
const MAX_HEIGHT: f32 = 328.0;
/// A longer query is a word that happens to follow a slash.
const MAX_QUERY: usize = 24;

/// What a row is called.
#[derive(Debug, Clone, Copy)]
enum Label {
    Text,
    Heading(usize),
    BulletList,
    NumberedList,
    TaskList,
    Quote,
    CodeBlock,
    Divider,
    Image,
}

impl Label {
    fn text(self, strings: &Strings) -> &'static str {
        match self {
            Label::Text => strings.text,
            Label::Heading(level) => strings.heading[level - 1],
            Label::BulletList => strings.bullet_list,
            Label::NumberedList => strings.numbered_list,
            Label::TaskList => strings.task_list,
            Label::Quote => strings.quote,
            Label::CodeBlock => strings.code_block,
            Label::Divider => strings.divider,
            Label::Image => strings.image,
        }
    }
}

struct Item {
    icon: Icon,
    label: Label,
    /// The Markdown shortcut that does the same.
    hint: &'static str,
    /// Other words that find the row, in any language.
    keywords: &'static [&'static str],
    /// The commands that make the block (after the query is deleted).
    steps: fn() -> Vec<(String, Value)>,
}

fn step(name: &str, args: Value) -> (String, Value) {
    (name.to_owned(), args)
}

const ITEMS: &[Item] = &[
    Item {
        icon: Icon::Text,
        label: Label::Text,
        hint: "",
        keywords: &["text", "paragraph", "plain", "teks", "paragraf", "p"],
        steps: || vec![step("clearNodes", Value::Null)],
    },
    Item {
        icon: Icon::Heading1,
        label: Label::Heading(1),
        hint: "#",
        keywords: &["h1", "heading", "title", "judul"],
        steps: || vec![step("setHeading", json!({"level": 1}))],
    },
    Item {
        icon: Icon::Heading2,
        label: Label::Heading(2),
        hint: "##",
        keywords: &["h2", "heading", "subtitle", "judul"],
        steps: || vec![step("setHeading", json!({"level": 2}))],
    },
    Item {
        icon: Icon::Heading3,
        label: Label::Heading(3),
        hint: "###",
        keywords: &["h3", "heading", "judul"],
        steps: || vec![step("setHeading", json!({"level": 3}))],
    },
    Item {
        icon: Icon::List,
        label: Label::BulletList,
        hint: "-",
        keywords: &["bullet", "list", "ul", "unordered", "daftar", "poin"],
        steps: || vec![step("toggleBulletList", Value::Null)],
    },
    Item {
        icon: Icon::ListOrdered,
        label: Label::NumberedList,
        hint: "1.",
        keywords: &["numbered", "ordered", "list", "ol", "daftar", "nomor"],
        steps: || vec![step("toggleOrderedList", Value::Null)],
    },
    Item {
        icon: Icon::ListTodo,
        label: Label::TaskList,
        hint: "[]",
        keywords: &["todo", "task", "checkbox", "check", "tugas", "centang"],
        steps: || vec![step("toggleTaskList", Value::Null)],
    },
    Item {
        icon: Icon::Quote,
        label: Label::Quote,
        hint: ">",
        keywords: &["quote", "blockquote", "citation", "kutipan"],
        steps: || vec![step("toggleBlockquote", Value::Null)],
    },
    Item {
        icon: Icon::SquareCode,
        label: Label::CodeBlock,
        hint: "```",
        keywords: &["code", "codeblock", "pre", "snippet", "kode"],
        steps: || vec![step("setCodeBlock", Value::Null)],
    },
    Item {
        icon: Icon::Minus,
        label: Label::Divider,
        hint: "---",
        keywords: &[
            "divider",
            "rule",
            "hr",
            "line",
            "separator",
            "pemisah",
            "garis",
        ],
        steps: || vec![step("setHorizontalRule", Value::Null)],
    },
    Item {
        icon: Icon::Image,
        label: Label::Image,
        hint: "",
        keywords: &["image", "picture", "photo", "img", "gambar", "foto"],
        // For the dry run only: picking asks for the picture first.
        steps: || vec![step("setImage", json!({"src": ""}))],
    },
];

/// The `/` and the query after it, up to the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Trigger {
    /// Position of the slash.
    from: usize,
    /// The caret.
    to: usize,
    query: String,
}

/// What the rows were built for.
#[derive(Debug, Clone, PartialEq)]
struct Built {
    from: usize,
    query: String,
    revision: u64,
    color: u32,
    strings: Strings,
}

/// A finger on the menu: a tap picks a row, a drag scrolls the rows.
#[derive(Debug, Clone, Copy)]
struct MenuTouch {
    start: (f32, f32),
    last_y: f32,
    moved: bool,
    /// The row selected before the finger came down, restored by a scroll.
    selected: Option<usize>,
}

#[derive(Debug, Default)]
pub(crate) struct Slash {
    pub enabled: bool,
    appear: Appear,
    menu: Menu,
    trigger: Option<Trigger>,
    /// `ITEMS` indices of the rows, in order.
    shown: Vec<usize>,
    built: Option<Built>,
    /// The slash whose menu was closed (Escape): it stays closed until
    /// the query ends.
    dismissed: Option<usize>,
    pressed: Option<usize>,
    touch: Option<MenuTouch>,
}

impl Slash {
    pub(crate) fn new() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

impl EditorView {
    /// Shows a menu of blocks when `/` is typed (on by default).
    pub fn set_slash_menu(&mut self, enabled: bool) {
        self.slash.enabled = enabled;
        if !enabled {
            self.slash.appear.jump(false);
        }
        self.request_redraw();
    }

    /// Whether the slash menu is (becoming) visible.
    pub fn slash_menu_shown(&self) -> bool {
        self.slash.appear.target()
    }

    /// The labels of the slash menu's rows, in order.
    pub fn slash_menu_items(&self) -> Vec<&'static str> {
        self.slash
            .shown
            .iter()
            .map(|&i| ITEMS[i].label.text(&self.strings))
            .collect()
    }

    /// The slash before the caret that starts a query, if any.
    fn find_trigger(&self) -> Option<Trigger> {
        let selection = self.editor.selection();
        if !matches!(selection, Selection::Text(_)) || !selection.empty() {
            return None;
        }
        let head = selection.head_pos();
        let block = head.parent();
        if !block.is_textblock() || block.node_type().spec().code {
            return None;
        }
        let start = head.start(head.depth());
        let offset = selection.head() - start;
        let units: Vec<u16> = rs_editor_text::textblock_text(block)
            .encode_utf16()
            .collect();
        let before = &units[..offset.min(units.len())];
        let slash = before.iter().rposition(|&unit| unit == u16::from(b'/'))?;
        let query = String::from_utf16(&before[slash + 1..]).ok()?;
        if query.chars().count() > MAX_QUERY
            || query
                .chars()
                .any(|c| c.is_whitespace() || c == '/' || c == '\u{fffc}')
        {
            return None;
        }
        if slash > 0 {
            let previous = char::from_u32(u32::from(before[slash - 1]));
            if !previous.is_some_and(char::is_whitespace) {
                return None;
            }
        }
        Some(Trigger {
            from: start + slash,
            to: start + offset,
            query,
        })
    }

    fn slash_allowed(&self) -> bool {
        self.slash.enabled
            && self.focused
            && self.editor.is_editable()
            && self.drag.is_none()
            && !self.link_editing()
    }

    /// Brings the menu in line with the text before the caret.
    pub(crate) fn update_slash(&mut self) {
        let trigger = if self.slash_allowed() {
            self.find_trigger()
        } else {
            None
        };
        if self.slash.dismissed.is_some()
            && trigger.as_ref().map(|t| t.from) != self.slash.dismissed
        {
            self.slash.dismissed = None;
        }
        let trigger = trigger.filter(|t| Some(t.from) != self.slash.dismissed);
        let mut wanted = false;
        if let Some(trigger) = &trigger {
            self.build_slash_rows(trigger);
            wanted = !self.slash.shown.is_empty() && self.place_slash(trigger);
        }
        self.slash.trigger = trigger;
        if wanted != self.slash.appear.target() {
            self.slash.appear.set(wanted, self.clock.now_ms());
            if !wanted {
                self.slash.pressed = None;
                self.slash.touch = None;
            }
            self.request_redraw();
        }
    }

    /// The rows matching the query that can turn this block into theirs.
    fn build_slash_rows(&mut self, trigger: &Trigger) {
        let palette = self.theme().palette.clone();
        let built = Built {
            from: trigger.from,
            query: trigger.query.clone(),
            revision: self.revision(),
            color: palette.popover_foreground.argb(),
            strings: self.strings,
        };
        if self.slash.built.as_ref() == Some(&built) {
            return;
        }
        let new_query = self
            .slash
            .built
            .as_ref()
            .is_none_or(|b| b.from != built.from || b.query != built.query);
        let query = trigger.query.to_lowercase();
        let strings = self.strings;
        let mut scored: Vec<(u8, usize)> = ITEMS
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let label = item.label.text(&strings).to_lowercase();
                let score = if query.is_empty() || label.starts_with(&query) {
                    0
                } else if item.keywords.iter().any(|k| k.starts_with(&query))
                    || label.split_whitespace().any(|w| w.starts_with(&query))
                {
                    1
                } else if label.contains(&query) {
                    2
                } else {
                    return None;
                };
                Some((score, index))
            })
            .collect();
        scored.sort();
        let mut shown = Vec::new();
        for (_, index) in scored {
            let steps = slash_steps(trigger, index);
            if self.can_chain(&steps).unwrap_or(false) {
                shown.push(index);
            }
        }
        let label_style = UiText::small(palette.popover_foreground);
        let hint_style = UiText {
            mono: true,
            ..UiText::extra_small(palette.muted_foreground)
        };
        let items = shown
            .iter()
            .map(|&index| {
                let item = &ITEMS[index];
                MenuItem {
                    icon: item.icon,
                    label: self.engine.ui_label(
                        item.label.text(&strings),
                        &label_style,
                        Some(WIDTH - 100.0),
                    ),
                    hint: (!item.hint.is_empty())
                        .then(|| self.engine.ui_label(item.hint, &hint_style, None)),
                }
            })
            .collect();
        let menu = &mut self.slash.menu;
        menu.items = items;
        if new_query {
            menu.selected = (!shown.is_empty()).then_some(0);
            menu.scroll = 0.0;
        } else {
            menu.selected = menu.selected.map(|s| s.min(shown.len().saturating_sub(1)));
            menu.scroll = menu.scroll.clamp(0.0, menu.max_scroll());
        }
        if shown.is_empty() {
            menu.selected = None;
        }
        self.slash.shown = shown;
        self.slash.built = Some(built);
    }

    /// Puts the menu under the slash's line (over it when there is more
    /// room above); false when the slash is off screen.
    fn place_slash(&mut self, trigger: &Trigger) -> bool {
        let Some(line) = self
            .layout()
            .caret_rect(trigger.from, Affinity::Downstream, 1.0)
        else {
            return false;
        };
        let line = self.to_view(line);
        let visible = self.visible_height();
        if line.bottom() < 0.0 || line.y > visible {
            return false;
        }
        let width = WIDTH.min(self.width - MARGIN * 2.0).max(120.0);
        let below = visible - MARGIN - (line.bottom() + GAP);
        let above = line.y - GAP - MARGIN;
        let menu = &mut self.slash.menu;
        menu.width = width;
        menu.max_height = MAX_HEIGHT;
        let full = menu.size().1;
        let fits_below = below >= full.min(160.0) || below >= above;
        menu.max_height = MAX_HEIGHT
            .min(if fits_below { below } else { above })
            .max(48.0);
        let height = menu.size().1;
        let x = (line.x - 12.0).clamp(MARGIN, (self.width - width - MARGIN).max(MARGIN));
        let y = if fits_below {
            line.bottom() + GAP
        } else {
            line.y - GAP - height
        };
        let moved = menu.origin != (x, y);
        menu.origin = (x, y);
        if let Some(selected) = menu.selected {
            menu.reveal(selected);
        }
        if moved {
            self.request_redraw();
        }
        true
    }

    /// Closes the menu until the query ends (Escape, a click elsewhere).
    fn dismiss_slash(&mut self) {
        if let Some(trigger) = &self.slash.trigger {
            self.slash.dismissed = Some(trigger.from);
        }
        self.update_slash();
    }

    /// Turns the block into row `index`'s, deleting the query.
    fn pick_slash(&mut self, row: usize) {
        let (Some(trigger), Some(&index)) = (self.slash.trigger.clone(), self.slash.shown.get(row))
        else {
            return;
        };
        self.finish_composition();
        // Its own undo step: undo brings the typed query back.
        self.close_history_now();
        if matches!(ITEMS[index].label, Label::Image) {
            // The query goes; the picture comes from the host's picker or
            // an address typed into the field.
            let _ = self.run_chain(&slash_steps(&trigger, index)[..1]);
            if self.image_picker {
                self.push_effect(Effect::PickImage);
            } else {
                self.edit_image_address();
            }
        } else {
            let _ = self.run_chain(&slash_steps(&trigger, index));
        }
        self.update_slash();
    }

    /// Keys while the menu shows: arrows choose, Enter or Tab picks,
    /// Escape closes. Returns whether the menu used the key.
    pub(crate) fn slash_key(&mut self, event: &KeyEvent) -> bool {
        self.update_slash();
        let m = event.modifiers;
        if !self.slash.appear.target() || m.ctrl || m.meta || m.alt {
            return false;
        }
        match event.key.as_str() {
            "ArrowDown" => self.slash.menu.select_by(1),
            "ArrowUp" => self.slash.menu.select_by(-1),
            "Enter" | "Tab" => {
                let row = self.slash.menu.selected.unwrap_or(0);
                self.pick_slash(row);
            }
            "Escape" => self.dismiss_slash(),
            _ => return false,
        }
        self.request_redraw();
        true
    }

    /// A line break typed while the menu shows (an IME's Enter) picks.
    pub(crate) fn slash_newline(&mut self) -> bool {
        self.update_slash();
        if !self.slash.appear.target() {
            return false;
        }
        let row = self.slash.menu.selected.unwrap_or(0);
        self.pick_slash(row);
        true
    }

    pub(crate) fn slash_animating(&self) -> bool {
        self.slash.appear.animating(self.clock.now_ms())
    }

    pub(crate) fn paint_slash(&self, canvas: &Canvas) {
        let progress = self.slash.appear.value(self.clock.now_ms());
        if progress > 0.0 {
            self.slash.menu.paint(canvas, self.theme(), progress);
        }
    }

    fn slash_at(&self, x: f32, y: f32) -> bool {
        self.slash.appear.target() && self.slash.menu.contains(x, y)
    }

    /// A press on the menu holds a row; a press elsewhere closes it.
    pub(crate) fn slash_pointer_down(&mut self, x: f32, y: f32) -> bool {
        if !self.slash.appear.target() {
            return false;
        }
        if self.slash.menu.contains(x, y) {
            self.slash.pressed = self.slash.menu.item_at(x, y);
            return true;
        }
        self.dismiss_slash();
        false
    }

    /// Hovering the menu selects the row under the pointer.
    pub(crate) fn slash_pointer_move(&mut self, x: f32, y: f32) -> Option<bool> {
        if !self.slash_at(x, y) {
            return None;
        }
        let row = self.slash.menu.item_at(x, y);
        if row.is_some() && row != self.slash.menu.selected {
            self.slash.menu.selected = row;
            self.request_redraw();
        }
        self.push_effect(Effect::Cursor(CursorIcon::Pointer));
        Some(true)
    }

    /// The release picks the pressed row when still on it.
    pub(crate) fn slash_pointer_up(&mut self, x: f32, y: f32) -> bool {
        let Some(pressed) = self.slash.pressed.take() else {
            return false;
        };
        if self.slash.menu.item_at(x, y) == Some(pressed) {
            self.pick_slash(pressed);
        }
        true
    }

    /// The menu as a list box for screen readers; its chosen row becomes
    /// the editor's active descendant.
    pub(crate) fn slash_a11y(&self, out: &mut OverlayNodes) {
        if !self.slash.appear.target() {
            return;
        }
        let menu = &self.slash.menu;
        let count = self.slash.shown.len();
        let mut children = Vec::with_capacity(count);
        for (row, &index) in self.slash.shown.iter().enumerate() {
            let id = NodeId(overlay_ids::SLASH_OPTION + row as u64);
            let mut option = overlay_node(
                Role::ListBoxOption,
                menu.item_rect(row),
                ITEMS[index].label.text(&self.strings),
            );
            option.set_selected(menu.selected == Some(row));
            option.set_position_in_set(row + 1);
            option.set_size_of_set(count);
            option.add_action(Action::Click);
            out.nodes.push((id, option));
            children.push(id);
        }
        let list_id = NodeId(overlay_ids::SLASH_LIST);
        let mut list = overlay_node(Role::ListBox, menu.rect(), self.strings.blocks_menu);
        list.set_children(children);
        out.nodes.push((list_id, list));
        out.roots.push(list_id);
        out.active = menu
            .selected
            .map(|row| NodeId(overlay_ids::SLASH_OPTION + row as u64));
    }

    /// A screen reader picked row `row`.
    pub(crate) fn slash_click(&mut self, row: usize) -> bool {
        self.update_slash();
        if !self.slash.appear.target() || row >= self.slash.shown.len() {
            return false;
        }
        self.pick_slash(row);
        true
    }

    /// A wheel over the menu scrolls its rows.
    pub(crate) fn slash_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if !self.slash_at(x, y) {
            return false;
        }
        if self.slash.menu.scroll_by(dy) {
            self.request_redraw();
        }
        true
    }

    /// A finger on the menu; true when the menu took the touch.
    pub(crate) fn slash_touch(&mut self, phase: TouchPhase, x: f32, y: f32) -> bool {
        let slop = self.profile.touch_slop;
        match phase {
            TouchPhase::Down => {
                if !self.slash_at(x, y) {
                    return false;
                }
                self.slash.touch = Some(MenuTouch {
                    start: (x, y),
                    last_y: y,
                    moved: false,
                    selected: self.slash.menu.selected,
                });
                if let Some(row) = self.slash.menu.item_at(x, y) {
                    self.slash.menu.selected = Some(row);
                }
                true
            }
            TouchPhase::Move => {
                let Some(touch) = &mut self.slash.touch else {
                    return false;
                };
                let started = !touch.moved && (x - touch.start.0).hypot(y - touch.start.1) > slop;
                touch.moved |= started;
                let dy = touch.last_y - y;
                touch.last_y = y;
                let (moved, selected) = (touch.moved, touch.selected);
                if started {
                    // A scroll, not a choice: the row under the finger
                    // was only highlighted while it might have been a tap.
                    self.slash.menu.selected = selected;
                }
                if moved {
                    self.slash.menu.scroll_by(dy);
                }
                true
            }
            TouchPhase::Up => {
                let Some(touch) = self.slash.touch.take() else {
                    return false;
                };
                if !touch.moved
                    && let Some(row) = self.slash.menu.item_at(x, y)
                {
                    self.pick_slash(row);
                }
                true
            }
            TouchPhase::Cancel => self.slash.touch.take().is_some(),
        }
    }
}

/// Deleting the query, then making row `index`'s block.
fn slash_steps(trigger: &Trigger, index: usize) -> Vec<(String, Value)> {
    let mut steps = vec![step(
        "deleteRange",
        json!({"from": trigger.from, "to": trigger.to}),
    )];
    steps.extend((ITEMS[index].steps)());
    steps
}

#[cfg(test)]
impl Slash {
    pub(crate) fn row_center(&self, row: usize) -> (f32, f32) {
        let rect = self.menu.item_rect(row);
        (rect.center_x(), rect.center_y())
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.menu.selected
    }
}
