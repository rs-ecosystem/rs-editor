//! Renderer-independent geometry and scene contract for OpenRich.
//!
//! Concrete backends such as Skia will live in separate crates and consume
//! this data. This crate must not depend on a windowing toolkit.

/// Logical pixel rectangle in host-view coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One operation in a renderer-neutral display list.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    Clear { color: Color },
    FillRect { rect: Rect, color: Color },
    DrawText { text: String, origin_x: f32, baseline_y: f32, color: Color, size: f32 },
}

/// RGBA color with channels normalized to 0.0–1.0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

/// A frame of renderer-neutral commands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayList {
    pub commands: Vec<DrawCommand>,
}
