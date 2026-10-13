//! Maps nodes and marks to Skia text styles using the theme.

use rs_editor_model::{MarkSet, Node};
use rs_editor_theme::{Color, Theme};
use skia_safe::font_style::{Slant, Weight, Width};
use skia_safe::textlayout::{StrutStyle, TextAlign, TextDecoration, TextStyle};
use skia_safe::{FontStyle, Paint};

use crate::fonts::SCRIPT_FALLBACKS;

/// The base style of a textblock.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockStyle {
    pub size: f32,
    pub line_height: f32,
    pub weight: u16,
    pub letter_spacing_em: f32,
    pub mono: bool,
    pub italic: bool,
    pub color: Color,
    pub align: TextAlign,
}

pub(crate) fn sk_color(color: Color) -> skia_safe::Color {
    skia_safe::Color::new(color.argb())
}

/// The theme's family, then the bundled script fonts, then `extra`
/// families added at run time (emoji and CJK fonts loaded by the host).
fn families(theme: &Theme, mono: bool, extra: &[String]) -> Vec<String> {
    let primary = if mono {
        &theme.typography.mono
    } else {
        &theme.typography.sans
    };
    std::iter::once(primary.clone())
        .chain(SCRIPT_FALLBACKS.iter().map(|f| (*f).to_owned()))
        .chain(extra.iter().cloned())
        .collect()
}

fn font_style(weight: u16, italic: bool) -> FontStyle {
    FontStyle::new(
        Weight::from(i32::from(weight)),
        Width::NORMAL,
        if italic {
            Slant::Italic
        } else {
            Slant::Upright
        },
    )
}

fn align_of(node: &Node) -> TextAlign {
    match node.attr("textAlign").and_then(|v| v.as_str()) {
        Some("center") => TextAlign::Center,
        Some("right") => TextAlign::Right,
        Some("justify") => TextAlign::Justify,
        _ => TextAlign::Left,
    }
}

/// The style of textblock `node` (paragraph, heading level, code block).
pub fn block_style(theme: &Theme, node: &Node, italic: bool) -> BlockStyle {
    let typography = &theme.typography;
    let palette = &theme.palette;
    let base = match node.type_name() {
        "heading" => theme.heading(node.attr("level").and_then(|v| v.as_u64()).unwrap_or(1)),
        _ if node.node_type().spec().code => typography.code_block,
        _ => typography.paragraph,
    };
    BlockStyle {
        size: base.size,
        line_height: base.line_height,
        weight: base.weight,
        letter_spacing_em: base.letter_spacing,
        mono: node.node_type().spec().code,
        italic,
        color: palette.foreground,
        align: align_of(node),
    }
}

/// A Skia style for the block itself (used for the strut and empty text).
pub fn base_text_style(theme: &Theme, block: &BlockStyle, extra: &[String]) -> TextStyle {
    let mut style = TextStyle::new();
    style.set_font_families(&families(theme, block.mono, extra));
    style.set_font_size(block.size);
    style.set_font_style(font_style(block.weight, block.italic));
    style.set_letter_spacing(block.letter_spacing_em * block.size);
    style.set_color(sk_color(block.color));
    style.set_height(block.line_height / block.size);
    style.set_height_override(true);
    style.set_half_leading(true);
    style
}

/// A strut that gives every line the block's line height, like CSS
/// `line-height`, so mixed fonts and emoji do not change line spacing.
pub fn strut(theme: &Theme, block: &BlockStyle, extra: &[String]) -> StrutStyle {
    let mut strut = StrutStyle::new();
    strut.set_font_families(&families(theme, block.mono, extra));
    strut.set_font_size(block.size);
    strut.set_height(block.line_height / block.size);
    strut.set_height_override(true);
    strut.set_half_leading(true);
    strut.set_strut_enabled(true);
    strut.set_force_strut_height(true);
    strut
}

/// The Skia style of a text run with `marks` inside `block`.
pub fn run_style(
    theme: &Theme,
    block: &BlockStyle,
    marks: &MarkSet,
    extra: &[String],
) -> TextStyle {
    let typography = &theme.typography;
    let palette = &theme.palette;
    let mut size = block.size;
    let mut weight = block.weight;
    let mut italic = block.italic;
    let mut mono = block.mono;
    let mut color = block.color;
    let mut decoration = TextDecoration::NO_DECORATION;
    let mut background: Option<Color> = None;
    let mut baseline_shift = 0.0;
    for mark in marks.iter() {
        match mark.mark_type().name() {
            "bold" => weight = weight.max(typography.bold_weight),
            "italic" => italic = true,
            "underline" => decoration |= TextDecoration::UNDERLINE,
            "strike" => decoration |= TextDecoration::LINE_THROUGH,
            "code" => {
                mono = true;
                size *= typography.inline_code_scale;
                weight = weight.max(typography.inline_code_weight);
                background = Some(palette.muted);
            }
            "link" => {
                color = palette.link;
                decoration |= TextDecoration::UNDERLINE;
            }
            "highlight" => {
                background = Some(
                    mark.attr("color")
                        .and_then(|c| c.as_str())
                        .and_then(Color::from_hex)
                        .unwrap_or(palette.highlight),
                );
            }
            "subscript" => {
                size *= typography.script_scale;
                baseline_shift = block.size * 0.2;
            }
            "superscript" => {
                size *= typography.script_scale;
                baseline_shift = -block.size * 0.35;
            }
            _ => {}
        }
    }
    let mut style = TextStyle::new();
    style.set_font_families(&families(theme, mono, extra));
    style.set_font_size(size);
    style.set_font_style(font_style(weight, italic));
    style.set_letter_spacing(block.letter_spacing_em * size);
    style.set_color(sk_color(color));
    style.set_height(block.line_height / size);
    style.set_height_override(true);
    style.set_half_leading(true);
    if decoration != TextDecoration::NO_DECORATION {
        style.set_decoration_type(decoration);
        style.set_decoration_color(sk_color(color));
    }
    if let Some(background) = background {
        let mut paint = Paint::default();
        paint.set_color(sk_color(background));
        paint.set_anti_alias(true);
        style.set_background_paint(&paint);
    }
    if baseline_shift != 0.0 {
        style.set_baseline_shift(baseline_shift);
    }
    style
}
