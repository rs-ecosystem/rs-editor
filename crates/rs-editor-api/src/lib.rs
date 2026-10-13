//! The host API every platform shell exposes: content, commands (Tiptap's
//! `chain`/`can`/`isActive`), touch, keys, the IME protocol and the
//! clipboard, over an [`EditorView`].
//!
//! Values that are not numbers cross as JSON strings so that every binding
//! (BoltFFI for Kotlin and Swift, the C ABI for WebAssembly) stays simple.
//! Coordinates are in [`Units`] chosen by the shell: device pixels on
//! Android, CSS pixels on the web. Drawing and accessibility need platform
//! objects and stay in the shells.

use rs_editor_commands::{KeyEvent, KeyPlatform, Modifiers};
use rs_editor_extensions::{Editor, EditorOptions};
use rs_editor_model::fragment_to_json;
use rs_editor_theme::Theme;
use rs_editor_view::{
    CursorIcon, EditorView, Effect, HapticKind, Insets, InteractionProfile, MenuItem,
    PointerButton, PointerEvent, TouchPhase, ViewOptions,
};
use serde_json::{Map, Value, json};

#[cfg(test)]
mod tests;

/// The unit of coordinates the shell passes and receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    /// Logical pixels (CSS pixels, points): what the view works in.
    Logical,
    /// Device pixels: logical pixels times the scale.
    Device,
}

/// How to create an [`EditorApi`].
#[derive(Debug, Clone)]
pub struct ApiOptions {
    /// Size in [`ApiOptions::units`].
    pub width: f32,
    pub height: f32,
    /// Device pixels per logical pixel.
    pub scale: f32,
    pub units: Units,
    pub dark: bool,
    /// A Tiptap JSON document, HTML, or empty for an empty document; with
    /// [`ApiOptions::markdown`], Markdown.
    pub content: String,
    pub markdown: bool,
    pub platform: KeyPlatform,
    /// Whether platform fonts may cover scripts the bundled fonts lack.
    pub system_fonts: bool,
    /// Space around the document, in logical pixels.
    pub padding: Insets,
    pub profile: InteractionProfile,
}

impl Default for ApiOptions {
    fn default() -> Self {
        Self {
            width: 800.0,
            height: 600.0,
            scale: 1.0,
            units: Units::Logical,
            dark: false,
            content: String::new(),
            markdown: false,
            platform: KeyPlatform::Other,
            system_fonts: true,
            padding: Insets {
                top: 24.0,
                right: 20.0,
                bottom: 24.0,
                left: 20.0,
            },
            profile: InteractionProfile::default(),
        }
    }
}

/// The editor as platform shells see it.
#[derive(Debug)]
pub struct EditorApi {
    view: EditorView,
    units: Units,
}

pub fn theme(dark: bool) -> Theme {
    if dark { Theme::dark() } else { Theme::light() }
}

fn menu_item_name(item: MenuItem) -> &'static str {
    match item {
        MenuItem::Cut => "cut",
        MenuItem::Copy => "copy",
        MenuItem::Paste => "paste",
        MenuItem::SelectAll => "selectAll",
    }
}

/// `[{"name": "toggleBold", "args": …}, …]` as chain steps.
fn steps(json: &str) -> Option<Vec<(String, Value)>> {
    let Value::Array(items) = serde_json::from_str(json).ok()? else {
        return None;
    };
    items
        .into_iter()
        .map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            let args = item.get("args").cloned().unwrap_or(Value::Null);
            Some((name, args))
        })
        .collect()
}

/// A document for `content`: Markdown when `markdown`, otherwise Tiptap
/// JSON, or else HTML.
fn initial_doc(content: &str, markdown: bool) -> Option<Value> {
    let content = content.trim();
    if content.is_empty() {
        return None;
    }
    if !markdown && let Ok(json) = serde_json::from_str(content) {
        return Some(json);
    }
    let schema = Editor::new(EditorOptions::default()).ok()?.schema().clone();
    let fragment = if markdown {
        rs_editor_markup::parse_markdown(&schema, content)
    } else {
        rs_editor_markup::parse_fragment(&schema, content)
    };
    (fragment.child_count() > 0)
        .then(|| json!({"type": "doc", "content": fragment_to_json(&fragment)}))
}

fn args(json: &str) -> Value {
    serde_json::from_str(json).unwrap_or(Value::Null)
}

impl EditorApi {
    pub fn new(options: ApiOptions) -> Self {
        let logical = match options.units {
            Units::Logical => 1.0,
            Units::Device => options.scale,
        };
        let view_options = |content: Option<Value>| ViewOptions {
            editor: EditorOptions {
                content,
                platform: options.platform,
                ..Default::default()
            },
            theme: theme(options.dark),
            width: options.width / logical,
            height: options.height / logical,
            scale: options.scale,
            padding: options.padding,
            system_fonts: options.system_fonts,
        };
        let mut view = EditorView::new(view_options(initial_doc(
            &options.content,
            options.markdown,
        )))
        .or_else(|_| EditorView::new(view_options(None)))
        .expect("an empty document is always valid");
        view.set_interaction_profile(options.profile);
        Self {
            view,
            units: options.units,
        }
    }

    pub fn view(&self) -> &EditorView {
        &self.view
    }

    pub fn view_mut(&mut self) -> &mut EditorView {
        &mut self.view
    }

    /// Shell units per logical pixel.
    fn unit(&self) -> f32 {
        match self.units {
            Units::Logical => 1.0,
            Units::Device => self.view.size().2,
        }
    }

    fn set_doc(&mut self, doc: Value) -> bool {
        // `setContent` reads `content` from its arguments when present.
        let args = json!({ "content": doc });
        self.view.run("setContent", &args).unwrap_or(false)
    }

    /// New size in shell units, and the device pixel ratio.
    pub fn resize(&mut self, width: f32, height: f32, scale: f32) {
        let logical = match self.units {
            Units::Logical => 1.0,
            Units::Device => scale,
        };
        self.view.resize(width / logical, height / logical, scale);
    }

    // --- content and commands (the Tiptap API) ------------------------------

    /// The document as Tiptap JSON.
    pub fn get_json(&self) -> String {
        self.view.editor().get_json().to_string()
    }

    /// The document as Tiptap-style HTML.
    pub fn get_html(&self) -> String {
        rs_editor_markup::serialize_fragment(self.view.editor().doc().content())
    }

    /// The document as GitHub-flavored Markdown.
    pub fn get_markdown(&self) -> String {
        rs_editor_markup::serialize_markdown(self.view.editor().doc().content())
    }

    /// Replaces the document with Markdown.
    pub fn set_markdown(&mut self, markdown: &str) -> bool {
        let schema = self.view.editor().schema().clone();
        let fragment = rs_editor_markup::parse_markdown(&schema, markdown);
        self.set_fragment(&fragment)
    }

    /// Adds a font (file bytes) for characters the bundled fonts lack; see
    /// the `missingCharacters` effect. Returns its family, or `""` for bytes
    /// that are not a font.
    pub fn add_font(&mut self, bytes: &[u8], family: &str) -> String {
        let family = (!family.is_empty()).then_some(family);
        self.view.add_font(bytes, family).unwrap_or_default()
    }

    /// Draws emoji from images the host supplies (`missingEmoji` effect,
    /// [`EditorApi::add_emoji_image`]) instead of a font.
    pub fn set_emoji_images(&mut self, on: bool) {
        self.view.set_emoji_images(on);
    }

    /// The image for emoji `grapheme`: unpremultiplied RGBA pixels.
    pub fn add_emoji_image(
        &mut self,
        grapheme: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> bool {
        self.view.add_emoji_image(grapheme, rgba, width, height)
    }

    /// Whether pasted plain text is read as Markdown (off by default).
    pub fn set_markdown_paste(&mut self, markdown: bool) {
        self.view.set_markdown_paste(markdown);
    }

    /// Plain text, blocks separated by `\n`.
    pub fn get_text(&self) -> String {
        self.view.editor().get_text("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.view.editor().is_empty()
    }

    /// Replaces the document with Tiptap JSON; false when it does not fit
    /// the schema.
    pub fn set_json(&mut self, json: &str) -> bool {
        serde_json::from_str::<Value>(json).is_ok_and(|doc| self.set_doc(doc))
    }

    /// Replaces the document with HTML, parsed as a paste would be.
    pub fn set_html(&mut self, html: &str) -> bool {
        let schema = self.view.editor().schema().clone();
        let fragment = rs_editor_markup::parse_fragment(&schema, html);
        self.set_fragment(&fragment)
    }

    fn set_fragment(&mut self, fragment: &rs_editor_model::Fragment) -> bool {
        let mut content = fragment_to_json(fragment);
        if content.as_array().is_none_or(Vec::is_empty) {
            content = json!([{"type": "paragraph"}]);
        }
        self.set_doc(json!({"type": "doc", "content": content}))
    }

    pub fn is_editable(&self) -> bool {
        self.view.editor().is_editable()
    }

    pub fn set_editable(&mut self, editable: bool) {
        self.view.editor_mut().set_editable(editable);
    }

    /// Runs a named command (`toggleBold`, `toggleHeading` with
    /// `{"level": 2}`, …); false when it did not apply or does not exist.
    pub fn run(&mut self, name: &str, args_json: &str) -> bool {
        self.view.run(name, &args(args_json)).unwrap_or(false)
    }

    pub fn can(&mut self, name: &str, args_json: &str) -> bool {
        self.view
            .can_chain(&[(name.to_string(), args(args_json))])
            .unwrap_or(false)
    }

    /// Runs `[{"name", "args"}, …]` as one transaction; true only if every
    /// command applied.
    pub fn run_chain(&mut self, steps_json: &str) -> bool {
        steps(steps_json).is_some_and(|steps| self.view.run_chain(&steps).unwrap_or(false))
    }

    pub fn can_chain(&mut self, steps_json: &str) -> bool {
        steps(steps_json).is_some_and(|steps| self.view.can_chain(&steps).unwrap_or(false))
    }

    /// Whether the mark or node `name` (with `attrs_json`, `{}` for any)
    /// is active at the selection.
    pub fn is_active(&self, name: &str, attrs_json: &str) -> bool {
        let attrs = match args(attrs_json) {
            Value::Object(attrs) => attrs,
            _ => Map::new(),
        };
        self.view.editor().is_active(name, &attrs)
    }

    /// Attributes of the mark or node `name` at the selection, as JSON.
    pub fn get_attributes(&self, name: &str) -> String {
        Value::Object(self.view.editor().get_attributes(name)).to_string()
    }

    /// The selection as JSON: `{anchor, head, from, to, empty}` (document
    /// positions).
    pub fn selection(&self) -> String {
        let selection = self.view.editor().selection();
        json!({
            "anchor": selection.anchor(),
            "head": selection.head(),
            "from": selection.from(),
            "to": selection.to(),
            "empty": selection.empty(),
        })
        .to_string()
    }

    /// Changes whenever the document or selection may have changed.
    pub fn revision(&self) -> u64 {
        self.view.revision()
    }

    /// Changes whenever the document may have changed.
    pub fn doc_revision(&self) -> u64 {
        self.view.doc_revision()
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.view.set_theme(theme(dark));
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.view.set_focused(focused);
    }

    /// Height at the bottom covered by a soft keyboard (and anything the
    /// app shows over the editor); the caret scrolls out from under it.
    pub fn set_obscured_bottom(&mut self, height: f32) {
        let height = height / self.unit();
        self.view.set_obscured_bottom(height);
    }

    /// Milliseconds until the next frame is needed (0 while animating), or
    /// -1 when idle.
    pub fn next_frame_in(&self) -> i64 {
        self.view
            .next_frame_in()
            .map_or(-1, |ms| i64::try_from(ms).unwrap_or(i64::MAX))
    }

    // --- pointer, touch and scrolling ---------------------------------------

    /// A touch event; `action` is 0 down, 1 move, 2 up, 3 cancel. Results
    /// (keyboard, menu, haptics) come through [`EditorApi::take_effects`].
    pub fn touch(&mut self, action: i32, x: f32, y: f32) {
        let unit = self.unit();
        let phase = match action {
            0 => TouchPhase::Down,
            1 => TouchPhase::Move,
            2 => TouchPhase::Up,
            _ => TouchPhase::Cancel,
        };
        self.view.touch(phase, x / unit, y / unit);
    }

    /// A mouse event; `kind` is 0 down, 1 move, 2 up. `modifiers` has bits
    /// 1 shift, 2 ctrl, 4 alt, 8 meta. Returns whether the view used it.
    pub fn pointer(&mut self, kind: i32, x: f32, y: f32, clicks: u32, modifiers: u32) -> bool {
        let unit = self.unit();
        let event = PointerEvent {
            x: x / unit,
            y: y / unit,
            button: PointerButton::Primary,
            modifiers: modifier_bits(modifiers),
            click_count: clicks.max(1),
        };
        match kind {
            0 => self.view.pointer_down(event),
            1 => self.view.pointer_move(event),
            _ => self.view.pointer_up(event),
        }
    }

    /// Scrolls by `dy`; `smooth` animates (mouse wheels), otherwise the
    /// offset moves at once (trackpads bring their own momentum).
    pub fn wheel(&mut self, dy: f32, smooth: bool) {
        let dy = dy / self.unit();
        if smooth {
            self.view.scroll_smooth_by(dy);
        } else {
            self.view.scroll_by(dy);
        }
    }

    /// Effects since the last call, as a JSON array of
    /// `{type: haptic|menu|magnifier|showKeyboard|clipboard|openUrl|cursor|imeArea|redraw|missingCharacters, …}`.
    /// Rectangles are `[left, top, right, bottom]`.
    pub fn take_effects(&mut self) -> String {
        let unit = self.unit();
        let rect = |r: rs_editor_view::Rect| {
            json!([r.x * unit, r.y * unit, r.right() * unit, r.bottom() * unit])
        };
        let effects: Vec<Value> = self
            .view
            .take_effects()
            .into_iter()
            .map(|effect| match effect {
                Effect::Redraw => json!({"type": "redraw"}),
                Effect::Haptic(kind) => json!({"type": "haptic", "kind": match kind {
                    HapticKind::LongPress => "longPress",
                    HapticKind::TextHandleMove => "textHandleMove",
                }}),
                Effect::SelectionMenu(menu) => json!({"type": "menu", "menu": menu.map(|m| json!({
                    "rect": rect(m.rect),
                    "items": m.items.iter().map(|item| menu_item_name(*item)).collect::<Vec<_>>(),
                }))}),
                Effect::Magnifier(at) => {
                    json!({"type": "magnifier", "at": at.map(|(x, y)| [x * unit, y * unit])})
                }
                Effect::ShowKeyboard => json!({"type": "showKeyboard"}),
                Effect::MissingCharacters(characters) => {
                    json!({"type": "missingCharacters", "characters": characters})
                }
                Effect::MissingEmoji(emoji) => json!({"type": "missingEmoji", "emoji": emoji}),
                Effect::Clipboard(content) => {
                    json!({"type": "clipboard", "text": content.text, "html": content.html})
                }
                Effect::OpenUrl(url) => json!({"type": "openUrl", "url": url}),
                Effect::Cursor(icon) => json!({"type": "cursor", "cursor": match icon {
                    CursorIcon::Text => "text",
                    CursorIcon::Pointer => "pointer",
                    CursorIcon::Default => "default",
                }}),
                Effect::ImeCursorArea(r) => json!({"type": "imeArea", "rect": rect(r)}),
            })
            .collect();
        Value::Array(effects).to_string()
    }

    /// A command chosen from the selection menu (`cut`, `copy`, `paste`,
    /// `selectAll`). For paste, the clipboard's text and HTML (HTML wins).
    /// Returns `{text, html}` JSON to put on the clipboard, or `""`.
    pub fn menu_action(&mut self, item: &str, text: Option<&str>, html: Option<&str>) -> String {
        let item = match item {
            "cut" => MenuItem::Cut,
            "copy" => MenuItem::Copy,
            "paste" => MenuItem::Paste,
            _ => MenuItem::SelectAll,
        };
        self.view
            .menu_action(item, text, html)
            .map(|content| json!({"text": content.text, "html": content.html}).to_string())
            .unwrap_or_default()
    }

    /// The platform closed the selection menu (back button, a tap elsewhere).
    pub fn menu_dismissed(&mut self) {
        self.view.selection_menu_dismissed();
    }

    // --- keys and the input method ------------------------------------------

    /// A key as a W3C key name (`Backspace`, `ArrowLeft`, `z`), with the
    /// physical `code` when known and modifier bits as in
    /// [`EditorApi::pointer`]. Returns whether the editor used it.
    pub fn key_down(&mut self, key: &str, code: &str, modifiers: u32) -> bool {
        let event = KeyEvent {
            key: key.to_string(),
            code: (!code.is_empty()).then(|| code.to_string()),
            modifiers: modifier_bits(modifiers),
        };
        self.view.key_down(&event)
    }

    /// Typed text outside a composition.
    pub fn insert_text(&mut self, text: &str) -> bool {
        self.view.insert_text(text)
    }

    /// Replaces the composition (or selection) with `text`; newlines split
    /// blocks like Enter (Android's `commitText`).
    pub fn commit_text(&mut self, text: &str) {
        let view = &mut self.view;
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                view.key_down(&KeyEvent::new("Enter", Modifiers::default()));
            }
            if view.composition_range().is_some() {
                view.commit_composition(line);
            } else if !line.is_empty() {
                view.insert_text(line);
            }
        }
    }

    /// Sets the composition text.
    pub fn set_composing_text(&mut self, text: &str) {
        self.view.set_composition(text);
    }

    /// Commits the composition with `text`.
    pub fn commit_composition(&mut self, text: &str) {
        self.view.commit_composition(text);
    }

    /// Makes `start..end` (UTF-16, within the IME block) the composition.
    pub fn set_composing_region(&mut self, start: i32, end: i32) {
        self.view
            .ime_set_composing_region(start.max(0) as usize, end.max(0) as usize);
    }

    /// Ends the composition, keeping its text.
    pub fn finish_composing(&mut self) {
        self.view.ime_finish_composition();
    }

    /// Deletes UTF-16 units around the selection.
    pub fn delete_surrounding(&mut self, before: i32, after: i32) {
        self.view
            .ime_delete_surrounding(before.max(0) as usize, after.max(0) as usize);
    }

    /// Sets the selection, in UTF-16 offsets within the IME block.
    pub fn set_selection(&mut self, anchor: i32, head: i32) {
        self.view
            .ime_set_selection(anchor.max(0) as usize, head.max(0) as usize);
    }

    /// Replaces `start..end` of the IME block with `text`; `composing`
    /// makes it the composition (EditContext `textupdate`).
    pub fn replace_text(&mut self, start: i32, end: i32, text: &str, composing: bool) -> bool {
        self.view
            .ime_replace(start.max(0) as usize, end.max(0) as usize, text, composing)
    }

    /// The IME block as JSON: `{start, text, selection, composition,
    /// caret}`; the caret is `[x, y, width, height]`.
    pub fn ime_state(&mut self) -> String {
        let unit = self.unit();
        let caret = self
            .view
            .caret_rect()
            .map(|r| [r.x * unit, r.y * unit, r.width * unit, r.height * unit]);
        let state = match self.view.ime_context() {
            Some(context) => json!({
                "start": context.start,
                "text": context.text,
                "selection": [context.selection.0, context.selection.1],
                "composition": context.composition.map(|(a, b)| [a, b]),
                "caret": caret,
            }),
            None => {
                json!({"start": -1, "text": "", "selection": [0, 0], "composition": null, "caret": caret})
            }
        };
        state.to_string()
    }

    /// Geometry for IME popups: the caret line (`[x, top, baseline,
    /// bottom]`) and the bounds of each UTF-16 unit in `from..to` of the
    /// IME block (`[left, top, right, bottom]`, `null` where not laid out).
    pub fn ime_cursor_info(&mut self, from: i32, to: i32) -> String {
        let unit = self.unit();
        let view = &mut self.view;
        let caret = view
            .ime_caret()
            .map(|c| [c.x * unit, c.top * unit, c.baseline * unit, c.bottom * unit]);
        let bounds: Vec<Value> = view
            .ime_character_bounds(from.max(0) as usize, to.max(from).max(0) as usize)
            .into_iter()
            .map(|rect| {
                rect.map_or(Value::Null, |r| {
                    json!([r.x * unit, r.y * unit, r.right() * unit, r.bottom() * unit])
                })
            })
            .collect();
        json!({"caret": caret, "bounds": bounds}).to_string()
    }

    // --- accessibility ------------------------------------------------------

    /// The accessibility tree (the one AccessKit adapters get) as nested
    /// JSON for hosts that mirror it into their own UI tree, such as an ARIA
    /// DOM on the web: `{id, role, bounds: [x, y, width, height], children,
    /// text?, label?, level?, checked?}` with bounds in shell units, plus
    /// `selection: {anchor: [id, index], focus: [id, index]}` in text runs.
    pub fn accessibility_json(&mut self) -> String {
        use rs_editor_view::accesskit::{Affine, Node, NodeId, Toggled};
        use std::collections::HashMap;

        let update = self.view.accessibility_tree();
        let nodes: HashMap<NodeId, Node> = update.nodes.into_iter().collect();
        // The editor node scales logical pixels to device pixels; shells in
        // logical units do without it.
        let base = match self.units {
            Units::Logical => Affine::IDENTITY,
            Units::Device => Affine::scale(f64::from(self.view.size().2)),
        };
        fn walk(
            nodes: &HashMap<NodeId, Node>,
            id: NodeId,
            parent: Affine,
            root: bool,
        ) -> Option<Value> {
            let node = nodes.get(&id)?;
            let transform = if root {
                parent
            } else {
                node.transform().map_or(parent, |t| parent * *t)
            };
            let mut out = Map::new();
            out.insert("id".into(), json!(id.0.to_string()));
            out.insert("role".into(), json!(format!("{:?}", node.role())));
            if let Some(bounds) = node.bounds() {
                let r = transform.transform_rect_bbox(bounds);
                out.insert(
                    "bounds".into(),
                    json!([r.x0, r.y0, r.x1 - r.x0, r.y1 - r.y0]),
                );
            }
            if let Some(text) = node.value() {
                out.insert("text".into(), json!(text));
            }
            if let Some(label) = node.label() {
                out.insert("label".into(), json!(label));
            }
            if let Some(level) = node.level() {
                out.insert("level".into(), json!(level));
            }
            if let Some(toggled) = node.toggled() {
                out.insert("checked".into(), json!(toggled == Toggled::True));
            }
            let children: Vec<Value> = node
                .children()
                .iter()
                .filter_map(|&child| walk(nodes, child, transform, false))
                .collect();
            if !children.is_empty() {
                out.insert("children".into(), Value::Array(children));
            }
            Some(Value::Object(out))
        }
        let root = rs_editor_view::EDITOR_NODE;
        let mut tree = walk(&nodes, root, base, true).unwrap_or(Value::Null);
        if let Some(selection) = nodes.get(&root).and_then(|n| n.text_selection())
            && let Value::Object(object) = &mut tree
        {
            let position = |p: rs_editor_view::accesskit::TextPosition| {
                json!([p.node.0.to_string(), p.character_index])
            };
            object.insert(
                "selection".into(),
                json!({"anchor": position(selection.anchor), "focus": position(selection.focus)}),
            );
        }
        tree.to_string()
    }

    /// An accessibility action on node `id` (from [`EditorApi::accessibility_json`]):
    /// `click` (toggles a checkbox) or `focus`. Returns whether it applied.
    pub fn accessibility_action(&mut self, id: &str, action: &str) -> bool {
        use rs_editor_view::accesskit::{Action, ActionRequest, NodeId, TreeId};
        let Ok(id) = id.parse::<u64>() else {
            return false;
        };
        let action = match action {
            "click" => Action::Click,
            "focus" => Action::Focus,
            _ => return false,
        };
        self.view.accessibility_action(&ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: NodeId(id),
            data: None,
        })
    }

    // --- clipboard -----------------------------------------------------------

    /// Copies or cuts the selection; `{text, html}` JSON (empty strings
    /// when nothing is selected).
    pub fn copy(&mut self, cut: bool) -> String {
        let copied = if cut {
            self.view.cut()
        } else {
            self.view.copy()
        };
        match copied {
            Some(content) => json!({"text": content.text, "html": content.html}),
            None => json!({"text": "", "html": ""}),
        }
        .to_string()
    }

    /// Pastes plain text; each line becomes a paragraph.
    pub fn paste(&mut self, text: &str) -> bool {
        self.view.paste_text(text)
    }

    /// Pastes clipboard HTML; false when it held nothing usable.
    pub fn paste_html(&mut self, html: &str) -> bool {
        self.view.paste_html(html)
    }
}

fn modifier_bits(bits: u32) -> Modifiers {
    Modifiers {
        shift: bits & 1 != 0,
        ctrl: bits & 2 != 0,
        alt: bits & 4 != 0,
        meta: bits & 8 != 0,
    }
}
