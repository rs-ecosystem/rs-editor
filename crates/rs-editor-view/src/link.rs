//! The link popover. With the caret in a link it shows the address and
//! buttons to open, copy, edit or remove the link; editing (also from
//! Mod-K or the bubble menu) swaps in a field for the address.
//!
//! While the field is open it owns text input: keys, typed text, IME
//! composition and paste go to the field instead of the document, and the
//! IME sees the field's text (`ImeContext::field`), so platform keyboards
//! work in it as in any text field.

use accesskit::{Action as AxAction, NodeId, Role};
use rs_editor_layout::{Affinity, Rect, TextLayout, UiText};
use rs_editor_state::Selection;
use rs_editor_ui::{
    Appear, BUTTON_SIZE, FIELD_HEIGHT, Icon, PANEL_PADDING, TextField, Toolbar, ToolbarButton,
    ToolbarEntry, begin_appear, paint_panel, paint_text,
};
use serde_json::{Value, json};
use skia_safe::{Canvas, Contains};

use crate::a11y::{OverlayNodes, overlay_ids, overlay_node};
use crate::{
    ClipboardContent, CursorIcon, EditorView, Effect, Granularity, ImeCaret, ImeContext, KeyEvent,
    TouchPhase,
};

/// Space between the link and the popover.
const GAP: f32 = 8.0;
/// Space kept from the view's edges.
const MARGIN: f32 = 8.0;
/// Widest the address is shown.
const URL_WIDTH: f32 = 220.0;
/// Width of the address field.
const FIELD_WIDTH: f32 = 260.0;
/// Space around the address label inside its area.
const URL_INSET: f32 = 8.0;
/// Space between the label or field and the buttons.
const SPACING: f32 = 4.0;

/// A link in the document: `from == to` when there is none yet (a link
/// to insert at the caret).
#[derive(Debug, Clone, PartialEq, Eq)]
struct LinkRange {
    from: usize,
    to: usize,
    href: String,
}

/// What the popover's buttons do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Open,
    Copy,
    Edit,
    Remove,
    Apply,
}

/// The part of the popover under a pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Url,
    Field,
    Button(usize),
}

#[derive(Debug)]
struct Editing {
    field: TextField,
    range: LinkRange,
    /// The field asks for an image's address (from the slash menu), not
    /// a link's.
    image: bool,
}

#[derive(Debug, Default)]
pub(crate) struct LinkPopover {
    appear: Appear,
    /// The link shown (view mode).
    shown: Option<LinkRange>,
    editing: Option<Editing>,
    actions: Vec<Action>,
    toolbar: Toolbar,
    /// The address, shaped (view mode).
    url: Option<(String, u32, TextLayout)>,
    rect: skia_safe::Rect,
    field_rect: skia_safe::Rect,
    url_rect: skia_safe::Rect,
    /// The link whose popover was closed with Escape, until the caret
    /// leaves it.
    dismissed: Option<(usize, usize)>,
    pressed: Option<Part>,
    /// A finger down on the popover.
    touch: Option<(f32, f32)>,
    /// A finger down elsewhere while the field is open.
    outside: Option<(f32, f32)>,
    /// When the finger on the popover came down.
    touch_at: u64,
    /// The last tap on the field (a second one soon after selects all).
    last_tap: u64,
    /// Until when the copy button shows a check (it just copied).
    copied_until: u64,
    /// The buttons were built showing that check.
    copied_shown: bool,
}

/// How long the copy button shows that it copied.
const COPIED_MS: u64 = 1_500;

/// One edit to the address field (see [`EditorView::field_input`]).
pub(crate) enum FieldInput<'a> {
    Key(&'a KeyEvent),
    Insert(&'a str),
    SetComposing(&'a str),
    Commit(&'a str),
    CancelComposition,
    FinishComposition,
    SetSelection(usize, usize),
    ComposingRegion(usize, usize),
    DeleteSurrounding(usize, usize),
    Replace(usize, usize, &'a str, bool),
    Delete(bool, Granularity),
}

impl EditorView {
    /// The copy button still shows that it copied.
    fn link_copied(&self) -> bool {
        self.clock.now_ms() < self.link.copied_until
    }

    /// Milliseconds until the copy button turns back from its check.
    pub(crate) fn link_deadline(&self) -> Option<u64> {
        self.link_copied()
            .then(|| self.link.copied_until - self.clock.now_ms())
    }

    /// Whether the address field is open (it then takes text input).
    pub fn link_editing(&self) -> bool {
        self.link.editing.is_some()
    }

    /// Whether the link popover is (becoming) visible.
    pub fn link_popover_shown(&self) -> bool {
        self.link.appear.target()
    }

    /// The address field's text and selection, while it is open.
    pub fn link_field(&self) -> Option<(String, usize, usize)> {
        self.link.editing.as_ref().map(|e| {
            let (a, h) = e.field.selection();
            (e.field.text().to_owned(), a, h)
        })
    }

    /// Opens the address field for the link at the caret or the selected
    /// text (Mod-K). Returns false when there is nothing to link (a
    /// selected image) or the editor is read-only.
    pub fn edit_link(&mut self) -> bool {
        if !self.editor.is_editable() {
            return false;
        }
        self.finish_composition();
        let selection = self.editor.selection().clone();
        let range = match &selection {
            Selection::Text(_) if selection.empty() => {
                let head = selection.head();
                self.link_around(head).unwrap_or(LinkRange {
                    from: head,
                    to: head,
                    href: String::new(),
                })
            }
            Selection::Text(_) => {
                let (from, to) = (selection.from(), selection.to());
                // Inside one link: the whole link is edited, not a piece.
                self.link_around(from + 1)
                    .filter(|link| link.from <= from && to <= link.to)
                    .unwrap_or(LinkRange {
                        from,
                        to,
                        href: String::new(),
                    })
            }
            _ => return false,
        };
        self.link.editing = Some(Editing {
            field: TextField::new(&range.href),
            range,
            image: false,
        });
        self.link.pressed = None;
        self.hide_menu();
        self.set_focused(true);
        self.reset_blink();
        self.request_redraw();
        self.after_field_change();
        true
    }

    /// The link around `pos` in its textblock (with `pos` inside it or at
    /// its end).
    fn link_around(&self, pos: usize) -> Option<LinkRange> {
        let doc = self.editor.doc().clone();
        let resolved = doc.resolve(pos).ok()?;
        let parent = resolved.parent();
        if !parent.is_textblock() {
            return None;
        }
        let mut offset = resolved.start(resolved.depth());
        let mut runs: Vec<(usize, usize, Option<String>)> = Vec::new();
        for child in parent.children() {
            let size = child.node_size();
            let href = child
                .marks()
                .iter()
                .find(|m| m.mark_type().name() == "link")
                .and_then(|m| m.attr("href"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            match runs.last_mut() {
                Some(last) if last.2.is_some() && last.2 == href => last.1 = offset + size,
                _ => runs.push((offset, offset + size, href)),
            }
            offset += size;
        }
        runs.into_iter().find_map(|(from, to, href)| {
            let href = href?;
            (from < pos && pos <= to).then_some(LinkRange { from, to, href })
        })
    }

    /// The link the popover should show without editing.
    fn link_wanted(&self) -> Option<LinkRange> {
        if !self.focused
            || self.composition.is_some()
            || self.drag.is_some()
            || self.touch_gesture_active()
            || self.slash_menu_shown()
        {
            return None;
        }
        let selection = self.editor.selection();
        if !matches!(selection, Selection::Text(_)) || !selection.empty() {
            return None;
        }
        self.link_around(selection.head())
            .filter(|link| self.link.dismissed != Some((link.from, link.to)))
    }

    /// Keeps the popover's link in step with a change to the document
    /// (an undo, a command from the host) made while it shows; the field
    /// closes when its text went away.
    pub(crate) fn map_link(&mut self, mapping: &rs_editor_transform::Mapping) {
        use rs_editor_transform::{Assoc, Mappable};
        let map = |from: usize, to: usize| {
            let (a, b) = (
                mapping.map(from, Assoc::After),
                mapping.map(to, Assoc::Before),
            );
            (a, b.max(a))
        };
        if let Some(editing) = &mut self.link.editing {
            let range = &mut editing.range;
            let had_text = range.from < range.to;
            let (from, to) = map(range.from, range.to);
            (range.from, range.to) = (from, to);
            if had_text && from == to {
                self.close_link_editor();
            }
        }
        if let Some((from, to)) = self.link.dismissed {
            self.link.dismissed = Some(map(from, to));
        }
    }

    /// Brings the popover in line with the caret.
    pub(crate) fn update_link(&mut self) {
        if self.link.editing.is_some() && !self.editor.is_editable() {
            // Read-only now: nothing to edit.
            self.close_link_editor();
        }
        let shown = if self.link.editing.is_some() {
            None
        } else {
            self.link_wanted()
        };
        if self.link.dismissed.is_some()
            && self
                .link_around(self.editor.selection().head())
                .map(|l| (l.from, l.to))
                != self.link.dismissed
        {
            self.link.dismissed = None;
        }
        if shown.as_ref().map(|l| (l.from, l.to))
            != self.link.shown.as_ref().map(|l| (l.from, l.to))
        {
            // Another link: its address was not copied.
            self.link.copied_until = 0;
        }
        let range = match (&self.link.editing, &shown) {
            (Some(editing), _) => Some(editing.range.clone()),
            (None, Some(link)) => Some(link.clone()),
            (None, None) => None,
        };
        let mut wanted = false;
        if let Some(range) = range {
            self.build_link_parts(shown.as_ref());
            wanted = self.place_link(&range);
        }
        if shown.is_none() && self.link.editing.is_none() {
            self.link.toolbar.hovered = None;
            self.link.pressed = None;
        }
        self.link.shown = shown;
        let wanted = wanted || self.link.editing.is_some();
        if wanted != self.link.appear.target() {
            self.link.appear.set(wanted, self.clock.now_ms());
            self.request_redraw();
        }
    }

    /// The buttons (and the shaped address) for the current mode.
    fn build_link_parts(&mut self, shown: Option<&LinkRange>) {
        let editable = self.editor.is_editable();
        let actions = match (&self.link.editing, shown) {
            (Some(editing), _) if editing.image || editing.range.href.is_empty() => {
                vec![Action::Apply]
            }
            (Some(_), _) => vec![Action::Apply, Action::Remove],
            (None, _) if editable => {
                vec![Action::Open, Action::Copy, Action::Edit, Action::Remove]
            }
            (None, _) => vec![Action::Open, Action::Copy],
        };
        let copied = self.link_copied();
        if actions != self.link.actions || copied != self.link.copied_shown {
            self.link.copied_shown = copied;
            self.link.toolbar.entries = actions
                .iter()
                .map(|action| {
                    ToolbarEntry::Button(ToolbarButton {
                        icon: match action {
                            Action::Open => Icon::ExternalLink,
                            Action::Copy if copied => Icon::Check,
                            Action::Copy => Icon::Copy,
                            Action::Edit => Icon::Pencil,
                            Action::Remove => Icon::Unlink,
                            Action::Apply => Icon::Check,
                        },
                        active: false,
                        enabled: true,
                    })
                })
                .collect();
            self.link.actions = actions;
            self.link.toolbar.hovered = None;
            self.link.toolbar.focused = None;
        }
        if let Some(link) = shown {
            let color = self.theme().palette.muted_foreground;
            // Narrow screens cut the address shorter; the buttons stay.
            let buttons = self.link.toolbar.size().0 - PANEL_PADDING * 2.0;
            let room = self.width - MARGIN * 2.0 - PANEL_PADDING * 2.0 - SPACING - buttons;
            let max = URL_WIDTH.min(room - URL_INSET * 2.0).max(24.0);
            let key = format!("{}\u{0}{}", link.href, max);
            let fresh = self
                .link
                .url
                .as_ref()
                .is_none_or(|(old, argb, _)| *old != key || *argb != color.argb());
            if fresh {
                let layout = self
                    .engine
                    .ui_label(&link.href, &UiText::small(color), Some(max));
                self.link.url = Some((key, color.argb(), layout));
            }
        }
    }

    /// Places the popover under the link (over it when there is no room);
    /// false when the link is off screen.
    fn place_link(&mut self, range: &LinkRange) -> bool {
        let layout = self.layout();
        let mut rects = if range.from < range.to {
            layout.selection_rects(range.from, range.to)
        } else {
            Vec::new()
        };
        if rects.is_empty() {
            rects.extend(layout.caret_rect(range.from, Affinity::Downstream, 1.0));
        }
        let mut bounds = rects.iter().map(|r| self.to_view(*r));
        let Some(first) = bounds.next() else {
            return false;
        };
        let (mut left, mut top, mut bottom) = (first.x, first.y, first.bottom());
        for r in bounds {
            left = left.min(r.x);
            top = top.min(r.y);
            bottom = bottom.max(r.bottom());
        }
        let visible = self.visible_height();
        if self.link.editing.is_none() && (bottom < 0.0 || top > visible) {
            return false;
        }

        let buttons = self.link.toolbar.size().0 - PANEL_PADDING * 2.0;
        let leading = if self.link.editing.is_some() {
            FIELD_WIDTH.min(self.width - MARGIN * 2.0 - PANEL_PADDING * 2.0 - SPACING - buttons)
        } else {
            // The address gives way on narrow screens; the buttons stay.
            let room = self.width - MARGIN * 2.0 - PANEL_PADDING * 2.0 - SPACING - buttons;
            self.link
                .url
                .as_ref()
                .map_or(0.0, |(_, _, url)| url.width() + URL_INSET * 2.0)
                .min(room)
        }
        .max(40.0);
        let width = PANEL_PADDING * 2.0 + leading + SPACING + buttons;
        let height = PANEL_PADDING * 2.0 + BUTTON_SIZE;
        let x = left.clamp(MARGIN, (self.width - width - MARGIN).max(MARGIN));
        // Below a finger's caret handle, which hangs under the line.
        let gap = if self.touch.finger_used {
            GAP + 28.0
        } else {
            GAP
        };
        let below = bottom + gap;
        let y = if below + height <= visible - MARGIN || top - GAP - height < MARGIN {
            below.min((visible - height - MARGIN).max(MARGIN))
        } else {
            top - GAP - height
        };
        // The field stays on screen while the document scrolls under it.
        let y = if self.link.editing.is_some() {
            y.clamp(MARGIN, (visible - height - MARGIN).max(MARGIN))
        } else {
            y
        };
        let rect = skia_safe::Rect::from_xywh(x, y, width, height);
        let inner_left = x + PANEL_PADDING;
        let inner_top = y + PANEL_PADDING;
        self.link.url_rect =
            skia_safe::Rect::from_xywh(inner_left, inner_top, leading, BUTTON_SIZE);
        self.link.field_rect = skia_safe::Rect::from_xywh(
            inner_left,
            y + (height - FIELD_HEIGHT) / 2.0,
            leading,
            FIELD_HEIGHT,
        );
        self.link.toolbar.origin = (inner_left + leading + SPACING - PANEL_PADDING, y);
        if rect != self.link.rect {
            self.link.rect = rect;
            self.request_redraw();
        }
        true
    }

    pub(crate) fn link_animating(&self) -> bool {
        self.link.appear.animating(self.clock.now_ms())
    }

    /// The address field's text, shaped.
    fn field_layout(&self, text: &str) -> TextLayout {
        let color = self.theme().palette.foreground;
        self.engine.ui_label(text, &UiText::small(color), None)
    }

    pub(crate) fn paint_link(&self, canvas: &Canvas) {
        let progress = self.link.appear.value(self.clock.now_ms());
        if progress <= 0.0 {
            return;
        }
        let theme = self.theme();
        let rect = self.link.rect;
        begin_appear(canvas, rect, progress);
        paint_panel(canvas, rect, theme, progress);
        if let Some(editing) = &self.link.editing {
            let layout = self.field_layout(editing.field.text());
            let placeholder = self.engine.ui_label(
                if editing.image {
                    self.strings.image_placeholder
                } else {
                    self.strings.link_placeholder
                },
                &UiText::small(theme.palette.muted_foreground),
                None,
            );
            canvas.save_layer_alpha_f(rect.with_outset((4.0, 4.0)), progress);
            editing.field.paint(
                canvas,
                theme,
                self.link.field_rect,
                &layout,
                Some(&placeholder),
                self.focused && self.link.toolbar.focused.is_none(),
                self.caret_visible() && self.link.toolbar.focused.is_none(),
            );
            canvas.restore();
            if editing.field.invalid {
                self.paint_link_refused(canvas, progress);
            }
        } else if let Some((_, _, url)) = &self.link.url {
            let area = self.link.url_rect;
            paint_text(
                canvas,
                url,
                area.left + URL_INSET,
                area.center_y() - url.height() / 2.0,
                progress,
            );
        }
        self.link.toolbar.paint_buttons(canvas, theme, progress);
        canvas.restore();
    }

    /// Why the address was refused, in a small panel under the popover.
    fn paint_link_refused(&self, canvas: &Canvas, progress: f32) {
        let theme = self.theme();
        let rect = self.link.rect;
        let room = (self.width - MARGIN * 2.0 - PANEL_PADDING * 4.0).max(40.0);
        let label = self.engine.ui_label(
            self.strings.link_refused,
            &UiText::extra_small(theme.palette.destructive),
            Some(room),
        );
        let width = label.width() + PANEL_PADDING * 4.0;
        let height = label.height() + PANEL_PADDING * 3.0;
        let left = rect.left.min(self.width - MARGIN - width).max(MARGIN);
        let note = skia_safe::Rect::from_xywh(left, rect.bottom + 4.0, width, height);
        paint_panel(canvas, note, theme, progress);
        paint_text(
            canvas,
            &label,
            note.left + PANEL_PADDING * 2.0,
            note.center_y() - label.height() / 2.0,
            progress,
        );
    }

    /// The popover as a group for screen readers: the address (a link),
    /// the buttons, and while editing the address field, which then has
    /// focus.
    pub(crate) fn link_a11y(&self, out: &mut OverlayNodes) {
        if !self.link.appear.target() {
            return;
        }
        let strings = &self.strings;
        let mut children = Vec::new();
        if let Some(editing) = &self.link.editing {
            let id = NodeId(overlay_ids::LINK_FIELD);
            let label = if editing.image {
                strings.image_placeholder
            } else {
                strings.link_address
            };
            let mut field = overlay_node(Role::TextInput, self.link.field_rect, label);
            field.set_value(editing.field.text());
            field.add_action(AxAction::Focus);
            if editing.field.invalid {
                field.set_invalid(accesskit::Invalid::True);
                field.set_description(strings.link_refused);
            }
            out.nodes.push((id, field));
            out.focus = Some(id);
            children.push(id);
        } else if let Some(link) = &self.link.shown {
            let id = NodeId(overlay_ids::LINK_URL);
            let mut url = overlay_node(Role::Link, self.link.url_rect, &link.href);
            url.set_url(link.href.as_str());
            url.add_action(AxAction::Click);
            out.nodes.push((id, url));
            children.push(id);
        }
        let rects = self.link.toolbar.entry_rects();
        for (index, (action, rect)) in self.link.actions.iter().zip(rects).enumerate() {
            let id = NodeId(overlay_ids::LINK_BUTTON + index as u64);
            let label = match action {
                Action::Open => strings.open_link,
                Action::Copy if self.link.copied_shown => strings.copied,
                Action::Copy => strings.copy_link,
                Action::Edit => strings.edit_link,
                Action::Remove => strings.remove_link,
                Action::Apply => strings.apply,
            };
            let mut button = overlay_node(Role::Button, rect, label);
            button.add_action(AxAction::Click);
            if self.link.toolbar.focused == Some(index) {
                out.focus = Some(id);
            }
            out.nodes.push((id, button));
            children.push(id);
        }
        let id = NodeId(overlay_ids::LINK_GROUP);
        let mut group = overlay_node(Role::Group, self.link.rect, strings.link_menu);
        group.set_children(children);
        out.nodes.push((id, group));
        out.roots.push(id);
    }

    /// A screen reader activated the address (`None`) or button `index`.
    pub(crate) fn link_click(&mut self, button: Option<usize>) -> bool {
        if !self.link.appear.target() {
            return false;
        }
        let part = match button {
            Some(index) if index < self.link.actions.len() => Part::Button(index),
            Some(_) => return false,
            None => Part::Url,
        };
        self.press_link_part(part);
        true
    }

    fn link_part_at(&self, x: f32, y: f32) -> Option<Part> {
        if !self.link.appear.target() || !self.link.rect.contains(skia_safe::Point::new(x, y)) {
            return None;
        }
        let point = skia_safe::Point::new(x, y);
        if let Some(button) = self.link.toolbar.button_at(x, y) {
            return Some(Part::Button(button));
        }
        if self.link.editing.is_some() {
            return self.link.field_rect.contains(point).then_some(Part::Field);
        }
        self.link.url_rect.contains(point).then_some(Part::Url)
    }

    fn run_link_action(&mut self, action: Action) {
        let link = self
            .link
            .editing
            .as_ref()
            .map(|e| e.range.clone())
            .or_else(|| self.link.shown.clone());
        let Some(link) = link else {
            return;
        };
        match action {
            Action::Open => {
                if let Some(href) = safe_url(&link.href) {
                    self.push_effect(Effect::OpenUrl(href));
                }
            }
            Action::Copy => {
                let html = format!(
                    "<a href=\"{}\">{}</a>",
                    escape_html(&link.href),
                    escape_html(&link.href)
                );
                self.push_effect(Effect::Clipboard(ClipboardContent {
                    text: link.href,
                    html,
                    json: Value::Null,
                }));
                // The button shows a check for a moment: it copied.
                self.link.copied_until = self.clock.now_ms() + COPIED_MS;
                self.request_redraw();
            }
            Action::Edit => {
                self.edit_link();
            }
            Action::Remove => {
                let head = self.editor.selection().head();
                let _ = self.run_chain(&[
                    step(
                        "setTextSelection",
                        json!({"from": link.from, "to": link.to}),
                    ),
                    step("unsetLink", Value::Null),
                    step("setTextSelection", json!({"from": head, "to": head})),
                ]);
                self.close_link_editor();
            }
            Action::Apply => self.apply_link(),
        }
        self.request_redraw();
    }

    /// Links the range to the field's address (Enter); an empty field
    /// removes the link.
    fn apply_link(&mut self) {
        let Some(editing) = &self.link.editing else {
            return;
        };
        let text = editing.field.text().trim().to_owned();
        let range = editing.range.clone();
        if editing.image {
            self.apply_image(&text);
            return;
        }
        if text.is_empty() {
            if !range.href.is_empty() {
                self.run_link_action(Action::Remove);
            }
            self.close_link_editor();
            return;
        }
        let href = normalize_href(&text);
        // Without text to link, the address goes in as typed.
        let (from, to) = if range.from == range.to {
            (range.from, range.from + text.encode_utf16().count())
        } else {
            (range.from, range.to)
        };
        let mut steps = Vec::new();
        if range.from == range.to {
            steps.push(step("insertContent", Value::String(text.clone())));
        }
        steps.extend([
            step("setTextSelection", json!({"from": from, "to": to})),
            step("setLink", json!({"href": href})),
            step("setTextSelection", json!({"from": to, "to": to})),
        ]);
        match self.run_chain(&steps) {
            Ok(true) => {
                self.close_link_editor();
                // The caret ends at the link: no popover over what was
                // just made.
                self.link.dismissed = Some((from, to));
            }
            _ => {
                if let Some(editing) = &mut self.link.editing {
                    editing.field.invalid = true;
                }
                self.request_redraw();
            }
        }
    }

    /// Opens the address field for an image to insert at the caret (the
    /// slash menu's Image row, when the host has no image picker).
    pub(crate) fn edit_image_address(&mut self) {
        let head = self.editor.selection().head();
        self.link.editing = Some(Editing {
            field: TextField::new(""),
            range: LinkRange {
                from: head,
                to: head,
                href: String::new(),
            },
            image: true,
        });
        self.link.pressed = None;
        self.hide_menu();
        self.reset_blink();
        self.request_redraw();
        self.after_field_change();
    }

    /// Inserts an image from the field's address; an empty field gives up.
    fn apply_image(&mut self, text: &str) {
        if text.is_empty() {
            self.close_link_editor();
            return;
        }
        let src = normalize_href(text);
        match self.run_chain(&[step("setImage", json!({"src": src}))]) {
            Ok(true) => self.close_link_editor(),
            _ => {
                if let Some(editing) = &mut self.link.editing {
                    editing.field.invalid = true;
                }
                self.request_redraw();
            }
        }
    }

    /// Closes the field without changing the link (Escape).
    pub(crate) fn close_link_editor(&mut self) {
        if self.link.editing.take().is_some() {
            self.link.pressed = None;
            self.reset_blink();
            self.request_redraw();
            // The IME goes back to the document.
            self.after_change();
        }
    }

    /// Escape on the popover: closes the field, or hides the popover for
    /// this link.
    pub(crate) fn link_escape(&mut self) -> bool {
        if let Some(editing) = &self.link.editing {
            let range = (editing.range.from, editing.range.to);
            self.close_link_editor();
            self.link.dismissed = Some(range);
            return true;
        }
        if let Some(link) = &self.link.shown {
            self.link.dismissed = Some((link.from, link.to));
            self.update_link();
            return true;
        }
        false
    }

    /// Input for the address field; `None` when the field is closed (the
    /// input is the document's).
    pub(crate) fn field_input(&mut self, input: FieldInput<'_>) -> Option<bool> {
        let apple = self.platform() == rs_editor_commands::KeyPlatform::Apple;
        self.link.editing.as_ref()?;
        if let FieldInput::Key(event) = &input
            && let Some(handled) = self.link_tab_key(event)
        {
            return Some(handled);
        }
        if !matches!(input, FieldInput::Key(_)) {
            // Typing goes back to the field.
            self.link.toolbar.focused = None;
        }
        let editing = self.link.editing.as_mut()?;
        let field = &mut editing.field;
        match input {
            FieldInput::Key(event) => {
                let m = event.modifiers;
                let command = if apple { m.meta } else { m.ctrl };
                let word = if apple { m.alt } else { m.ctrl };
                match event.key.as_str() {
                    "Enter" => {
                        self.apply_link();
                        return Some(true);
                    }
                    "Escape" => return Some(self.link_escape()),
                    "Backspace" if apple && m.meta => {
                        field.move_to_edge(false, true);
                        field.delete(false, false);
                    }
                    "Backspace" => field.delete(false, word),
                    "Delete" => field.delete(true, word),
                    "ArrowLeft" if apple && m.meta => field.move_to_edge(false, m.shift),
                    "ArrowRight" if apple && m.meta => field.move_to_edge(true, m.shift),
                    "ArrowLeft" => field.move_caret(false, word, m.shift),
                    "ArrowRight" => field.move_caret(true, word, m.shift),
                    "Home" | "ArrowUp" => field.move_to_edge(false, m.shift),
                    "End" | "ArrowDown" => field.move_to_edge(true, m.shift),
                    "a" | "A" if command => field.select_all(),
                    // Copy, cut and paste: the platform's clipboard events
                    // (which reach the field) do them.
                    "c" | "C" | "x" | "X" | "v" | "V" if command => return Some(false),
                    // Undo, formatting and the like would act on the
                    // document behind the field.
                    _ if command => return Some(true),
                    _ => return Some(false),
                }
            }
            FieldInput::Insert(text) if text == "\n" || text == "\r" => {
                self.apply_link();
                return Some(true);
            }
            FieldInput::Insert(text) | FieldInput::Commit(text) => field.insert(text),
            FieldInput::SetComposing(text) => field.set_composing(text),
            FieldInput::CancelComposition => field.set_composing(""),
            FieldInput::FinishComposition => field.finish_composition(),
            FieldInput::SetSelection(anchor, head) => field.set_selection(anchor, head),
            FieldInput::ComposingRegion(from, to) => field.set_composing_region(from, to),
            FieldInput::DeleteSurrounding(before, after) => {
                field.delete_surrounding(before, after);
            }
            FieldInput::Replace(from, to, text, composing) => {
                field.replace(from, to, text, composing);
            }
            FieldInput::Delete(forward, granularity) => match granularity {
                Granularity::Grapheme => field.delete(forward, false),
                Granularity::Word => field.delete(forward, true),
                Granularity::Line | Granularity::Document => {
                    field.move_to_edge(forward, true);
                    field.delete(forward, false);
                }
            },
        }
        self.after_field_change();
        Some(true)
    }

    /// Tab moves between the field and the popover's buttons (Shift+Tab
    /// back), and a focused button takes Enter and Space; `None` when the
    /// key is the field's.
    fn link_tab_key(&mut self, event: &KeyEvent) -> Option<bool> {
        let count = self.link.actions.len();
        let focused = self.link.toolbar.focused;
        let m = event.modifiers;
        if m.ctrl || m.meta || m.alt {
            return None;
        }
        let next = match (event.key.as_str(), focused) {
            ("Tab", None) if m.shift => count.checked_sub(1),
            ("Tab", None) => (count > 0).then_some(0),
            ("Tab", Some(i)) if m.shift => i.checked_sub(1),
            ("Tab", Some(i)) => (i + 1 < count).then_some(i + 1),
            ("Enter" | " ", Some(i)) => {
                self.link.toolbar.focused = None;
                self.press_link_part(Part::Button(i));
                self.request_redraw();
                return Some(true);
            }
            ("Escape", Some(_)) => {
                self.link.toolbar.focused = None;
                self.request_redraw();
                return Some(true);
            }
            (_, Some(_)) => {
                // Other keys edit the address again.
                self.link.toolbar.focused = None;
                self.request_redraw();
                return None;
            }
            _ => return None,
        };
        self.link.toolbar.focused = next;
        self.request_redraw();
        Some(true)
    }

    /// Keeps the field's caret in view and the IME told where it is.
    fn after_field_change(&mut self) {
        self.reset_blink();
        self.request_redraw();
        self.update_link();
        let Some(text) = self
            .link
            .editing
            .as_ref()
            .map(|e| e.field.text().to_owned())
        else {
            return;
        };
        let layout = self.field_layout(&text);
        let rect = self.link.field_rect;
        if let Some(editing) = &mut self.link.editing {
            editing.field.reveal_caret(rect, &layout);
            let caret = editing.field.caret_rect(rect, &layout);
            self.push_effect(Effect::ImeCursorArea(Rect::new(
                caret.left,
                caret.top,
                caret.width(),
                caret.height(),
            )));
        }
    }

    /// The IME's view of the field.
    pub(crate) fn field_ime_context(&self) -> Option<ImeContext> {
        let field = &self.link.editing.as_ref()?.field;
        Some(ImeContext {
            start: 0,
            text: field.text().to_owned(),
            selection: field.selection(),
            composition: field.composition(),
            field: true,
        })
    }

    /// The field's caret (view coordinates).
    pub(crate) fn field_caret_rect(&self) -> Option<Rect> {
        let field = &self.link.editing.as_ref()?.field;
        let caret = field.caret_rect(self.link.field_rect, &self.field_layout(field.text()));
        Some(Rect::new(
            caret.left,
            caret.top,
            caret.width(),
            caret.height(),
        ))
    }

    pub(crate) fn field_ime_caret(&self) -> Option<ImeCaret> {
        let rect = self.field_caret_rect()?;
        Some(ImeCaret {
            x: rect.x,
            top: rect.y,
            baseline: rect.y + rect.height * 0.8,
            bottom: rect.bottom(),
        })
    }

    /// Bounds of the field's UTF-16 units `from..to` (view coordinates).
    pub(crate) fn field_character_bounds(
        &self,
        from: usize,
        to: usize,
    ) -> Option<Vec<Option<Rect>>> {
        let field = &self.link.editing.as_ref()?.field;
        let layout = self.field_layout(field.text());
        let caret = self.field_caret_rect()?;
        let origin_x = caret.x - layout.caret(field.selection().1, Affinity::Downstream).x;
        let top = self.link.field_rect.center_y() - layout.height() / 2.0;
        let index = layout.index();
        let len = layout.len();
        Some(
            (from..to)
                .map(|offset| {
                    if offset >= len {
                        return None;
                    }
                    let start = index.snap(offset, Affinity::Upstream);
                    let end = index.next_boundary(start).unwrap_or(len);
                    let r = *layout.selection_rects(start..end).first()?;
                    Some(Rect::new(origin_x + r.x, top + r.y, r.width, r.height))
                })
                .collect(),
        )
    }

    /// The field's selected text, for copy and cut.
    pub(crate) fn field_copy(&mut self, cut: bool) -> Option<Option<ClipboardContent>> {
        let field = &mut self.link.editing.as_mut()?.field;
        let text = field.selected_text();
        if text.is_empty() {
            return Some(None);
        }
        if cut {
            field.delete(false, false);
            self.after_field_change();
        }
        Some(Some(ClipboardContent {
            html: escape_html(&text),
            text,
            json: Value::Null,
        }))
    }

    fn field_offset_at(&self, x: f32) -> Option<usize> {
        let field = &self.link.editing.as_ref()?.field;
        let layout = self.field_layout(field.text());
        Some(field.offset_at(self.link.field_rect, &layout, x))
    }

    /// A press on the popover; true when it took the press. A press
    /// elsewhere closes the field.
    pub(crate) fn link_pointer_down(
        &mut self,
        x: f32,
        y: f32,
        click_count: u32,
        shift: bool,
    ) -> bool {
        let Some(part) = self.link_part_at(x, y) else {
            if self.link.appear.target() && self.link.rect.contains(skia_safe::Point::new(x, y)) {
                return true;
            }
            self.close_link_editor();
            return false;
        };
        self.link.pressed = Some(part);
        if part == Part::Field
            && let Some(offset) = self.field_offset_at(x)
            && let Some(editing) = &mut self.link.editing
        {
            if click_count >= 2 {
                editing.field.select_all();
            } else {
                editing.field.move_to(offset, shift);
            }
            self.set_focused(true);
            self.after_field_change();
        }
        self.request_redraw();
        true
    }

    /// Hover and field drag selection; `Some` when the popover took the move.
    pub(crate) fn link_pointer_move(&mut self, x: f32, y: f32) -> Option<bool> {
        if self.link.pressed == Some(Part::Field) {
            if let Some(offset) = self.field_offset_at(x)
                && let Some(editing) = &mut self.link.editing
            {
                editing.field.move_to(offset, true);
                self.after_field_change();
            }
            return Some(true);
        }
        let inside =
            self.link.appear.target() && self.link.rect.contains(skia_safe::Point::new(x, y));
        if !inside && self.link.pressed.is_none() {
            if self.link.toolbar.hovered.take().is_some() {
                self.request_redraw();
            }
            return None;
        }
        let part = self.link_part_at(x, y);
        let hovered = match part {
            Some(Part::Button(index)) => Some(index),
            _ => None,
        };
        if hovered != self.link.toolbar.hovered {
            self.link.toolbar.hovered = hovered;
            self.request_redraw();
        }
        self.push_effect(Effect::Cursor(match part {
            Some(Part::Field) => CursorIcon::Text,
            Some(_) => CursorIcon::Pointer,
            None => CursorIcon::Default,
        }));
        Some(true)
    }

    /// The release runs the pressed button (or opens the address) when
    /// still on it.
    pub(crate) fn link_pointer_up(&mut self, x: f32, y: f32) -> bool {
        let Some(pressed) = self.link.pressed.take() else {
            return false;
        };
        if self.link_part_at(x, y) == Some(pressed) {
            self.press_link_part(pressed);
        }
        self.request_redraw();
        true
    }

    /// The selection menu over the field (a long press, a double tap).
    fn show_field_menu(&mut self) {
        let Some(editing) = &self.link.editing else {
            return;
        };
        let mut items = Vec::new();
        if !editing.field.range().is_empty() {
            items.extend([crate::MenuItem::Cut, crate::MenuItem::Copy]);
        }
        items.push(crate::MenuItem::Paste);
        if !editing.field.is_empty() {
            items.push(crate::MenuItem::SelectAll);
        }
        let field = self.link.field_rect;
        self.show_menu_at(crate::SelectionMenu {
            rect: Rect::new(field.left, field.top, field.width(), field.height()),
            items,
        });
    }

    /// The field takes the selection menu's choices while it is open.
    pub(crate) fn field_menu_action(
        &mut self,
        item: crate::MenuItem,
        text: Option<&str>,
    ) -> Option<Option<ClipboardContent>> {
        if !self.link_editing() {
            return None;
        }
        Some(match item {
            crate::MenuItem::Copy => self.copy(),
            crate::MenuItem::Cut => self.cut(),
            crate::MenuItem::Paste => {
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    self.paste_text(text);
                }
                None
            }
            crate::MenuItem::SelectAll => {
                if let Some(editing) = &mut self.link.editing {
                    editing.field.select_all();
                }
                self.after_field_change();
                None
            }
        })
    }

    fn press_link_part(&mut self, part: Part) {
        match part {
            Part::Url => self.run_link_action(Action::Open),
            Part::Button(index) => {
                if let Some(&action) = self.link.actions.get(index) {
                    self.run_link_action(action);
                }
            }
            Part::Field => {}
        }
    }

    /// A finger on the popover; true when it took the touch. A tap
    /// elsewhere closes the field.
    pub(crate) fn link_touch(&mut self, phase: TouchPhase, x: f32, y: f32) -> bool {
        match phase {
            TouchPhase::Down => {
                if self.link.appear.target() && self.link.rect.contains(skia_safe::Point::new(x, y))
                {
                    self.link.touch = Some((x, y));
                    self.link.touch_at = self.clock.now_ms();
                    return true;
                }
                // A finger elsewhere may only be scrolling to look: the
                // field closes on a tap, not on the touch.
                self.link.outside = self.link.editing.is_some().then_some((x, y));
                false
            }
            TouchPhase::Move => self.link.touch.is_some(),
            TouchPhase::Up => {
                let Some(start) = self.link.touch.take() else {
                    if let Some(down) = self.link.outside.take()
                        && (x - down.0).hypot(y - down.1) <= self.profile.touch_slop
                    {
                        self.close_link_editor();
                    }
                    return false;
                };
                let slop = self.profile.touch_slop;
                if (x - start.0).hypot(y - start.1) <= slop
                    && let Some(part) = self.link_part_at(x, y)
                {
                    if part == Part::Field {
                        let now = self.clock.now_ms();
                        let held =
                            now.saturating_sub(self.link.touch_at) >= self.profile.long_press_ms;
                        let double = now.saturating_sub(self.link.last_tap) <= 300;
                        self.link.last_tap = if double { 0 } else { now };
                        if let Some(offset) = self.field_offset_at(x)
                            && let Some(editing) = &mut self.link.editing
                        {
                            if double {
                                // A double tap selects the whole address.
                                editing.field.select_all();
                            } else if !held {
                                editing.field.move_to(offset, false);
                            }
                        }
                        self.after_field_change();
                        if held || double {
                            // As in platform fields: Paste, Select all (and
                            // Cut/Copy for selected text).
                            self.show_field_menu();
                        } else {
                            self.hide_menu();
                        }
                        self.push_effect(Effect::ShowKeyboard);
                    } else {
                        self.press_link_part(part);
                    }
                }
                true
            }
            TouchPhase::Cancel => self.link.touch.take().is_some(),
        }
    }
}

fn step(name: &str, args: Value) -> (String, Value) {
    (name.to_owned(), args)
}

/// An address as typed, made absolute: `rs-editor.dev` becomes
/// `https://rs-editor.dev` and `me@example.com` a `mailto:` link.
pub(crate) fn normalize_href(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    // `localhost:3000` and `example.com:8080` are a host and a port, not a
    // scheme: only letters (and + - .) before a colon not followed by a
    // port make one.
    let has_scheme = lower.split_once(':').is_some_and(|(scheme, rest)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            && !scheme.contains('.')
            && scheme != "localhost"
            && !rest.chars().next().is_some_and(|c| c.is_ascii_digit())
    });
    if has_scheme || text.starts_with('#') || text.starts_with('/') || text.starts_with('.') {
        return text.to_owned();
    }
    if text.contains('@') && !text.contains('/') {
        return format!("mailto:{text}");
    }
    let scheme = if lower.starts_with("localhost") {
        "http"
    } else {
        "https"
    };
    format!("{scheme}://{text}")
}

/// `href` if it is safe to open: web, mail and phone addresses, and
/// relative ones. A `javascript:` or `data:` link stored in a document (JSON
/// is not checked as typed links are) never runs.
pub(crate) fn safe_url(href: &str) -> Option<String> {
    let href = href.trim();
    let scheme = href
        .split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
        .filter(|scheme| {
            scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        });
    match scheme.as_deref() {
        None => (!href.is_empty()).then(|| href.to_owned()),
        Some("http" | "https" | "mailto" | "tel") => Some(href.to_owned()),
        Some(_) => None,
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
impl LinkPopover {
    pub(crate) fn copied_shown(&self) -> bool {
        self.copied_shown
    }

    pub(crate) fn focused_button(&self) -> Option<usize> {
        self.toolbar.focused
    }

    pub(crate) fn button_center(&self, index: usize) -> (f32, f32) {
        let rect = self.toolbar.entry_rects()[index];
        (rect.center_x(), rect.center_y())
    }

    pub(crate) fn field_center(&self) -> (f32, f32) {
        (self.field_rect.center_x(), self.field_rect.center_y())
    }

    pub(crate) fn invalid(&self) -> bool {
        self.editing.as_ref().is_some_and(|e| e.field.invalid)
    }
}
