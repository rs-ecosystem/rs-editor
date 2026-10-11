# ADR 0002: Skia sebagai renderer default yang opsional

- **Status:** diterima sebagai arah awal; detail layout masih perlu spike
- **Tanggal:** 2026-10-11

## Konteks

Skia menjadi sasaran rendering utama, tetapi core harus dapat dipakai tanpa mengikat aplikasi konsumen pada Skia atau build native Skia.

## Keputusan

Renderer Skia akan berada di crate terpisah dan menjadi backend resmi pertama. Crate core dan renderer-neutral tidak mengaktifkan Skia secara default. Layout/render contract ditinjau setelah spike teks multilingual dan target web.

## Konsekuensi

- Konsumen dapat memilih fitur Skia atau backend lain.
- Build dan distribusi native tetap menjadi perhatian adapter.
- Jalur browser perlu keputusan tersendiri antara CanvasKit dan backend WASM lain.
