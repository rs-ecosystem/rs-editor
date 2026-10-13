//! Visual tokens for Rs Editor in shadcn/ui terms.
//!
//! Colors come from a `tokens.json` with the shape used by Rs Organizer
//! (`{"light": {...}, "dark": {...}, "radius": {...}, "font": {...}}`), so
//! the editor matches the host app exactly. Typography and spacing follow
//! shadcn/ui's typography styles.

use serde_json::Value;
use thiserror::Error;

/// The tokens bundled with Rs Editor (shadcn neutral, Geist).
pub const DEFAULT_TOKENS: &str = include_str!("../../../design/tokens.json");

/// An sRGB color with alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// Parses `#RGB`, `#RRGGBB` or `#RRGGBBAA`.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let digits = hex.strip_prefix('#')?;
        let channel = |i: usize| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok();
        match digits.len() {
            3 => {
                let expand = |i: usize| {
                    u8::from_str_radix(&digits[i..i + 1], 16)
                        .ok()
                        .map(|v| v * 17)
                };
                Some(Self::rgb(expand(0)?, expand(1)?, expand(2)?))
            }
            6 => Some(Self::rgb(channel(0)?, channel(2)?, channel(4)?)),
            8 => Some(Self::rgba(
                channel(0)?,
                channel(2)?,
                channel(4)?,
                channel(6)?,
            )),
            _ => None,
        }
    }

    /// The same color with alpha `alpha` (0–1).
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self {
            a: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            ..self
        }
    }

    /// 0xAARRGGBB, as Skia's `SkColor`.
    pub fn argb(self) -> u32 {
        (u32::from(self.a) << 24)
            | (u32::from(self.r) << 16)
            | (u32::from(self.g) << 8)
            | u32::from(self.b)
    }
}

/// Light or dark appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
}

/// shadcn color tokens plus the editor-specific ones.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub background: Color,
    pub foreground: Color,
    pub popover: Color,
    pub popover_foreground: Color,
    pub primary: Color,
    pub primary_foreground: Color,
    pub secondary: Color,
    pub secondary_foreground: Color,
    pub muted: Color,
    pub muted_foreground: Color,
    pub accent: Color,
    pub accent_foreground: Color,
    pub destructive: Color,
    pub border: Color,
    pub input: Color,
    pub ring: Color,
    /// Text selection highlight.
    pub selection: Color,
    /// The text cursor.
    pub caret: Color,
    /// Background of the `highlight` mark.
    pub highlight: Color,
    pub link: Color,
}

/// Font and line height of one block style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    /// Absolute line height in logical pixels.
    pub line_height: f32,
    /// 100–900.
    pub weight: u16,
    /// Extra space between characters, in ems (negative tightens).
    pub letter_spacing: f32,
}

/// Typography following shadcn/ui's typography page.
#[derive(Debug, Clone, PartialEq)]
pub struct Typography {
    pub sans: String,
    pub mono: String,
    pub paragraph: TextStyle,
    /// `h1`..`h6`.
    pub headings: [TextStyle; 6],
    /// Code blocks.
    pub code_block: TextStyle,
    /// Inline code size relative to the surrounding text.
    pub inline_code_scale: f32,
    pub inline_code_weight: u16,
    pub bold_weight: u16,
    /// Sub- and superscript size relative to the surrounding text.
    pub script_scale: f32,
}

/// Spacing in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spacing {
    /// Space between top-level blocks.
    pub block_gap: f32,
    /// Extra space above headings.
    pub heading_gap: f32,
    /// Indentation of list content.
    pub list_indent: f32,
    /// Space between list items.
    pub list_item_gap: f32,
    pub quote_indent: f32,
    pub quote_border: f32,
    pub code_block_padding: f32,
    /// Horizontal padding of inline code.
    pub inline_code_padding: f32,
    pub checkbox_size: f32,
    pub caret_width: f32,
}

/// Corner radii (`sm`, `md`, `lg`, `xl`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radius {
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
}

/// Everything the renderer needs to look like the host app.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub scheme: ColorScheme,
    pub palette: Palette,
    pub typography: Typography,
    pub spacing: Spacing,
    pub radius: Radius,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ThemeError {
    #[error("invalid tokens JSON: {0}")]
    Json(String),
    #[error("missing or invalid color token `{0}`")]
    Color(String),
}

fn heading(size: f32, line_height: f32, weight: u16, letter_spacing: f32) -> TextStyle {
    TextStyle {
        size,
        line_height,
        weight,
        letter_spacing,
    }
}

impl Theme {
    /// Builds a theme from Rs Organizer–style tokens.
    pub fn from_tokens_json(json: &str, scheme: ColorScheme) -> Result<Self, ThemeError> {
        let tokens: Value =
            serde_json::from_str(json).map_err(|e| ThemeError::Json(e.to_string()))?;
        let section = match scheme {
            ColorScheme::Light => "light",
            ColorScheme::Dark => "dark",
        };
        let colors = tokens
            .get(section)
            .and_then(Value::as_object)
            .ok_or_else(|| ThemeError::Json(format!("missing `{section}`")))?;
        let color = |name: &str| {
            colors
                .get(name)
                .and_then(Value::as_str)
                .and_then(Color::from_hex)
                .ok_or_else(|| ThemeError::Color(name.to_owned()))
        };
        let optional = |name: &str, fallback: Color| color(name).unwrap_or(fallback);
        let foreground = color("foreground")?;
        let primary = color("primary")?;
        let (selection, highlight) = match scheme {
            ColorScheme::Light => (
                Color::rgb(0x3B, 0x82, 0xF6).with_alpha(0.22),
                Color::rgb(0xFE, 0xF0, 0x8A),
            ),
            ColorScheme::Dark => (
                Color::rgb(0x60, 0xA5, 0xFA).with_alpha(0.32),
                Color::rgb(0x85, 0x4D, 0x0E),
            ),
        };
        let palette = Palette {
            background: color("background")?,
            foreground,
            popover: optional("popover", color("background")?),
            popover_foreground: optional("popover-foreground", foreground),
            primary,
            primary_foreground: color("primary-foreground")?,
            secondary: color("secondary")?,
            secondary_foreground: optional("secondary-foreground", foreground),
            muted: color("muted")?,
            muted_foreground: color("muted-foreground")?,
            accent: color("accent")?,
            accent_foreground: optional("accent-foreground", foreground),
            destructive: color("destructive")?,
            border: color("border")?,
            input: optional("input", color("border")?),
            ring: color("ring")?,
            selection: optional("selection", selection),
            caret: optional("caret", foreground),
            highlight: optional("highlight", highlight),
            link: optional("link", primary),
        };
        let radius_of = |name: &str, fallback: f32| {
            tokens
                .pointer(&format!("/radius/{name}"))
                .and_then(Value::as_f64)
                .map_or(fallback, |v| v as f32)
        };
        let font_of = |name: &str, fallback: &str| {
            tokens
                .pointer(&format!("/font/{name}"))
                .and_then(Value::as_str)
                .unwrap_or(fallback)
                .to_owned()
        };
        let base = 16.0;
        Ok(Self {
            scheme,
            palette,
            typography: Typography {
                sans: font_of("sans", "Geist"),
                mono: font_of("mono", "Geist Mono"),
                paragraph: heading(base, 28.0, 400, 0.0),
                headings: [
                    heading(36.0, 40.0, 800, -0.025),
                    heading(30.0, 36.0, 600, -0.025),
                    heading(24.0, 32.0, 600, -0.025),
                    heading(20.0, 28.0, 600, -0.025),
                    heading(18.0, 28.0, 600, 0.0),
                    heading(16.0, 28.0, 600, 0.0),
                ],
                code_block: heading(14.0, 24.0, 400, 0.0),
                inline_code_scale: 0.875,
                inline_code_weight: 600,
                bold_weight: 700,
                script_scale: 0.75,
            },
            spacing: Spacing {
                block_gap: 12.0,
                heading_gap: 12.0,
                list_indent: 24.0,
                list_item_gap: 4.0,
                quote_indent: 24.0,
                quote_border: 2.0,
                code_block_padding: 16.0,
                inline_code_padding: 4.8,
                checkbox_size: 16.0,
                caret_width: 1.5,
            },
            radius: Radius {
                sm: radius_of("sm", 6.0),
                md: radius_of("md", 8.0),
                lg: radius_of("lg", 10.0),
                xl: radius_of("xl", 14.0),
            },
        })
    }

    pub fn light() -> Self {
        Self::from_tokens_json(DEFAULT_TOKENS, ColorScheme::Light)
            .expect("bundled tokens are valid")
    }

    pub fn dark() -> Self {
        Self::from_tokens_json(DEFAULT_TOKENS, ColorScheme::Dark).expect("bundled tokens are valid")
    }

    /// The style of heading `level` (1–6, clamped).
    pub fn heading(&self, level: u64) -> TextStyle {
        self.typography.headings[(level.clamp(1, 6) - 1) as usize]
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_tokens_match_rs_organizer_shadcn_palette() {
        let light = Theme::light();
        assert_eq!(light.palette.background, Color::rgb(0xFF, 0xFF, 0xFF));
        assert_eq!(light.palette.foreground, Color::rgb(0x0A, 0x0A, 0x0A));
        assert_eq!(light.palette.muted_foreground, Color::rgb(0x73, 0x73, 0x73));
        assert_eq!(light.radius.md, 8.0);
        assert_eq!(light.typography.sans, "Geist");
        assert_eq!(light.typography.mono, "Geist Mono");
        let dark = Theme::dark();
        assert_eq!(dark.palette.background, Color::rgb(0x0A, 0x0A, 0x0A));
        assert_eq!(dark.palette.border, Color::rgba(0xFF, 0xFF, 0xFF, 0x1A));
        assert_eq!(light.heading(1).weight, 800);
        assert_eq!(light.heading(9).size, 16.0);
    }

    #[test]
    fn colors_parse_and_convert() {
        assert_eq!(Color::from_hex("#fff"), Some(Color::rgb(255, 255, 255)));
        assert_eq!(
            Color::from_hex("#E7000B1A"),
            Some(Color::rgba(0xE7, 0, 0x0B, 0x1A))
        );
        assert_eq!(Color::from_hex("red"), None);
        assert_eq!(Color::rgba(1, 2, 3, 4).argb(), 0x0401_0203);
    }

    #[test]
    fn custom_tokens_override_editor_colors() {
        let json = r##"{"light": {"background": "#000000", "foreground": "#ffffff", "primary": "#111111",
            "primary-foreground": "#eeeeee", "secondary": "#222222", "muted": "#333333",
            "muted-foreground": "#444444", "accent": "#555555", "destructive": "#ff0000",
            "border": "#666666", "ring": "#777777", "selection": "#00ff0040"}}"##;
        let theme = Theme::from_tokens_json(json, ColorScheme::Light).unwrap();
        assert_eq!(theme.palette.selection, Color::rgba(0, 255, 0, 0x40));
        assert_eq!(theme.palette.caret, Color::rgb(255, 255, 255));
        assert!(matches!(
            Theme::from_tokens_json(json, ColorScheme::Dark),
            Err(ThemeError::Json(_))
        ));
    }
}
