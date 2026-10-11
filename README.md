# OpenRich

OpenRich adalah engine rich text lintas platform yang sedang dibangun dengan Rust. Engine dirancang headless: model dokumen dan operasi editing dapat digunakan tanpa framework UI, sementara renderer dan adapter platform dipasang secara terpisah.

Nama ini masih nama kerja.

Lihat [roadmap dan status goal](docs/roadmap.md) untuk milestone yang selesai, sebagian, dan belum dimulai.

Untuk menjalankan demo interaktif browser, gunakan `cargo run -p openrich-demo` lalu buka `http://127.0.0.1:4173`. Di macOS, `./scripts/run-macos-demo.sh` membuka host native WKWebView. Panduan interaksi dan batas demo ada di [interactive demo](docs/interactive-demo.md).

## Status

**Checkpoint demo G0–G3 tersedia pada macOS:** core Rust headless, document layout Skia, playground browser, dan host jendela WKWebView. API masih eksperimental dan belum siap untuk aplikasi produksi.

## Prinsip arsitektur

- `openrich-document` menyimpan model dokumen dan selection tanpa ketergantungan UI.
- `openrich-editor` mengelola operasi dan state editing di atas model dokumen.
- `openrich-render` mendefinisikan data dan kontrak render; `openrich-render-skia` menjadi backend native pertama.
- Adapter aplikasi meneruskan input platform ke core dan memetakan semantik editor ke API accessibility platform.
- Crux dapat memakai engine sebagai konsumen, tetapi tidak menjadi dependency core.

## Struktur awal

```text
crates/
├── openrich-document/  # schema dokumen, posisi, dan selection
├── openrich-editor/    # state editor dan command editing
├── openrich-layout/    # pemetaan posisi teks untuk backend layout
├── openrich-render/    # kontrak scene dan renderer
├── openrich-render-skia/ # document layout, selection, caret, dan drawing Skia
├── openrich-transaction/ # transaksi yang dihasilkan editor
└── openrich-demo/        # server lokal, browser playground, host WKWebView macOS
└── openrich-demo/        # server lokal, browser playground, host WKWebView macOS
```

## Operasi yang sudah tersedia

- Dokumen JSON versi 2 dengan paragraf, heading, daftar, kutipan, mark inline, dan ID stabil untuk block/list item. Dokumen versi 1 dimigrasikan saat dibaca.
- Validasi schema dasar, termasuk ID unik dalam dokumen, level heading, list, dan target tautan; versi, node, atau field yang tidak dikenal ditolak.
- Posisi caret dihitung dalam extended grapheme cluster; editor mendukung selection lintas paragraf dan navigasi yang melewati blok nonteks.
- Editor headless mendukung insert, backspace/delete, split/join blok, toggle mark, undo, dan redo.
- Setiap perubahan mengembalikan transaction yang dapat diterapkan host secara atomik setelah state awal dan dokumen hasil divalidasi.
- `openrich-layout` memetakan posisi grapheme core ke UTF-16 untuk mesin teks/platform.
- `openrich-render-skia` menyediakan layout dokumen, styling inline dasar (bold, italic, underline, strike, code, dan link), hit testing, selection/caret geometry, dan drawing ke canvas host.
- Undo mengelompokkan rangkaian ketikan; composition IME hanya masuk ke dokumen setelah commit dan menjadi satu undo.
- Demo menjalankan renderer Skia sebagai raster software RGBA (PNG untuk frame besar), dengan geometri interaksi tersimpan di browser, overlay caret/selection lokal, semantic HTML untuk input/accessibility, serta indikator FPS dan waktu render.

Operasi edit saat ini bekerja pada paragraf atau heading tingkat atas, termasuk selection lintas blok. Layout dokumen dapat menampilkan struktur bersarang; editing child node, atribut link yang dapat dikonfigurasi, validasi screen reader/IME per platform, dan adapter framework masih perlu dilengkapi.

## Mulai mengembangkan

Gunakan toolchain Rust stabil yang mendukung edition 2024. Perintah validasi workspace:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
```

Backend Skia memakai binding `skia-safe` dengan fitur `textlayout`; build native membutuhkan Ninja dan download atau build dependency Skia. Baseline build macOS ARM64 dicatat di [`docs/build-validation.md`](docs/build-validation.md). Adapter platform belum tersedia.

## Lisensi

Target lisensi proyek adalah MIT OR Apache-2.0. File lisensi dan audit dependency akan diselesaikan sebelum rilis publik.
