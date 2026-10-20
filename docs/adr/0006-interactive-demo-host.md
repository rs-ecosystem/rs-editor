# ADR 0006: Host demo interaktif dengan server Rust lokal dan WKWebView

- Status: digantikan oleh [ADR 0007](0007-rs-editor-v2-architecture.md)
- Tanggal: 2026-10-11

## Konteks

G3 membutuhkan demo yang menjalankan core Rust, memperlihatkan renderer Skia, menerima input platform, dan dapat diuji tanpa membangun adapter permanen untuk setiap toolkit.

## Keputusan

`openrich-demo` menjalankan `EditorState` dan `SkiaDocumentLayout` di proses Rust lokal. Host HTTP loopback menyediakan state/command JSON dan scene atomik berisi geometri serta pixel raster Skia. Frame kecil memakai RGBA langsung, sedangkan frame di atas 8 MiB memakai PNG. UI browser memakai contenteditable semantic DOM sebagai lapisan input, clipboard, IME, dan accessibility; frame Skia menjadi lapisan visual. Launcher macOS membuka UI yang sama di jendela WKWebView dan mengelola lifecycle proses server.

Browser dapat dijalankan langsung dengan `cargo run -p openrich-demo`; host macOS dibuka melalui `scripts/run-macos-demo.sh`. Tidak ada dependency UI toolkit yang masuk ke crate core.

## Konsekuensi

- Demo membuktikan kontrak core, renderer, dan input host pada satu loop lokal tanpa membuat core bergantung pada DOM atau SwiftUI.
- Semantic DOM menyediakan peran heading, paragraf, daftar, dan kontrol input yang dapat dibaca accessibility API browser/WebKit.
- Preedit IME tetap sementara di host; hanya teks commit yang masuk ke core sebagai satu transaksi.
- Klik, drag, seleksi kata/paragraf, dan navigasi panah memakai geometri Skia yang tersimpan di host. Caret/selection digambar dalam overlay rAF lokal; aksi ini tidak memerlukan round-trip HTTP atau raster ulang teks.
- Perubahan dokumen/viewport menghasilkan scene baru. Pixel dan geometri berasal dari layout yang sama; respons lama dibatalkan/ditolak dan buffer transfer dibatasi sebelum memakai fallback PNG.
- Worker HTTP persisten mempertahankan cache font per thread; shaping/raster tidak memegang mutex editor.
- HTTP dan raster software tetap merupakan jalur demo. Adapter surface langsung atau CanvasKit/WASM diperlukan untuk integrasi produksi lintas platform.
- Uji VoiceOver dan input method fisik perlu dilakukan pada setiap host/browser yang akan dinyatakan didukung.
