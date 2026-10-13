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
    /// …where the slash menu does not apply (read-only, unfocused).
    pub write_something: &'static str,
    /// "Heading" in "Heading 4".
    pub heading_word: &'static str,
    pub list_item: &'static str,
    pub task_item: &'static str,
    // Names for screen readers.
    pub blocks_menu: &'static str,
    pub formatting: &'static str,
    /// Bubble menu buttons: bold, italic, underline, strike, code,
    /// highlight, link.
    pub marks: [&'static str; 7],
    pub link_menu: &'static str,
    pub link_address: &'static str,
    pub open_link: &'static str,
    pub copy_link: &'static str,
    pub edit_link: &'static str,
    pub remove_link: &'static str,
    pub apply: &'static str,
    pub move_block: &'static str,
    pub move_up: &'static str,
    pub move_down: &'static str,
    pub no_results: &'static str,
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
    write_something: "Write something\u{2026}",
    heading_word: "Heading",
    list_item: "List",
    task_item: "To-do",
    blocks_menu: "Blocks",
    formatting: "Formatting",
    marks: [
        "Bold",
        "Italic",
        "Underline",
        "Strikethrough",
        "Code",
        "Highlight",
        "Link",
    ],
    link_menu: "Link",
    link_address: "Link address",
    open_link: "Open link",
    copy_link: "Copy link",
    edit_link: "Edit link",
    remove_link: "Remove link",
    apply: "Apply",
    move_block: "Select or drag to move the block",
    move_up: "Move up",
    move_down: "Move down",
    no_results: "No results",
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
    write_something: "Tulis sesuatu\u{2026}",
    heading_word: "Judul",
    list_item: "Daftar",
    task_item: "Tugas",
    blocks_menu: "Blok",
    formatting: "Format",
    marks: [
        "Tebal",
        "Miring",
        "Garis bawah",
        "Coret",
        "Kode",
        "Sorotan",
        "Tautan",
    ],
    link_menu: "Tautan",
    link_address: "Alamat tautan",
    open_link: "Buka tautan",
    copy_link: "Salin tautan",
    edit_link: "Ubah tautan",
    remove_link: "Hapus tautan",
    apply: "Terapkan",
    move_block: "Pilih atau seret untuk memindahkan blok",
    move_up: "Pindah ke atas",
    move_down: "Pindah ke bawah",
    no_results: "Tidak ada hasil",
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
