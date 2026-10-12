//! Editor state for OpenRich: the document, the selection, stored marks
//! and plugin state, changed only through [`Transaction`]s.
//!
//! A Rust port of [prosemirror-state](https://github.com/ProseMirror/prosemirror-state)
//! (MIT, see `LICENSE-ProseMirror`).

mod plugin;
mod selection;
mod state;
mod transaction;

pub use plugin::{Clock, ManualClock, Plugin, PluginValue, SystemClock};
pub use selection::{AllSelection, NodeSelection, Selection, SelectionBookmark, TextSelection};
pub use state::{AppliedTransaction, EditorState, EditorStateConfig, StateError};
pub use transaction::{MetaValue, Transaction, meta};

#[cfg(any(test, feature = "test-util"))]
pub mod testing;

#[cfg(test)]
mod tests;
