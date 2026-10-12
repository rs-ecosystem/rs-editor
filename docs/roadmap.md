# Roadmap OpenRich v2

> Pembaruan terakhir: 2026-10-13. Status hanya mencerminkan apa yang sudah ada dan teruji di repo. Roadmap v1 diarsipkan di [legacy-roadmap-v1.md](legacy-roadmap-v1.md). Arsitekturnya dijelaskan di [ADR 0007](adr/0007-openrich-v2-architecture.md).

## Ringkasan

| Fase | Lingkup | Status |
| --- | --- | --- |
| P0 | Kerangka workspace, CI, ADR, spike Skia (WASM, Android) | **Sebagian.** Workspace, CI dan ADR selesai. Spike Skia web dan Android belum dikerjakan. |
| P1 | Core headless: model, transform, state, history, command, extension, StarterKit | **Selesai** |
| P2 | Layout inkremental, render Skia, view, tema shadcn, dev harness desktop | **Sebagian besar.** Tema, layout, render, view dan harness macOS jalan. Virtualisasi, pohon a11y dan scroll kinetik belum. |
| P3 | Web: Skia/WASM, EditContext dan textarea, clipboard HTML/Markdown, DOM ARIA | Belum dimulai |
| P4 | Android: SurfaceView/GL, InputConnection, gesture sentuh, AccessKit | Belum dimulai |
| P5 | UI di dalam canvas (bubble menu, slash menu, popover link, drag handle) dan paritas Tiptap | Belum dimulai |
| P6 | iOS dan macOS: UITextInput, NSTextInputClient, Metal | Belum dimulai |
| P7 | Windows dan Linux: winit, IME, AccessKit | Belum dimulai |
| P8 | Tabel, mention, hardening, API 1.0 | Belum dimulai |

## P1 — Core headless · Selesai

| Crate | Isi | Bukti |
| --- | --- | --- |
| `openrich-model` | Schema dengan content expression (DFA), mark set ternormalisasi, pohon node immutable, `ResolvedPos`, replace, JSON Tiptap | Port tes model ProseMirror dan tes UTF-16/JSON |
| `openrich-transform` | Step, `StepMap`/`Mapping`, replace dengan fitting, lift/wrap/split/join, mark, set block type | Seluruh `test-trans.ts` dan `test-mapping.ts` diport; property test transform acak |
| `openrich-state` | `Selection` (text, node, all), bookmark, `Transaction`, `EditorState`, plugin, `Clock` | Port `test-selection.ts` dan `test-state.ts` |
| `openrich-history` | Undo/redo dari step terbalik, pengelompokan waktu dan komposisi IME, rebase kolaborasi | Port `test-history.ts` |
| `openrich-commands` | Command dasar, list, keymap per platform | Port `test-commands.ts`, tes list dan keymap |
| `openrich-text` | Batas grapheme dan kata dalam UTF-16 | Tes emoji ZWJ, kombinasi diakritik, kata |
| `openrich-extensions` | Sistem extension, StarterKit, input rule markdown, `Editor` headless | Tes end-to-end dan property test sesi editing acak |

StarterKit mencakup paragraf, heading, blockquote, code block, garis horizontal, hard break, bullet/ordered/task list, gambar, link (dengan autolink dan validasi href), bold, italic, underline, strike, code, highlight, subscript, superscript, perataan teks, typography, history, character count, dan placeholder.

## P2 — Layout, render, view · Sebagian besar

| Crate | Isi | Bukti |
| --- | --- | --- |
| `openrich-theme` | Token shadcn light/dark dari format `design/tokens.json`, tipografi heading/paragraf/kode, spacing editor | Tes parsing token dan warna |
| `openrich-layout` | Font Geist/Geist Mono terbundel (variable, wght 400–800) plus Noto Arab/Devanagari, layout blok `SkParagraph` dengan cache per node, caret dengan affinity, hit-test, seleksi, navigasi vertikal dan visual, render dokumen (bullet, nomor, checkbox, quote, code, garis, gambar) | 11 tes, snapshot PNG lewat `OPENRICH_SNAPSHOT_DIR` |
| `openrich-view` | `EditorView`: keyboard per platform (grapheme/kata/baris/halaman), mouse (klik, klik ganda/tiga, drag, shift-klik), checkbox task, komposisi IME sebagai satu langkah undo, clipboard teks/JSON, scroll, kedip caret, placeholder | 15 tes headless |
| `openrich-desktop` | Shell winit + softbuffer (raster CPU), keyboard, IME, mouse, scroll, clipboard; contoh `perf` | Diuji manual di macOS |

Ukuran saat ini (raster CPU, macOS, `--release`, contoh `perf`): ketikan sampai frame selesai digambar median 0,7 ms di 1.000 blok dan 2,9 ms di 10.000 blok (p95 3,6 ms). Biaya ini masih naik linear terhadap jumlah blok karena posisi semua blok dihitung ulang. Membuka dokumen 10.000 blok butuh sekitar 430 ms.

Menjalankan harness:

```bash
cargo run -p openrich-desktop --release
```

Argumen `--dark` memakai tema gelap, `--large 10000` membuka dokumen panjang, dan F2 mengganti tema.

Sisa P2: layout hanya untuk blok di sekitar viewport (biaya per ketikan tidak lagi bergantung panjang dokumen), scroll anchoring, scroll kinetik, pohon aksesibilitas, golden pixel test di CI.

## Berikutnya

1. **Spike P0:** Skia di Emscripten/WebGL dan Android `SurfaceView`, `InputConnection`, EditContext.
2. **Sisa P2** di atas.
3. **P3 dan P4** (web dan Android) dijalankan setelah P2, sesuai prioritas.
