//! The platform-independent editor view.
//!
//! [`EditorView`] owns the headless [`Editor`], lays the document out with
//! Skia and turns raw platform input (keys, text, IME composition,
//! pointer, scroll) into editing. Platform adapters only forward events,
//! provide a canvas, and act on the [`Effect`]s the view emits, so the
//! editor behaves the same everywhere.

mod a11y;
mod blocks;
mod bubble;
mod ime;
mod input;
mod link;
mod paint;
mod pointer;
mod scroll;
mod slash;
mod strings;
mod touch;

use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use rs_editor_commands::KeyPlatform;
use rs_editor_extensions::{Editor, EditorOptions, ExtensionError};
use rs_editor_layout::{DocumentLayout, LayoutEngine};
use rs_editor_model::Node;
use rs_editor_state::{Clock, EditorState, SystemClock, TextDirection, Transaction, ViewContext};
use rs_editor_theme::Theme;
use rs_editor_transform::{Assoc, Mappable};

pub use a11y::EDITOR_NODE;
pub use accesskit;
pub use ime::{ImeCaret, ImeContext};
pub use input::Granularity;
pub use pointer::{PointerButton, PointerEvent};
pub use rs_editor_commands::{KeyEvent, Modifiers};
pub use rs_editor_layout::{Affinity, Rect};
pub use scroll::{FRAME_MS, VelocityTracker};
pub use touch::{
    Handle, HandleKind, HandleStyle, HapticKind, InteractionProfile, LongPressAction, MenuItem,
    SelectionMenu, TouchPhase,
};

/// Space between the view's edges and the document.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Insets {
    pub const fn uniform(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

/// Pointer cursor shapes the host should show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorIcon {
    Text,
    Pointer,
    Default,
    /// Over the drag handle.
    Grab,
    /// Dragging a block.
    Grabbing,
}

/// Requests from the view to the platform adapter.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Draw a new frame.
    Redraw,
    /// The caret moved; position the IME candidate window here (view coordinates).
    ImeCursorArea(Rect),
    /// The user activated a link.
    OpenUrl(String),
    Cursor(CursorIcon),
    /// Put this content on the clipboard: copies the editor makes itself
    /// (the link popover's copy button). Copies the host asks for
    /// ([`EditorView::copy`], [`EditorView::menu_action`]) come back as
    /// return values instead.
    Clipboard(ClipboardContent),
    /// Play haptic feedback.
    Haptic(HapticKind),
    /// Show (or move) the floating selection menu; `None` hides it.
    SelectionMenu(Option<SelectionMenu>),
    /// Show the magnifier centered on this text point (view coordinates);
    /// `None` hides it.
    Magnifier(Option<(f32, f32)>),
    /// A tap on the text: bring up the soft keyboard.
    ShowKeyboard,
    /// Characters laid out that no font could show (Unicode scalar values),
    /// for hosts that load fonts on demand ([`EditorView::add_font`]).
    MissingCharacters(Vec<u32>),
    /// Emoji (graphemes) laid out without an image, when emoji are drawn
    /// from images ([`EditorView::set_emoji_images`]).
    MissingEmoji(Vec<String>),
    /// Image sources laid out without a picture: load each and give it to
    /// [`EditorView::add_image`] (or [`EditorView::image_failed`]).
    MissingImages(Vec<String>),
    /// The user chose to insert an image: show the app's image picker and
    /// insert the result (`setImage`) at the caret. Only with
    /// [`EditorView::set_image_picker`].
    PickImage,
}

/// Copied content in the formats the clipboard takes.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardContent {
    /// Plain text; blocks are separated by newlines.
    pub text: String,
    /// HTML for other apps; pasting it back into Rs Editor is exact.
    pub html: String,
    /// The copied slice as JSON (`{content, openStart, openEnd}`).
    pub json: serde_json::Value,
}

/// Options for [`EditorView::new`].
pub struct ViewOptions {
    pub editor: EditorOptions,
    pub theme: Theme,
    /// Logical size of the view.
    pub width: f32,
    pub height: f32,
    /// Device pixels per logical pixel.
    pub scale: f32,
    pub padding: Insets,
    /// Let platform fonts cover emoji and scripts the bundle lacks.
    pub system_fonts: bool,
}

impl fmt::Debug for ViewOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewOptions")
            .field("editor", &self.editor)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("scale", &self.scale)
            .finish()
    }
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            editor: EditorOptions::default(),
            theme: Theme::light(),
            width: 800.0,
            height: 600.0,
            scale: 1.0,
            padding: Insets::uniform(24.0),
            system_fonts: true,
        }
    }
}

/// An IME composition written into the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Composition {
    pub id: u64,
    pub from: usize,
    pub to: usize,
}

/// The editor view.
pub struct EditorView {
    pub(crate) editor: Editor,
    pub(crate) engine: LayoutEngine,
    layout: Option<LaidOut>,
    /// Top-level block (position) and offset to keep at the top of the
    /// view across the next relayout.
    pending_anchor: Option<(usize, f32)>,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) scale: f32,
    pub(crate) padding: Insets,
    pub(crate) scroll_y: f32,
    /// Height at the bottom covered by something else (a soft keyboard).
    pub(crate) obscured_bottom: f32,
    pub(crate) scroll: scroll::Scroll,
    pub(crate) focused: bool,
    pub(crate) clock: Arc<dyn Clock>,
    /// When the caret last became visible (resets the blink).
    pub(crate) blink_start: u64,
    pub(crate) goal_x: Option<f32>,
    /// Affinity of the selection head (which line a wrap position shows on).
    pub(crate) affinity: Affinity,
    pub(crate) drag: Option<pointer::Drag>,
    pub(crate) composition: Option<Composition>,
    next_composition: u64,
    scroll_seen: u64,
    /// Bumped by every transaction, so hosts can tell when to refresh
    /// toolbar state.
    revision: u64,
    /// Bumped by transactions that change the document.
    doc_revision: u64,
    /// Pasted plain text is read as Markdown.
    pub(crate) markdown_paste: bool,
    effects: Vec<Effect>,
    pub(crate) a11y: a11y::A11y,
    pub(crate) touch: touch::TouchState,
    pub(crate) profile: InteractionProfile,
    pub(crate) bubble: bubble::Bubble,
    pub(crate) blocks: blocks::BlockHandle,
    pub(crate) slash: slash::Slash,
    pub(crate) link: link::LinkPopover,
    /// The UI's words, in the user's language.
    pub(crate) strings: strings::Strings,
    /// The host picks images ([`Effect::PickImage`]).
    pub(crate) image_picker: bool,
    /// Placeholder text set by the app, over the localized default.
    placeholder: Option<String>,
}

impl fmt::Debug for EditorView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditorView")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("scroll_y", &self.scroll_y)
            .field("focused", &self.focused)
            .finish()
    }
}

/// A layout and what it was computed for.
struct LaidOut {
    doc: Node,
    width: f32,
    layout: Rc<DocumentLayout>,
}

/// Caret blink half-period in milliseconds.
pub const BLINK_MS: u64 = 530;

impl EditorView {
    pub fn new(options: ViewOptions) -> Result<Self, ExtensionError> {
        let clock: Arc<dyn Clock> = options
            .editor
            .clock
            .clone()
            .unwrap_or_else(|| Arc::new(SystemClock));
        let mut editor_options = options.editor;
        editor_options.clock = Some(clock.clone());
        let editor = Editor::new(editor_options)?;
        let scroll_seen = editor.state().scroll_to_selection();
        Ok(Self {
            editor,
            engine: LayoutEngine::new(options.theme, options.system_fonts),
            layout: None,
            pending_anchor: None,
            width: options.width,
            height: options.height,
            scale: options.scale,
            padding: options.padding,
            scroll_y: 0.0,
            obscured_bottom: 0.0,
            scroll: scroll::Scroll::Idle,
            focused: false,
            blink_start: clock.now_ms(),
            clock,
            goal_x: None,
            affinity: Affinity::Downstream,
            drag: None,
            composition: None,
            next_composition: 1,
            scroll_seen,
            revision: 0,
            doc_revision: 0,
            markdown_paste: false,
            effects: Vec::new(),
            a11y: a11y::A11y::default(),
            touch: touch::TouchState::default(),
            profile: InteractionProfile::default(),
            bubble: bubble::Bubble::new(),
            blocks: blocks::BlockHandle::new(),
            slash: slash::Slash::new(),
            link: link::LinkPopover::default(),
            strings: strings::strings_for("en"),
            image_picker: false,
            placeholder: None,
        })
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Mutable access for running commands; the view re-syncs afterwards.
    pub fn editor_mut(&mut self) -> &mut Editor {
        self.request_redraw();
        self.revision += 1;
        self.doc_revision += 1;
        &mut self.editor
    }

    /// Changes whenever the document or selection may have changed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Changes whenever the document may have changed (not for selection
    /// moves).
    pub fn doc_revision(&self) -> u64 {
        self.doc_revision
    }

    pub fn state(&self) -> &EditorState {
        self.editor.state()
    }

    /// The language of the editor's own UI (menus, placeholders), as a
    /// BCP 47 tag such as `"id-ID"`; English when there is no translation.
    pub fn set_locale(&mut self, locale: &str) {
        self.strings = strings::strings_for(locale);
        self.request_redraw();
    }

    pub fn theme(&self) -> &Theme {
        self.engine.theme()
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.engine.set_theme(theme);
        self.layout = None;
        self.request_redraw();
    }

    pub fn resize(&mut self, width: f32, height: f32, scale: f32) {
        let follow_caret = self.follows_caret_to(height - self.obscured_bottom);
        self.width = width;
        self.height = height;
        self.scale = scale;
        self.after_viewport_change(follow_caret);
    }

    /// How much of the bottom of the view something covers, such as a soft
    /// keyboard sliding over it. Cheaper than resizing while the keyboard
    /// animates: only scrolling follows, so the caret stays visible.
    pub fn set_obscured_bottom(&mut self, height: f32) {
        let height = height.clamp(0.0, self.height);
        if height == self.obscured_bottom {
            return;
        }
        let follow_caret = self.follows_caret_to(self.height - height);
        self.obscured_bottom = height;
        self.after_viewport_change(follow_caret);
    }

    pub fn obscured_bottom(&self) -> f32 {
        self.obscured_bottom
    }

    /// The height of the part of the view that can be seen.
    pub fn visible_height(&self) -> f32 {
        (self.height - self.obscured_bottom).max(0.0)
    }

    /// When the visible part shrinks under a visible caret (a soft keyboard
    /// opening), the caret should stay on screen like in platform text
    /// fields.
    fn follows_caret_to(&mut self, visible: f32) -> bool {
        let before = self.visible_height();
        self.focused
            && visible < before
            && self
                .caret_rect()
                .is_some_and(|r| r.y >= 0.0 && r.bottom() <= before)
    }

    fn after_viewport_change(&mut self, follow_caret: bool) {
        self.clamp_scroll();
        if follow_caret {
            self.scroll_into_view();
        }
        self.request_redraw();
    }

    pub fn set_padding(&mut self, padding: Insets) {
        self.padding = padding;
        self.request_redraw();
    }

    pub fn size(&self) -> (f32, f32, f32) {
        (self.width, self.height, self.scale)
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn set_focused(&mut self, focused: bool) {
        if self.focused != focused {
            self.focused = focused;
            if !focused {
                self.finish_composition();
            }
            self.reset_blink();
            self.request_redraw();
        }
    }

    /// Width available to the document.
    pub fn content_width(&self) -> f32 {
        (self.width - self.padding.left - self.padding.right).max(40.0)
    }

    /// The current layout. It is recomputed when the document or width
    /// changed, or when the viewport or selection moved past the laid-out
    /// window; the block at the top of the view keeps its place on screen
    /// while heights above it change (scroll anchoring).
    pub fn layout(&mut self) -> Rc<DocumentLayout> {
        // Anchoring moves the viewport, which may need one more pass.
        for _ in 0..4 {
            let width = self.content_width();
            let doc = self.editor.doc().clone();
            let (anchor_pos, head) = {
                let selection = self.editor.selection();
                (selection.anchor(), selection.head())
            };
            let top = self.scroll_y - self.padding.top;
            if let Some(current) = &self.layout
                && current.doc.ptr_eq(&doc)
                && current.width == width
                && current
                    .layout
                    .covers(top - self.height / 2.0, top + self.height * 1.5)
                && current.layout.shows(head)
                && current.layout.shows(anchor_pos)
            {
                return current.layout.clone();
            }
            let anchor = self.pending_anchor.take().or_else(|| {
                self.layout
                    .as_ref()
                    .filter(|current| current.doc.ptr_eq(&doc))
                    .and_then(|current| self.anchor_in(&current.layout))
            });
            let window = (top - self.height, top + self.height * 2.0);
            let layout =
                Rc::new(
                    self.engine
                        .layout_window(&doc, width, Some(window), &[head, anchor_pos]),
                );
            if let Some((pos, offset)) = anchor
                && let Some(top) = layout
                    .top_at_pos(pos)
                    .and_then(|index| layout.tops.get(index))
            {
                let max = (layout.height + self.padding.top + self.padding.bottom
                    - self.visible_height())
                .max(0.0);
                self.scroll_y = (top.y + offset + self.padding.top).clamp(0.0, max);
            }
            self.layout = Some(LaidOut { doc, width, layout });
            let missing = self.engine.take_missing_characters();
            if !missing.is_empty() {
                self.push_effect(Effect::MissingCharacters(missing));
            }
            let emoji = self.engine.take_missing_emoji();
            if !emoji.is_empty() {
                self.push_effect(Effect::MissingEmoji(emoji));
            }
            let images = self.engine.take_missing_images();
            if !images.is_empty() {
                self.push_effect(Effect::MissingImages(images));
            }
        }
        self.layout
            .as_ref()
            .map(|current| current.layout.clone())
            .expect("layout was just computed")
    }

    /// Draws emoji with images from the host instead of a font, for
    /// platforms whose fonts the editor cannot use (the web): emoji without
    /// an image are reported through [`Effect::MissingEmoji`] and drawn
    /// once [`EditorView::add_emoji_image`] supplies one.
    pub fn set_emoji_images(&mut self, on: bool) {
        self.engine.set_emoji_images(on);
        self.layout = None;
        self.request_redraw();
    }

    /// The image for emoji `grapheme`: `width` × `height` pixels of
    /// unpremultiplied RGBA. Returns false for a buffer of the wrong size.
    pub fn add_emoji_image(
        &mut self,
        grapheme: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> bool {
        let row = width as usize * 4;
        if width == 0 || height == 0 || rgba.len() != row * height as usize {
            return false;
        }
        let info = skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let Some(image) =
            skia_safe::images::raster_from_data(&info, skia_safe::Data::new_copy(rgba), row)
        else {
            return false;
        };
        self.engine.add_emoji_image(grapheme, image);
        self.request_redraw();
        true
    }

    /// The picture for image nodes with `src`: `width` × `height` pixels of
    /// RGBA (`premultiplied` or not), `scale` pixels per CSS pixel. Images without
    /// `width` and `height` attributes take its size. Returns false for a
    /// buffer of the wrong size.
    pub fn add_image(
        &mut self,
        src: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
        scale: f32,
        premultiplied: bool,
    ) -> bool {
        let row = width as usize * 4;
        if width == 0 || height == 0 || rgba.len() != row * height as usize {
            return false;
        }
        let info = skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            if premultiplied {
                skia_safe::AlphaType::Premul
            } else {
                skia_safe::AlphaType::Unpremul
            },
            None,
        );
        let Some(image) =
            skia_safe::images::raster_from_data(&info, skia_safe::Data::new_copy(rgba), row)
        else {
            return false;
        };
        self.engine.add_image(src, image, scale);
        self.relayout_keeping_place();
        true
    }

    /// `src` could not be loaded; its image shows as broken.
    pub fn image_failed(&mut self, src: &str) {
        self.engine.image_failed(src);
        self.relayout_keeping_place();
    }

    /// Whether the app shows its own image picker ([`Effect::PickImage`])
    /// when the user inserts an image; otherwise the editor asks for an
    /// address.
    pub fn set_image_picker(&mut self, on: bool) {
        self.image_picker = on;
    }

    /// Text shown in an empty line with the caret (and in an empty
    /// document); `None` restores the default, which hints at the slash
    /// menu in the UI's language.
    pub fn set_placeholder(&mut self, text: Option<String>) {
        self.placeholder = text;
        self.request_redraw();
    }

    pub(crate) fn placeholder_override(&self) -> Option<&str> {
        self.placeholder.as_deref()
    }

    /// Lays out again (fonts or images changed sizes) with the block at the
    /// top of the view staying in place.
    fn relayout_keeping_place(&mut self) {
        self.pending_anchor = self
            .pending_anchor
            .or_else(|| self.layout.as_ref().and_then(|l| self.anchor_in(&l.layout)));
        self.layout = None;
        self.request_redraw();
    }

    /// Adds a font (file bytes) for characters the bundled fonts lack, such
    /// as emoji or CJK; text is laid out again. Returns the font's family,
    /// or `None` for bytes that are not a font.
    pub fn add_font(&mut self, bytes: &[u8], family: Option<&str>) -> Option<String> {
        let family = self.engine.add_font(bytes, family)?;
        self.relayout_keeping_place();
        Some(family)
    }

    /// The top-level block at the top of the view and how far into it the
    /// view starts.
    fn anchor_in(&self, layout: &DocumentLayout) -> Option<(usize, f32)> {
        let top = self.scroll_y - self.padding.top;
        let block = layout.tops.get(layout.top_at_y(top)?)?;
        Some((block.pos, top - block.y))
    }

    /// Height of the scrollable content.
    pub fn content_height(&mut self) -> f32 {
        let padding = self.padding.top + self.padding.bottom;
        self.layout().height + padding
    }

    pub fn scroll_y(&self) -> f32 {
        self.scroll_y
    }

    /// Scrolls by `dy` at once (trackpads, which animate on their own),
    /// stopping any scroll animation.
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll_to(self.scroll_y + dy);
    }

    pub fn scroll_to(&mut self, y: f32) {
        self.stop_scrolling();
        self.scroll_to_inner(y);
    }

    pub(crate) fn scroll_to_inner(&mut self, y: f32) {
        let before = self.scroll_y;
        self.scroll_y = y;
        self.clamp_scroll();
        if self.scroll_y != before {
            self.request_redraw();
        }
    }

    fn clamp_scroll(&mut self) {
        let max = (self.content_height() - self.visible_height()).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max);
    }

    /// Converts view coordinates to document coordinates.
    pub fn to_document(&self, x: f32, y: f32) -> (f32, f32) {
        (x - self.padding.left, y - self.padding.top + self.scroll_y)
    }

    /// Converts document coordinates to view coordinates.
    pub fn to_view(&self, rect: Rect) -> Rect {
        rect.translate(self.padding.left, self.padding.top - self.scroll_y)
    }

    /// The caret (or selection head) rectangle in view coordinates.
    pub fn caret_rect(&mut self) -> Option<Rect> {
        let selection = self.editor.selection().clone();
        let width = self.theme().spacing.caret_width;
        let affinity = self.affinity;
        let rect = self
            .layout()
            .caret_rect(selection.head(), affinity, width)?;
        Some(self.to_view(rect))
    }

    pub(crate) fn reset_blink(&mut self) {
        self.blink_start = self.clock.now_ms();
    }

    pub(crate) fn request_redraw(&mut self) {
        if !self.effects.contains(&Effect::Redraw) {
            self.effects.push(Effect::Redraw);
        }
    }

    pub(crate) fn push_effect(&mut self, effect: Effect) {
        self.effects.push(effect);
    }

    /// Effects produced since the last call.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Milliseconds until the view should draw again without input: the
    /// next animation frame while scrolling, or the next caret blink.
    pub fn next_frame_in(&self) -> Option<u64> {
        if self.is_scrolling()
            || self.bubble_animating()
            || self.slash_animating()
            || self.link_animating()
            || self.block_handle_animating()
        {
            return Some(FRAME_MS);
        }
        let touch = self.touch_deadline();
        let blink = self.blink_deadline();
        match (touch, blink) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn blink_deadline(&self) -> Option<u64> {
        let collapsed = match self.link_field() {
            Some((_, anchor, head)) => anchor == head,
            None => self.editor.selection().empty(),
        };
        if !self.focused || !collapsed {
            return None;
        }
        let elapsed = self.clock.now_ms().saturating_sub(self.blink_start);
        Some(BLINK_MS - elapsed % BLINK_MS)
    }

    pub(crate) fn caret_visible(&self) -> bool {
        let elapsed = self.clock.now_ms().saturating_sub(self.blink_start);
        self.focused && (elapsed / BLINK_MS).is_multiple_of(2)
    }

    /// Applies `tr` and keeps the view in sync (scrolling, blink, IME).
    pub fn dispatch(&mut self, tr: Transaction) -> bool {
        let before = self.editor.doc().clone();
        let anchor = self.pending_anchor.take().or_else(|| {
            self.layout
                .as_ref()
                .filter(|current| current.doc.ptr_eq(&before))
                .and_then(|current| self.anchor_in(&current.layout))
        });
        let mapped = anchor.map(|(pos, offset)| (tr.mapping.map(pos, Assoc::After), offset));
        let mapping = tr.mapping.clone();
        // A plugin may refuse it (a character limit): nothing changed.
        if !matches!(self.editor.dispatch(tr), Ok(applied) if !applied.is_empty()) {
            self.pending_anchor = anchor;
            return false;
        }
        self.pending_anchor = if self.editor.doc().ptr_eq(&before) {
            anchor.filter(|_| self.layout.as_ref().is_some_and(|l| !l.doc.ptr_eq(&before)))
        } else {
            mapped
        };
        self.revision += 1;
        if !self.editor.doc().ptr_eq(&before) {
            self.doc_revision += 1;
            self.map_link(&mapping);
        }
        self.after_change();
        true
    }

    /// Runs a command with this view as its context.
    pub fn run_command(&mut self, command: &rs_editor_state::Command) -> bool {
        let mut produced = Vec::new();
        let applied = {
            let context = ViewQuery::new(self);
            command(
                self.editor.state(),
                Some(&mut |tr| produced.push(tr)),
                Some(&context),
            )
        };
        for tr in produced {
            self.dispatch(tr);
        }
        applied
    }

    /// Runs a named command (`"toggleBold"`, …).
    pub fn run(&mut self, name: &str, args: &serde_json::Value) -> Result<bool, ExtensionError> {
        let command = self.editor.manager().command(name, args)?;
        Ok(self.run_command(&command))
    }

    /// Runs named commands as one transaction, like Tiptap's
    /// `chain()…run()`; `true` only if every command applied.
    pub fn run_chain(
        &mut self,
        steps: &[(String, serde_json::Value)],
    ) -> Result<bool, ExtensionError> {
        let command = self.chain_of(steps)?;
        Ok(self.run_command(&command))
    }

    /// Whether [`EditorView::run_chain`] would apply every command.
    pub fn can_chain(
        &mut self,
        steps: &[(String, serde_json::Value)],
    ) -> Result<bool, ExtensionError> {
        let command = self.chain_of(steps)?;
        let context = ViewQuery::new(self);
        Ok(command(self.editor.state(), None, Some(&context)))
    }

    fn chain_of(
        &self,
        steps: &[(String, serde_json::Value)],
    ) -> Result<rs_editor_state::Command, ExtensionError> {
        let manager = self.editor.manager();
        let commands = steps
            .iter()
            .map(|(name, args)| manager.command(name, args))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rs_editor_extensions::helpers::chain(commands))
    }

    pub(crate) fn after_change(&mut self) {
        self.reset_blink();
        self.request_redraw();
        let scroll = self.editor.state().scroll_to_selection();
        if scroll != self.scroll_seen {
            self.scroll_seen = scroll;
            self.scroll_into_view();
        }
        if let Some(rect) = self.caret_rect() {
            self.push_effect(Effect::ImeCursorArea(rect));
        }
    }

    /// Scrolls so the selection head is visible with a margin.
    pub fn scroll_into_view(&mut self) {
        let Some(rect) = self.caret_rect() else {
            return;
        };
        let margin = 24.0;
        if rect.y < margin {
            self.scroll_by(rect.y - margin);
        } else if rect.bottom() > self.visible_height() - margin {
            self.scroll_by(rect.bottom() - (self.visible_height() - margin));
        }
    }

    pub(crate) fn platform(&self) -> KeyPlatform {
        self.editor.manager().platform()
    }

    pub(crate) fn new_composition_id(&mut self) -> u64 {
        let id = self.next_composition;
        self.next_composition += 1;
        id
    }
}

/// Answers [`ViewContext`] questions from the current layout.
pub(crate) struct ViewQuery {
    layout: Rc<DocumentLayout>,
}

impl ViewQuery {
    pub(crate) fn new(view: &mut EditorView) -> Self {
        Self {
            layout: view.layout(),
        }
    }
}

impl ViewContext for ViewQuery {
    fn end_of_textblock(&self, dir: TextDirection, state: &EditorState) -> bool {
        let selection = state.selection();
        let head = selection.head_pos();
        if !head.parent().is_textblock() {
            return true;
        }
        let offset = head.parent_offset();
        let len = head.parent().content().size();
        match dir {
            TextDirection::Backward => offset == 0,
            TextDirection::Forward => offset == len,
            TextDirection::Up
            | TextDirection::Down
            | TextDirection::Left
            | TextDirection::Right => {
                let Some(text) = self.layout.text_at(head.pos()) else {
                    return true;
                };
                let line = text.layout.line_index(offset, Affinity::Downstream);
                let lines = text.layout.lines().len();
                match dir {
                    TextDirection::Up => line == 0,
                    TextDirection::Down => line + 1 == lines,
                    TextDirection::Left => {
                        let stops = text.layout.caret_stops(line);
                        let caret = text.layout.caret(offset, Affinity::Downstream);
                        line == 0 && stops.first().is_none_or(|s| s.x >= caret.x - 0.5)
                    }
                    _ => {
                        let stops = text.layout.caret_stops(line);
                        let caret = text.layout.caret(offset, Affinity::Downstream);
                        line + 1 == lines && stops.last().is_none_or(|s| s.x <= caret.x + 0.5)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
