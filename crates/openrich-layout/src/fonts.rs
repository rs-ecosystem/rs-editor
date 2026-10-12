//! Bundled fonts. Every platform shapes with the same font bytes, so text
//! measures identically everywhere.

use skia_safe::font_arguments::VariationPosition;
use skia_safe::font_arguments::variation_position::Coordinate;
use skia_safe::textlayout::{FontCollection, TypefaceFontProvider};
use skia_safe::{FontArguments, FontMgr, FourByteTag, Typeface};

/// Font files compiled into the library: `(file name, bytes)`.
pub const BUNDLED_FONTS: &[(&str, &[u8])] = &[
    (
        "Geist-Variable.ttf",
        include_bytes!("../assets/fonts/Geist-Variable.ttf"),
    ),
    (
        "GeistMono-Variable.ttf",
        include_bytes!("../assets/fonts/GeistMono-Variable.ttf"),
    ),
    (
        "NotoSansArabic-Regular.ttf",
        include_bytes!("../assets/fonts/NotoSansArabic-Regular.ttf"),
    ),
    (
        "NotoSansArabic-Bold.ttf",
        include_bytes!("../assets/fonts/NotoSansArabic-Bold.ttf"),
    ),
    (
        "NotoSansDevanagari-Regular.ttf",
        include_bytes!("../assets/fonts/NotoSansDevanagari-Regular.ttf"),
    ),
    (
        "NotoSansDevanagari-Bold.ttf",
        include_bytes!("../assets/fonts/NotoSansDevanagari-Bold.ttf"),
    ),
];

/// Weights instantiated from the variable fonts.
const WEIGHTS: [u16; 5] = [400, 500, 600, 700, 800];

/// Families used when a character is missing from the primary family.
pub const SCRIPT_FALLBACKS: &[&str] = &["Noto Sans Arabic", "Noto Sans Devanagari"];

fn instance(base: &Typeface, weight: u16) -> Option<Typeface> {
    let coordinates = [Coordinate {
        axis: FourByteTag::from_chars('w', 'g', 'h', 't'),
        value: f32::from(weight),
    }];
    let args = FontArguments::new().set_variation_design_position(VariationPosition {
        coordinates: &coordinates,
    });
    base.clone_with_arguments(&args)
}

/// Fonts for laying out documents: the bundled families first, then the
/// platform's fonts for emoji and scripts the bundle lacks (unless
/// `system_fallback` is off, for fully deterministic tests).
pub fn font_collection(system_fallback: bool) -> FontCollection {
    let manager = FontMgr::default();
    let mut provider = TypefaceFontProvider::new();
    for (name, bytes) in BUNDLED_FONTS {
        let Some(base) = manager.new_from_data(skia_safe::Data::new_copy(bytes), None) else {
            continue;
        };
        if name.ends_with("-Variable.ttf") {
            let family = if name.starts_with("GeistMono") {
                "Geist Mono"
            } else {
                "Geist"
            };
            for weight in WEIGHTS {
                if let Some(face) = instance(&base, weight) {
                    provider.register_typeface(face, Some(family));
                }
            }
        } else {
            provider.register_typeface(base, None);
        }
    }
    let mut fonts = FontCollection::new();
    let provider: FontMgr = provider.into();
    fonts.set_asset_font_manager(Some(provider));
    if system_fallback {
        fonts.set_default_font_manager(Some(manager), None);
    } else {
        fonts.disable_font_fallback();
    }
    fonts
}
