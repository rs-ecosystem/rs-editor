# ADR 0005: Identitas node dan schema dokumen v2

- Status: digantikan oleh [ADR 0007](0007-openrich-v2-architecture.md)
- Tanggal: 2026-10-11

## Konteks

Schema v1 mengidentifikasi blok hanya berdasarkan posisi. Posisi berubah saat blok disisipkan atau dipindahkan, sehingga tidak cukup untuk mempertahankan referensi dari adapter, layout, selection, maupun fitur kolaborasi di masa depan.

## Keputusan

Schema v2 memberi setiap block dan list item `NodeId` berupa string desimal. ID dialokasikan monotonik dari counter 64-bit per proses dan harus unik di dalam satu dokumen; validasi dokumen juga mengamati ID yang sudah dimuat agar alokasi berikutnya tidak bertabrakan. Format string menjaga ID tetap utuh di JSON pada runtime seperti JavaScript yang tidak dapat merepresentasikan semua integer 64-bit. Editor mempertahankan ID blok yang ada ketika mengubah konten, split mempertahankan ID pada sisi kiri dan membuat ID baru untuk sisi kanan, sedangkan join mempertahankan ID blok kiri.

`Document::from_json` membaca v1 dan mengubahnya ke v2 dengan membuat ID untuk setiap node. Penulisan selalu menghasilkan v2. Versi schema yang tidak didukung serta node dan field asing ditolak secara eksplisit.

## Konsekuensi

- Adapter dapat menyimpan referensi node yang tidak bergantung pada indeks blok.
- Aplikasi yang membaca v1 dapat melanjutkan memakai data tersebut tanpa migrasi eksternal.
- Identitas hanya dijamin unik dalam satu dokumen. Penggabungan dokumen dan identitas lintas peer memerlukan rancangan allocator/namespace tersendiri.
- Schema v2 mengubah bentuk JSON; pembaca v1 tidak dapat membaca output v2.
