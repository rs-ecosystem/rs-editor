# ADR 0003: Skia Paragraph menjadi backend layout pertama

- **Status:** diterima untuk spike; belum dikunci sebagai API publik
- **Tanggal:** 2026-10-11

## Konteks

Editor menyimpan posisi caret dalam extended grapheme cluster. Skia Paragraph menyediakan shaping dan wrapping, selection rectangles, serta hit testing, tetapi indeks teks yang dipakai Skia berbasis UTF-16 code unit. Core perlu tetap bebas dari dependensi Skia.

## Keputusan

- `openrich-layout` menyediakan pemetaan offset grapheme ke UTF-16 sebagai batas antara model dokumen dan mesin teks.
- Spike native pertama memakai binding `skia-safe` versi `0.153.3` dengan fitur `textlayout` dan `skia_safe::textlayout::Paragraph`.
- Crate Skia nantinya terpisah dari `openrich-document`, `openrich-editor`, dan `openrich-layout`.
- CanvasKit untuk browser tetap menjadi keputusan adapter web tersendiri; target WASM pada binding Skia native tidak dianggap sebagai dukungan browser otomatis.

## Status implementasi awal

- Layout satu paragraf dan drawing ke `Canvas` tersedia.
- Hit testing mengubah offset UTF-16 Skia menjadi grapheme; selection geometry mengubah range grapheme menjadi UTF-16.
- Text run mendukung bold, italic, underline, strike, dan keluarga font monospace untuk code. Link ditandai underline; pengelolaan navigasi URL berada pada host.
- API belum diverifikasi melalui build native karena toolchain Cargo belum tersedia pada lingkungan kerja ini.

## Kriteria spike

1. Layout paragraf dengan wrapping, campuran marks, emoji, aksara RTL, dan fallback font.
2. Hit testing Skia → UTF-16 → grapheme menghasilkan posisi core yang valid.
3. Range core → UTF-16 → selection rectangles mempertahankan batas grapheme.
4. Skia menggambar paragraph yang sama dengan data yang dipakai hit testing.
5. Catat build time, ukuran binary, font fallback, dan kebutuhan ICU per target.

## Konsekuensi

- Adapter offset menjadi bagian penting kontrak layout dan sekarang diimplementasikan tanpa dependency Skia.
- Layout geometris lintas backend belum distabilkan; hasil spike menentukan pembagian layout dan render.
- Versi Skia dan feature flags masih keputusan eksperimen sampai native build dan matriks target dijalankan.

## Referensi

- [Rust Skia: `skia-safe` dan dukungan target](https://github.com/rust-skia/rust-skia)
- [`skia-safe` 0.153.3: fitur `textlayout`](https://docs.rs/crate/skia-safe/0.153.3)
- [Binding Paragraph Rust: layout, paint, hit testing, dan selection boxes](https://github.com/rust-skia/rust-skia/blob/master/skia-safe/src/modules/paragraph/paragraph.rs)
- [API Skia Paragraph menggunakan text code-unit indices](https://skia.googlesource.com/skia/+/refs/heads/main/modules/skparagraph/include/Paragraph.h)
- [CanvasKit text shaping dan paragraph layout di browser](https://skia.org/docs/user/modules/quickstart/)
