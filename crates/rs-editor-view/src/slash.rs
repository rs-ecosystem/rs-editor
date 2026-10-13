//! The slash menu: typing `/` at the start of a block or after a space
//! opens a list of blocks to turn the line into, filtered by what follows
//! the slash (`/hea` → headings, `/judul 2`), like Notion and Tiptap's
//! slash command template.
//!
//! Only a slash just typed (or committed by an IME) arms the menu; moving
//! the caret into existing text such as `/docs` never opens it. While the
//! caret stays in the query the menu follows the document, so typing,
//! deleting and IME composition keep it right; leaving the query disarms
//! it. A query that matches nothing shows "No results", and once it holds
//! a space with no match the slash is just text again.

use rs_editor_layout::{Affinity, UiText};
use rs_editor_state::Selection;
use rs_editor_ui::{Appear, ITEM_HEIGHT, Icon, Menu, MenuItem, PANEL_PADDING};
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
/// Room for every row and the panel's padding.
const MAX_HEIGHT: f32 = ITEMS.len() as f32 * ITEM_HEIGHT + 2.0 * PANEL_PADDING;
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
    /// The end of the word the caret is in: a pick deletes up to here.
    end: usize,
    query: String,
}

/// Slashes that start a query (Chinese and Japanese IMEs type the
/// full-width one).
fn is_slash(c: char) -> bool {
    c == '/' || c == '\u{ff0f}'
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
    /// The row pressed: a tap picks this one, not whatever is under the
    /// finger by the time it lifts.
    row: Option<usize>,
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
    /// The slash just typed that the menu belongs to.
    armed: Option<usize>,
    /// Arrow keys chose the row: a mouse resting over the menu does not
    /// take the choice over until it moves (from where it was first seen).
    keyboard: Option<Option<(f32, f32)>>,
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

    /// The query of the armed slash, while the caret is in it.
    fn find_trigger(&self) -> Option<Trigger> {
        let from = self.slash.armed?;
        let selection = self.editor.selection();
        if !matches!(selection, Selection::Text(_)) || !selection.empty() {
            return None;
        }
        let caret = selection.head();
        let head = selection.head_pos();
        let block = head.parent();
        let start = head.start(head.depth());
        if !block.is_textblock() || block.node_type().spec().code || from < start || caret <= from {
            return None;
        }
        let units: Vec<u16> = rs_editor_text::textblock_text(block)
            .encode_utf16()
            .collect();
        let (slash, offset) = (from - start, caret - start);
        if offset > units.len()
            || !char::decode_utf16([units[slash]])
                .next()?
                .is_ok_and(is_slash)
        {
            return None;
        }
        let query = String::from_utf16(&units[slash + 1..offset]).ok()?;
        if query.chars().count() > MAX_QUERY
            || query.starts_with(char::is_whitespace)
            || query.contains(['\n', '\u{fffc}'])
            || query.chars().any(is_slash)
        {
            return None;
        }
        // The rest of the word after the caret goes with the query.
        let rest = String::from_utf16_lossy(&units[offset..]);
        let word: usize = rest
            .chars()
            .take_while(|&c| !c.is_whitespace() && !is_slash(c) && c != '\u{fffc}')
            .map(char::len_utf16)
            .sum();
        Some(Trigger {
            from,
            to: caret,
            end: caret + word,
            query,
        })
    }

    /// After typing: a slash just typed (within the last `typed` UTF-16
    /// units before the caret), at the start of a block or after a space
    /// and outside code, arms the menu.
    pub(crate) fn arm_slash(&mut self, typed: usize) {
        if !self.slash.enabled {
            return;
        }
        let selection = self.editor.selection();
        if !matches!(selection, Selection::Text(_)) || !selection.empty() {
            return;
        }
        let head = selection.head_pos();
        let block = head.parent();
        if !block.is_textblock() || block.node_type().spec().code {
            return;
        }
        // Inline code takes slashes as code too.
        if head.marks().iter().any(|m| m.mark_type().name() == "code") {
            return;
        }
        let start = head.start(head.depth());
        let offset = selection.head() - start;
        let units: Vec<u16> = rs_editor_text::textblock_text(block)
            .encode_utf16()
            .collect();
        let before = String::from_utf16_lossy(&units[..offset.min(units.len())]);
        let Some((index, _)) = before.char_indices().rev().find(|&(_, c)| is_slash(c)) else {
            return;
        };
        let slash = before[..index].encode_utf16().count();
        if slash + typed < offset {
            return;
        }
        let previous = before[..index].chars().last();
        if previous.is_some_and(|c| !c.is_whitespace()) {
            return;
        }
        self.slash.armed = Some(start + slash);
        self.slash.dismissed = None;
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
        if trigger.is_none() && self.slash_allowed() {
            // The caret left the query (or the slash went): it is text now.
            self.slash.armed = None;
        }
        if self.slash.dismissed.is_some()
            && trigger.as_ref().map(|t| t.from) != self.slash.dismissed
        {
            self.slash.dismissed = None;
        }
        let trigger = trigger.filter(|t| Some(t.from) != self.slash.dismissed);
        let mut wanted = false;
        if let Some(trigger) = &trigger {
            self.build_slash_rows(trigger);
            let words = trigger.query.contains(char::is_whitespace);
            if self.slash.shown.is_empty() && words {
                // "/bullet lists" with no match: the slash is just text.
                self.slash.armed = None;
            } else {
                wanted = self.place_slash(trigger);
            }
        }
        self.slash.trigger = trigger.filter(|_| self.slash.armed.is_some());
        if self.slash.trigger.is_none() {
            wanted = false;
        }
        if wanted != self.slash.appear.target() {
            self.slash.appear.set(wanted, self.clock.now_ms());
            if !wanted {
                self.slash.pressed = None;
                self.slash.touch = None;
                self.slash.keyboard = None;
            }
            self.request_redraw();
        }
    }

    /// The rows matching the query that can turn this block into theirs.
    fn build_slash_rows(&mut self, trigger: &Trigger) {
        let built = Built {
            from: trigger.from,
            query: trigger.query.clone(),
            revision: self.revision(),
            color: self.theme().palette.popover_foreground.argb(),
            strings: self.strings,
        };
        if self.slash.built.as_ref() == Some(&built) {
            return;
        }
        let palette = self.theme().palette.clone();
        let new_query = self
            .slash
            .built
            .as_ref()
            .is_none_or(|b| b.from != built.from || b.query != built.query);
        let query = trigger.query.trim_end().to_lowercase();
        let strings = self.strings;
        // A list or quote row would undo the list or quote the caret is in.
        let head = self.editor.selection().head_pos().clone();
        let ancestors: Vec<String> = (0..=head.depth())
            .map(|depth| head.node(depth).type_name().to_owned())
            .collect();
        let active: Vec<Label> = [
            (Label::BulletList, "bulletList"),
            (Label::NumberedList, "orderedList"),
            (Label::TaskList, "taskList"),
            (Label::Quote, "blockquote"),
        ]
        .into_iter()
        .filter(|(_, name)| ancestors.iter().any(|a| a == name))
        .map(|(label, _)| label)
        .collect();
        let already = |label: Label| {
            active
                .iter()
                .any(|a| std::mem::discriminant(a) == std::mem::discriminant(&label))
        };
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
            if already(ITEMS[index].label) {
                continue;
            }
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
        let empty = self.engine.ui_label(
            strings.no_results,
            &UiText::small(palette.muted_foreground),
            None,
        );
        let menu = &mut self.slash.menu;
        menu.items = items;
        menu.empty = Some(empty);
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
        // Placing never scrolls to the chosen row: the rows a finger or a
        // wheel scrolled to stay (arrow keys reveal what they choose).
        menu.scroll = menu.scroll.clamp(0.0, menu.max_scroll());
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
        self.slash.armed = None;
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
        // What is typed next is its own undo step too.
        self.close_history_now();
        self.update_slash();
    }

    /// Keys while the menu shows: arrows choose, Enter or Tab picks,
    /// Escape closes. Returns whether the menu used the key.
    pub(crate) fn slash_key(&mut self, event: &KeyEvent) -> bool {
        self.update_slash();
        let m = event.modifiers;
        // Shift-Enter (a line break) and Shift-Tab (outdent) keep theirs.
        if !self.slash.appear.target() || m.ctrl || m.meta || m.alt || m.shift {
            return false;
        }
        let rows = self.slash.shown.len();
        match event.key.as_str() {
            "ArrowDown" | "ArrowUp" | "Home" | "End" | "PageDown" | "PageUp" if rows > 0 => {
                let menu = &mut self.slash.menu;
                match event.key.as_str() {
                    "ArrowDown" => menu.select_by(1),
                    "ArrowUp" => menu.select_by(-1),
                    "Home" => menu.select_by(-(menu.selected.unwrap_or(0) as i32)),
                    "End" => menu.select_by(rows as i32 - 1 - menu.selected.unwrap_or(0) as i32),
                    "PageDown" => {
                        menu.select_by(5.min(rows as i32 - 1 - menu.selected.unwrap_or(0) as i32))
                    }
                    _ => menu.select_by(-(5.min(menu.selected.unwrap_or(0) as i32))),
                }
                // The keyboard chose: a mouse resting on the menu waits.
                self.slash.keyboard = Some(None);
            }
            "Enter" | "Tab" if rows == 0 => {
                // Nothing to pick: the key does what it always does.
                self.slash.armed = None;
                self.update_slash();
                return false;
            }
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
        if !self.slash.appear.target() || self.slash.shown.is_empty() {
            self.slash.armed = None;
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
            self.stop_scrolling();
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
        // After arrow keys, the pointer must really move to choose.
        match self.slash.keyboard {
            Some(None) => {
                self.slash.keyboard = Some(Some((x, y)));
                return Some(true);
            }
            Some(Some(seen)) if (x - seen.0).hypot(y - seen.1) < 3.0 => return Some(true),
            Some(Some(_)) => self.slash.keyboard = None,
            None => {}
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
                // A fling of the document stops, so rows hold still.
                self.stop_scrolling();
                let row = self.slash.menu.item_at(x, y);
                self.slash.touch = Some(MenuTouch {
                    start: (x, y),
                    last_y: y,
                    moved: false,
                    row,
                });
                if row.is_some() {
                    self.slash.menu.selected = row;
                }
                true
            }
            TouchPhase::Move => {
                let Some(touch) = &mut self.slash.touch else {
                    return false;
                };
                touch.moved |= (x - touch.start.0).hypot(y - touch.start.1) > slop;
                let dy = touch.last_y - y;
                touch.last_y = y;
                // The pressed row keeps its highlight and moves with the
                // finger, as in a platform select; a scroll only does not
                // pick it.
                if touch.moved {
                    self.slash.menu.scroll_by(dy);
                }
                true
            }
            TouchPhase::Up => {
                let Some(touch) = self.slash.touch.take() else {
                    return false;
                };
                if !touch.moved
                    && let Some(row) = touch.row
                    && self.slash.menu.item_at(x, y) == Some(row)
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
        json!({"from": trigger.from, "to": trigger.end}),
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

    pub(crate) fn scroll(&self) -> f32 {
        self.menu.scroll
    }
}
