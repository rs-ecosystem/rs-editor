# Rs Editor

Rs Editor adalah library rich text editor dalam Rust yang dirender dengan Skia. Targetnya web, Android, iOS, macOS, Windows dan Linux, dengan tampilan dan rasa yang sama di semua platform, fitur setara Tiptap, dan gaya shadcn/ui.

Sebelumnya bernama OpenRich; implementasi v1 di `legacy/` dan dokumen v1 yang diarsipkan masih memakai nama lama. Arsitekturnya dijelaskan di [ADR 0007](docs/adr/0007-rs-editor-v2-architecture.md), dan statusnya di [roadmap](docs/roadmap.md).

## Status

**v2, fase P1 selesai: core headless.** Model dokumen, transform, state, undo, command, keymap, input rule dan sistem extension bergaya Tiptap sudah berjalan dan teruji, tetapi belum ada layout, render, maupun adapter platform. API masih bisa berubah.

Implementasi v1 (server HTTP + DOM mirror) dibekukan di [`legacy/`](legacy/) sebagai workspace terpisah.

## Contoh

```rust
use rs_editor_extensions::{Editor, EditorOptions};
use serde_json::json;

let mut editor = Editor::new(EditorOptions {
    content: Some(json!({"type": "doc", "content": [{"type": "paragraph"}]})),
    ..Default::default()
})?;

editor.type_text("# ");             // input rule markdown → heading
editor.type_text("Halo dunia");
editor.select(1, 5);
editor.run("toggleBold", &json!(null))?;
assert!(editor.is_active("bold", &Default::default()));

let tiptap_json = editor.get_json();  // JSON yang sama dengan Tiptap
```

Command dipanggil dengan nama dan argumen JSON (`run`, `can`, `chain`), sehingga API yang sama bisa diekspor ke Kotlin, Swift dan TypeScript.

## Struktur

```text
crates/
├── rs-editor-model/       # schema, node immutable, posisi UTF-16, JSON Tiptap
├── rs-editor-transform/   # step, mapping, replace/lift/wrap/split/join
├── rs-editor-state/       # EditorState, Selection, Transaction, plugin
├── rs-editor-history/     # undo/redo dari step terbalik
├── rs-editor-commands/    # command editing, list, keymap
├── rs-editor-text/        # batas grapheme dan kata (UTF-16)
└── rs-editor-extensions/  # extension Tiptap-style, StarterKit, Editor headless
legacy/                   # implementasi v1, tidak lagi dikembangkan
```

Model, transform, state, history, command, list dan keymap adalah port dari [ProseMirror](https://prosemirror.net) (MIT). Lisensi ProseMirror disertakan di setiap crate tersebut.

## Mengembangkan

Toolchain dipin di `rust-toolchain.toml`.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features
cargo test --workspace
```

Property test bisa dijalankan lebih lama dengan `PROPTEST_CASES`, misalnya:

```sh
PROPTEST_CASES=20000 cargo test --release -p rs-editor-transform -p rs-editor-extensions proptests
```

## Lisensi

Target lisensi proyek adalah MIT OR Apache-2.0. File lisensi dan audit dependency akan diselesaikan sebelum rilis publik.
