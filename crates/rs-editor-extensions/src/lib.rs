//! Tiptap-style extensions and a headless editor for Rs Editor.
//!
//! Extensions contribute schema (nodes, marks, global attributes), named
//! commands, key bindings, input rules and plugins. [`Editor`] combines
//! them with an [`rs_editor_state::EditorState`] behind an API meant for
//! hosts in any language: commands are invoked by name with JSON
//! arguments (`run("toggleHeading", {"level": 2})`), content is Tiptap
//! JSON, and toolbar state comes from [`Editor::is_active`] and
//! [`Editor::can`].
//!
//! ```
//! use rs_editor_extensions::{Editor, EditorOptions};
//! use serde_json::json;
//!
//! # fn main() -> Result<(), rs_editor_extensions::ExtensionError> {
//! let mut editor = Editor::new(EditorOptions {
//!     content: Some(json!({"type": "doc", "content": [{"type": "paragraph"}]})),
//!     ..Default::default()
//! })?;
//!
//! editor.type_text("# ");            // markdown input rule: heading
//! editor.type_text("Halo dunia");
//! editor.select(1, 5);
//! editor.run("toggleBold", &json!(null))?;
//! assert!(editor.is_active("bold", &Default::default()));
//! assert_eq!(editor.get_json()["content"][0]["type"], "heading");
//! # Ok(())
//! # }
//! ```

mod core_commands;
mod editor;
mod extension;
pub mod helpers;
pub mod inputrules;
pub mod starter_kit;

pub use core_commands::*;
pub use editor::{ChainBuilder, Editor, EditorEvent, EditorOptions, Listener};
pub use extension::{
    CommandFactory, Extension, ExtensionContext, ExtensionError, ExtensionManager, GlobalAttribute,
};

#[cfg(test)]
mod proptests;
#[cfg(test)]
mod tests;
