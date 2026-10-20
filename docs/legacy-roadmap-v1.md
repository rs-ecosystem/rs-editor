# Roadmap dan Status Goal OpenRich

> Pembaruan terakhir: 2026-10-11. Status mencerminkan bukti yang ada di repo saat ini, bukan estimasi atau rencana yang belum dikerjakan.

## Ringkasan status

| Goal | Status | Bukti saat ini |
| --- | --- | --- |
| G0. Fondasi repo dan baseline build | **Selesai** | Workspace Rust, ADR arsitektur, `Cargo.lock`, format/check/test lulus pada macOS ARM64. Lihat [baseline build](build-validation.md). |
| G1. Model dokumen dan core editing | **Selesai untuk demo** | Schema v2, migrasi, ID stabil, selection lintas paragraf, navigasi, clipboard text, history grouping, composition IME, transaksi atomik, dan tes invariant/Unicode tersedia. |
| G2. Layout dan renderer Skia | **Selesai untuk demo** | Layout dokumen termasuk nested list/quote, hit testing, selection/caret geometry, RTL/mixed-script fixture, software raster smoke test, dan resize/DPI endpoint tersedia. |
| G3. Demo editor, input, IME dan accessibility | **Selesai untuk demo macOS** | Browser playground dan jendela native WKWebView berjalan dengan core Rust/Skia; keyboard, format, undo/redo, selection, clipboard, import/export JSON, semantic DOM, dan IME event bridge diimplementasikan. Uji manual VoiceOver/IME pada host tambahan menjadi validasi lintas platform berikutnya. |
| G4. C ABI dan WebAssembly | **Belum dimulai** | Belum ada crate FFI/WASM, header C, package TypeScript atau contoh integrasi. |
| G5. Adapter lintas framework | **Belum dimulai** | Belum ada adapter/contoh untuk SwiftUI, Compose, React, Flutter, C# atau Qt. |
| G6. Kesiapan rilis 1.0 | **Belum dimulai** | Lisensi final, migrasi/versioning stabil, governance, security/dependency review, dan release process belum tersedia. |

**Checkpoint yang tersedia untuk dicoba:** G0 selesai dan G1–G3 sudah memenuhi lingkup demo macOS. Validasi VoiceOver/IME pada host tambahan, fuzzing, editing node bersarang, C ABI/WASM, adapter framework, dan kesiapan rilis tetap menjadi pekerjaan lanjutan.

## Goal dan kriteria selesai

### G0 — Fondasi repo dan baseline build · Selesai

- [x] Workspace Rust memisahkan document, editor, transaction, layout, renderer, dan backend Skia.
- [x] Core tidak memiliki dependency pada Crux atau UI toolkit.
- [x] `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, dan `cargo test --workspace` lulus pada macOS ARM64.
- [x] Dependency dikunci di `Cargo.lock`; hasil dan kebutuhan build native dicatat di [build-validation.md](build-validation.md).

### G1 — Model dokumen dan core editing · Selesai untuk demo

- [x] Schema JSON versi 2 dengan ID stabil untuk block/list item, migrasi otomatis dari v1, validasi dasar, posisi grapheme, commands editing, transaction, dan undo/redo.
- [x] Tes dasar untuk insert/delete grapheme, mark aktif, split/join, undo/redo, dan konversi Unicode.
- [x] Parse–serialize round-trip untuk struktur bertingkat; node/field asing ditolak eksplisit dan invariant dasar divalidasi.
- [x] Host dapat menerapkan transaction secara atomik; mismatch langkah, dokumen invalid, atau selection invalid tidak mengubah dokumen.
- [x] Tetapkan ID node stabil yang unik dalam dokumen, migrasi schema v1 → v2, dan fixture compatibility.
- [x] Tambahkan property-style invariant tests atas corpus Unicode untuk round-trip dan transaksi, termasuk fixture RTL.
- [x] Selesaikan selection lintas paragraf, navigasi yang melewati blok nonteks, grouping history, clipboard plain text, dan state komposisi IME.
- [ ] Tambahkan fuzz target berjangka panjang dan dukungan selection/editing di dalam child list/blockquote.

### G2 — Layout dan renderer Skia · Selesai untuk demo

- [x] Skia Paragraph melakukan shaping/wrapping satu blok, inline marks dasar, drawing, hit testing, dan selection geometry.
- [x] Pemetaan grapheme ↔ UTF-16 diuji untuk emoji, combining mark, dan emoji ZWJ.
- [x] Tambahkan document layout untuk paragraf, heading, nested lists, blockquote, dan horizontal rule, beserta mapping hit-test, selection, dan caret.
- [x] Tambahkan fixture RTL/mixed script, system font fallback, dan resize/DPI rendering.
- [x] Tambahkan rendering caret/selection dan software raster PNG smoke harness.
- [x] Pilih jalur browser demo: Rust lokal merender frame PNG dengan Skia; input/accessibility memakai semantic DOM. CanvasKit/WASM ditunda ke G4.
- [ ] Tambahkan custom font loading dan golden pixel tests yang stabil lintas host/font.

### G3 — Demo editor, input, IME, dan accessibility · Demo tersedia

- [x] Buat Rust local host, playground web, dan app shell macOS WKWebView untuk membuka, mengedit, serta mengunduh/memuat JSON.
- [x] Teruskan keyboard, pointer selection, focus, resize/DPI, clipboard, select-all, dan command editing ke engine.
- [x] Implementasikan preedit/update/commit/cancel IME pada browser dan WKWebView; commit satu komposisi menjadi satu undo.
- [x] Ekspos semantic HTML/AX tree untuk paragraf, heading, list, link, editor, tombol, caret, dan selection.
- [x] Sediakan acceptance checklist untuk alur mengetik, format, selection, paste, undo/redo, IME, aksesibilitas, resize, dan simpan ulang.
- [ ] Jalankan checklist dengan VoiceOver dan IME Jepang/Cina pada kombinasi OS/browser tambahan sebelum menyatakan dukungan lintas platform.

### G4 — C ABI dan WebAssembly · Belum dimulai

- [ ] Rilis facade C dengan opaque handles, error codes, buffer ownership/length eksplisit, lifecycle, dan versi API.
- [ ] Pastikan panic tidak melewati FFI boundary; uji input null/invalid, lifecycle, dan penggunaan C dari contoh bersih.
- [ ] Buat facade WASM dan package TypeScript yang dapat dipakai dari JavaScript tanpa langkah build Rust manual.
- [ ] Luluskan acceptance editing dan IME browser dari G3 lewat API WASM.

### G5 — Adapter lintas framework · Belum dimulai

- [ ] Sediakan contoh/adapters SwiftUI/UIKit dan Android Compose/View dengan dokumentasi input, surface, lifecycle, threading, accessibility, dan packaging.
- [ ] Sediakan adapter React/Vue/Svelte melalui package TypeScript/WASM.
- [ ] Sediakan plugin Flutter dan binding/contoh C#/.NET yang memakai ABI bersama; Qt/C++ dapat ditambahkan dengan kontrak yang sama.
- [ ] Untuk setiap target yang dinyatakan didukung, build package pada project contoh bersih dan jalankan editing, IME, resize/DPI, serta accessibility smoke checklist.

### G6 — Kesiapan rilis 1.0 · Belum dimulai

- [ ] Tetapkan lisensi kode dan dokumentasi, audit dependency/font/assets, dan tambahkan dokumen lisensi yang diperlukan.
- [ ] Tetapkan kebijakan schema/FFI versioning, migrasi, SemVer, deprecation, dan release notes.
- [ ] Tambahkan CI untuk core dan setiap adapter yang berstatus stabil; dokumentasikan matrix OS/arsitektur yang benar-benar diuji.
- [ ] Lengkapi README, quickstart, API/adapter docs, CONTRIBUTING, code of conduct, security reporting, governance, issue/RFC templates.
- [ ] Satu proyek eksternal dapat mengikuti quickstart tanpa bantuan maintainer; semua klaim dukungan memiliki bukti build/test.

## Urutan kerja berikutnya

1. Minta pengguna mencoba checkpoint G0–G3 lewat [panduan demo](interactive-demo.md) dan catat masalah input/visual.
2. Tambahkan fuzz target dan tutup dukungan child-node editing berdasarkan penggunaan nyata.
3. Bangun G4, lalu adapter G5 menggunakan kontrak core dan demo yang sudah diuji.
4. Kerjakan G6 setelah API dan adapter yang akan didukung dapat dibuktikan.

Urutan ini menjaga demo akhir tetap menjadi target: editor yang dapat diedit sungguhan dari beberapa bahasa/framework, dengan Skia sebagai renderer default opsional, core Rust headless, API C/WASM, IME, accessibility, serta proses rilis yang dapat direproduksi.
