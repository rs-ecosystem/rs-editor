//! Framework-independent document model for OpenRich.
//!
//! This crate intentionally has no UI or rendering dependencies. Public types
//! are still experimental and may change before the first stable release.
//! Deserialization rejects unknown fields and variants so schema data is never
//! silently discarded by an older client.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

/// Current version of the serialized document format.
pub const DOCUMENT_SCHEMA_VERSION: u32 = 2;
const LEGACY_SCHEMA_VERSION: u32 = 1;
static NEXT_NODE_ID: AtomicU64 = AtomicU64::new(1);

/// Stable identity for a block or list item. IDs are unique within a document.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new() -> Self {
        #[allow(deprecated)]
        let id = NEXT_NODE_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .expect("OpenRich node ID space exhausted");
        Self(id.to_string())
    }

    fn observe(&self) -> Result<(), DocumentError> {
        let value = self
            .0
            .parse::<u64>()
            .map_err(|_| DocumentError::InvalidNodeId)?;
        let next = value.checked_add(1).ok_or(DocumentError::InvalidNodeId)?;
        NEXT_NODE_ID.fetch_max(next, Ordering::Relaxed);
        Ok(())
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

/// A document is an ordered collection of editable blocks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

    /// Checks the schema version and document invariants.
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.schema_version != DOCUMENT_SCHEMA_VERSION {
            return Err(DocumentError::UnsupportedSchemaVersion(self.schema_version));
        }
        if self.blocks.is_empty() {
            return Err(DocumentError::DocumentMustContainBlock);
        }
        let mut ids = HashSet::new();
        for block in &self.blocks {
            validate_block(block, &mut ids)?;
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
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(DocumentError::Deserialize)?;
        let version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| {
                DocumentError::Deserialize(
                    serde_json::from_value::<SchemaVersion>(value.clone()).unwrap_err(),
                )
            })?;
        let document = match version {
            LEGACY_SCHEMA_VERSION => {
                let legacy: DocumentV1 =
                    serde_json::from_value(value).map_err(DocumentError::Deserialize)?;
                legacy.migrate()
            }
            DOCUMENT_SCHEMA_VERSION => {
                serde_json::from_value(value).map_err(DocumentError::Deserialize)?
            }
            other => return Err(DocumentError::UnsupportedSchemaVersion(other)),
        };
        document.validate()?;
        Ok(document)
    }
}

/// A block-level document node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Block {
    Formatted {
        id: NodeId,
        content: Vec<Inline>,
        #[serde(default)]
        format: BlockFormat,
    },
    Paragraph {
        id: NodeId,
        content: Vec<Inline>,
    },
    Heading {
        id: NodeId,
        level: u8,
        content: Vec<Inline>,
    },
    BulletList {
        id: NodeId,
        items: Vec<ListItem>,
    },
    OrderedList {
        id: NodeId,
        start: u32,
        items: Vec<ListItem>,
    },
    Blockquote {
        id: NodeId,
        blocks: Vec<Block>,
    },
    HorizontalRule {
        id: NodeId,
    },
}

/// Formatting for an editable top-level paragraph/list item.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockFormat {
    #[serde(default)]
    pub heading_level: Option<u8>,
    #[serde(default)]
    pub alignment: TextAlignment,
    #[serde(default)]
    pub list: ListStyle,
    #[serde(default)]
    pub checked: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListStyle {
    #[default]
    None,
    Bullet,
    Ordered,
    Checkbox,
}

impl Block {
    /// Creates an empty paragraph.
    pub fn paragraph() -> Self {
        Self::Paragraph {
            id: NodeId::new(),
            content: Vec::new(),
        }
    }

    /// Returns the inline content for text-bearing blocks.
    pub fn inline_content(&self) -> Option<&[Inline]> {
        match self {
            Self::Formatted { content, .. }
            | Self::Paragraph { content, .. }
            | Self::Heading { content, .. } => Some(content),
            Self::BulletList { .. }
            | Self::OrderedList { .. }
            | Self::Blockquote { .. }
            | Self::HorizontalRule { .. } => None,
        }
    }

    /// Returns mutable inline content for paragraph and heading blocks.
    pub fn inline_content_mut(&mut self) -> Option<&mut Vec<Inline>> {
        match self {
            Self::Formatted { content, .. }
            | Self::Paragraph { content, .. }
            | Self::Heading { content, .. } => Some(content),
            Self::BulletList { .. }
            | Self::OrderedList { .. }
            | Self::Blockquote { .. }
            | Self::HorizontalRule { .. } => None,
        }
    }

    /// Counts user-perceived characters in a text-bearing block.
    pub fn text_len(&self) -> Option<usize> {
        let content = self.inline_content()?;
        let text = content
            .iter()
            .map(|inline| match inline {
                Inline::Text { text, .. } => text.as_str(),
            })
            .fold(String::new(), |mut combined, part| {
                combined.push_str(part);
                combined
            });
        Some(text.graphemes(true).count())
    }
}

fn validate_block(block: &Block, ids: &mut HashSet<NodeId>) -> Result<(), DocumentError> {
    let id = match block {
        Block::Formatted { id, .. }
        | Block::Paragraph { id, .. }
        | Block::Heading { id, .. }
        | Block::BulletList { id, .. }
        | Block::OrderedList { id, .. }
        | Block::Blockquote { id, .. }
        | Block::HorizontalRule { id } => id.clone(),
    };
    register_id(id, ids)?;
    match block {
        Block::Formatted {
            content, format, ..
        } => {
            if let Some(level) = format.heading_level {
                if !(1..=6).contains(&level) {
                    return Err(DocumentError::InvalidHeadingLevel(level));
                }
            }
            validate_inline_content(content)
        }
        Block::Paragraph { content, .. } => validate_inline_content(content),
        Block::Heading { level, content, .. } => {
            if !(1..=6).contains(level) {
                return Err(DocumentError::InvalidHeadingLevel(*level));
            }
            validate_inline_content(content)
        }
        Block::BulletList { items, .. } | Block::OrderedList { items, .. } => {
            if items.is_empty() {
                return Err(DocumentError::EmptyList);
            }
            for item in items {
                register_id(item.id.clone(), ids)?;
                if item.blocks.is_empty() {
                    return Err(DocumentError::EmptyListItem);
                }
                for child in &item.blocks {
                    validate_block(child, ids)?;
                }
            }
            Ok(())
        }
        Block::Blockquote { blocks, .. } => {
            if blocks.is_empty() {
                return Err(DocumentError::EmptyBlockquote);
            }
            for child in blocks {
                validate_block(child, ids)?;
            }
            Ok(())
        }
        Block::HorizontalRule { .. } => Ok(()),
    }
}

fn register_id(id: NodeId, ids: &mut HashSet<NodeId>) -> Result<(), DocumentError> {
    id.observe()?;
    if !ids.insert(id.clone()) {
        return Err(DocumentError::DuplicateNodeId(id));
    }
    Ok(())
}

fn validate_inline_content(content: &[Inline]) -> Result<(), DocumentError> {
    for inline in content {
        match inline {
            Inline::Text { marks, .. } => {
                for mark in marks {
                    if let Mark::Link { href } = mark {
                        if href.trim().is_empty() {
                            return Err(DocumentError::EmptyLinkTarget);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// A list item containing one or more blocks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListItem {
    pub id: NodeId,
    pub blocks: Vec<Block>,
}

impl ListItem {
    pub fn new(blocks: Vec<Block>) -> Self {
        Self {
            id: NodeId::new(),
            blocks,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaVersion {
    #[serde(rename = "schema_version")]
    _schema_version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentV1 {
    schema_version: u32,
    blocks: Vec<BlockV1>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum BlockV1 {
    Paragraph { content: Vec<Inline> },
    Heading { level: u8, content: Vec<Inline> },
    BulletList { items: Vec<ListItemV1> },
    OrderedList { start: u32, items: Vec<ListItemV1> },
    Blockquote { blocks: Vec<BlockV1> },
    HorizontalRule,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListItemV1 {
    blocks: Vec<BlockV1>,
}

impl DocumentV1 {
    fn migrate(self) -> Document {
        debug_assert_eq!(self.schema_version, LEGACY_SCHEMA_VERSION);
        Document {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            blocks: self.blocks.into_iter().map(BlockV1::migrate).collect(),
        }
    }
}

impl BlockV1 {
    fn migrate(self) -> Block {
        match self {
            Self::Paragraph { content } => Block::Paragraph {
                id: NodeId::new(),
                content,
            },
            Self::Heading { level, content } => Block::Heading {
                id: NodeId::new(),
                level,
                content,
            },
            Self::BulletList { items } => Block::BulletList {
                id: NodeId::new(),
                items: items.into_iter().map(ListItemV1::migrate).collect(),
            },
            Self::OrderedList { start, items } => Block::OrderedList {
                id: NodeId::new(),
                start,
                items: items.into_iter().map(ListItemV1::migrate).collect(),
            },
            Self::Blockquote { blocks } => Block::Blockquote {
                id: NodeId::new(),
                blocks: blocks.into_iter().map(Self::migrate).collect(),
            },
            Self::HorizontalRule => Block::HorizontalRule { id: NodeId::new() },
        }
    }
}

impl ListItemV1 {
    fn migrate(self) -> ListItem {
        ListItem {
            id: NodeId::new(),
            blocks: self.blocks.into_iter().map(BlockV1::migrate).collect(),
        }
    }
}

/// Inline text and marks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Inline {
    Text { text: String, marks: Vec<Mark> },
}

impl Inline {
    /// Creates plain text with no marks.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            marks: Vec::new(),
        }
    }
}

/// Formatting applied to an inline text run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
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
/// `offset` counts extended grapheme clusters in the block's concatenated text.
/// Adapters map platform offsets such as UTF-16 into this model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub anchor: Position,
    pub focus: Position,
}

impl Selection {
    pub fn caret(position: Position) -> Self {
        Self {
            anchor: position.clone(),
            focus: position,
        }
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor.block_index == self.focus.block_index && self.anchor.offset == self.focus.offset
    }
}

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("unsupported document schema version: {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("a document must contain at least one top-level block")]
    DocumentMustContainBlock,
    #[error("node ID must be a decimal u64 below u64::MAX")]
    InvalidNodeId,
    #[error("duplicate node ID: {0:?}")]
    DuplicateNodeId(NodeId),
    #[error("heading level must be between 1 and 6, got {0}")]
    InvalidHeadingLevel(u8),
    #[error("a list must contain at least one item")]
    EmptyList,
    #[error("a list item must contain at least one block")]
    EmptyListItem,
    #[error("a blockquote must contain at least one block")]
    EmptyBlockquote,
    #[error("link target must not be empty")]
    EmptyLinkTarget,
    #[error("could not serialize document: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("could not parse document JSON: {0}")]
    Deserialize(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_v2_round_trips_nested_blocks_and_marks() {
        let document = Document {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![
                Block::Heading {
                    id: NodeId::new(),
                    level: 2,
                    content: vec![Inline::Text {
                        text: "OpenRich".into(),
                        marks: vec![
                            Mark::Bold,
                            Mark::Link {
                                href: "https://example.com".into(),
                            },
                        ],
                    }],
                },
                Block::BulletList {
                    id: NodeId::new(),
                    items: vec![ListItem {
                        id: NodeId::new(),
                        blocks: vec![Block::paragraph()],
                    }],
                },
                Block::Blockquote {
                    id: NodeId::new(),
                    blocks: vec![Block::Paragraph {
                        id: NodeId::new(),
                        content: vec![Inline::text("quoted")],
                    }],
                },
                Block::HorizontalRule { id: NodeId::new() },
            ],
        };

        let json = document.to_json().unwrap();
        assert_eq!(Document::from_json(&json).unwrap(), document);
    }

    #[test]
    fn migrates_v1_documents_and_preserves_assigned_ids_afterward() {
        let legacy = r#"{
            "schema_version": 1,
            "blocks": [
                {"type":"paragraph","content":[{"type":"text","text":"legacy","marks":[]}]},
                {"type":"bullet_list","items":[{"blocks":[{"type":"paragraph","content":[]}]}]}
            ]
        }"#;

        let migrated = Document::from_json(legacy).unwrap();
        assert_eq!(migrated.schema_version, DOCUMENT_SCHEMA_VERSION);
        let first_id = match &migrated.blocks[0] {
            Block::Paragraph { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        let json = migrated.to_json().unwrap();
        let restored = Document::from_json(&json).unwrap();
        assert_eq!(restored, migrated);
        assert!(matches!(&restored.blocks[0], Block::Paragraph { id, .. } if *id == first_id));

        let allocated = Block::paragraph();
        assert!(
            matches!(allocated, Block::Paragraph { id, .. } if id.0.parse::<u64>().unwrap() > first_id.0.parse::<u64>().unwrap())
        );
    }

    #[test]
    fn rejects_duplicate_ids_across_nested_nodes() {
        let shared = NodeId::new();
        let document = Document {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![
                Block::Paragraph {
                    id: shared.clone(),
                    content: Vec::new(),
                },
                Block::Blockquote {
                    id: NodeId::new(),
                    blocks: vec![Block::Paragraph {
                        id: shared.clone(),
                        content: Vec::new(),
                    }],
                },
            ],
        };
        assert!(
            matches!(document.validate(), Err(DocumentError::DuplicateNodeId(id)) if id == shared)
        );
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let json = r#"{"schema_version":99,"blocks":[]}"#;
        assert!(matches!(
            Document::from_json(json),
            Err(DocumentError::UnsupportedSchemaVersion(99))
        ));
    }

    #[test]
    fn rejects_unknown_nodes_and_fields_instead_of_dropping_them() {
        let unknown_node = r#"{"schema_version":1,"blocks":[{"type":"widget"}]}"#;
        let unknown_field =
            r#"{"schema_version":1,"blocks":[{"type":"paragraph","content":[],"align":"center"}]}"#;

        assert!(matches!(
            Document::from_json(unknown_node),
            Err(DocumentError::Deserialize(_))
        ));
        assert!(matches!(
            Document::from_json(unknown_field),
            Err(DocumentError::Deserialize(_))
        ));
    }

    #[test]
    fn rejects_invalid_document_invariants_before_serializing() {
        let invalid = Document {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![Block::Heading {
                id: NodeId::new(),
                level: 7,
                content: Vec::new(),
            }],
        };

        assert!(matches!(
            invalid.to_json(),
            Err(DocumentError::InvalidHeadingLevel(7))
        ));
    }

    #[test]
    fn unicode_content_round_trips_without_normalization() {
        let corpus = [
            "",
            "ascii",
            "مرحبا",
            "שלום",
            "हिन्दी",
            "e\u{301}",
            "👩‍💻",
            "🙂🇮🇩",
        ];
        for text in corpus {
            let document = Document {
                schema_version: DOCUMENT_SCHEMA_VERSION,
                blocks: vec![Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text(text)],
                }],
            };
            let encoded = document.to_json().unwrap();
            assert_eq!(
                Document::from_json(&encoded).unwrap(),
                document,
                "fixture {text:?}"
            );
        }
    }
}
