//! Markup in and out of Rs Editor documents.
//!
//! HTML is what the clipboard speaks between apps: [`serialize_slice`]
//! writes Tiptap-compatible HTML for copying, and [`parse_slice`] reads HTML
//! from browsers, Google Docs, Word or GitHub into a valid slice of any
//! schema. [`parse_markdown`] and [`serialize_markdown`] read and write
//! GitHub-flavored Markdown.

mod dom;
mod markdown;
mod parse;
mod serialize;

pub use markdown::{parse_markdown, serialize_markdown};
pub use parse::{parse_fragment, parse_slice};
pub use serialize::{serialize_fragment, serialize_slice};

#[cfg(test)]
mod tests;
