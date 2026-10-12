//! Tiptap-style extensions and a headless editor for OpenRich.
//!
//! Extensions contribute schema (nodes, marks, global attributes), named
//! commands, key bindings, input rules and plugins. [`Editor`] combines
//! them with an [`openrich_state::EditorState`] behind an API meant for
//! hosts in any language: commands are invoked by name with JSON
//! arguments (`run("toggleHeading", {"level": 2})`), content is Tiptap
//! JSON, and toolbar state comes from [`Editor::is_active`] and
//! [`Editor::can`].

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
mod tests;
