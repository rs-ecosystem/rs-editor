# Baseline build OpenRich

Validasi lokal pertama dilakukan pada 2026-10-11.

## Environment

| Komponen | Versi/target |
| --- | --- |
| OS | macOS 15.7.7 (24G720) |
| Rust host | `aarch64-apple-darwin` |
| `rustc` | 1.99.0 |
| Cargo | 1.99.0 |
| Ninja | 1.13.2 |
| Skia binding | `skia-safe` 0.153.3, fitur `textlayout` |

## Hasil

| Perintah | Hasil |
| --- | --- |
| `cargo fmt --all -- --check` | Lulus |
| `cargo check --workspace --all-targets --locked --offline` | Lulus |
| `cargo test --workspace --locked --offline` | Lulus: 29 tes unit, 0 gagal |
| `node --check crates/openrich-demo/web/app.js` | Lulus |
| `swiftc -parse-as-library ... OpenRichDemo.swift` | Lulus pada macOS 15 ARM64 |
| `./scripts/run-macos-demo.sh` | Host WKWebView terbuka; input teks dan undo diverifikasi melalui accessibility tree |
| Incremental `cargo build -p openrich-demo --locked --offline` | 0,75 detik pada cache yang sudah terisi |
| `target/debug/openrich-demo` | 27 MB debug binary; bukan ukuran rilis |

Tes unit mencakup migrasi schema v1 → v2, round-trip Unicode/RTL, validasi ID node, apply transaction atomik, operasi insert/delete dan selection lintas paragraf, grapheme emoji, history grouping, komposisi IME, pemetaan grapheme–UTF-16, document layout Skia, hit testing, selection/caret geometry, mixed RTL, pemetaan mark ke style Skia, dan software raster PNG. Pemeriksaan `--locked --offline` memastikan hasil konsisten dengan Cargo.lock tanpa mengakses jaringan.

Seleksi pointer sekarang memakai hit testing Skia pada koordinat frame yang sama dengan rendering, termasuk snapping di luar glyph dan arah seleksi terbalik. Drag dua arah, seleksi lintas paragraf, dan copy diverifikasi di browser; regresi hit testing emoji/wrapping diuji pada skala 1×, 2×, dan 3×.

## Optimasi interaksi dan pengukuran performa

- `node --test crates/openrich-demo/tests/interaction.test.mjs`: 8 tes lulus untuk geometri lokal, grapheme emoji, RTL, seleksi terbalik/lintas blok, wrapped lines, navigasi panah, dan key repeat.
- `OPENRICH_DEMO_URL=http://127.0.0.1:4174 node --test crates/openrich-demo/tests/scene.test.mjs`: lulus; payload RGBA dan geometri dibandingkan dengan hit testing Skia asli pada DPI 1×, 2×, dan 3×. Gunakan host uji terpisah karena tes mengubah selection.
- Browser: drag + 40 penekanan arrow/Shift+arrow tidak menambah request aksi ataupun scene (tetap 0 dan 1 setelah initial load). UI sekitar 60 FPS; P95 input→draw overlay 7 ms pada rangkaian tersebut.
- Delapan frame fixture setelah warmup pada 960 logical px, DPI 2×: scene server sekitar 4,8–5,4 ms, raster 0,8–1,0 ms; round-trip lokal sekitar 9,8–12,6 ms setelah request pertama. Ini pengukuran debug build pada host baseline, bukan klaim FPS maksimum/GPU.
- Panel UI memperlihatkan FPS UI, FPS redraw Skia aktual yang ditampilkan, raster ms, scene ms, dan P95 input→draw. Saat diam Skia FPS = 0 karena event-driven.

## Catatan build native

Perbaikan visual demo: tinggi minimum hanya diterapkan pada area editor, bukan gambar PNG, agar frame tidak meregang. Batas tinggi raster diskalakan bersama DPI. Pemeriksaan browser Retina menunjukkan frame 1920×360 ditampilkan sebagai 960×180 (rasio identik), teks isi 16 px, heading pertama 26,4 px, dan highlight selection sejajar dengan teks. Font/heading serta padding DOM input disamakan dengan renderer, termasuk pada breakpoint kecil.

Arsip Skia prebuilt untuk konfigurasi `aarch64-apple-darwin-textlayout` menghasilkan HTTP 404. `skia-bindings` beralih ke build dari source; Ninja diperlukan untuk jalur tersebut. Build source berhasil setelah Ninja tersedia. Validasi ini hanya mencakup host macOS ARM64. Belum ada matriks target lain atau golden pixel test lintas mesin; hasil font bergantung pada font yang tersedia di host. Uji VoiceOver dan IME perlu dilakukan manual pada kombinasi OS/browser tujuan.
