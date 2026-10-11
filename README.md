# OpenRich

OpenRich adalah engine rich text lintas platform yang sedang dibangun dengan Rust. Engine dirancang headless: model dokumen dan operasi editing dapat digunakan tanpa framework UI, sementara renderer dan adapter platform dipasang secara terpisah.

Nama ini masih nama kerja.

## Status

**Tahap awal: fondasi core.** API belum stabil dan belum siap untuk digunakan aplikasi produksi.

## Prinsip arsitektur

- `openrich-document` menyimpan model dokumen dan selection tanpa ketergantungan UI.
- `openrich-editor` akan mengelola operasi dan state editing di atas model dokumen.
- `openrich-render` mendefinisikan data dan kontrak render; implementasi Skia akan ditambahkan sebagai crate opsional.
- Adapter aplikasi meneruskan input platform ke core dan memetakan semantik editor ke API accessibility platform.
- Crux dapat memakai engine sebagai konsumen, tetapi tidak menjadi dependency core.

## Struktur awal

```text
crates/
├── openrich-document/  # schema dokumen, posisi, dan selection
├── openrich-editor/    # state editor dan command editing
└── openrich-render/    # kontrak scene dan renderer
```

## Mulai mengembangkan

Gunakan toolchain Rust stabil yang mendukung edition 2024. Workspace ini belum menyediakan adapter platform atau renderer Skia; keduanya akan ditambahkan setelah kontrak core dan keputusan layout tervalidasi.

## Lisensi

Target lisensi proyek adalah MIT OR Apache-2.0. File lisensi dan audit dependency akan diselesaikan sebelum rilis publik.
