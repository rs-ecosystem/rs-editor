//! The floating surface of popovers and toolbars (shadcn's popover: the
//! popover color, a hairline border, a medium radius and a soft shadow).

use rs_editor_theme::Theme;
use skia_safe::{BlurStyle, Canvas, MaskFilter, Paint, PaintStyle, RRect, Rect};

use crate::{sk, with_alpha};

/// Space between a panel's edge and its content.
pub const PANEL_PADDING: f32 = 4.0;

/// Paints a panel filling `rect` at `opacity`.
pub fn paint_panel(canvas: &Canvas, rect: Rect, theme: &Theme, opacity: f32) {
    let radius = theme.radius.md;
    let shape = RRect::new_rect_xy(rect, radius, radius);
    let dark = theme.palette.background.argb() & 0x00ff_ffff < 0x0080_8080;

    let mut shadow = Paint::default();
    shadow.set_anti_alias(true);
    shadow.set_color(with_alpha(
        skia_safe::Color::BLACK,
        opacity * if dark { 0.5 } else { 0.12 },
    ));
    shadow.set_mask_filter(MaskFilter::blur(BlurStyle::Normal, 6.0, false));
    canvas.draw_rrect(shape.with_offset((0.0, 4.0)), &shadow);

    let mut fill = Paint::default();
    fill.set_anti_alias(true);
    fill.set_color(with_alpha(sk(theme.palette.popover), opacity));
    canvas.draw_rrect(shape, &fill);

    let mut border = Paint::default();
    border.set_anti_alias(true);
    border.set_style(PaintStyle::Stroke);
    border.set_stroke_width(1.0);
    border.set_color(with_alpha(sk(theme.palette.border), opacity));
    canvas.draw_rrect(shape.with_inset((0.5, 0.5)), &border);
}
