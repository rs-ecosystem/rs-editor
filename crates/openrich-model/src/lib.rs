//! Document model for OpenRich: a schema-checked, immutable node tree.
//!
//! This is a Rust port of the model layer of
//! [ProseMirror](https://prosemirror.net) (MIT, see `LICENSE-ProseMirror`),
//! so documents, positions and JSON stay interoperable with Tiptap.
//!
//! Positions count UTF-16 code units inside text, plus one for every node
//! boundary, exactly like ProseMirror. A position may never fall inside a
//! surrogate pair; [`Node::resolve`] rejects such positions.

mod content;
mod error;
mod fragment;
mod json;
mod mark;
mod node;
mod replace;
mod resolved;
mod schema;
pub mod text;

#[cfg(any(test, feature = "test-util"))]
pub mod testing;

pub use content::ContentMatch;
pub use error::{ModelError, Result};
pub use fragment::{Fragment, IndexOffset};
pub use json::fragment_to_json;
pub use mark::{Mark, MarkSet};
pub use node::{ChildInfo, Node, NodeVisitor};
pub use replace::Slice;
pub use resolved::{NodeRange, ResolvedPos};
pub use schema::{AttrSpec, Attrs, MarkSpec, MarkType, NodeSpec, NodeType, Schema, SchemaSpec};

#[cfg(test)]
mod tests;
