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

/// Receives the transaction a command produces. `None` asks the command
/// only whether it would apply (like ProseMirror's `can()` dry run).
pub type Dispatch<'a> = Option<&'a mut dyn FnMut(Transaction)>;

/// A command: inspects the state and, if it applies, dispatches a transaction.
pub type CommandFn = fn(&EditorState, Dispatch) -> bool;

#[cfg(any(test, feature = "test-util"))]
pub mod testing;

#[cfg(test)]
mod tests;
