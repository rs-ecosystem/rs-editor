//! Documents to open in the harness.

use serde_json::{Value, json};

fn text(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

fn marked(text: &str, marks: &[&str]) -> Value {
    let marks: Vec<Value> = marks.iter().map(|m| json!({"type": m})).collect();
    json!({"type": "text", "text": text, "marks": marks})
}

fn p(content: Vec<Value>) -> Value {
    json!({"type": "paragraph", "content": content})
}

fn heading(level: u8, title: &str) -> Value {
    json!({"type": "heading", "attrs": {"level": level}, "content": [text(title)]})
}

fn item(kind: &str, content: Vec<Value>) -> Value {
    json!({"type": kind, "content": [p(content)]})
}

fn task(checked: bool, label: &str) -> Value {
    json!({"type": "taskItem", "attrs": {"checked": checked}, "content": [p(vec![text(label)])]})
}

/// A tour of the StarterKit nodes and marks.
pub fn document() -> Value {
    json!({"type": "doc", "content": [
        heading(1, "OpenRich"),
        p(vec![
            text("Editor teks kaya yang digambar dengan "),
            marked("Skia", &["bold"]),
            text(", sama persis di web, Android, iOS dan desktop. Coba "),
            marked("tebal", &["bold"]),
            text(", "),
            marked("miring", &["italic"]),
            text(", "),
            marked("garis bawah", &["underline"]),
            text(", "),
            marked("coret", &["strike"]),
            text(", "),
            marked("kode", &["code"]),
            text(" dan "),
            marked("sorotan", &["highlight"]),
            text("."),
        ]),
        heading(2, "Pintasan"),
        json!({"type": "bulletList", "content": [
            item("listItem", vec![text("Ketik "), marked("# ", &["code"]), text(", "), marked("- ", &["code"]), text(", "), marked("1. ", &["code"]), text(", "), marked("[ ] ", &["code"]), text(" atau "), marked("> ", &["code"]), text(" di awal baris.")]),
            item("listItem", vec![text("Klik ganda memilih kata, klik tiga kali memilih blok.")]),
            item("listItem", vec![text("Tab dan Shift-Tab mengubah tingkat list.")]),
        ]}),
        heading(2, "Tugas"),
        json!({"type": "taskList", "content": [
            task(true, "Model dokumen dan transform"),
            task(true, "Layout dan render Skia"),
            task(false, "Web (WebGL + EditContext)"),
            task(false, "Android (SurfaceView + InputConnection)"),
        ]}),
        json!({"type": "blockquote", "content": [p(vec![text("Satu jalur render untuk semua platform, jadi teks diukur dengan cara yang sama di mana-mana.")])]}),
        heading(3, "Banyak aksara"),
        p(vec![text("العربية من اليمين إلى اليسار، हिन्दी देवनागरी, emoji 👩‍💻🎉 dan 日本語.")]),
        json!({"type": "codeBlock", "attrs": {"language": "rust"}, "content": [text("fn main() {\n    println!(\"halo, OpenRich\");\n}")]}),
        json!({"type": "horizontalRule"}),
        json!({"type": "orderedList", "content": [
            item("listItem", vec![text("Undo dan redo: Cmd/Ctrl-Z, Shift-Cmd/Ctrl-Z.")]),
            item("listItem", vec![text("F2 mengganti tema terang/gelap.")]),
        ]}),
        json!({"type": "paragraph"}),
    ]})
}

/// `n` paragraphs, for checking that editing stays smooth on long documents.
pub fn large(n: usize) -> Value {
    let blocks: Vec<Value> = (0..n)
        .map(|i| {
            if i % 50 == 0 {
                heading(2, &format!("Bagian {}", i / 50 + 1))
            } else {
                p(vec![text(&format!(
                    "Paragraf {i}. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua."
                ))])
            }
        })
        .collect();
    json!({"type": "doc", "content": blocks})
}
