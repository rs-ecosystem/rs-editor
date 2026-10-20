use thiserror::Error;

/// Errors produced while building schemas, nodes and slices.
///
/// The model never panics on caller input; every operation that can be fed an
/// invalid position, attribute or content returns one of these instead.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelError {
    #[error("invalid schema: {0}")]
    Schema(String),
    #[error("unknown node type `{0}`")]
    UnknownNodeType(String),
    #[error("unknown mark type `{0}`")]
    UnknownMarkType(String),
    #[error("missing required attribute `{attr}` on `{owner}`")]
    MissingAttr { owner: String, attr: String },
    #[error("invalid value for attribute `{attr}` on `{owner}`: {reason}")]
    InvalidAttr {
        owner: String,
        attr: String,
        reason: String,
    },
    #[error("invalid content for node `{0}`")]
    InvalidContent(String),
    #[error("invalid marks on a node inside `{0}`")]
    InvalidMarks(String),
    #[error("empty text nodes are not allowed")]
    EmptyText,
    #[error("position {pos} is outside the document (size {size})")]
    PositionOutOfRange { pos: usize, size: usize },
    #[error("position {0} splits a UTF-16 surrogate pair")]
    PositionInsideCharacter(usize),
    #[error("{0}")]
    Replace(String),
    #[error("invalid JSON: {0}")]
    Json(String),
}

pub type Result<T, E = ModelError> = std::result::Result<T, E>;
