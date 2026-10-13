//! A one-line text field, like shadcn's Input, for text the editor asks
//! for inside its canvas (a link's address).
//!
//! The field keeps its own text, selection and IME composition in UTF-16
//! units, the offsets platform input methods use, so the view can hand
//! keyboard and IME input to it in place of the document. Text is shaped
//! by the caller ([`rs_editor_layout::LayoutEngine::ui_label`]).

use std::ops::Range;

use rs_editor_layout::TextLayout;
use rs_editor_text::{Affinity, TextIndexMap, next_word_end, prev_word_start};
use rs_editor_theme::Theme;
use skia_safe::{Canvas, Paint, PaintStyle, RRect, Rect};

use crate::sk;

/// Height of the field (shadcn's `h-8`).
pub const FIELD_HEIGHT: f32 = 32.0;
/// Space between the field's edge and its text.
const TEXT_INSET: f32 = 8.0;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextField {
    text: String,
    anchor: usize,
    head: usize,
    composition: Option<(usize, usize)>,
    /// Shown with the destructive color (the text was refused).
    pub invalid: bool,
    /// How far the text is scrolled left to keep the caret in view.
    scroll_x: f32,
}

impl TextField {
    /// A field holding `text`, all of it selected.
    pub fn new(text: &str) -> Self {
        let mut field = Self {
            text: text.to_owned(),
            ..Self::default()
        };
        field.select_all();
        field
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Length in UTF-16 units.
    pub fn len(&self) -> usize {
        self.text.encode_utf16().count()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Anchor and head.
    pub fn selection(&self) -> (usize, usize) {
        (self.anchor, self.head)
    }

    /// The selection as an ordered range.
    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn composition(&self) -> Option<(usize, usize)> {
        self.composition
    }

    pub fn selected_text(&self) -> String {
        let range = self.range();
        self.slice(range.start, range.end)
    }

    /// The text between two UTF-16 offsets.
    pub fn slice(&self, from: usize, to: usize) -> String {
        let units: Vec<u16> = self.text.encode_utf16().collect();
        let (from, to) = (from.min(units.len()), to.min(units.len()));
        String::from_utf16_lossy(&units[from.min(to)..to])
    }

    fn index(&self) -> TextIndexMap {
        TextIndexMap::from_text(&self.text)
    }

    /// `offset` moved off a surrogate pair's second half and inside the text.
    fn clamp(&self, offset: usize) -> usize {
        let units: Vec<u16> = self.text.encode_utf16().collect();
        let mut offset = offset.min(units.len());
        if offset > 0 && offset < units.len() && (0xDC00..0xE000).contains(&units[offset]) {
            offset -= 1;
        }
        offset
    }

    /// Replaces `from..to` with `text`; the caret ends after it.
    fn splice(&mut self, from: usize, to: usize, text: &str) {
        let (from, to) = (self.clamp(from.min(to)), self.clamp(from.max(to)));
        let mut units: Vec<u16> = self.text.encode_utf16().collect();
        units.splice(from..to, text.encode_utf16());
        self.text = String::from_utf16_lossy(&units);
        let caret = from + text.encode_utf16().count();
        self.anchor = caret;
        self.head = caret;
        self.invalid = false;
    }

    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        self.anchor = self.clamp(anchor);
        self.head = self.clamp(head);
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.head = self.len();
    }

    /// Typed or pasted text, replacing the composition or the selection.
    pub fn insert(&mut self, text: &str) {
        let range = match self.composition.take() {
            Some((from, to)) => from..to,
            None => self.range(),
        };
        // One line: line breaks become spaces.
        let text = text.replace(['\n', '\r'], " ");
        self.splice(range.start, range.end, &text);
    }

    /// The IME's uncommitted text, replacing the composition (or the
    /// selection when none is going on).
    pub fn set_composing(&mut self, text: &str) {
        let range = match self.composition {
            Some((from, to)) => from..to,
            None => self.range(),
        };
        self.splice(range.start, range.end, text);
        self.composition = (!text.is_empty()).then_some((range.start, self.head));
    }

    /// Ends the composition, keeping its text.
    pub fn finish_composition(&mut self) {
        self.composition = None;
    }

    /// Marks existing text as the composition.
    pub fn set_composing_region(&mut self, from: usize, to: usize) {
        let (from, to) = (self.clamp(from.min(to)), self.clamp(from.max(to)));
        self.composition = (from < to).then_some((from, to));
    }

    /// Replaces `from..to` with `text`; while `composing`, the new text is
    /// the composition (EditContext `textupdate`).
    pub fn replace(&mut self, from: usize, to: usize, text: &str, composing: bool) {
        let from = self.clamp(from.min(to));
        self.splice(from, to.max(from), text);
        self.composition = (composing && !text.is_empty()).then_some((from, self.head));
    }

    /// Deletes `before` units before the selection and `after` units
    /// after it, keeping whole graphemes.
    pub fn delete_surrounding(&mut self, before: usize, after: usize) {
        let index = self.index();
        let range = self.range();
        let end = index.snap((range.end + after).min(self.len()), Affinity::Downstream);
        let start = index.snap(range.start.saturating_sub(before), Affinity::Upstream);
        self.splice(range.end, end, "");
        self.splice(start, range.start, "");
        self.composition = None;
    }

    /// Backspace (`forward` false) or Delete: the selection, or one
    /// grapheme or word next to the caret.
    pub fn delete(&mut self, forward: bool, word: bool) {
        self.composition = None;
        let range = self.range();
        if !range.is_empty() {
            self.splice(range.start, range.end, "");
            return;
        }
        let caret = self.head;
        let target = self.step(caret, forward, word);
        self.splice(caret.min(target), caret.max(target), "");
    }

    fn step(&self, from: usize, forward: bool, word: bool) -> usize {
        let index = self.index();
        match (forward, word) {
            (true, true) => next_word_end(&self.text, from),
            (false, true) => prev_word_start(&self.text, from),
            (true, false) => index.next_boundary(from).unwrap_or(self.len()),
            (false, false) => index.prev_boundary(from).unwrap_or(0),
        }
    }

    /// Arrow keys: one grapheme or word, collapsing a selection first.
    pub fn move_caret(&mut self, forward: bool, word: bool, extend: bool) {
        self.composition = None;
        let range = self.range();
        let target = if !extend && !range.is_empty() && !word {
            if forward { range.end } else { range.start }
        } else {
            self.step(self.head, forward, word)
        };
        self.move_to(target, extend);
    }

    /// Home and End.
    pub fn move_to_edge(&mut self, end: bool, extend: bool) {
        self.composition = None;
        self.move_to(if end { self.len() } else { 0 }, extend);
    }

    pub fn move_to(&mut self, offset: usize, extend: bool) {
        self.head = self.clamp(offset);
        if !extend {
            self.anchor = self.head;
        }
    }

    /// The inner rectangle the text shows in, for a field filling `rect`.
    pub fn text_rect(rect: Rect) -> Rect {
        rect.with_inset((TEXT_INSET, 0.0))
    }

    /// The text offset at view `x`, for a field filling `rect` with text
    /// laid out as `layout`.
    pub fn offset_at(&self, rect: Rect, layout: &TextLayout, x: f32) -> usize {
        let local = x - Self::text_rect(rect).left + self.scroll_x;
        let (offset, _) = layout.offset_at(local, layout.height() / 2.0);
        offset
    }

    /// Where the caret is drawn, for IME popups (view coordinates).
    pub fn caret_rect(&self, rect: Rect, layout: &TextLayout) -> Rect {
        let caret = layout.caret(self.head, Affinity::Downstream);
        let inner = Self::text_rect(rect);
        let top = rect.center_y() - layout.height() / 2.0;
        Rect::from_xywh(
            inner.left + caret.x - self.scroll_x,
            top + caret.y,
            1.0,
            caret.height,
        )
    }

    /// Scrolls the text so the caret shows inside a field filling `rect`.
    pub fn reveal_caret(&mut self, rect: Rect, layout: &TextLayout) {
        let visible = Self::text_rect(rect).width();
        let caret = layout.caret(self.head, Affinity::Downstream).x;
        let mut scroll = self.scroll_x;
        if caret - scroll > visible - 1.0 {
            scroll = caret - visible + 1.0;
        } else if caret < scroll {
            scroll = caret;
        }
        let content = layout.lines().first().map_or(0.0, |l| l.left + l.width);
        scroll = scroll.min((content - visible + 1.0).max(0.0)).max(0.0);
        self.scroll_x = scroll;
    }

    /// Paints the field filling `rect`: its frame (with the focus ring when
    /// `focused`), `layout` (the text) or `placeholder` when empty, the
    /// selection, the composition underline and, when `caret` is true, the
    /// caret.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        canvas: &Canvas,
        theme: &Theme,
        rect: Rect,
        layout: &TextLayout,
        placeholder: Option<&TextLayout>,
        focused: bool,
        caret: bool,
    ) {
        let palette = &theme.palette;
        let radius = theme.radius.md;
        let shape = RRect::new_rect_xy(rect, radius, radius);
        let accent = if self.invalid {
            palette.destructive
        } else if focused {
            palette.ring
        } else {
            palette.input
        };
        // The focus ring: 3 px of the ring color at half strength (or the
        // destructive one, refused).
        if focused || self.invalid {
            let mut ring = Paint::default();
            ring.set_anti_alias(true);
            ring.set_style(PaintStyle::Stroke);
            ring.set_stroke_width(3.0);
            ring.set_color(sk(accent.with_alpha(if self.invalid { 0.2 } else { 0.5 })));
            canvas.draw_rrect(shape.with_outset((1.5, 1.5)), &ring);
        }
        let mut border = Paint::default();
        border.set_anti_alias(true);
        border.set_style(PaintStyle::Stroke);
        border.set_stroke_width(1.0);
        border.set_color(sk(accent));
        canvas.draw_rrect(shape.with_inset((0.5, 0.5)), &border);

        let inner = Self::text_rect(rect);
        let x = inner.left - self.scroll_x;
        let y = rect.center_y() - layout.height() / 2.0;
        canvas.save();
        canvas.clip_rect(inner.with_outset((1.0, 0.0)), None, true);
        let range = self.range();
        if !range.is_empty() {
            let mut fill = Paint::default();
            fill.set_color(sk(palette.selection));
            for r in layout.selection_rects(range.clone()) {
                canvas.draw_rect(Rect::from_xywh(x + r.x, y + r.y, r.width, r.height), &fill);
            }
        }
        match placeholder {
            Some(placeholder) if self.text.is_empty() => {
                placeholder.paint(
                    canvas,
                    inner.left,
                    rect.center_y() - placeholder.height() / 2.0,
                );
            }
            _ => layout.paint(canvas, x, y),
        }
        if let Some((from, to)) = self.composition {
            let mut underline = Paint::default();
            underline.set_color(sk(palette.foreground));
            for r in layout.selection_rects(from..to) {
                canvas.draw_rect(
                    Rect::from_xywh(x + r.x, y + r.bottom() - 2.0, r.width, 1.0),
                    &underline,
                );
            }
        }
        if caret && range.is_empty() {
            let at = self.caret_rect(rect, layout);
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_color(sk(palette.caret));
            canvas.draw_rect(
                Rect::from_xywh(at.left, at.top, theme.spacing.caret_width, at.height()),
                &paint,
            );
        }
        canvas.restore();
    }
}
