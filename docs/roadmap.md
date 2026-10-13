# Roadmap Rs Editor v2

> Pembaruan terakhir: 2026-10-13. Status hanya mencerminkan apa yang sudah ada dan teruji di repo. Roadmap v1 diarsipkan di [legacy-roadmap-v1.md](legacy-roadmap-v1.md). Arsitekturnya dijelaskan di [ADR 0007](adr/0007-rs-editor-v2-architecture.md).

## Ringkasan

| Fase | Lingkup | Status |
| --- | --- | --- |
| P0 | Kerangka workspace, CI, ADR, spike Skia (WASM, Android) | **Selesai.** Lihat [laporan spike](spikes/p0-web-android.md). |
| P1 | Core headless: model, transform, state, history, command, extension, StarterKit | **Selesai** |
| P2 | Layout inkremental, render Skia, view, tema shadcn, dev harness desktop | **Selesai** |
| P3 | Web: Skia/WASM, EditContext dan textarea, clipboard HTML/Markdown, DOM ARIA | **Hampir selesai.** Milestone 1–2 selesai, milestone 3 tinggal uji screen reader sungguhan dan Firefox |
| P4 | Android: SurfaceView/GL, InputConnection, seleksi sentuh native (handle, magnifier, menu, haptik), AccessKit, library Kotlin dan Compose | **Selesai.** Navigasi TalkBack dan Samsung Keyboard jadi tugas lanjutan |
| P5 | UI di dalam canvas (bubble menu, slash menu, popover link, drag handle) dan paritas Tiptap | **Selesai.** Bubble menu, menu blok `/`, popover tautan, drag handle (blok dan item list), gambar, aksesibilitas menu. Uji screen reader sungguhan jadi tugas lanjutan |
| P6 | iOS dan macOS: UITextInput, NSTextInputClient, Metal | Belum dimulai |
| P7 | Windows dan Linux: winit, IME, AccessKit | Belum dimulai |
| P8 | Tabel, mention, hardening, API 1.0 | Belum dimulai |

Urutan pengerjaan: P4 dikerjakan sebelum P3. Interaksi sentuh dibangun dan diuji di Android lebih dulu, lalu dipakai ulang oleh web di HP (P3) dan iOS (P6).

## P1 — Core headless · Selesai

| Crate | Isi | Bukti |
| --- | --- | --- |
| `rs-editor-model` | Schema dengan content expression (DFA), mark set ternormalisasi, pohon node immutable, `ResolvedPos`, replace, JSON Tiptap | Port tes model ProseMirror dan tes UTF-16/JSON |
| `rs-editor-transform` | Step, `StepMap`/`Mapping`, replace dengan fitting, lift/wrap/split/join, mark, set block type | Seluruh `test-trans.ts` dan `test-mapping.ts` diport; property test transform acak |
| `rs-editor-state` | `Selection` (text, node, all), bookmark, `Transaction`, `EditorState`, plugin, `Clock` | Port `test-selection.ts` dan `test-state.ts` |
| `rs-editor-history` | Undo/redo dari step terbalik, pengelompokan waktu dan komposisi IME, rebase kolaborasi | Port `test-history.ts` |
| `rs-editor-commands` | Command dasar, list, keymap per platform | Port `test-commands.ts`, tes list dan keymap |
| `rs-editor-text` | Batas grapheme dan kata dalam UTF-16 | Tes emoji ZWJ, kombinasi diakritik, kata |
| `rs-editor-extensions` | Sistem extension, StarterKit, input rule markdown, `Editor` headless | Tes end-to-end dan property test sesi editing acak |

StarterKit mencakup paragraf, heading, blockquote, code block, garis horizontal, hard break, bullet/ordered/task list, gambar, link (dengan autolink dan validasi href), bold, italic, underline, strike, code, highlight, subscript, superscript, perataan teks, typography, history, character count, dan placeholder.

## P2 — Layout, render, view · Selesai

| Crate | Isi | Bukti |
| --- | --- | --- |
| `rs-editor-theme` | Token shadcn light/dark dari format `design/tokens.json`, tipografi heading/paragraf/kode, spacing editor | Tes parsing token dan warna |
| `rs-editor-layout` | Font Geist/Geist Mono terbundel (variable, wght 400–800) plus Noto Arab/Devanagari. Layout `SkParagraph` yang malas per blok top-level: fragment di-cache per node, blok jauh dari viewport memakai tinggi perkiraan, posisi blok disimpan per chunk 64 blok. Caret dengan affinity, hit-test, seleksi, navigasi vertikal dan visual, render dokumen (bullet, nomor, checkbox, quote, code, garis, gambar) | 13 tes, termasuk layout incremental yang harus identik dengan layout dari nol setelah edit di batas chunk |
| `rs-editor-view` | `EditorView`: keyboard per platform, mouse, checkbox task, komposisi IME sebagai satu langkah undo, clipboard teks/JSON, kedip caret, placeholder. Window layout ±1 layar dengan scroll anchoring. Scroll halus (roda) dan momentum (fling) dengan `VelocityTracker`. Pohon aksesibilitas AccessKit: text run per baris dengan geometri karakter, seleksi, action pilih teks, ketik, dan centang task | 25 tes headless dan 4 golden pixel test (`tests/golden`) |
| `rs-editor-desktop` | Shell winit + softbuffer (raster CPU), keyboard, IME, mouse, scroll, clipboard, adapter `accesskit_winit`; contoh `perf` | Diuji manual di macOS, termasuk pohon AX yang terbaca sistem |

Ukuran saat ini (raster CPU, macOS, `--release`, contoh `perf`):

| Dokumen | Buka + frame pertama | Ketikan sampai frame (median / p95) |
| --- | --- | --- |
| 1.000 blok | 36 ms | 0,6 / 0,7 ms |
| 10.000 blok | 64 ms | 0,7 / 1,0 ms |
| 100.000 blok | 272 ms | 2,2 / 3,0 ms |

Pada 100.000 blok, sisa biaya ada di model dokumen: setiap perubahan menyalin array anak level atas (perilaku yang sama dengan ProseMirror). Kalau dokumen sepanjang itu jadi target, anak level atas perlu disimpan dalam pohon persisten.

Golden pixel test memakai `RS_EDITOR_UPDATE_GOLDEN=1 cargo test -p rs-editor-view --test golden` untuk memperbarui gambar. Kegagalan menulis gambar aktual dan diff ke `target/golden-failures/`, dan CI mengunggahnya sebagai artifact. Golden disimpan per OS (`tests/golden/macos`, `tests/golden/linux`): prebuilt Skia untuk Apple merasterisasi tepi glyph dengan CoreText, sedangkan Linux memakai FreeType, jadi tepinya sedikit berbeda walaupun layout identik.

Menjalankan harness:

```bash
cargo run -p rs-editor-desktop --release
```

Argumen `--dark` memakai tema gelap, `--large 10000` membuka dokumen panjang, dan F2 mengganti tema.

Catatan yang dibawa ke fase berikutnya:

- Paragraf RTL (Arab) masih rata kiri. Arah paragraf perlu dideteksi dari teks (atau atribut `dir`) dan dipakai untuk perataan.
- Pohon aksesibilitas dibangun ulang dalam O(jumlah blok) per update saat teknologi bantu aktif. Ini cukup untuk 10.000 blok, belum untuk 100.000.
- Mengetik lewat `AXSelectedText` di macOS belum didukung adapter AccessKit. Screen reader tetap bisa membaca, memindahkan seleksi, dan mengetik lewat keyboard biasa.

## P3 — Web · Berjalan

Target: editor di web HP terasa seperti di Android native, dengan API yang sama dengan Tiptap.

### Milestone 1: paket web dan sentuhan · Selesai

- **Crate `rs-editor-api`:** API host yang dipakai bersama semua shell. Isinya konten (JSON, HTML, teks), perintah (`run`, `can`, chain, `isActive`, `getAttributes`, revisi dokumen dan seleksi), sentuhan, mouse, keyboard, protokol IME, dan clipboard. Satuan koordinatnya dipilih per shell: piksel perangkat untuk Android, piksel CSS untuk web. Kelas Kotlin `NativeEditor` sekarang hanya pembungkus tipis, dan ekspor `rse_*` di web memakai API yang sama.
- **Paket `@rs-editor/web` (`platforms/web`):**
  - `createEditor` dengan API ala Tiptap:
    - perintah: `chain().focus()…run()` (nama perintah apa pun lewat Proxy), `can()`, `commands`, `isActive`, `getAttributes`;
    - konten: `getJSON`/`getHTML`/`getText`, `setContent`;
    - event: `transaction`, `update`, `selectionUpdate`, `focus`, `blur`.
  - Tipe tersedia di `index.d.ts`. Tidak ada langkah build JS.
- **Sentuhan di web HP memakai pengenal gesture Rust yang sama dengan Android:**
  - menu seleksi DOM bergaya shadcn;
  - magnifier dari salinan frame canvas;
  - `navigator.vibrate`;
  - keyboard muncul saat teks diketuk;
  - `visualViewport` untuk keyboard yang menutupi halaman.
- **Batas karakter IME:** `characterboundsupdate` di EditContext sekarang memakai batas tiap karakter yang sebenarnya, bukan kotak caret.
- **Ditemukan dan diperbaiki saat uji di Chrome Android (Xiaomi, Android 15):**
  - **Seleksi tertutup blok biru tua.** Blend multiply butuh salinan framebuffer, dan WebGL tidak bisa menyalin framebuffer RGB ke tekstur RGBA. Perbaikannya: canvas WebGL dengan alpha.
  - **Menu hilang setelah tekan lama.** Gestur tekan lama bawaan Chrome pada elemen yang bisa diedit mengirim `pointercancel`. Perbaikannya: `preventDefault` pada `touchstart`.
- **Uji di perangkat:**
  - ketuk ganda dan tekan lama memilih kata, lalu menu muncul;
  - tarik handle memperluas per kata dan menyusut per karakter, dengan magnifier;
  - toolbar demo menyala sesuai seleksi;
  - ketikan Gboard lewat EditContext;
  - scroll dengan momentum fling.

### Milestone 2: Markdown, font, dan menu desktop · Selesai

- **Markdown di `rs-editor-markup`:**
  - **Impor:** Markdown diubah ke HTML oleh pulldown-cmark (GFM: coretan, task list, tabel), lalu dibaca parser HTML yang sama, sehingga hasilnya selalu valid. Parser HTML sekarang juga mengenali task list tanpa class (`<li><input type="checkbox">`), seperti yang ditulis renderer Markdown dan banyak situs.
  - **Ekspor:** serializer bergaya prosemirror-markdown. Garis bawah, sorotan, sub/superskrip ditulis sebagai tag HTML. Di tempat aturan *flanking* CommonMark akan membuat `**`/`*`/`~~` gagal dibaca (tanda baca di tepi, delimiter bersarang atau berdempetan), mark itu ditulis sebagai `<strong>`/`<em>`/`<s>`.
  - **Escape:** teks yang mirip sintaks Markdown di-escape: `#`, `1.`, `-` di awal baris, `!` sebelum link, `#` di akhir judul, dan entity. Spasi yang akan hilang ditulis sebagai `&nbsp;`, lalu dikembalikan menjadi spasi saat dibaca.
  - **Tes:** property test round-trip (16.000 kasus acak saat pengembangan).
  - **Yang hilang** karena memang tidak ada di Markdown: perataan teks, target link, warna sorotan, paragraf kosong, dan mark pada spasi di tepi teks bermark.
- **API Markdown:**
  - `getMarkdown`/`setMarkdown` di semua platform (`markdown` di Kotlin, `contentType: "markdown"` di web seperti Tiptap 3), termasuk konten awal Markdown yang tidak bisa di-undo jadi kosong.
  - Opsi `pasteMarkdown` membaca teks polos yang ditempel sebagai Markdown. Satu paragraf digabung ke paragraf yang sedang diedit.
- **Font sesuai kebutuhan:**
  - `LayoutEngine::add_font` menambah font cadangan saat runtime, dan layout mencatat karakter tanpa glyph (`unresolved_codepoints` SkParagraph). View mengirimnya sebagai effect `missingCharacters`.
  - Paket web memuat font dari daftar `fonts` aplikasi yang rentang Unicode-nya cocok, dan menyediakan `editor.addFont()` untuk aplikasi yang memuat font sendiri. Preset `notoFonts` (emoji di jsDelivr, Jepang/Korea di Google Fonts) hanya dipakai kalau aplikasi memilihnya.
  - Diuji dengan font lokal: teks Jepang dan Korea tampil setelah font dimuat.
- **Menu klik kanan di web desktop:** menu yang sama dengan menu sentuh, muncul di posisi kursor, dan ditutup dengan Escape atau klik di luar. Klik kanan tidak lagi memindahkan caret.
- **Klik ganda/tiga kali di web:** dihitung sendiri ketika `pointerdown.detail` bernilai 0, yang diizinkan oleh spesifikasi Pointer Events.
- **`selection`** (`{anchor, head, from, to, empty}`) tersedia di API.

### Milestone 3: aksesibilitas dan browser lain · Berjalan

Sudah:

- **DOM cermin untuk screen reader (`platforms/web/src/a11y.js`):**
  - `EditorApi::accessibility_json` mengekspor pohon yang sama dengan yang diterima AccessKit, lengkap dengan koordinat absolut.
  - Paket web membangunnya menjadi DOM tak terlihat yang ditumpuk di atas canvas: `h1`–`h6`, `ul`/`li`, `pre`, `blockquote`, `role="checkbox"` dengan `aria-checked`, dan satu span per baris di posisi aslinya.
  - Elemen yang tidak berubah dipakai ulang berdasarkan id, dan pembaruan dijalankan saat browser senggang.
  - Klik pada checkbox dari screen reader diteruskan sebagai aksi AccessKit `Click`.
- **Pembaruan inkremental:** `accessibility_update_json` hanya mengirim node yang berubah, memakai diff yang sama dengan adapter Android. Cermin memposisikan elemen relatif terhadap parent-nya lewat transform AccessKit, sehingga scroll hanya menggeser satu elemen. Pengukuran di dokumen 10.000 blok: pembaruan setelah mengetik turun dari 2.020 ms menjadi 47 ms, scroll dari 290 ms menjadi 20 ms, dan pengumuman tombol panah dari 64 ms menjadi 9 ms.
- **Label checkbox** diambil dari teks task-nya (`aria-labelledby`).
- **Umpan balik suara untuk navigasi keyboard** lewat `aria-live`: karakter (kiri/kanan), kata (Ctrl/Alt), baris (atas/bawah, Page), atau teks yang dipilih (Shift).
- **Diuji di Chrome lewat pohon aksesibilitasnya:** judul, list, checkbox, dan semua baris teks terekspos; klik checkbox dari cermin mengubah dokumen; pengumuman cocok dengan tiap tombol.
- **Emoji tanpa unduhan di web:** layout menjadikan setiap kluster emoji (ZWJ, bendera, keycap, warna kulit, `FE0F`) satu placeholder kotak berukuran sekitar 1,2 em, sedangkan `TextLayout` memetakan offset dokumen ↔ paragraf sehingga caret, seleksi, dan hit-test tetap per grapheme. Browser menggambar emoji dengan font sistem di canvas 2D (effect `missingEmoji` → `add_emoji_image`), lalu Skia menempelkan gambarnya. Kalau aplikasi memberi font emoji lewat `fonts`, emoji kembali dirender sebagai teks. Di Chrome, warna kulit, bendera, keycap, keluarga ZWJ, dan ❤️ tampil benar; panah melompati satu emoji utuh; dan pengumuman screen reader menyebut emojinya (indeks dihitung per grapheme).
- **CJK per subset:** font CJK dimuat dalam subset per rentang Unicode (format WOFF, paket fontsource) yang dipilih berdasarkan `unicode.json`. Demo dengan teks Jepang dan Korea hanya mengunduh 179 KB, dibanding 10–14 MB untuk font utuh. Skia di wasm terbukti membaca WOFF. Pendekatan ini sama dengan Flutter Web. Tiptap dan Notion tidak menghadapi masalah ini karena teksnya digambar browser (DOM).
- **Fokus di jalur textarea:** sebelumnya klik mengembalikan fokus ke canvas sehingga caret hilang di Safari; sekarang sudah diperbaiki.
- **Jalur textarea** (Safari, Firefox) diuji di Chrome dengan `?input=textarea`: ketik, Backspace, Enter, dan undo berfungsi. Textarea kini menjadi kotak teks berlabel, sedangkan canvas disembunyikan dari pohon aksesibilitas.

Belum:

- Uji dengan screen reader sungguhan (VoiceOver, NVDA, TalkBack di Chrome Android).
- Uji di Safari (macOS, iOS) dan Firefox.
- Cermin seleksi DOM (sekarang seleksi diumumkan, bukan dicerminkan).
- Node konten masih membawa daftar semua blok setiap kali berubah (sekitar 140 KB JSON untuk 10.000 blok). Kalau perlu, daftar ini bisa dikirim sebagai selisih saja.

## P4 — Android · Selesai

Target: editor terasa seperti kolom teks bawaan Android, bukan sekadar tampil benar.

### Milestone 1: seleksi sentuh terasa native · Selesai

Sudah ada dan diuji (15 tes skenario headless di `rs-editor-view/src/touch.rs`, plus uji manual di perangkat 23049PCD8G):

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

Inti di Rust (`rs-editor-view`), dipakai juga oleh web di HP (P3) dan iOS (P6):

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

### Milestone 2: input dan platform · Selesai (TalkBack jadi tugas lanjutan)

Sudah:

- `CursorAnchorInfo` (posisi caret dengan baseline, batas tiap karakter komposisi, batas editor untuk tulisan tangan) dikirim saat IME memintanya lewat `requestCursorUpdates`. Gboard memintanya dalam mode MONITOR, dan `dumpsys input_method` di perangkat menunjukkan posisi caret, baseline, batas editor dan matriks layar yang benar. Keyboard Xiaomi tidak memintanya.
- Crate `rs-editor-markup`: HTML ↔ dokumen. Parser bergaya ProseMirror (selalu menghasilkan dokumen valid; Google Docs, Word, GitHub, tag soup), serializer bergaya Tiptap dengan JSON slice tersemat sehingga salin-tempel di Rs Editor sendiri persis. 11 tes termasuk property test tag soup.
- Salin menghasilkan teks + HTML dan tempel memakai HTML lebih dulu, di desktop (arboard), web (`text/html`) dan Android (`ClipData.newHtmlText`). Di perangkat: kata tebal yang disalin lalu ditempel tetap tebal.
- TalkBack lewat AccessKit (`accesskit_android`, delegate tertanam sebagai dex). `uiautomator dump` di perangkat menunjukkan editor sebagai `EditText` berisi seluruh teks dokumen, blok sebagai `View`, list sebagai `ListView`, task sebagai `CheckBox` dengan status yang benar, semuanya dengan posisi di layar.

Belum: Samsung Keyboard (tidak ada di perangkat uji), `addVisibleLineBounds` (API 34) untuk tulisan tangan stylus.

**Tugas lanjutan: TalkBack.** Uji langsung di perangkat (TalkBack 17.0.1, 2026-10-13):

- Berjalan: editor dibacakan sebagai kolom edit berisi seluruh teks; struktur (judul, paragraf, list, checkbox dengan statusnya) dan posisinya terekspos.
- Probe debug yang meniru TalkBack (`adb shell am broadcast -a com.rseditor.demo.A11Y_PROBE --ei granularity 4`, ada di demo Java lama sampai commit fefb288) memindahkan kursor per baris dengan benar. Pemetaan posisi di batas paragraf sudah diperbaiki, dan algoritma traversal AccessKit diuji di `screen_reader_line_steps_move_forward_through_every_line`.
- Belum berjalan: dengan kontrol membaca "Lines", usapan TalkBack di perangkat tidak mengirim `ACTION_NEXT_AT_MOVEMENT_GRANULARITY` (log hanya menunjukkan lepas/pasang fokus), sehingga pengguna tidak bisa bergerak per baris. Juga, usap kanan/kiri tidak berpindah per judul/paragraf karena semuanya berada di dalam satu kolom edit.
- Arah: pelajari cara Chrome mengekspos `contenteditable` ke TalkBack (navigasi per paragraf dan judul tanpa membaca dua kali) dan terapkan lewat AccessKit; cari tahu kenapa TalkBack tidak mengirim aksi granularitas ke node kita.

Rencana awal milestone:

- `CursorAnchorInfo` agar popup kandidat dan input tulisan tangan berada di tempat yang benar.
- Uji Gboard dan Samsung Keyboard: komposisi, koreksi otomatis, input suara, swipe typing.
- Clipboard kaya (HTML) dan tempel dari aplikasi lain.
- AccessKit untuk TalkBack.

### Milestone 3: paket library · Selesai

- **Binding Kotlin lewat BoltFFI 0.29.3** (sama dengan rs-organizer):
  - `crates/rs-editor-android/src/api.rs` mengekspor kelas `NativeEditor`, yang mencakup konten (JSON, HTML, teks), perintah, sentuhan, IME, dan clipboard. Nilai terstruktur dikirim sebagai string JSON.
  - API ini juga dibangun di host, sehingga bisa dites dengan `cargo test -p rs-editor-android`. Salah satu tesnya memastikan setiap perintah di `RsEditorChain` Kotlin benar-benar ada.
  - JNI tangan hanya tersisa untuk hal yang butuh objek Java (`shell.rs`): Surface/EGL dan AccessKit.
- **Packaging:** `platforms/android/package-native.py` menyalin pendekatan rs-organizer. Skrip ini menjalankan build binding-expansion, mengompilasi `jni_glue.c`, lalu me-link staticlib dengan clang++ tanpa whole-archive (menghindari simbol HarfBuzz ganda dari Skia), plus `-z max-page-size=16384`.
  - BoltFFI membaca metadata dari build host memakai variabel lingkungan yang tidak dilacak Cargo. Karena itu skrip memakai target dir terpisah (`target/boltffi-metadata`).
- **Proyek Gradle (Kotlin DSL, versi sama dengan rs-organizer):**
  - `:rs-editor` berisi `RsEditorView` dengan API ala Tiptap:
    - konten: `json`, `html`, `text`, `isEditable`;
    - perintah: `chain().focus().toggleBold().run()`, `can()…run()`, `isActive`, `getAttributes`;
    - `OnChangeListener` yang dipicu oleh `revision`.
    - Konten awal diberikan lewat konstruktor, sehingga undo tidak bisa mengosongkan dokumen.
  - `:rs-editor-compose` berisi `RsEditor` dan `rememberRsEditorState`. Pembacaan seperti `isActive` bersifat observable, jadi toolbar ikut berubah saat seleksi berpindah.
  - `:sample` adalah aplikasi Compose dengan toolbar di atas keyboard. Tombol menyala saat aktif dan nonaktif bila `can()` bernilai false.
- **Perbaikan inti yang ditemukan lewat toolbar:** dry run `chain` sebelumnya memeriksa setiap perintah terhadap state awal. Akibatnya `can()` untuk perintah gabungan (misalnya bullet list → task list, yaitu `clearNodes` lalu `wrapInList`) salah bernilai false. Sekarang dry run mensimulasikan rantainya, sama seperti Tiptap.
- **Uji di perangkat (Gboard):**
  - B/I membuat kata terpilih tebal-miring dan kedua tombolnya menyala; keyboard tetap terbuka.
  - Mengetik lalu undo dari toolbar berfungsi, dan redo menjadi aktif.
  - Paragraf menjadi list, dan list item menjadi task item.
  - H2 dan kutipan nonaktif di dalam list item, sesuai skema.
  - Keluar/masuk aplikasi dan background/foreground berjalan tanpa crash.
  - `uiautomator` masih menampilkan `EditText` dan `CheckBox`.

## P5 — UI di dalam canvas dan paritas Tiptap · Selesai

UI editor digambar Skia di canvas yang sama, sehingga tampilannya identik di semua platform. Toolbar tetap bisa dibuat aplikasi lewat API perintah.

### Milestone 1: komponen UI dan bubble menu · Selesai

- **Crate `rs-editor-ui`:** komponen bergaya shadcn/ui yang digambar Skia.
  - Ikon Lucide (path SVG → `PathBuilder`), digambar dengan stroke 2 unit dari 24 dengan ujung bulat, seperti aslinya.
  - `paint_panel`: latar popover, border, radius `md`, dan bayangan blur.
  - `Toolbar`: tombol 32 px; aktif memakai warna accent, hover memakai muted, nonaktif 50%. Hit test dan pratinjau PNG lewat `RS_EDITOR_UI_PREVIEW` untuk dicek mata.
  - `Appear`: animasi muncul 150 ms dan hilang 100 ms (ease-out cubic, zoom 0,95 → 1), seperti popover shadcn.
- **Bubble menu** (`rs-editor-view/src/bubble.rs`), seperti BubbleMenu Tiptap:
  - Tombol: tebal, miring, garis bawah, coret, kode, sorotan. Status aktif dan nonaktif dihitung ulang hanya saat revisi editor berubah.
  - Muncul untuk seleksi teks yang dibuat dengan mouse atau trackpad, setelah tombol dilepas. Seleksi sentuh tetap memakai menu seleksi platform, karena itu yang diharapkan pengguna HP.
  - Berada 8 px di atas seleksi, pindah ke bawah bila tidak muat, dan hilang saat seleksi keluar layar, kosong, atau sedang komposisi IME.
  - Klik tombol tidak memindahkan seleksi; kursor berubah menjadi pointer di atas tombol.
  - Bisa dimatikan: `set_bubble_menu(false)` di Rust, opsi `bubbleMenu: false` di web.
  - Android native belum meneruskan mouse dan trackpad ke editor, jadi di sana menu ini belum pernah muncul. Itu tugas lanjutan.
- **Uji di Chrome desktop:** klik ganda memilih kata dan menu muncul dengan animasi; B membuat teks tebal dan tombolnya menyala; undo dan klik di tempat lain menyembunyikan menu; seleksi tarik dan tema gelap benar.

### Milestone 2: menu blok dan popover tautan · Selesai

- **Komponen baru di `rs-editor-ui`:**
  - `Menu`: daftar perintah seperti Command shadcn. Baris 32 px berisi ikon, label, dan petunjuk di kanan; baris terpilih memakai warna accent; isinya bisa di-scroll.
  - `TextField`: kolom satu baris seperti Input shadcn, dengan ring fokus, warna destructive saat ditolak, seleksi, garis bawah komposisi, dan scroll horizontal. Teks, seleksi, dan komposisinya disimpan dalam satuan UTF-16, satuan yang dipakai IME.
  - Label UI dibentuk oleh `LayoutEngine::ui_label` dengan font tema, lalu dipotong dengan elipsis.
- **Menu blok** (`rs-editor-view/src/slash.rs`), seperti Notion dan template slash command Tiptap:
  - Muncul saat `/` diketik di awal blok atau setelah spasi, tetapi tidak di blok kode. Statusnya diturunkan dari dokumen seperti plugin Suggestion Tiptap, sehingga mengetik, menghapus, undo, dan IME tetap konsisten.
  - Baris: teks, judul 1–3, list poin, list bernomor, daftar tugas, kutipan, blok kode, pemisah, masing-masing dengan pintasan Markdown-nya. Kueri menyaring label dan kata kunci dalam dua bahasa (`h2`, `todo`, `kutipan`). Baris yang perintahnya tidak bisa jalan di posisi itu (dry run `can`) disembunyikan.
  - Panah memilih, Enter atau Tab menerapkan, Esc menutup sampai kueri berakhir. Enter dari IME (baris baru) juga menerapkan. Hover memilih, klik menerapkan, roda mouse menggulir menu. Di layar sentuh, ketuk menerapkan dan geser menggulir menu.
  - Menghapus kueri dan mengubah blok terjadi dalam satu transaksi dengan langkah undo sendiri: undo mengembalikan `/kueri`.
  - Menu muncul di bawah baris, atau di atasnya kalau ruang di bawah lebih sempit (misalnya tertutup keyboard).
- **Popover tautan** (`rs-editor-view/src/link.rs`):
  - Dengan caret di dalam tautan, popover menampilkan alamat dan tombol buka, salin, ubah, dan hapus. Di editor read-only hanya ada buka dan salin. Esc menyembunyikannya sampai caret keluar dari tautan itu.
  - Kolom alamat dibuka lewat Mod-K, tombol tautan di bubble menu, tombol ubah, atau API (`edit_link`, `editor.editLink()`, `RsEditorView.editLink()`).
  - Enter menerapkan tautan ke seleksi atau seluruh tautan yang ada. Tanpa seleksi, alamat dimasukkan sebagai teks bertaut. Alamat dilengkapi otomatis (`https://`, `mailto:`). Alamat yang ditolak extension Link membuat kolom merah dan tetap terbuka. Kolom kosong menghapus tautan.
  - Selama kolom terbuka, semua input teks diarahkan ke kolom: tombol, teks, komposisi IME, salin/potong/tempel, dan seleksi lewat mouse atau sentuhan. IME melihat teks kolom (`ImeContext::field`; `start` −2 di API). Adapter web memasang `inputmode="url"`, dan Android memulai ulang input dengan `TYPE_TEXT_VARIATION_URI` dan tombol Go.
  - Handle dan menu seleksi native disembunyikan selama kolom terbuka.
- **Bahasa UI:** `set_locale` (Indonesia, Inggris sebagai cadangan). Web memakai `navigator.language`, Android `Locale.getDefault()`, desktop `LANG`.
- **Pemanasan shader:** WebGL mengompilasi program GPU saat pertama dipakai, sehingga popover pertama butuh 150–680 ms. Shell web (`rse_warm_up`, saat idle) dan Android (setelah frame pertama) kini menggambar setiap komponen sekali di surface offscreen. Frame terlama saat menu pertama muncul turun menjadi sekitar 19 ms.
- **Uji:**
  - 14 tes view baru (menu blok dan tautan) dan 3 tes `rs-editor-ui` (kolom teks, IME, menu).
  - Chrome desktop, mode EditContext dan textarea: menu blok, popover, dan kolom alamat lewat keyboard sungguhan.
  - Android dengan Gboard: menu blok di atas keyboard, scroll dengan jari, ketuk Kutipan; kolom alamat dengan keyboard URL dan tombol Go.
- **Belum:**
  - Menu dan popover belum masuk pohon aksesibilitas (listbox dan kolom teks untuk screen reader).
  - Placeholder bawaan ("Write something…") belum ikut `locale`.

### Milestone 3: blok dan konten · Selesai

- **Drag handle** (`rs-editor-view/src/blocks.rs`), seperti Notion dan DragHandle Tiptap:
  - Grip ikon Lucide di margin kiri, sejajar baris pertama blok. Dengan mouse, grip mengikuti blok di bawah kursor dan hilang saat kursor keluar dari editor (`pointer_leave`). Di layar sentuh, grip muncul untuk blok yang berisi caret.
  - Klik grip memilih blok (NodeSelection), sehingga Backspace menghapus blok dan salin menyalinnya.
  - Menarik grip memindahkan blok. Selama ditarik, blok asal diredupkan dan garis menunjukkan posisi jatuhnya. Di dekat tepi atas atau bawah, view ikut scroll. Esc membatalkan. Satu pemindahan menjadi satu langkah undo.
  - Di Android, area grip dikecualikan dari gesture Back sistem (`systemGestureExclusionRects`), karena grip berada di tepi kiri layar.
  - Bisa dimatikan: `set_drag_handle(false)`, `dragHandle: false`, `RsEditorView.dragHandle`.
  - Setiap item list punya grip sendiri, di kiri penanda list (lihat milestone 4).
- **Gambar:**
  - Layout mencatat `src` yang belum punya gambar (efek `MissingImages`). Host mendekode gambar dan memberikan pikselnya lewat `add_image`, atau memanggil `image_failed`. Gambar tanpa atribut `width`/`height` memakai ukuran aslinya, dibatasi lebar kolom. Saat menunggu, gambar tampil sebagai kotak muted; kalau gagal, kotak itu diberi tanda silang. Gambar digambar dengan sudut membulat dan sampling mipmap.
  - Web: browser mendekode gambar (format apa pun yang didukungnya; gambar lintas origin butuh CORS), maksimal 2048 px.
  - Android: `ImageLoader` membaca `data:`, `content:`/`file:`, dan `http(s)`, lalu `BitmapFactory` mendekodenya dengan inSampleSize di thread latar.
  - Menu `/` punya baris Gambar. Kalau aplikasi punya pemilih gambar (`uploadImage` di web, `onPickImage` di Android/Compose), editor mengirim efek `PickImage`. Kalau tidak, editor meminta alamat gambar lewat kolom yang sama dengan tautan.
  - Web juga menerima gambar yang ditempel atau dijatuhkan ke editor bila `uploadImage` ada. Sampel Android memakai Photo Picker sistem.
  - Node selection gambar kini membungkus gambarnya, bukan seluruh lebar kolom.
- **Tempel tautan** (seperti `linkOnPaste` Tiptap): alamat yang ditempel di atas teks terpilih menautkan teks itu; tanpa seleksi, alamat masuk sebagai tautan. Di blok kode, alamat ditempel sebagai teks biasa.
- **Placeholder:** teks bawaan mengikuti bahasa UI. Baris kosong menampilkan "Tulis, atau ketik “/” untuk perintah", dan judul kosong menampilkan namanya ("Judul 1"). Aplikasi bisa menggantinya (`placeholder`).
- **Character count:** `character_count()` di API, `editor.storage.characterCount.characters()`/`words()` di web (sama dengan Tiptap), `characters`/`words` di Kotlin dan Compose. Batas karakter diatur saat editor dibuat (`characterLimit`).
- **Text-align dan list bertingkat** sudah ada sejak P1–P2: perintah `setTextAlign`, pintasan Mod-Shift-L/E/R/J, serta Tab dan Shift-Tab di list.
- **Perbaikan dari uji pengguna:** handle seleksi sentuh kini boleh saling melewati seperti di Android native. Handle yang ditarik melewati handle lain bertukar peran (awal ↔ akhir), dan seleksi terus mengikuti jari.
- **Uji:**
  - 9 tes view baru (drag handle dengan mouse dan sentuhan, gambar, menu Gambar, tempel tautan, placeholder). Tes handle seleksi diperbarui untuk perilaku saling melewati.
  - Golden image placeholder diperbarui.
  - Chrome: gambar SVG dimuat, grip muncul saat hover, blok gambar dipindah lewat drag dengan garis jatuh terlihat, undo.
  - Android (perangkat): gambar `data:` dimuat lewat BitmapFactory, grip muncul untuk blok caret, paragraf dipindah ke bawah list dengan jari (gesture Back tidak terpicu), handle seleksi saling melewati.
- **Belum** (dikerjakan di milestone 4): menarik item list satu per satu, aksesibilitas menu, gambar di harness desktop.

### Milestone 4: aksesibilitas menu dan item list · Selesai

- **Aksesibilitas menu dan popover** (pohon AccessKit yang sama untuk TalkBack, VoiceOver, dan mirror ARIA di web):
  - Menu `/` menjadi list box. Baris terpilihnya menjadi *active descendant* editor (pola combobox), sehingga panah atas/bawah membacakan baris tanpa memindahkan fokus dari teks.
  - Bubble menu menjadi toolbar berisi tombol toggle (status tebal dan lainnya terbaca).
  - Popover tautan menjadi grup berisi tautan dan tombol-tombolnya. Saat kolom alamat terbuka, fokus pindah ke kolom itu (TextInput dengan nilainya).
  - Grip menjadi tombol "Pilih atau seret untuk memindahkan blok".
  - Semua bisa diaktifkan dengan Click dari screen reader, dan labelnya mengikuti bahasa UI.
  - Mirror web: role ARIA listbox/option/toolbar/button/link/group/textbox, `aria-activedescendant` dan `aria-expanded` pada input yang fokus, nama input berganti menjadi nama kolom alamat saat kolom itu fokus, dan pengumuman baris tidak terdengar saat panah dipakai di menu.
- **Drag item list:**
  - Grip muncul di kiri penanda item yang berada di bawah mouse (item terdalam) atau yang berisi caret.
  - Item bisa dijatuhkan di antara item list lain yang menerima jenisnya (listItem ke list poin/bernomor, taskItem ke daftar tugas), di kedalaman mana pun. Kalau dua list berakhir di tempat yang sama, posisi horizontal pointer menentukan kedalamannya.
  - Dijatuhkan di antara blok tingkat atas, item menjadi list sendiri dengan jenis list asalnya. Item terakhir sebuah list membawa list itu, sehingga tidak tersisa list kosong.
  - Di awal atau akhir list, item tetap berada di list itu.
- **Harness desktop:** gambar dari path atau `file://` didekode Skia (PNG, JPEG, GIF, BMP).
- **Perbaikan dari uji pengguna — salin/potong di web kosong:** `copy()`/`cut()` juga mengirim efek Clipboard. Akibatnya, setelah event `copy` browser mengisi clipboard, `navigator.clipboard.write` asinkron ikut berjalan dan bisa balapan, lalu clipboard kosong. Menu Salin juga menulis dua kali. Sekarang salinan yang diminta host hanya dikembalikan sebagai nilai; efek Clipboard tersisa untuk salinan dari editor sendiri (tombol salin di popover tautan). Ini berlaku juga di Android dan desktop.
- **Uji:** 2 tes aksesibilitas menu, 3 tes drag item list, tes salin/potong tanpa efek, dan tes dekode gambar desktop. Di Chrome, mirror ARIA menu `/` (listbox, 11 option, `aria-activedescendant`) dicek dan bersih kembali setelah Esc; event copy dan cut mengisi clipboard tanpa penulisan asinkron.
- **Belum:**
  - Uji dengan screen reader sungguhan (VoiceOver, NVDA, TalkBack).
  - Uji drag item list di perangkat Android: build berhenti karena disk hampir penuh.

## Berikutnya

1. **P6:** iOS dan macOS (UITextInput, NSTextInputClient, Metal).
2. **P3 (web):** uji screen reader sungguhan (VoiceOver, TalkBack di Chrome Android) dan Firefox.
3. **Tugas lanjutan Android:** navigasi TalkBack (lihat milestone 2), Samsung Keyboard, `addVisibleLineBounds`, paket AAR/Maven.
4. **Desktop (P7):** menu klik kanan Potong/Salin/Tempel memakai sinyal `SelectionMenu` yang sama.

Spike web dan Android sudah membuktikan jalurnya: [laporan spike P0](spikes/p0-web-android.md).
