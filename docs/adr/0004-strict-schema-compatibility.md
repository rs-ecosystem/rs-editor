# ADR 0004: Tolak data schema yang belum dikenal

- **Status:** digantikan oleh [ADR 0007](0007-openrich-v2-architecture.md)
- **Tanggal:** 2026-10-11

## Konteks

Serializer berbasis struct dapat mengabaikan field asing secara default. Jika client lama membaca lalu menulis ulang dokumen yang lebih baru, field yang belum dikenal dapat hilang tanpa pemberitahuan. Engine belum mendefinisikan mekanisme round-trip untuk node extension asing.

## Keputusan

- Parser menolak schema version yang tidak didukung, node/mark variant yang tidak dikenal, serta field asing pada document, block, inline, mark, position, selection, dan transaction.
- Tidak ada jaminan mempertahankan data yang tidak dikenal. Kebutuhan extension round-trip harus dirancang sebelum schema publik stabil.
- Perubahan format yang tidak dapat dibaca client lama harus memakai versi schema baru dan migrasi eksplisit sebelum rilis stabil.

## Konsekuensi

- Client lama gagal dengan error yang terlihat, bukan memuat sebagian data dan menghapus sisanya saat serialize.
- Menambah field memerlukan koordinasi versi schema/API atau atribut opsional yang memang kompatibel.
- Tes mencakup nested document round-trip, unknown node/field rejection, dan validasi invariant.
