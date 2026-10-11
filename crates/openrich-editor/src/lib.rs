//! Headless editing state and commands for OpenRich.

use openrich_document::{Document, Position, Selection};
use thiserror::Error;

/// Mutable editor state, independent of any UI framework.
#[derive(Clone, Debug)]
pub struct EditorState {
    document: Document,
    selection: Selection,
}

impl Default for EditorState {
    fn default() -> Self {
        let document = Document::new();
        let selection = Selection::caret(Position {
            block_index: 0,
            offset: 0,
            affinity: Default::default(),
        });
        Self { document, selection }
    }
}

impl EditorState {
    pub fn new(document: Document) -> Result<Self, EditorError> {
        document.validate()?;
        let selection = Selection::caret(Position {
            block_index: 0,
            offset: 0,
            affinity: Default::default(),
        });
        Ok(Self { document, selection })
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Sets the selection after checking that both endpoints reference blocks.
    pub fn set_selection(&mut self, selection: Selection) -> Result<(), EditorError> {
        let block_count = self.document.blocks.len();
        if selection.anchor.block_index >= block_count || selection.focus.block_index >= block_count {
            return Err(EditorError::PositionOutOfBounds);
        }
        self.selection = selection;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum EditorError {
    #[error(transparent)]
    Document(#[from] openrich_document::DocumentError),
    #[error("selection endpoint refers to a block outside the document")]
    PositionOutOfBounds,
}
