//! Markup in and out of OpenRich documents.
//!
//! HTML is what the clipboard speaks between apps: [`serialize_slice`]
//! writes Tiptap-compatible HTML for copying, and [`parse_slice`] reads HTML
//! from browsers, Google Docs, Word or GitHub into a valid slice of any
//! schema. Markdown follows in P3.

mod dom;
mod parse;
mod serialize;

pub use parse::{parse_fragment, parse_slice};
pub use serialize::{serialize_fragment, serialize_slice};

#[cfg(test)]
mod tests;
