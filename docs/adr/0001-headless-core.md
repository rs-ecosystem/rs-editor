# ADR 0001: Core editor bersifat headless

- **Status:** diterima untuk fondasi awal
- **Tanggal:** 2026-10-11

## Konteks

Editor akan dipakai dari beberapa bahasa dan framework UI. Core yang bergantung pada Crux atau renderer tertentu akan membatasi adapter dan memperumit FFI.

## Keputusan

Model dokumen dan state editing berada di crate Rust tanpa dependency UI. Renderer menjadi crate terpisah dan mengonsumsi geometri atau display list. Adapter host mengelola event platform, lifecycle, surface, dan accessibility bridge.

## Konsekuensi

- Core dapat digunakan dari aplikasi Rust/Crux tanpa membuat Crux dependency.
- API lintas bahasa dapat dibangun di atas tipe inti yang tidak membawa object UI.
- Kontrak layout, font shaping, dan display list perlu divalidasi sebelum stabil.
- Aksesibilitas memerlukan semantic output dan integrasi khusus pada tiap platform.
