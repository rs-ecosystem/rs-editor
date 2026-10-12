//! Document changes for OpenRich: steps, position mapping and the
//! [`Transform`] helpers built on them.
//!
//! A Rust port of [prosemirror-transform](https://github.com/ProseMirror/prosemirror-transform)
//! (MIT, see `LICENSE-ProseMirror`). Steps serialize to the same JSON, so
//! change logs stay compatible with ProseMirror/Tiptap collaboration tools.

mod map;
mod mark;
mod replace;
mod step;
mod structure;
mod transform;

pub use map::{Assoc, MapResult, Mappable, Mapping, Recover, StepMap};
pub use replace::replace_step;
pub use step::{
    AttrStep, DocAttrStep, MarkStep, NodeMarkStep, ReplaceAroundStep, ReplaceStep, Step, StepError,
};
pub use structure::{
    can_join, can_split, drop_point, find_wrapping, insert_point, join_point, lift_target,
};
pub use transform::{MarkMatch, NodeTypeWithAttrs, Transform, TransformError, TransformResult};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod proptests;
