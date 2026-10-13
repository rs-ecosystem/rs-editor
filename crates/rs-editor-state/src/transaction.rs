//! Transactions (`transaction.ts`): a [`Transform`] plus selection, stored
//! marks, time and metadata.

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rs_editor_model::{Mark, MarkSet, MarkType, Node, Slice};
use rs_editor_transform::{Transform, TransformResult};

use crate::selection::{Selection, resolve_clamped};
use crate::state::EditorState;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Value stored with [`Transaction::set_meta`].
pub type MetaValue = Arc<dyn Any + Send + Sync>;

/// Well-known metadata keys.
pub mod meta {
    /// `bool`: `false` keeps the change out of the undo history.
    pub const ADD_TO_HISTORY: &str = "addToHistory";
    /// `Arc<Transaction>`: the root transaction an appended transaction responds to.
    pub const APPENDED_TRANSACTION: &str = "appendedTransaction";
    /// `&'static str`: the UI event behind the change (`"paste"`, `"drop"`, `"cut"`, …).
    pub const UI_EVENT: &str = "uiEvent";
    /// `u64`: id of the IME composition this change belongs to; changes of
    /// one composition form a single undo step.
    pub const COMPOSITION: &str = "composition";
    /// `bool`: the selection change came from a pointer.
    pub const POINTER: &str = "pointer";
}

/// A change to an [`EditorState`]. Derefs to the underlying [`Transform`],
/// so all document-changing helpers are available directly.
#[derive(Clone)]
pub struct Transaction {
    transform: Transform,
    id: u64,
    pub time: u64,
    cur_selection: Selection,
    cur_selection_for: usize,
    selection_set: bool,
    stored_marks: Option<MarkSet>,
    /// Number of steps for which `stored_marks` is valid; adding a step
    /// clears stored marks, like ProseMirror's `addStep` override.
    stored_marks_for: usize,
    stored_marks_set: bool,
    scrolled_into_view: bool,
    meta: HashMap<String, MetaValue>,
}

impl fmt::Debug for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transaction")
            .field("id", &self.id)
            .field("steps", &self.transform.steps)
            .field("selection", &self.selection())
            .field("meta", &self.meta.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Deref for Transaction {
    type Target = Transform;

    fn deref(&self) -> &Transform {
        &self.transform
    }
}

impl DerefMut for Transaction {
    fn deref_mut(&mut self) -> &mut Transform {
        &mut self.transform
    }
}

impl Transaction {
    pub(crate) fn new(state: &EditorState, time: u64) -> Self {
        Self {
            transform: Transform::new(state.doc().clone()),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            time,
            cur_selection: state.selection().clone(),
            cur_selection_for: 0,
            selection_set: false,
            stored_marks: state.stored_marks().cloned(),
            stored_marks_for: 0,
            stored_marks_set: false,
            scrolled_into_view: false,
            meta: HashMap::new(),
        }
    }

    /// Unique id of this transaction.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn transform(&self) -> &Transform {
        &self.transform
    }

    /// The selection, mapped through the steps added since it was set.
    pub fn selection(&self) -> Selection {
        if self.cur_selection_for < self.transform.steps.len() {
            let mapping = self.transform.mapping.slice_from(self.cur_selection_for);
            self.cur_selection.map(&self.transform.doc, &mapping)
        } else {
            self.cur_selection.clone()
        }
    }

    pub fn set_selection(&mut self, selection: Selection) -> &mut Self {
        debug_assert!(
            selection.from_pos().doc() == &self.transform.doc,
            "selection must point into the transaction's current document"
        );
        self.cur_selection = selection;
        self.cur_selection_for = self.transform.steps.len();
        self.selection_set = true;
        self.stored_marks = None;
        self.stored_marks_set = false;
        self.stored_marks_for = self.transform.steps.len();
        self
    }

    /// Whether the selection was explicitly set.
    pub fn selection_set(&self) -> bool {
        self.selection_set
    }

    /// Marks applied to the next typed text, if any were set.
    pub fn stored_marks(&self) -> Option<&MarkSet> {
        if self.stored_marks_for == self.transform.steps.len() {
            self.stored_marks.as_ref()
        } else {
            None
        }
    }

    pub fn set_stored_marks(&mut self, marks: Option<MarkSet>) -> &mut Self {
        self.stored_marks = marks;
        self.stored_marks_for = self.transform.steps.len();
        self.stored_marks_set = true;
        self
    }

    pub fn stored_marks_set(&self) -> bool {
        self.stored_marks_set && self.stored_marks_for == self.transform.steps.len()
    }

    fn current_marks(&self, at_head: bool) -> MarkSet {
        if let Some(marks) = self.stored_marks() {
            return marks.clone();
        }
        let selection = self.selection();
        if at_head {
            selection.head_pos().marks()
        } else {
            selection.from_pos().marks()
        }
    }

    /// Sets stored marks unless `marks` already match what typing would use.
    pub fn ensure_marks(&mut self, marks: MarkSet) -> &mut Self {
        if !self.current_marks(false).same_set(&marks) {
            self.set_stored_marks(Some(marks));
        }
        self
    }

    pub fn add_stored_mark(&mut self, mark: &Mark) -> &mut Self {
        let marks = mark.add_to_set(&self.current_marks(true));
        self.ensure_marks(marks)
    }

    pub fn remove_stored_mark(&mut self, mark: &Mark) -> &mut Self {
        let marks = mark.remove_from_set(&self.current_marks(true));
        self.ensure_marks(marks)
    }

    pub fn remove_stored_mark_type(&mut self, ty: &MarkType) -> &mut Self {
        let marks = ty.remove_from_set(&self.current_marks(true));
        self.ensure_marks(marks)
    }

    pub fn set_time(&mut self, time: u64) -> &mut Self {
        self.time = time;
        self
    }

    pub fn replace_selection(&mut self, slice: Slice) -> TransformResult<&mut Self> {
        let selection = self.selection();
        selection.replace(self, Some(slice))?;
        Ok(self)
    }

    /// Replaces the selection with `node`, giving it the marks typing here
    /// would get when `inherit_marks` is set.
    pub fn replace_selection_with(
        &mut self,
        node: Node,
        inherit_marks: bool,
    ) -> TransformResult<&mut Self> {
        let selection = self.selection();
        let node = if inherit_marks {
            let marks = match self.stored_marks() {
                Some(marks) => marks.clone(),
                None if selection.empty() => selection.from_pos().marks(),
                None => selection
                    .from_pos()
                    .marks_across(selection.to_pos())
                    .unwrap_or_else(MarkSet::none),
            };
            node.mark(marks)
        } else {
            node
        };
        selection.replace_with(self, node)?;
        Ok(self)
    }

    pub fn delete_selection(&mut self) -> TransformResult<&mut Self> {
        let selection = self.selection();
        selection.replace(self, None)?;
        Ok(self)
    }

    /// Replaces the selection with text, keeping the marks typing would use.
    pub fn insert_text(&mut self, text: &str) -> TransformResult<&mut Self> {
        if text.is_empty() {
            return self.delete_selection();
        }
        let node = self
            .transform
            .doc
            .node_type()
            .schema()
            .text(text, MarkSet::none())?;
        self.replace_selection_with(node, true)
    }

    /// Inserts text at `from..to` instead of the selection.
    pub fn insert_text_at(
        &mut self,
        text: &str,
        from: usize,
        to: usize,
    ) -> TransformResult<&mut Self> {
        if text.is_empty() {
            self.transform.delete_range(from, to)?;
            return Ok(self);
        }
        let marks = match self.stored_marks() {
            Some(marks) => marks.clone(),
            None => {
                let rfrom = self.transform.doc.resolve(from)?;
                if to == from {
                    rfrom.marks()
                } else {
                    rfrom
                        .marks_across(&self.transform.doc.resolve(to)?)
                        .unwrap_or_else(MarkSet::none)
                }
            }
        };
        let node = self.transform.doc.node_type().schema().text(text, marks)?;
        let len = node.node_size();
        self.transform.replace_range_with(from, to, node)?;
        let selection = self.selection();
        if !selection.empty() && selection.to() == from + len {
            let near = Selection::near(&resolve_clamped(&self.transform.doc, selection.to()), 1);
            self.set_selection(near);
        }
        Ok(self)
    }

    pub fn set_meta<T: Any + Send + Sync>(&mut self, key: &str, value: T) -> &mut Self {
        self.meta.insert(key.to_owned(), Arc::new(value));
        self
    }

    pub fn get_meta<T: Any + Send + Sync>(&self, key: &str) -> Option<&T> {
        self.meta
            .get(key)
            .and_then(|value| value.downcast_ref::<T>())
    }

    /// Copies every metadata entry of `other` onto this transaction.
    pub fn copy_meta_from(&mut self, other: &Transaction) -> &mut Self {
        for (key, value) in &other.meta {
            self.meta.insert(key.clone(), value.clone());
        }
        self
    }

    pub fn has_meta(&self, key: &str) -> bool {
        self.meta.contains_key(key)
    }

    /// True when no metadata was attached (a plain user edit).
    pub fn is_generic(&self) -> bool {
        self.meta.is_empty()
    }

    pub fn scroll_into_view(&mut self) -> &mut Self {
        self.scrolled_into_view = true;
        self
    }

    pub fn scrolled_into_view(&self) -> bool {
        self.scrolled_into_view
    }
}

impl Transaction {
    /// The root transaction this one was appended to, if any.
    pub fn appended_to(&self) -> Option<&Arc<Transaction>> {
        self.get_meta::<Arc<Transaction>>(meta::APPENDED_TRANSACTION)
    }
}
