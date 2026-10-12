//! Atomic, inspectable changes produced by the OpenRich editor.

use openrich_document::{Document, Inline, Position, Selection};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Version of the transaction representation.
pub const TRANSACTION_VERSION: u32 = 1;

/// A single reversible change within a transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Replaces the inline content of one top-level text block.
    ReplaceInlineContent {
        block_index: usize,
        before: Vec<Inline>,
        after: Vec<Inline>,
    },
    /// Replaces the complete document for a structural change or history step.
    ReplaceDocument { before: Document, after: Document },
}

/// An atomic document change with selection context for observers and adapters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub version: u32,
    pub steps: Vec<Step>,
    pub selection_before: Selection,
    pub selection_after: Selection,
}

impl Transaction {
    pub fn new(steps: Vec<Step>, selection_before: Selection, selection_after: Selection) -> Self {
        Self {
            version: TRANSACTION_VERSION,
            steps,
            selection_before,
            selection_after,
        }
    }

    /// Applies this transaction atomically to `document`.
    ///
    /// Every step must match the state it expects to replace. Changes are first
    /// applied to a clone and become visible only if all steps and the final
    /// document and selection validate successfully.
    pub fn apply(&self, document: &mut Document) -> Result<(), TransactionError> {
        if self.version != TRANSACTION_VERSION {
            return Err(TransactionError::UnsupportedVersion(self.version));
        }
        if self.steps.is_empty() {
            return Err(TransactionError::EmptyTransaction);
        }

        document.validate()?;
        validate_selection(document, &self.selection_before, SelectionEndpoint::Before)?;

        let mut candidate = document.clone();
        for (step_index, step) in self.steps.iter().enumerate() {
            match step {
                Step::ReplaceInlineContent {
                    block_index,
                    before,
                    after,
                } => {
                    let block = candidate.blocks.get(*block_index).ok_or(
                        TransactionError::BlockOutOfBounds {
                            step_index,
                            block_index: *block_index,
                        },
                    )?;
                    let current = block
                        .inline_content()
                        .ok_or(TransactionError::BlockNotText {
                            step_index,
                            block_index: *block_index,
                        })?;
                    if current != before {
                        return Err(TransactionError::InlineContentMismatch {
                            step_index,
                            block_index: *block_index,
                        });
                    }
                    *candidate.blocks[*block_index]
                        .inline_content_mut()
                        .expect("the text block was checked above") = after.clone();
                }
                Step::ReplaceDocument { before, after } => {
                    if &candidate != before {
                        return Err(TransactionError::DocumentMismatch { step_index });
                    }
                    candidate = after.clone();
                }
            }
            candidate.validate()?;
        }

        validate_selection(&candidate, &self.selection_after, SelectionEndpoint::After)?;
        *document = candidate;
        Ok(())
    }
}

fn validate_selection(
    document: &Document,
    selection: &Selection,
    endpoint: SelectionEndpoint,
) -> Result<(), TransactionError> {
    for position in [&selection.anchor, &selection.focus] {
        validate_position(document, position).map_err(|_| TransactionError::InvalidSelection {
            endpoint,
            block_index: position.block_index,
            offset: position.offset,
        })?;
    }
    Ok(())
}

fn validate_position(document: &Document, position: &Position) -> Result<(), ()> {
    let block = document.blocks.get(position.block_index).ok_or(())?;
    let length = block.text_len().ok_or(())?;
    if position.offset > length {
        return Err(());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionEndpoint {
    Before,
    After,
}

#[derive(Debug, Error)]
pub enum TransactionError {
    #[error("unsupported transaction version: {0}")]
    UnsupportedVersion(u32),
    #[error("transaction must contain at least one step")]
    EmptyTransaction,
    #[error("step {step_index} refers to block {block_index}, outside the document")]
    BlockOutOfBounds {
        step_index: usize,
        block_index: usize,
    },
    #[error("step {step_index} refers to non-text block {block_index}")]
    BlockNotText {
        step_index: usize,
        block_index: usize,
    },
    #[error("step {step_index} expected different inline content in block {block_index}")]
    InlineContentMismatch {
        step_index: usize,
        block_index: usize,
    },
    #[error("step {step_index} expected a different document")]
    DocumentMismatch { step_index: usize },
    #[error("{endpoint:?} selection position {block_index}:{offset} is invalid")]
    InvalidSelection {
        endpoint: SelectionEndpoint,
        block_index: usize,
        offset: usize,
    },
    #[error(transparent)]
    Document(#[from] openrich_document::DocumentError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrich_document::{Affinity, Block, DOCUMENT_SCHEMA_VERSION, Inline, NodeId, Position};

    fn document(text: &str) -> Document {
        Document {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![Block::Paragraph {
                id: NodeId("1".into()),
                content: vec![Inline::text(text)],
            }],
        }
    }

    fn selection(offset: usize) -> Selection {
        Selection::caret(Position {
            block_index: 0,
            offset,
            affinity: Affinity::Downstream,
        })
    }

    #[test]
    fn applies_valid_step_and_selection_atomically() {
        let mut current = document("before");
        let transaction = Transaction::new(
            vec![Step::ReplaceInlineContent {
                block_index: 0,
                before: vec![Inline::text("before")],
                after: vec![Inline::text("after")],
            }],
            selection(6),
            selection(5),
        );

        transaction.apply(&mut current).unwrap();

        assert_eq!(current, document("after"));
    }

    #[test]
    fn failed_later_step_leaves_document_unchanged() {
        let original = document("before");
        let mut current = original.clone();
        let transaction = Transaction::new(
            vec![
                Step::ReplaceInlineContent {
                    block_index: 0,
                    before: vec![Inline::text("before")],
                    after: vec![Inline::text("changed")],
                },
                Step::ReplaceDocument {
                    before: document("not-the-intermediate-state"),
                    after: document("after"),
                },
            ],
            selection(0),
            selection(5),
        );

        assert!(matches!(
            transaction.apply(&mut current),
            Err(TransactionError::DocumentMismatch { step_index: 1 })
        ));
        assert_eq!(current, original);
    }

    #[test]
    fn invalid_after_document_or_selection_leaves_document_unchanged() {
        let original = document("before");
        let mut current = original.clone();
        let invalid_after = Document {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![Block::Heading {
                id: NodeId::new(),
                level: 8,
                content: Vec::new(),
            }],
        };
        let invalid_document_transaction = Transaction::new(
            vec![Step::ReplaceDocument {
                before: original.clone(),
                after: invalid_after,
            }],
            selection(0),
            selection(0),
        );
        assert!(invalid_document_transaction.apply(&mut current).is_err());
        assert_eq!(current, original);

        let invalid_selection_transaction = Transaction::new(
            vec![Step::ReplaceInlineContent {
                block_index: 0,
                before: vec![Inline::text("before")],
                after: vec![Inline::text("after")],
            }],
            selection(6),
            selection(99),
        );
        assert!(matches!(
            invalid_selection_transaction.apply(&mut current),
            Err(TransactionError::InvalidSelection {
                endpoint: SelectionEndpoint::After,
                ..
            })
        ));
        assert_eq!(current, original);
    }
}
