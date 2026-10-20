//! Editor state for Rs Editor: the document, the selection, stored marks
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

/// Reborrows a [`Dispatch`] so it can be passed on and used again.
pub fn reborrow<'a>(dispatch: &'a mut Dispatch<'_>) -> Dispatch<'a> {
    match dispatch {
        Some(dispatch) => Some(&mut **dispatch),
        None => None,
    }
}

/// Direction for [`ViewContext::end_of_textblock`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextDirection {
    /// Logical backward (Backspace).
    Backward,
    /// Logical forward (Delete).
    Forward,
    Left,
    Right,
    Up,
    Down,
}

/// What a command may ask the view. Commands also run without a view
/// (headless, tests); they then fall back to logical answers.
pub trait ViewContext {
    /// Whether the cursor is at the visual end of its textblock in `dir`
    /// (accounts for bidi text and line wrapping).
    fn end_of_textblock(&self, dir: TextDirection, state: &EditorState) -> bool;
}

/// A command: inspects the state and, if it applies, dispatches a transaction.
pub type CommandFn = fn(&EditorState, Dispatch, Option<&dyn ViewContext>) -> bool;

/// A shareable command, e.g. built by a parameterized constructor.
pub type Command =
    std::sync::Arc<dyn Fn(&EditorState, Dispatch, Option<&dyn ViewContext>) -> bool + Send + Sync>;

/// Wraps a plain command function as a [`Command`].
pub fn command(f: CommandFn) -> Command {
    std::sync::Arc::new(f)
}

/// Runs `commands` in order until one applies.
pub fn chain_commands(commands: Vec<Command>) -> Command {
    std::sync::Arc::new(
        move |state: &EditorState, mut dispatch: Dispatch, view: Option<&dyn ViewContext>| {
            commands.iter().any(|cmd| {
                cmd(
                    state,
                    dispatch
                        .as_deref_mut()
                        .map(|d| d as &mut dyn FnMut(Transaction)),
                    view,
                )
            })
        },
    )
}

#[cfg(any(test, feature = "test-util"))]
pub mod testing;

#[cfg(test)]
mod tests;
