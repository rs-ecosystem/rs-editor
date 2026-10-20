//! Emoji drawn by the host. On the web there are no system fonts to shape
//! emoji with, but the browser can draw them: with emoji images on, each
//! emoji grapheme is laid out as a square placeholder and painted with an
//! image the host supplies (see [`crate::LayoutEngine::add_emoji_image`]).

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use skia_safe::Image;
use unicode_segmentation::UnicodeSegmentation;

/// Emoji images by grapheme, and the graphemes laid out without one.
#[derive(Default)]
pub(crate) struct EmojiImages {
    pub images: HashMap<String, Image>,
    pub missing: BTreeSet<String>,
}

pub(crate) type SharedEmoji = Rc<RefCell<EmojiImages>>;

/// Emoji with default emoji presentation in the Basic Multilingual Plane
/// (most others need U+FE0F to show as emoji).
const BMP_EMOJI: &[(u32, u32)] = &[
    (0x231a, 0x231b),
    (0x23e9, 0x23ec),
    (0x23f0, 0x23f0),
    (0x23f3, 0x23f3),
    (0x25fd, 0x25fe),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267f, 0x267f),
    (0x2693, 0x2693),
    (0x26a1, 0x26a1),
    (0x26aa, 0x26ab),
    (0x26bd, 0x26be),
    (0x26c4, 0x26c5),
    (0x26ce, 0x26ce),
    (0x26d4, 0x26d4),
    (0x26ea, 0x26ea),
    (0x26f2, 0x26f3),
    (0x26f5, 0x26f5),
    (0x26fa, 0x26fa),
    (0x26fd, 0x26fd),
    (0x2705, 0x2705),
    (0x270a, 0x270b),
    (0x2728, 0x2728),
    (0x274c, 0x274c),
    (0x274e, 0x274e),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27b0, 0x27b0),
    (0x27bf, 0x27bf),
    (0x2b1b, 0x2b1c),
    (0x2b50, 0x2b50),
    (0x2b55, 0x2b55),
];

/// Whether a grapheme shows as an emoji: one with emoji presentation (by
/// default or through U+FE0F), a keycap, a flag, or a sequence of them.
pub(crate) fn is_emoji(grapheme: &str) -> bool {
    grapheme.chars().any(|c| {
        let c = u32::from(c);
        c == 0xfe0f
            || c == 0x20e3
            || (0x1f1e6..=0x1f1ff).contains(&c)
            || ((0x1f000..=0x1faff).contains(&c)
                // Text-style symbols in that range without U+FE0F.
                && !(0x1f100..=0x1f1e5).contains(&c))
            || BMP_EMOJI.iter().any(|&(a, b)| (a..=b).contains(&c))
    }) && !grapheme.chars().any(|c| c == '\u{fe0e}')
}

/// Splits `text` into runs of plain text and single emoji graphemes.
pub(crate) fn segments(text: &str) -> Vec<(&str, bool)> {
    let mut out: Vec<(&str, bool)> = Vec::new();
    let mut plain_start = None;
    for (start, grapheme) in text.grapheme_indices(true) {
        if is_emoji(grapheme) {
            if let Some(from) = plain_start.take() {
                out.push((&text[from..start], false));
            }
            out.push((grapheme, true));
        } else if plain_start.is_none() {
            plain_start = Some(start);
        }
    }
    if let Some(from) = plain_start {
        out.push((&text[from..], false));
    }
    out
}
