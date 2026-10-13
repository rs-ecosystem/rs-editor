# ADR 0007: Arsitektur Rs Editor v2

- Status: diterima
- Tanggal: 2026-10-12
- Menggantikan: 0004, 0005 dan 0006. 0001–0003 tetap berlaku dengan penyesuaian di bawah.

## Konteks

Implementasi v1 (sekarang di `legacy/`) membuktikan bahwa layout SkParagraph dapat dipakai untuk editing, tetapi tidak bisa menjadi library multi-platform:

- Skia berjalan di server HTTP. Setiap ketikan menunggu round-trip, layout ulang seluruh dokumen, dan transfer piksel.
- Input bergantung pada DOM `contenteditable` transparan yang fontnya harus sama persis dengan Skia, dan sekitar 1.400 baris logika input ditulis khusus browser.
- Posisi berupa indeks blok top-level, sehingga list bertingkat dan kutipan tidak dapat diedit. Undo menyimpan snapshot seluruh dokumen.

Tujuan v2 adalah satu library Rust dengan tampilan dan rasa yang sama di web, Android, iOS, macOS, Windows dan Linux, fitur setara Tiptap, dan gaya shadcn/ui.

## Keputusan

1. **Model dan transform mengikuti ProseMirror.** Model, transform, state, history, command, list, keymap dan input rule diport dari ProseMirror (MIT). Posisi berupa bilangan bulat dalam satuan UTF-16, plus satu untuk setiap batas node. Satuan ini sama dengan SkParagraph, Android, UIKit/AppKit, EditContext dan JSON Tiptap.
2. **JSON kompatibel dengan Tiptap.** StarterKit memakai nama node dan mark Tiptap (`bulletList`, `codeBlock`, `taskItem`, …). Dokumen Tiptap dapat dimuat dan disimpan tanpa konversi. Schema tetap ketat: node, mark, atau atribut yang tidak valid ditolak saat dimuat.
3. **API lintas bahasa berupa named command dengan argumen JSON** (`run("toggleHeading", {"level": 2})`), ditambah `can`, `is_active`, `get_attributes` dan `chain`. Bentuk ini mudah diekspor lewat FFI ke Kotlin, Swift dan TypeScript.
4. **Semua logika berada di Rust**, termasuk layout, render, gesture, protokol IME, dan UI di dalam editor (bubble menu, slash menu, handle seleksi). Adapter platform hanya menyediakan permukaan GPU, meneruskan event mentah, dan menerjemahkan protokol IME platform.
5. **Waktu disuntikkan lewat `Clock`**, sehingga pengelompokan undo dan tes bersifat deterministik.
6. **Perbaikan atas ProseMirror** yang ditemukan oleh property test:
   - Inversi `ReplaceAroundStep` jatuh ke `ReplaceStep` biasa ketika inversi gap tidak valid, misalnya setelah mengangkat anak kedua list item keluar dari list.
   - Undo memakai fitting `replace` ketika step terbalik ditolak karena perubahan di luar history.
   - Autolink hanya berjalan setelah spasi diketik, tidak pernah setelah undo atau redo.

## Konsekuensi

- `legacy/` dibekukan sebagai workspace terpisah dan akan dihapus setelah layout dan render v2 menggantikannya.
- Fitur tabel, mention dan kolaborasi dapat dibangun di atas step dan mapping yang sama dengan ekosistem ProseMirror.
- Property test (sesi editing acak dan transform acak) menjadi bagian CI. Setiap step harus selalu valid, dapat di-invert, dan dapat diserialisasi.
- Biaya: port ini besar dan harus mengikuti ProseMirror secara cermat. Tes ProseMirror diport bersama kodenya.
