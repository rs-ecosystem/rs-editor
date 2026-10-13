//! Document layout and painting on Skia's SkParagraph.
//!
//! Text offsets inside a block are UTF-16 units equal to document
//! positions: inline atoms are one U+FFFC placeholder, hard breaks one
//! `\n`. All fonts are bundled (Geist, Geist Mono, Noto for Arabic and
//! Devanagari), so geometry is identical on every platform; the platform's
//! fonts only cover emoji and scripts the bundle lacks.

mod document;
mod emoji;
pub mod fonts;
mod geometry;
pub mod render;
mod style;
mod text;
mod tops;

pub use document::{
    BlockBox, BulletStyle, Decoration, DocumentLayout, Hit, LayoutEngine, TextBox, VisualDirection,
};
pub use geometry::Rect;
pub use rs_editor_text::Affinity;
pub use text::{Caret, CaretStop, Line, TextLayout};
pub use tops::{TopBlock, Tops};

#[cfg(test)]
mod tests;
