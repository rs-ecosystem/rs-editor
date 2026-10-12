//! Paints a [`DocumentLayout`] with the theme's shadcn colors.

use openrich_theme::{Color, ColorScheme, Theme};
use skia_safe::{BlendMode, Canvas, Paint, PaintCap, PaintJoin, PaintStyle, PathBuilder, RRect};

use crate::document::{BulletStyle, Decoration, DocumentLayout};
use crate::geometry::Rect;
use crate::style::sk_color;
use crate::text::TextLayout;

fn fill(color: Color) -> Paint {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(sk_color(color));
    paint
}

fn stroke(color: Color, width: f32) -> Paint {
    let mut paint = fill(color);
    paint.set_style(PaintStyle::Stroke);
    paint.set_stroke_width(width);
    paint
}

fn rounded(rect: Rect, radius: f32) -> RRect {
    RRect::new_rect_xy(rect.to_skia(), radius, radius)
}

/// Paints everything of `layout` that intersects `viewport` (document
/// coordinates). The caller sets up scrolling and device scale.
pub fn paint_document(canvas: &Canvas, layout: &DocumentLayout, theme: &Theme, viewport: Rect) {
    let palette = &theme.palette;
    for decoration in &layout.decorations {
        match decoration {
            Decoration::CodeBackground { rect } if rect.intersects(&viewport) => {
                canvas.draw_rrect(rounded(*rect, theme.radius.md), &fill(palette.muted));
            }
            Decoration::QuoteBar { rect } if rect.intersects(&viewport) => {
                canvas.draw_rect(rect.to_skia(), &fill(palette.border));
            }
            _ => {}
        }
    }
    for text in &layout.texts {
        if text.rect.intersects(&viewport) {
            text.layout.paint(canvas, text.rect.x, text.rect.y);
        }
    }
    for decoration in &layout.decorations {
        paint_decoration(canvas, decoration, theme, viewport);
    }
}

fn paint_decoration(canvas: &Canvas, decoration: &Decoration, theme: &Theme, viewport: Rect) {
    let palette = &theme.palette;
    match decoration {
        Decoration::Bullet {
            cx,
            cy,
            radius,
            style,
        } => {
            if !viewport.contains(viewport.x, *cy) {
                return;
            }
            match style {
                BulletStyle::Disc => {
                    canvas.draw_circle((*cx, *cy), *radius, &fill(palette.foreground));
                }
                BulletStyle::Circle => {
                    canvas.draw_circle(
                        (*cx, *cy),
                        *radius - 0.5,
                        &stroke(palette.foreground, 1.25),
                    );
                }
                BulletStyle::Square => {
                    let side = radius * 1.8;
                    canvas.draw_rect(
                        Rect::new(cx - side / 2.0, cy - side / 2.0, side, side).to_skia(),
                        &fill(palette.foreground),
                    );
                }
            }
        }
        Decoration::Number { x, y, layout } => {
            if viewport.contains(viewport.x, *y)
                || viewport.contains(viewport.x, y + layout.height())
            {
                layout.paint(canvas, *x, *y);
            }
        }
        Decoration::Checkbox { rect, checked, .. } => {
            if !rect.intersects(&viewport) {
                return;
            }
            let shape = rounded(*rect, 4.0);
            if *checked {
                canvas.draw_rrect(shape, &fill(palette.primary));
                let mut check = stroke(palette.primary_foreground, 1.75);
                check.set_stroke_cap(PaintCap::Round);
                check.set_stroke_join(PaintJoin::Round);
                let s = rect.width / 16.0;
                let mut path = PathBuilder::new();
                path.move_to((rect.x + 4.0 * s, rect.y + 8.5 * s));
                path.line_to((rect.x + 6.75 * s, rect.y + 11.0 * s));
                path.line_to((rect.x + 12.0 * s, rect.y + 5.25 * s));
                canvas.draw_path(&path.detach(), &check);
            } else {
                canvas.draw_rrect(shape, &fill(palette.background));
                let inset = Rect::new(
                    rect.x + 0.5,
                    rect.y + 0.5,
                    rect.width - 1.0,
                    rect.height - 1.0,
                );
                canvas.draw_rrect(rounded(inset, 3.5), &stroke(palette.input, 1.0));
            }
        }
        Decoration::Rule { rect } => {
            if rect.intersects(&viewport) {
                canvas.draw_rect(rect.to_skia(), &fill(palette.border));
            }
        }
        Decoration::Image { rect, .. } => {
            if rect.intersects(&viewport) {
                canvas.draw_rrect(rounded(*rect, theme.radius.md), &fill(palette.muted));
                let inset = Rect::new(
                    rect.x + 0.5,
                    rect.y + 0.5,
                    rect.width - 1.0,
                    rect.height - 1.0,
                );
                canvas.draw_rrect(
                    rounded(inset, theme.radius.md),
                    &stroke(palette.border, 1.0),
                );
            }
        }
        Decoration::CodeBackground { .. } | Decoration::QuoteBar { .. } => {}
    }
}

/// Fills selection highlight rectangles.
/// Tints the selected ranges. Paint it after the document: multiply (light)
/// or screen (dark) keeps text and code/highlight backgrounds visible under
/// the tint, and looks the same as plain blending on the page background.
pub fn paint_selection(canvas: &Canvas, rects: &[Rect], theme: &Theme) {
    let mut paint = fill(theme.palette.selection);
    paint.set_blend_mode(match theme.scheme {
        ColorScheme::Light => BlendMode::Multiply,
        ColorScheme::Dark => BlendMode::Screen,
    });
    for rect in rects {
        canvas.draw_rect(rect.to_skia(), &paint);
    }
}

/// Outlines a selected leaf block (rule, image) with the focus ring color.
pub fn paint_node_selection(canvas: &Canvas, rect: Rect, theme: &Theme) {
    let outset = Rect::new(
        rect.x - 2.0,
        rect.y - 2.0,
        rect.width + 4.0,
        rect.height + 4.0,
    );
    canvas.draw_rrect(
        rounded(outset, theme.radius.sm),
        &stroke(theme.palette.ring, 2.0),
    );
}

/// Draws the caret bar.
pub fn paint_caret(canvas: &Canvas, rect: Rect, theme: &Theme) {
    canvas.draw_rrect(rounded(rect, rect.width / 2.0), &fill(theme.palette.caret));
}

/// Draws a placeholder label at (`x`, `y`).
pub fn paint_placeholder(canvas: &Canvas, layout: &TextLayout, x: f32, y: f32) {
    layout.paint(canvas, x, y);
}
