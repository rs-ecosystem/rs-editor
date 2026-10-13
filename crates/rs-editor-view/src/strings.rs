//! Words the editor's own UI shows, in the user's language.

/// UI text for one language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Strings {
    pub text: &'static str,
    pub heading: [&'static str; 3],
    pub bullet_list: &'static str,
    pub numbered_list: &'static str,
    pub task_list: &'static str,
    pub quote: &'static str,
    pub code_block: &'static str,
    pub divider: &'static str,
    pub link_placeholder: &'static str,
    pub image: &'static str,
    pub image_placeholder: &'static str,
    /// Shown in an empty line with the caret.
    pub placeholder: &'static str,
}

const ENGLISH: Strings = Strings {
    text: "Text",
    heading: ["Heading 1", "Heading 2", "Heading 3"],
    bullet_list: "Bullet list",
    numbered_list: "Numbered list",
    task_list: "To-do list",
    quote: "Quote",
    code_block: "Code block",
    divider: "Divider",
    link_placeholder: "Paste or type a link",
    image: "Image",
    image_placeholder: "Paste an image link",
    placeholder: "Write, or type \u{201c}/\u{201d} for commands",
};

const INDONESIAN: Strings = Strings {
    text: "Teks",
    heading: ["Judul 1", "Judul 2", "Judul 3"],
    bullet_list: "Daftar poin",
    numbered_list: "Daftar bernomor",
    task_list: "Daftar tugas",
    quote: "Kutipan",
    code_block: "Blok kode",
    divider: "Pemisah",
    link_placeholder: "Tempel atau ketik tautan",
    image: "Gambar",
    image_placeholder: "Tempel tautan gambar",
    placeholder: "Tulis, atau ketik \u{201c}/\u{201d} untuk perintah",
};

/// The strings for a BCP 47 language tag (`"id-ID"`, `"en"`); English
/// for languages without a translation.
pub(crate) fn strings_for(locale: &str) -> Strings {
    let language = locale
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match language.as_str() {
        "id" | "in" => INDONESIAN,
        _ => ENGLISH,
    }
}
