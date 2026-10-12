//! The platform-independent editor view.
//!
//! [`EditorView`] owns the headless [`Editor`], lays the document out with
//! Skia and turns raw platform input (keys, text, IME composition,
//! pointer, scroll) into editing. Platform adapters only forward events,
//! provide a canvas, and act on the [`Effect`]s the view emits, so the
//! editor behaves the same everywhere.

mod input;
mod paint;
mod pointer;

use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use openrich_commands::KeyPlatform;
use openrich_extensions::{Editor, EditorOptions, ExtensionError};
use openrich_layout::{DocumentLayout, LayoutEngine, Rect};
use openrich_model::Node;
use openrich_state::{Clock, EditorState, SystemClock, TextDirection, Transaction, ViewContext};
use openrich_theme::Theme;
use openrich_transform::{Assoc, Mappable};

pub use input::Granularity;
pub use openrich_commands::{KeyEvent, Modifiers};
pub use openrich_layout::Affinity;
pub use pointer::{PointerButton, PointerEvent};

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
    /// Put this content on the clipboard.
    Clipboard {
        text: String,
        json: serde_json::Value,
    },
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
    effects: Vec<Effect>,
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
            focused: false,
            blink_start: clock.now_ms(),
            clock,
            goal_x: None,
            affinity: Affinity::Downstream,
            drag: None,
            composition: None,
            next_composition: 1,
            scroll_seen,
            effects: Vec::new(),
        })
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Mutable access for running commands; the view re-syncs afterwards.
    pub fn editor_mut(&mut self) -> &mut Editor {
        self.request_redraw();
        &mut self.editor
    }

    pub fn state(&self) -> &EditorState {
        self.editor.state()
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
        self.width = width;
        self.height = height;
        self.scale = scale;
        self.clamp_scroll();
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
                let max =
                    (layout.height + self.padding.top + self.padding.bottom - self.height).max(0.0);
                self.scroll_y = (top.y + offset + self.padding.top).clamp(0.0, max);
            }
            self.layout = Some(LaidOut { doc, width, layout });
        }
        self.layout
            .as_ref()
            .map(|current| current.layout.clone())
            .expect("layout was just computed")
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

    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll_to(self.scroll_y + dy);
    }

    pub fn scroll_to(&mut self, y: f32) {
        let before = self.scroll_y;
        self.scroll_y = y;
        self.clamp_scroll();
        if self.scroll_y != before {
            self.request_redraw();
        }
    }

    fn clamp_scroll(&mut self) {
        let max = (self.content_height() - self.height).max(0.0);
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

    /// Milliseconds until the caret blinks next, when the view should
    /// redraw even without input.
    pub fn next_frame_in(&self) -> Option<u64> {
        if !self.focused || !self.editor.selection().empty() {
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
        if self.editor.dispatch(tr).is_err() {
            self.pending_anchor = anchor;
            return false;
        }
        self.pending_anchor = if self.editor.doc().ptr_eq(&before) {
            anchor.filter(|_| self.layout.as_ref().is_some_and(|l| !l.doc.ptr_eq(&before)))
        } else {
            mapped
        };
        self.after_change();
        true
    }

    /// Runs a command with this view as its context.
    pub fn run_command(&mut self, command: &openrich_state::Command) -> bool {
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
        } else if rect.bottom() > self.height - margin {
            self.scroll_by(rect.bottom() - (self.height - margin));
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
