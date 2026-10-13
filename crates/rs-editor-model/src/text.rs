//! UTF-16 helpers. Document positions count UTF-16 code units so they agree
//! with SkParagraph, Android, UIKit/AppKit, EditContext and Tiptap JSON.

/// Number of UTF-16 code units in `text`.
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// Byte index of the UTF-16 offset `offset`, or `None` when it is past the end
/// or falls between the two halves of a surrogate pair.
pub fn utf16_to_byte(text: &str, offset: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += ch.len_utf16();
        if units > offset {
            return None;
        }
    }
    (units == offset).then_some(text.len())
}

/// UTF-16 offset of byte index `byte`, which must be a char boundary.
pub fn byte_to_utf16(text: &str, byte: usize) -> usize {
    utf16_len(&text[..byte])
}

/// Slices `text` by UTF-16 offsets, or `None` if either end is invalid.
pub fn slice_utf16(text: &str, from: usize, to: usize) -> Option<&str> {
    if from > to {
        return None;
    }
    let start = utf16_to_byte(text, from)?;
    let end = utf16_to_byte(text, to)?;
    Some(&text[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_follow_utf16_code_units() {
        let text = "a🙂b";
        assert_eq!(utf16_len(text), 4);
        assert_eq!(utf16_to_byte(text, 1), Some(1));
        assert_eq!(utf16_to_byte(text, 2), None, "inside the surrogate pair");
        assert_eq!(utf16_to_byte(text, 3), Some(5));
        assert_eq!(utf16_to_byte(text, 4), Some(6));
        assert_eq!(utf16_to_byte(text, 5), None);
        assert_eq!(slice_utf16(text, 1, 3), Some("🙂"));
        assert_eq!(byte_to_utf16(text, 5), 3);
    }
}
