# Roadmap OpenRich v2

> Pembaruan terakhir: 2026-10-13. Status hanya mencerminkan apa yang sudah ada dan teruji di repo. Roadmap v1 diarsipkan di [legacy-roadmap-v1.md](legacy-roadmap-v1.md). Arsitekturnya dijelaskan di [ADR 0007](adr/0007-openrich-v2-architecture.md).

## Ringkasan

| Fase | Lingkup | Status |
| --- | --- | --- |
| P0 | Kerangka workspace, CI, ADR, spike Skia (WASM, Android) | **Selesai.** Lihat [laporan spike](spikes/p0-web-android.md). |
| P1 | Core headless: model, transform, state, history, command, extension, StarterKit | **Selesai** |
| P2 | Layout inkremental, render Skia, view, tema shadcn, dev harness desktop | **Selesai** |
| P3 | Web: Skia/WASM, EditContext dan textarea, clipboard HTML/Markdown, DOM ARIA | Pondasi dari spike; dikerjakan **setelah P4** |
| P4 | Android: SurfaceView/GL, InputConnection, seleksi sentuh native (handle, magnifier, menu, haptik), AccessKit | **Berikutnya.** Pondasi dari spike |
| P5 | UI di dalam canvas (bubble menu, slash menu, popover link, drag handle) dan paritas Tiptap | Belum dimulai |
| P6 | iOS dan macOS: UITextInput, NSTextInputClient, Metal | Belum dimulai |
| P7 | Windows dan Linux: winit, IME, AccessKit | Belum dimulai |
| P8 | Tabel, mention, hardening, API 1.0 | Belum dimulai |

Urutan pengerjaan: P4 dikerjakan sebelum P3. Interaksi sentuh dibangun dan diuji di Android lebih dulu, lalu dipakai ulang oleh web di HP (P3) dan iOS (P6).

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

## P2 — Layout, render, view · Selesai

| Crate | Isi | Bukti |
| --- | --- | --- |
| `openrich-theme` | Token shadcn light/dark dari format `design/tokens.json`, tipografi heading/paragraf/kode, spacing editor | Tes parsing token dan warna |
| `openrich-layout` | Font Geist/Geist Mono terbundel (variable, wght 400–800) plus Noto Arab/Devanagari. Layout `SkParagraph` yang malas per blok top-level: fragment di-cache per node, blok jauh dari viewport memakai tinggi perkiraan, posisi blok disimpan per chunk 64 blok. Caret dengan affinity, hit-test, seleksi, navigasi vertikal dan visual, render dokumen (bullet, nomor, checkbox, quote, code, garis, gambar) | 13 tes, termasuk layout incremental yang harus identik dengan layout dari nol setelah edit di batas chunk |
| `openrich-view` | `EditorView`: keyboard per platform, mouse, checkbox task, komposisi IME sebagai satu langkah undo, clipboard teks/JSON, kedip caret, placeholder. Window layout ±1 layar dengan scroll anchoring. Scroll halus (roda) dan momentum (fling) dengan `VelocityTracker`. Pohon aksesibilitas AccessKit: text run per baris dengan geometri karakter, seleksi, action pilih teks, ketik, dan centang task | 25 tes headless dan 4 golden pixel test (`tests/golden`) |
| `openrich-desktop` | Shell winit + softbuffer (raster CPU), keyboard, IME, mouse, scroll, clipboard, adapter `accesskit_winit`; contoh `perf` | Diuji manual di macOS, termasuk pohon AX yang terbaca sistem |

Ukuran saat ini (raster CPU, macOS, `--release`, contoh `perf`):

| Dokumen | Buka + frame pertama | Ketikan sampai frame (median / p95) |
| --- | --- | --- |
| 1.000 blok | 36 ms | 0,6 / 0,7 ms |
| 10.000 blok | 64 ms | 0,7 / 1,0 ms |
| 100.000 blok | 272 ms | 2,2 / 3,0 ms |

Pada 100.000 blok, sisa biaya ada di model dokumen: setiap perubahan menyalin array anak level atas (perilaku yang sama dengan ProseMirror). Kalau dokumen sepanjang itu jadi target, anak level atas perlu disimpan dalam pohon persisten.

Golden pixel test memakai `OPENRICH_UPDATE_GOLDEN=1 cargo test -p openrich-view --test golden` untuk memperbarui gambar. Kegagalan menulis gambar aktual dan diff ke `target/golden-failures/`, dan CI mengunggahnya sebagai artifact. Golden disimpan per OS (`tests/golden/macos`, `tests/golden/linux`): prebuilt Skia untuk Apple merasterisasi tepi glyph dengan CoreText, sedangkan Linux memakai FreeType, jadi tepinya sedikit berbeda walaupun layout identik.

Menjalankan harness:

```bash
cargo run -p openrich-desktop --release
```

Argumen `--dark` memakai tema gelap, `--large 10000` membuka dokumen panjang, dan F2 mengganti tema.

Catatan yang dibawa ke fase berikutnya:

- Paragraf RTL (Arab) masih rata kiri. Arah paragraf perlu dideteksi dari teks (atau atribut `dir`) dan dipakai untuk perataan.
- Pohon aksesibilitas dibangun ulang dalam O(jumlah blok) per update saat teknologi bantu aktif. Ini cukup untuk 10.000 blok, belum untuk 100.000.
- Mengetik lewat `AXSelectedText` di macOS belum didukung adapter AccessKit. Screen reader tetap bisa membaca, memindahkan seleksi, dan mengetik lewat keyboard biasa.

## P4 — Android · Berikutnya

Target: editor terasa seperti kolom teks bawaan Android, bukan sekadar tampil benar.

### Milestone 1: seleksi sentuh terasa native · Selesai

Sudah ada dan diuji (15 tes skenario headless di `openrich-view/src/touch.rs`, plus uji manual di perangkat 23049PCD8G):

- Ketuk menaruh caret dengan handle tetesan yang hilang setelah 4 detik; ketuk caret lagi memunculkan menu Paste/Select all.
- Tekan lama memilih kata (haptik `LONG_PRESS`, magnifier), lalu geser memperluas per kata.
- Ketuk dua dan tiga kali memilih kata dan paragraf.
- Handle Material digambar Skia. Menarik handle: keluar melompat per kata, kembali per karakter, tidak bisa melewati handle lain, auto-scroll di tepi, haptik `TEXT_HANDLE_MOVE` setiap perubahan, dan `Magnifier` asli Android.
- Menu mengambang asli (`ActionMode`) dengan label lokal sistem dan aksi aplikasi lain (`PROCESS_TEXT`), disembunyikan selama scroll lalu muncul lagi.
- Mengetik, tombol, atau mouse mengakhiri mode seleksi sentuh.

- Ketuk handle caret membuka/menutup menu Paste; salin lalu tempel lewat menu diuji di perangkat.
- Keyboard terbuka: caret yang terlihat tetap terlihat saat view mengecil.
- Profil iOS: tekan lama menggeser caret di bawah magnifier; handle gaya Apple (bar dan knob) diuji headless.
- Menu yang ditutup sistem (tombol back) disinkronkan ke view.

Belum:

- Handle gaya Apple belum diuji di perangkat iOS (P6).
- Samsung Keyboard (tidak ada di perangkat uji). Gboard sudah diuji: ketik, saran, koreksi otomatis dan batal-koreksi (lewat `replaceText`) berjalan; lihat [laporan spike](spikes/p0-web-android.md).

Rencana awal milestone:

Inti di Rust (`openrich-view`), dipakai juga oleh web di HP (P3) dan iOS (P6):

- Pengenal gesture sentuh: ketuk menaruh caret, ketuk caret memunculkan menu (Tempel), ketuk dua kali dan tekan lama memilih kata, ketuk tiga kali memilih paragraf.
- Handle seleksi: dua handle untuk rentang dan satu handle di bawah caret. Area sentuhnya lebih besar dari gambarnya, menarik handle cepat memperluas per kata, menarik ke tepi layar memicu auto-scroll, dan handle tidak boleh melewati handle lainnya.
- `InteractionProfile` per platform: bentuk handle (tetesan Material, lolipop iOS), timing tekan lama, ukuran slop.
- Sinyal ke platform:
  - `Haptic(jenis)` saat tekan lama dan setiap seleksi pindah ke karakter atau kata baru;
  - `SelectionMenu { rect, items }` saat seleksi muncul atau berubah; disembunyikan selama scroll atau drag, lalu muncul lagi setelah selesai;
  - `Magnifier { at }` selama handle atau caret ditarik.

Adapter Android:

- Menu mengambang asli (`ActionMode.TYPE_FLOATING`): Potong, Salin, Tempel, Pilih semua, plus item sistem (Terjemahkan, Bagikan, asisten).
- `android.widget.Magnifier` untuk kaca pembesar.
- `performHapticFeedback` dengan konstanta sistem (`LONG_PRESS`, `TEXT_HANDLE_MOVE`), mengikuti setelan getar pengguna.
- Handle digambar Skia dengan bentuk dan warna Material.

Tes: skenario gesture headless (posisi, granularitas, auto-scroll, urutan sinyal haptik dan menu) dan uji manual di perangkat.

### Milestone 2: input dan platform · Berjalan

Sudah:

- `CursorAnchorInfo` (posisi caret dengan baseline, batas tiap karakter komposisi, batas editor untuk tulisan tangan) dikirim saat IME memintanya lewat `requestCursorUpdates`. Keyboard Xiaomi tidak memintanya; belum diuji dengan keyboard yang memakainya.
- Crate `openrich-markup`: HTML ↔ dokumen. Parser bergaya ProseMirror (selalu menghasilkan dokumen valid; Google Docs, Word, GitHub, tag soup), serializer bergaya Tiptap dengan JSON slice tersemat sehingga salin-tempel di OpenRich sendiri persis. 11 tes termasuk property test tag soup.
- Salin menghasilkan teks + HTML dan tempel memakai HTML lebih dulu, di desktop (arboard), web (`text/html`) dan Android (`ClipData.newHtmlText`).

Rencana awal milestone:

- `CursorAnchorInfo` agar popup kandidat dan input tulisan tangan berada di tempat yang benar.
- Uji Gboard dan Samsung Keyboard: komposisi, koreksi otomatis, input suara, swipe typing.
- Clipboard kaya (HTML) dan tempel dari aplikasi lain.
- AccessKit untuk TalkBack.

### Milestone 3: paket library

- Binding Kotlin (BoltFFI) menggantikan JNI tangan, modul Gradle `openrich-android`, dan contoh aplikasi Compose.
- API `chain/can/isActive` dari Kotlin.

## Berikutnya

1. **P4 (Android):** milestone di atas, dimulai dari seleksi sentuh.
2. **P3 (web):** paket `@openrich/web`, clipboard HTML/Markdown, DOM ARIA dari pohon aksesibilitas, font emoji/CJK yang dimuat bertahap, uji di Safari, Firefox dan Chrome Android. Seleksi sentuh untuk web di HP memakai inti dari P4.
3. **Desktop (P7):** menu klik kanan Potong/Salin/Tempel memakai sinyal `SelectionMenu` yang sama.

Spike web dan Android sudah membuktikan jalurnya: [laporan spike P0](spikes/p0-web-android.md).
