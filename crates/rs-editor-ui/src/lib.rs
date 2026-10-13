//! The editor's own UI, drawn with Skia inside the canvas so it looks the
//! same on every platform: popover panels, toolbars of icon buttons and
//! menus in the shadcn/ui style of the theme, with Lucide icons.
//!
//! Widgets here know nothing of documents; the view decides what they show
//! and what their buttons do.

mod icons;
mod panel;
mod toolbar;
mod tween;

pub use icons::Icon;
pub use panel::{PANEL_PADDING, paint_panel};
pub use toolbar::{Toolbar, ToolbarButton, ToolbarEntry};
pub use tween::Appear;

pub(crate) fn sk(color: rs_editor_theme::Color) -> skia_safe::Color {
    skia_safe::Color::new(color.argb())
}

pub(crate) fn with_alpha(color: skia_safe::Color, opacity: f32) -> skia_safe::Color {
    color.with_a((f32::from(color.a()) * opacity.clamp(0.0, 1.0)).round() as u8)
}

#[cfg(test)]
mod tests;
