//! Editing commands for Rs Editor: the generic commands every editor needs
//! (delete, join, split, lift, toggle mark, set block type), list
//! commands and key bindings.
//!
//! Ports of [prosemirror-commands](https://github.com/ProseMirror/prosemirror-commands),
//! [prosemirror-schema-list](https://github.com/ProseMirror/prosemirror-schema-list) and
//! [prosemirror-keymap](https://github.com/ProseMirror/prosemirror-keymap)
//! (MIT, see `LICENSE-ProseMirror`). Every command has the signature
//! `(state, dispatch, view) -> bool`; calling it with `dispatch = None`
//! only reports whether it would apply.

mod base;
mod keymap;
mod list;

pub use base::*;
pub use keymap::{KeyEvent, KeyPlatform, Keymap, Modifiers, base_keymap, normalize_key_name};
pub use list::{
    lift_list_item, sink_list_item, split_list_item, split_list_item_keep_marks, wrap_in_list,
    wrap_range_in_list,
};

#[cfg(test)]
mod tests;
