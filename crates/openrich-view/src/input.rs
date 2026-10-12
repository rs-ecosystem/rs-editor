//! Keyboard, text, IME composition and clipboard input.

use openrich_commands::{KeyEvent, KeyPlatform};
use openrich_history::close_history;
use openrich_layout::{Affinity, VisualDirection};
use openrich_model::{Fragment, MarkSet, Slice};
use openrich_state::{NodeSelection, Selection, TextSelection, meta};
use serde_json::Value;

use crate::{Composition, EditorView, Effect, ViewQuery};

/// How far a deletion or caret movement reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Grapheme,
    Word,
    Line,
    Document,
}

impl EditorView {
    /// A key press. Returns whether the editor used it.
    pub fn key_down(&mut self, event: &KeyEvent) -> bool {
        if !self.editor.is_editable() && !is_navigation(&event.key) {
            return false;
        }
        if self.composition.is_some() {
            // The IME owns keys while composing.
            return false;
        }
        let handled = {
            let context = ViewQuery::new(self);
            self.editor.handle_key(event, Some(&context))
        };
        if handled {
            self.goal_x = None;
            self.affinity = Affinity::Downstream;
            self.after_change();
            return true;
        }
        let m = event.modifiers;
        let apple = self.platform() == KeyPlatform::Apple;
        let word = if apple { m.alt } else { m.ctrl };
        let line = apple && m.meta;
        let granularity = if line {
            Granularity::Line
        } else if word {
            Granularity::Word
        } else {
            Granularity::Grapheme
        };
        let extend = m.shift;
        match event.key.as_str() {
            "Backspace" => self.delete(false, granularity),
            "Delete" => self.delete(true, granularity),
            "ArrowLeft" => self.move_horizontal(VisualDirection::Left, granularity, extend),
            "ArrowRight" => self.move_horizontal(VisualDirection::Right, granularity, extend),
            "ArrowUp" if apple && m.meta => self.move_document_edge(false, extend),
            "ArrowDown" if apple && m.meta => self.move_document_edge(true, extend),
            "ArrowUp" => self.move_vertical(true, extend),
            "ArrowDown" => self.move_vertical(false, extend),
            "Home" if m.ctrl => self.move_document_edge(false, extend),
            "End" if m.ctrl => self.move_document_edge(true, extend),
            "Home" => self.move_line_edge(false, extend),
            "End" => self.move_line_edge(true, extend),
            "PageUp" => self.move_page(true, extend),
            "PageDown" => self.move_page(false, extend),
            "Escape" => false,
            _ => false,
        }
    }

    /// Text typed or pasted as plain characters, replacing the selection.
    pub fn insert_text(&mut self, text: &str) -> bool {
        if text.is_empty() || !self.editor.is_editable() {
            return false;
        }
        self.finish_composition();
        let selection = self.editor.selection().clone();
        let ok = self
            .editor
            .handle_text_input(selection.from(), selection.to(), text);
        if ok {
            self.goal_x = None;
            self.affinity = Affinity::Downstream;
            self.after_change();
        }
        ok
    }

    fn set_selection(&mut self, selection: Selection, affinity: Affinity) -> bool {
        let mut tr = self.editor.state().tr();
        tr.set_selection(selection);
        tr.scroll_into_view();
        self.affinity = affinity;
        self.dispatch(tr)
    }

    fn select_to(&mut self, pos: usize, affinity: Affinity, extend: bool) -> bool {
        let doc = self.editor.doc().clone();
        let Ok(head) = doc.resolve(pos) else {
            return false;
        };
        let selection = if extend {
            let anchor = self.editor.selection().anchor_pos().clone();
            TextSelection::between(&anchor, &head, None)
        } else {
            Selection::Text(TextSelection::new(head.clone(), head))
        };
        self.set_selection(selection, affinity)
    }

    fn move_horizontal(
        &mut self,
        direction: VisualDirection,
        granularity: Granularity,
        extend: bool,
    ) -> bool {
        self.goal_x = None;
        let selection = self.editor.selection().clone();
        let forward = direction == VisualDirection::Right;
        // An arrow without Shift collapses a range to its side.
        if !extend && !selection.empty() && granularity == Granularity::Grapheme {
            if let Selection::Node(_) = selection {
                let pos = if forward {
                    selection.to()
                } else {
                    selection.from()
                };
                let doc = self.editor.doc().clone();
                let Ok(at) = doc.resolve(pos) else {
                    return false;
                };
                return self.set_selection(
                    Selection::near(&at, if forward { 1 } else { -1 }),
                    Affinity::Downstream,
                );
            }
            let pos = if forward {
                selection.to()
            } else {
                selection.from()
            };
            return self.select_to(pos, Affinity::Downstream, false);
        }
        let head = selection.head();
        let affinity = self.affinity;
        let target = match granularity {
            Granularity::Grapheme => self.visual_step(head, affinity, direction),
            Granularity::Word => self.word_step(head, forward),
            Granularity::Line => self
                .layout()
                .line_edge(head, affinity, forward)
                .map(|(pos, affinity)| Target::Text(pos, affinity)),
            Granularity::Document => None,
        };
        match target {
            Some(Target::Text(pos, affinity)) => self.select_to(pos, affinity, extend),
            Some(Target::Node(selection)) if !extend => {
                self.set_selection(selection, Affinity::Downstream)
            }
            Some(Target::Node(selection)) => {
                // Extending over a node selects through it.
                let pos = if forward {
                    selection.to()
                } else {
                    selection.from()
                };
                self.select_to(pos, Affinity::Downstream, true)
            }
            None => false,
        }
    }

    /// One visual step, entering a selectable node between blocks.
    fn visual_step(
        &mut self,
        head: usize,
        affinity: Affinity,
        direction: VisualDirection,
    ) -> Option<Target> {
        let layout = self.layout().clone();
        let forward = direction == VisualDirection::Right;
        let doc = self.editor.doc().clone();
        if let Some(text) = layout.text_at(head) {
            let at_edge = if forward {
                head == text.end()
            } else {
                head == text.pos
            };
            let rtl_free = !text.layout.text().chars().any(is_rtl);
            if at_edge && rtl_free {
                // Leaving the block: ProseMirror-style search for the next selection.
                let outside = if forward {
                    text.node_pos + text.node.node_size()
                } else {
                    text.node_pos
                };
                let resolved = doc.resolve(outside).ok()?;
                let found = Selection::find_from(&resolved, if forward { 1 } else { -1 }, false)?;
                return Some(match found {
                    Selection::Node(_) => Target::Node(found),
                    _ => Target::Text(found.head(), Affinity::Downstream),
                });
            }
        }
        layout
            .move_visual(head, affinity, direction)
            .map(|(pos, affinity)| Target::Text(pos, affinity))
    }

    fn word_step(&mut self, head: usize, forward: bool) -> Option<Target> {
        let layout = self.layout().clone();
        let text = layout.text_at(head)?;
        let offset = head - text.pos;
        let content = text.layout.text();
        let target = if forward {
            if offset == text.layout.len() {
                return self.visual_step(head, Affinity::Downstream, VisualDirection::Right);
            }
            openrich_text::next_word_end(content, offset)
        } else {
            if offset == 0 {
                return self.visual_step(head, Affinity::Downstream, VisualDirection::Left);
            }
            openrich_text::prev_word_start(content, offset)
        };
        Some(Target::Text(text.pos + target, Affinity::Downstream))
    }

    fn move_vertical(&mut self, up: bool, extend: bool) -> bool {
        let selection = self.editor.selection().clone();
        let head = selection.head();
        let affinity = self.affinity;
        let goal = match self.goal_x {
            Some(goal) => goal,
            None => match self.layout().caret_rect(head, affinity, 0.0) {
                Some(rect) => rect.x,
                None => 0.0,
            },
        };
        let target = self.layout().move_vertical(head, affinity, up, goal);
        let moved = match target {
            Some((pos, affinity)) => self.select_to(pos, affinity, extend),
            None => self.move_document_edge(!up, extend),
        };
        self.goal_x = Some(goal);
        moved
    }

    fn move_line_edge(&mut self, end: bool, extend: bool) -> bool {
        self.goal_x = None;
        let head = self.editor.selection().head();
        let affinity = self.affinity;
        match self.layout().line_edge(head, affinity, end) {
            Some((pos, affinity)) => self.select_to(pos, affinity, extend),
            None => false,
        }
    }

    fn move_document_edge(&mut self, end: bool, extend: bool) -> bool {
        self.goal_x = None;
        let doc = self.editor.doc().clone();
        let target = if end {
            Selection::at_end(&doc)
        } else {
            Selection::at_start(&doc)
        };
        self.select_to(target.head(), Affinity::Downstream, extend)
    }

    fn move_page(&mut self, up: bool, extend: bool) -> bool {
        let Some(caret) = self.caret_rect() else {
            return false;
        };
        let page = (self.height - 48.0).max(40.0);
        let (x, y) = self.to_document(caret.x, caret.center_y() + if up { -page } else { page });
        let hit = self.layout().hit(x, y);
        self.scroll_by(if up { -page } else { page });
        match hit {
            Some(openrich_layout::Hit::Text { pos, affinity }) => {
                self.select_to(pos, affinity, extend)
            }
            _ => self.move_document_edge(!up, extend),
        }
    }

    /// Deletes backward or forward from a collapsed cursor by `granularity`.
    /// Commands bound to Backspace/Delete (joining blocks, deleting a
    /// selection) run first through the keymap.
    pub fn delete(&mut self, forward: bool, granularity: Granularity) -> bool {
        let selection = self.editor.selection().clone();
        if !selection.empty() {
            let mut tr = self.editor.state().tr();
            if tr.delete_selection().is_err() {
                return false;
            }
            tr.scroll_into_view();
            return self.dispatch(tr);
        }
        let head = selection.head();
        let layout = self.layout().clone();
        let Some(text) = layout.text_at(head) else {
            return false;
        };
        let offset = head - text.pos;
        let content = text.layout.text();
        let index = text.layout.index();
        let target = match (granularity, forward) {
            (Granularity::Grapheme, false) => index.prev_boundary(offset),
            (Granularity::Grapheme, true) => index.next_boundary(offset),
            (Granularity::Word, false) => Some(openrich_text::prev_word_start(content, offset)),
            (Granularity::Word, true) => Some(openrich_text::next_word_end(content, offset)),
            (Granularity::Line, false) => layout
                .line_edge(head, self.affinity, false)
                .map(|(p, _)| p - text.pos),
            (Granularity::Line, true) => layout
                .line_edge(head, self.affinity, true)
                .map(|(p, _)| p - text.pos),
            (Granularity::Document, _) => None,
        };
        let Some(target) = target.filter(|t| *t != offset) else {
            return false;
        };
        let (from, to) = if forward {
            (head, text.pos + target)
        } else {
            (text.pos + target, head)
        };
        let mut tr = self.editor.state().tr();
        if tr.delete(from, to).is_err() {
            return false;
        }
        tr.scroll_into_view();
        self.affinity = Affinity::Downstream;
        self.dispatch(tr)
    }

    // ---- IME composition ------------------------------------------------

    /// Shows the IME's in-progress `text` (preedit) in place of the
    /// selection or the current composition. Composed text is written into
    /// the document so layout, wrapping and the candidate window position
    /// are exact; it becomes one undo step when committed.
    pub fn set_composition(&mut self, text: &str) -> bool {
        if !self.editor.is_editable() {
            return false;
        }
        let (id, from, to) = match self.composition {
            Some(composition) => (composition.id, composition.from, composition.to),
            None => {
                // A composition never joins the typing before it.
                self.close_history_now();
                let selection = self.editor.selection().clone();
                (self.new_composition_id(), selection.from(), selection.to())
            }
        };
        let mut tr = self.editor.state().tr();
        let ok = if text.is_empty() {
            tr.delete(from, to).is_ok()
        } else {
            tr.insert_text_at(text, from, to).is_ok()
        };
        if !ok {
            return false;
        }
        tr.set_meta(meta::COMPOSITION, id);
        let end = from + openrich_model::text::utf16_len(text);
        if let Ok(caret) = tr.doc.resolve(end) {
            let selection = Selection::Text(TextSelection::new(caret.clone(), caret));
            tr.set_selection(selection);
        }
        self.composition = (!text.is_empty()).then_some(Composition { id, from, to: end });
        self.dispatch(tr)
    }

    /// Commits the composition with `text` (often equal to the preedit).
    pub fn commit_composition(&mut self, text: &str) -> bool {
        if self.composition.is_none() {
            return self.insert_text(text);
        }
        let ok = self.set_composition(text);
        self.composition = None;
        self.close_history_now();
        ok
    }

    /// Removes the uncommitted composition (Escape in the IME).
    pub fn cancel_composition(&mut self) -> bool {
        if self.composition.is_none() {
            return false;
        }
        let ok = self.set_composition("");
        self.composition = None;
        ok
    }

    /// Ends a composition, keeping its text (focus loss, clicks).
    pub(crate) fn finish_composition(&mut self) {
        if self.composition.take().is_some() {
            self.close_history_now();
        }
    }

    /// The composed range, for drawing its underline.
    pub fn composition_range(&self) -> Option<(usize, usize)> {
        self.composition.map(|c| (c.from, c.to))
    }

    pub(crate) fn close_history_now(&mut self) {
        let mut tr = self.editor.state().tr();
        close_history(&mut tr);
        let _ = self.editor.dispatch(tr);
    }

    // ---- clipboard --------------------------------------------------------

    /// The selection as plain text and as a JSON slice.
    pub fn copy(&mut self) -> Option<(String, Value)> {
        let selection = self.editor.selection().clone();
        if selection.empty() {
            return None;
        }
        let slice = selection.content();
        let text = slice
            .content
            .text_between(0, slice.content.size(), Some("\n"), Some(""));
        let json = serde_json::json!({
            "content": openrich_model::fragment_to_json(&slice.content),
            "openStart": slice.open_start,
            "openEnd": slice.open_end,
        });
        self.push_effect(Effect::Clipboard {
            text: text.clone(),
            json: json.clone(),
        });
        Some((text, json))
    }

    pub fn cut(&mut self) -> Option<(String, Value)> {
        let copied = self.copy()?;
        let mut tr = self.editor.state().tr();
        tr.delete_selection().ok()?;
        tr.set_meta(meta::UI_EVENT, "cut");
        tr.scroll_into_view();
        self.dispatch(tr);
        Some(copied)
    }

    /// Pastes plain text; each line becomes a paragraph.
    pub fn paste_text(&mut self, text: &str) -> bool {
        if !self.editor.is_editable() || text.is_empty() {
            return false;
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = text.split('\n').collect();
        if lines.len() == 1 {
            return self.insert_text(&text);
        }
        let schema = self.editor.schema().clone();
        let Ok(paragraph) = schema.node_type("paragraph") else {
            return self.insert_text(&text);
        };
        let mut blocks = Vec::new();
        for line in &lines {
            let content = if line.is_empty() {
                Fragment::empty()
            } else {
                match schema.text(*line, MarkSet::none()) {
                    Ok(node) => Fragment::from_node(node),
                    Err(_) => Fragment::empty(),
                }
            };
            match paragraph.create(None, content, MarkSet::none()) {
                Ok(node) => blocks.push(node),
                Err(_) => return false,
            }
        }
        self.paste_slice(Slice::new(Fragment::from_array(blocks), 1, 1))
    }

    /// Pastes a JSON slice produced by [`EditorView::copy`].
    pub fn paste_json(&mut self, json: &Value) -> bool {
        let schema = self.editor.schema().clone();
        let Ok(content) = schema.fragment_from_json(json.get("content").unwrap_or(&Value::Null))
        else {
            return false;
        };
        let open = |key: &str| json.get(key).and_then(Value::as_u64).unwrap_or(0) as usize;
        self.paste_slice(Slice::new(content, open("openStart"), open("openEnd")))
    }

    fn paste_slice(&mut self, slice: Slice) -> bool {
        self.finish_composition();
        let mut tr = self.editor.state().tr();
        if tr.replace_selection(slice).is_err() {
            return false;
        }
        tr.set_meta(meta::UI_EVENT, "paste");
        tr.scroll_into_view();
        self.affinity = Affinity::Downstream;
        self.dispatch(tr)
    }

    /// Selects the node at `pos` (e.g. an image that was clicked).
    pub fn select_node(&mut self, pos: usize) -> bool {
        match NodeSelection::create(self.editor.doc(), pos) {
            Ok(selection) => self.set_selection(selection, Affinity::Downstream),
            Err(_) => false,
        }
    }

    /// Selects text `anchor..head`.
    pub fn select_text(&mut self, anchor: usize, head: usize, affinity: Affinity) -> bool {
        let doc = self.editor.doc().clone();
        let size = doc.content().size();
        let (Ok(a), Ok(h)) = (doc.resolve(anchor.min(size)), doc.resolve(head.min(size))) else {
            return false;
        };
        self.set_selection(TextSelection::between(&a, &h, None), affinity)
    }
}

enum Target {
    Text(usize, Affinity),
    Node(Selection),
}

fn is_navigation(key: &str) -> bool {
    matches!(
        key,
        "ArrowLeft"
            | "ArrowRight"
            | "ArrowUp"
            | "ArrowDown"
            | "Home"
            | "End"
            | "PageUp"
            | "PageDown"
    )
}

fn is_rtl(ch: char) -> bool {
    matches!(ch as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}
