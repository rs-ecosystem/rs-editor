//! Framework-independent document model for OpenRich.
//!
//! This crate intentionally has no UI or rendering dependencies. Public types
//! are still experimental and may change before the first stable release.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current version of the serialized document format.
pub const DOCUMENT_SCHEMA_VERSION: u32 = 1;

/// A document is an ordered collection of editable blocks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Version of the serialized schema used by this document.
    pub schema_version: u32,
    /// Top-level blocks in reading order.
    pub blocks: Vec<Block>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![Block::paragraph()],
        }
    }
}

impl Document {
    /// Creates an empty document containing one editable paragraph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks the schema version and invariants needed by the initial model.
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.schema_version != DOCUMENT_SCHEMA_VERSION {
            return Err(DocumentError::UnsupportedSchemaVersion(self.schema_version));
        }
        if self.blocks.is_empty() {
            return Err(DocumentError::DocumentMustContainBlock);
        }
        Ok(())
    }

    /// Serializes this document as compact JSON after validating it.
    pub fn to_json(&self) -> Result<String, DocumentError> {
        self.validate()?;
        serde_json::to_string(self).map_err(DocumentError::Serialize)
    }

    /// Parses a document from JSON and validates its schema version.
    pub fn from_json(json: &str) -> Result<Self, DocumentError> {
        let document: Self = serde_json::from_str(json).map_err(DocumentError::Deserialize)?;
        document.validate()?;
        Ok(document)
    }
}

/// A block-level document node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Paragraph { content: Vec<Inline> },
    Heading { level: u8, content: Vec<Inline> },
    BulletList { items: Vec<ListItem> },
    OrderedList { start: u32, items: Vec<ListItem> },
    Blockquote { blocks: Vec<Block> },
    HorizontalRule,
}

impl Block {
    /// Creates an empty paragraph.
    pub fn paragraph() -> Self {
        Self::Paragraph { content: Vec::new() }
    }

    /// Returns the inline content for text-bearing blocks.
    pub fn inline_content(&self) -> Option<&[Inline]> {
        match self {
            Self::Paragraph { content } | Self::Heading { content, .. } => Some(content),
            Self::BulletList { .. }
            | Self::OrderedList { .. }
            | Self::Blockquote { .. }
            | Self::HorizontalRule => None,
        }
    }
}

/// A list item containing one or more blocks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

/// Inline text and marks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inline {
    Text { text: String, marks: Vec<Mark> },
}

impl Inline {
    /// Creates plain text with no marks.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into(), marks: Vec::new() }
    }
}

/// Formatting applied to an inline text run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Mark {
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
    Link { href: String },
}

/// Stable position within an editable block.
///
/// `offset` is a Unicode scalar-value offset in v0. Editing commands must snap
/// user-visible caret positions to extended grapheme boundaries. Adapters are
/// responsible for mapping platform offsets such as UTF-16 into this model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub block_index: usize,
    pub offset: usize,
    pub affinity: Affinity,
}

/// Determines which side of a boundary a position belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Affinity {
    #[default]
    Downstream,
    Upstream,
}

/// A caret or a directed text range.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub anchor: Position,
    pub focus: Position,
}

impl Selection {
    pub fn caret(position: Position) -> Self {
        Self { anchor: position.clone(), focus: position }
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }
}

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("unsupported document schema version: {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("a document must contain at least one top-level block")]
    DocumentMustContainBlock,
    #[error("could not serialize document: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("could not parse document JSON: {0}")]
    Deserialize(#[source] serde_json::Error),
}
