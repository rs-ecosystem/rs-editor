# Roadmap OpenRich v2

> Pembaruan terakhir: 2026-10-12. Status hanya mencerminkan apa yang sudah ada dan teruji di repo. Roadmap v1 diarsipkan di [legacy-roadmap-v1.md](legacy-roadmap-v1.md). Arsitekturnya dijelaskan di [ADR 0007](adr/0007-openrich-v2-architecture.md).

## Ringkasan

| Fase | Lingkup | Status |
| --- | --- | --- |
| P0 | Kerangka workspace, CI, ADR, spike Skia (WASM, Android) | **Sebagian.** Workspace, CI dan ADR selesai. Spike Skia menunggu ruang disk. |
| P1 | Core headless: model, transform, state, history, command, extension, StarterKit | **Selesai** |
| P2 | Layout inkremental, render Skia, view, tema shadcn, dev harness desktop | Belum dimulai |
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

## Berikutnya

1. **Ruang disk.** Spike Skia (P0) membutuhkan sekitar 10 GB untuk binary Skia, Emscripten SDK, dan cache build Android.
2. **P2:** layout inkremental di atas `SkParagraph` dengan cache per konten, virtualisasi viewport, caret dengan affinity, navigasi visual bidi, display list, tema shadcn yang membaca format `design/tokens.json` dari rs-organizer, dan dev harness desktop.
3. **P3 dan P4** (web dan Android) dijalankan setelah P2, sesuai prioritas.
