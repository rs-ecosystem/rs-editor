//! The text an input method sees.
//!
//! Platform IMEs (Android `InputConnection`, the web's `EditContext`,
//! `UITextInput`, `NSTextInputClient`) work on a flat string with a
//! selection and a composing region. The view gives them the textblock
//! around the selection head, in UTF-16 units: hard breaks are `\n` and
//! inline atoms U+FFFC, so offsets map one-to-one onto document positions.
//! When the caret leaves the block, the context's `start` changes and the
//! adapter restarts its input session.

use openrich_layout::Affinity;
use openrich_state::Selection;

use crate::{Composition, EditorView};

/// The IME's view of the text around the selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImeContext {
    /// Document position of the block's first character; identifies the
    /// block (a different value means a new input session).
    pub start: usize,
    pub text: String,
    /// Selection anchor and head, relative to `start`, clamped to the block.
    pub selection: (usize, usize),
    pub composition: Option<(usize, usize)>,
}

impl ImeContext {
    pub fn len(&self) -> usize {
        self.text.encode_utf16().count()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Up to `n` UTF-16 units before the selection.
    pub fn text_before(&self, n: usize) -> String {
        let end = self.selection.0.min(self.selection.1);
        self.slice(end.saturating_sub(n), end)
    }

    /// Up to `n` UTF-16 units after the selection.
    pub fn text_after(&self, n: usize) -> String {
        let start = self.selection.0.max(self.selection.1);
        self.slice(start, (start + n).min(self.len()))
    }

    pub fn selected_text(&self) -> String {
        let (a, b) = self.selection;
        self.slice(a.min(b), a.max(b))
    }

    /// The text between two UTF-16 offsets (moved off surrogate halves).
    pub fn slice(&self, from: usize, to: usize) -> String {
        let units: Vec<u16> = self.text.encode_utf16().collect();
        let fix = |mut i: usize| {
            i = i.min(units.len());
            if i > 0 && i < units.len() && (0xDC00..0xE000).contains(&units[i]) {
                i -= 1;
            }
            i
        };
        let (from, to) = (fix(from), fix(to.max(from)));
        String::from_utf16_lossy(&units[from..to])
    }
}

impl EditorView {
    /// The textblock around the selection head, or `None` when the head is
    /// not in a textblock (a selected image or rule).
    pub fn ime_context(&self) -> Option<ImeContext> {
        let selection = self.editor.selection();
        let head = selection.head_pos();
        let block = head.parent();
        if !block.is_textblock() || matches!(selection, Selection::Node(_)) {
            return None;
        }
        let start = head.start(head.depth());
        let len = block.content().size();
        let relative = |pos: usize| pos.clamp(start, start + len) - start;
        let composition = self
            .composition
            .filter(|c| c.from >= start && c.to <= start + len)
            .map(|c| (c.from - start, c.to - start));
        Some(ImeContext {
            start,
            text: openrich_text::textblock_text(block),
            selection: (relative(selection.anchor()), relative(selection.head())),
            composition,
        })
    }

    /// Sets the selection inside the IME's block (`setSelection`).
    pub fn ime_set_selection(&mut self, anchor: usize, head: usize) -> bool {
        let Some(context) = self.ime_context() else {
            return false;
        };
        let len = context.len();
        self.select_text(
            context.start + anchor.min(len),
            context.start + head.min(len),
            Affinity::Downstream,
        )
    }

    /// Marks existing text as the composition (`setComposingRegion`), as
    /// keyboards do when the caret returns to a word to offer suggestions.
    pub fn ime_set_composing_region(&mut self, from: usize, to: usize) -> bool {
        let Some(context) = self.ime_context() else {
            return false;
        };
        let len = context.len();
        let (from, to) = (from.min(to).min(len), from.max(to).min(len));
        if from == to {
            self.finish_composition();
            return true;
        }
        let (abs_from, abs_to) = (context.start + from, context.start + to);
        // Keyboards re-assert the region after every keystroke; the same
        // region keeps the composition (and its undo step).
        if self
            .composition
            .is_some_and(|c| c.from == abs_from && c.to == abs_to)
        {
            return true;
        }
        self.close_history_now();
        let id = self.new_composition_id();
        self.composition = Some(Composition {
            id,
            from: abs_from,
            to: abs_to,
        });
        self.request_redraw();
        true
    }

    /// Ends the composition keeping its text, when the IME already wrote
    /// the final text (EditContext `compositionend`).
    pub fn ime_finish_composition(&mut self) {
        self.finish_composition();
        self.request_redraw();
    }

    /// Deletes `before` UTF-16 units before the selection and `after` units
    /// after it, within the block (`deleteSurroundingText`).
    pub fn ime_delete_surrounding(&mut self, before: usize, after: usize) -> bool {
        self.end_touch_mode();
        let Some(context) = self.ime_context() else {
            return false;
        };
        let (a, b) = context.selection;
        let (from, to) = (a.min(b), a.max(b));
        let index = openrich_text::TextIndexMap::from_text(&context.text);
        // Never split a grapheme: widen to the nearest boundaries.
        let start = index.snap(from.saturating_sub(before), Affinity::Upstream);
        let end = index.snap((to + after).min(context.len()), Affinity::Downstream);
        let mut ok = true;
        if to < end {
            ok &= self.delete_range(context.start + to, context.start + end);
        }
        if start < from {
            ok &= self.delete_range(context.start + start, context.start + from);
        }
        ok
    }

    /// Replaces `from..to` of the IME's block with `text`. While
    /// `composing`, the replaced range becomes the composition (EditContext
    /// `textupdate` during a composition); otherwise the text is typed,
    /// running input rules.
    pub fn ime_replace(&mut self, from: usize, to: usize, text: &str, composing: bool) -> bool {
        let Some(context) = self.ime_context() else {
            return false;
        };
        let len = context.len();
        let (from, to) = (from.min(len), to.min(len).max(from.min(len)));
        let (abs_from, abs_to) = (context.start + from, context.start + to);
        if composing {
            match self.composition {
                Some(current) => {
                    self.composition = Some(Composition {
                        id: current.id,
                        from: abs_from,
                        to: abs_to,
                    });
                }
                None => {
                    self.close_history_now();
                    let id = self.new_composition_id();
                    self.composition = Some(Composition {
                        id,
                        from: abs_from,
                        to: abs_to,
                    });
                }
            }
            return self.set_composition(text);
        }
        self.finish_composition();
        if text.is_empty() {
            return from == to || self.delete_range(abs_from, abs_to);
        }
        self.select_text(abs_from, abs_to, Affinity::Downstream) && self.insert_text(text)
    }

    fn delete_range(&mut self, from: usize, to: usize) -> bool {
        let mut tr = self.editor.state().tr();
        if tr.delete(from, to).is_err() {
            return false;
        }
        if let Some(composition) = &mut self.composition {
            // Keep the composition in step with the deletion.
            let shift = |p: usize| {
                if p >= to {
                    p - (to - from)
                } else {
                    p.min(from)
                }
            };
            composition.from = shift(composition.from);
            composition.to = shift(composition.to);
        }
        tr.scroll_into_view();
        self.dispatch(tr)
    }
}
