# Spike P0: web dan Android

> 2026-10-13. Tujuan: membuktikan bahwa `EditorView` yang sama (layout, render, editing di Rust) bisa berjalan di browser lewat WebAssembly + WebGL2 dan di Android lewat `SurfaceView` + OpenGL ES, dengan input dari IME sungguhan.

## Ringkasan

| Spike | Hasil | Status |
| --- | --- | --- |
| S1 Skia di Emscripten/WebGL2 | Editor penuh berjalan di Chromium. wasm 8,95 MB mentah, **2,65 MB brotli** (font dan data ICU ikut di dalamnya) | Berhasil, dengan satu solusi build (lihat bawah) |
| S2 EGL + Ganesh di `SurfaceView` | Berjalan di perangkat 23049PCD8G (Android 15, arm64). Library 20 MB (7,5 MB gzip), alignment 16 KB | Berhasil; satu bug lifecycle ditemukan dan diperbaiki |
| S3 `InputConnection` | Keyboard bawaan Xiaomi (`com.preff.kb`): ketik, kapitalisasi otomatis, saran kata, komposisi ulang, Backspace, Enter | Berhasil; Gboard dan Samsung Keyboard belum dicoba |
| S4 EditContext dan fallback textarea | Keduanya berjalan di Chromium (ketik, Enter, Backspace, input rule, undo/redo) | Berhasil di Chromium; Safari dan Firefox belum dicoba |

## S1 — Web (`crates/rs-editor-web`)

- **Build:** `crates/rs-editor-web/build.sh` (emsdk 6.0.12, Rust 1.98, `wasm32-unknown-emscripten`). Skia memakai binary prebuilt `ganesh-gl-jpegd-jpege-pdf-textlayout`.
- **Masalah model exception.** rustc selalu men-link dengan `-fwasm-exceptions`. Sementara itu, pustaka C di dalam Skia prebuilt (libpng, libjpeg, FreeType) memakai setjmp/longjmp gaya JavaScript (`invoke_*`), dan emsdk 6 menolak kombinasi keduanya.
  - **Solusi:** build web memakai `panic = "abort"`, dan `emcc-link.sh` membuang `-fwasm-exceptions`, memakai `-sSUPPORT_LONGJMP=emscripten`, serta mendefinisikan tag `__cpp_exception` yang masih dirujuk std.
  - **Alternatif jangka panjang:** build Skia sendiri dengan wasm SjLj. Ini baru perlu kalau web membutuhkan unwinding.
- **Input:**
  - EditContext dipakai untuk Chrome/Edge. Teks yang dilihat IME adalah blok teks di sekitar caret (protokol `ime_*` di `rs-editor-view`).
  - Textarea tersembunyi dipakai sebagai fallback (`?input=textarea` untuk memaksanya). Textarea tidak pernah dipakai untuk geometri.
- **Ukuran (Chromium, macOS):** muat 150–750 ms (frame pertama menyertakan kompilasi shader); frame berikutnya 0,3–1,5 ms.
- **Belum:**
  - Emoji dan CJK belum tampil di web karena tidak ada font sistem di wasm. Rencananya font dimuat bertahap di P3.
  - Safari, Firefox, dan Chrome Android belum diuji.
  - `characterboundsupdate` masih memakai kotak caret untuk semua karakter.

## S2 — Android (`crates/rs-editor-android`, `platforms/android`)

- **Arsitektur:**
  - Java memegang `SurfaceView`, loop `Choreographer`, dan `InputConnection`.
  - Rust memegang EGL (ES 3), Skia Ganesh GL, gesture sentuh (tap, ketuk ganda/tiga, tekan lama untuk memilih kata, scroll + fling dengan `VelocityTracker`), dan editing.
- **Build:**
  - Library: `platforms/android/build-native.sh` (NDK 27.1, API 26, `-z max-page-size=16384`). Sejak P4 milestone 3 diganti `platforms/android/package-native.py` (BoltFFI).
  - APK: Gradle 8.13 + AGP 8.13.2, offline.
  - `libc++` ter-link statis. Dependensinya hanya library sistem (`libEGL`, `libGLESv2`, `libandroid`, `liblog`).
- **Bug lifecycle yang ditemukan:**
  - **Gejala:** setelah kembali dari background dan keyboard muncul, gambar bergeser dan menyisakan area kosong.
  - **Penyebab:** buffer window EGL baru berganti ukuran setelah `eglSwapBuffers` berikutnya.
  - **Perbaikan:** ukuran render target diambil dari `eglQuerySurface`, dan satu frame lagi diminta selama ukurannya masih berubah.
- **Font:** emoji berwarna dan CJK tampil lewat font sistem Android; Latin, Arab, dan Devanagari memakai font terbundel.

## S3 — `InputConnection`

Urutan panggilan dari keyboard Xiaomi saat mengetik "Halo" lalu memilih saran:

```text
finishComposingText
commitText " " 1
setComposingText "H" 1
setComposingRegion 134 135      ← diulang setelah setiap huruf
setComposingText "Ha" 1
…
commitText "HaloBCA" 1
deleteSurroundingText 1 0
setComposingRegion 134 140      ← menyusun ulang kata sebelum caret
setComposingText "HaloB" 1
```

Temuan:

- Keyboard mengulang `setComposingRegion` dengan rentang yang sama setelah setiap huruf. Sebelumnya ini membuat langkah undo terpecah per huruf; sekarang rentang yang sama tidak melakukan apa-apa (ada tesnya).
- Kapitalisasi otomatis bekerja lewat `getCursorCapsMode`, dan saran kata muncul dari `getTextBeforeCursor`.
- `getExtractedText` sudah diisi, untuk keyboard yang memerlukannya (Samsung).
- **Belum:**
  - Gboard dan Samsung Keyboard belum dicoba, karena mengganti keyboard berarti mengubah setelan perangkat.
  - `CursorAnchorInfo` (posisi popup kandidat, input tulisan tangan) belum diisi.
  - Tekan lama saat ini juga memunculkan keyboard; di P4 seharusnya memunculkan handle dan menu seleksi.

### Gboard (diuji 2026-10-13, P4)

Gboard mengirim tiap huruf langsung dengan `commitText` tanpa komposisi, tetapi tetap menampilkan saran dan kapitalisasi otomatis.

| Aksi | Panggilan dari Gboard |
| --- | --- |
| Memilih saran "Halos" setelah "Halo" | `commitText "s "` |
| Backspace | `sendKeyEvent KEYCODE_DEL` |
| Enter | `commitText "\n"` |
| Koreksi otomatis | `deleteSurroundingText 4 0`, `commitCorrection`, lalu `commitText "halo "` |
| Batal-koreksi (↶) | `replaceText 135 139 "lao"` (API 33+) |

`replaceText` awalnya tidak diteruskan ke editor, sehingga batal-koreksi gagal. Sekarang `replaceText` diteruskan ke `ime_replace`, dan batal-koreksi sudah diuji di perangkat. Glide typing tidak bisa disimulasikan lewat adb; hasilnya tetap datang lewat `commitText`/`setComposingText`. Samsung Keyboard tidak terpasang di perangkat uji.

## Keputusan untuk fase berikutnya

1. Protokol IME berbasis blok (`ImeContext`) dipakai bersama oleh EditContext, textarea, dan `InputConnection`. Di P6 protokol yang sama dipakai untuk `UITextInput`/`NSTextInputClient`.
2. Web memakai `panic = "abort"`. Kalau suatu saat unwinding dibutuhkan di web, Skia harus dibangun sendiri.
3. Prebuilt Skia untuk macOS, iOS, dan Windows tidak memakai FreeType, sehingga tepi glyph sedikit berbeda dari Linux, Android, dan web (layout tetap identik). Golden test disimpan per OS. Supaya piksel benar-benar identik, Skia perlu dibangun sendiri dengan FreeType di platform tersebut; keputusan ini dibuat di P6.
