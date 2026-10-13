# Performa demo Rs Editor

## Jalur interaksi

Frame Skia hanya dibuat saat dokumen, format, atau viewport berubah. Pixel teks dan geometri hit testing berasal dari satu layout. Browser menyimpan bounds grapheme dan caret stops, sehingga klik, drag, pemilihan kata/paragraf, serta panah/Shift+panah tidak memerlukan HTTP, shaping, encoding, atau penggantian DOM dokumen.

Caret dan highlight digambar pada canvas overlay dengan `requestAnimationFrame`; gerakan pointer dalam frame yang sama digabung menjadi satu paint. Seleksi terbaru dikirim bersama command yang mengubah dokumen. Respons command lama tidak boleh mengembalikan seleksi ke posisi input sebelumnya. IME begin membawa seleksi lokal terbaru ke core.

Host Rust memakai empat worker persisten dan antrean terbatas, dengan font manager tersimpan per thread. Dokumen disalin di bawah mutex lalu shaping/raster dilakukan setelah lock dilepas. Scene sampai 8 MiB memakai RGBA langsung dengan byte order browser; scene besar memakai PNG lossless dengan kompresi ringan. Request scene lama dibatalkan, dan identitas dokumen/viewport diperiksa sebelum frame diterapkan.

## Membaca panel

| Indikator | Arti |
| --- | --- |
| UI FPS | Frekuensi callback rAF browser, diperbarui tiap 500 ms. Bukan FPS Skia. |
| Skia FPS redraw | Jumlah frame Skia baru yang diterima dan ditampilkan dalam satu detik terakhir. Nol ketika diam atau hanya memindahkan caret adalah perilaku yang diharapkan. |
| Raster ms | Waktu surface allocation/clear/drawing software Skia terakhir di Rust. |
| Scene ms | Layout, geometri, raster, dan persiapan transfer terakhir di server; tidak termasuk jaringan, parsing JSON, atau upload pixel di browser. Tooltip memberikan rinciannya. |
| Input→draw p95 | Persentil ke-95 dari hingga 200 sampel terakhir, dari input lokal sampai drawing overlay dalam rAF. Bukan pengukuran fisik layar. |

Scene pertama dapat lebih lambat karena inisialisasi font/cache. Mengetik satu karakter hanya menghasilkan satu perubahan; angka Skia FPS mengikuti jumlah redraw, sehingga nilainya tidak menunjukkan kapasitas maksimum renderer. Demo ini memakai raster software, bukan benchmark GPU Skia.

## Bukti lokal 2026-10-11

Pada macOS ARM64 baseline, debug build, fixture tiga blok termasuk emoji/RTL, viewport 960 logical px dan skala 2×:

- Delapan frame setelah warmup: scene server sekitar 4,8–5,4 ms; raster 0,8–1,0 ms. Round-trip lokal sekitar 9,8–12,6 ms setelah request pertama.
- Drag dan 40 penekanan arrow/Shift+arrow: tidak ada request HTTP tambahan untuk aksi atau scene. UI sekitar 60 FPS; input→draw P95 sekitar 7 ms pada rangkaian uji tersebut.
- Delapan tes interaksi lokal dan satu tes parity geometri native lulus. Nilai berubah mengikuti dokumen, font, beban CPU, viewport, refresh rate, dan host.

## Mengulang pemeriksaan

Jalankan demo lalu perhatikan panel saat drag, menahan panah, mengetik, dan resize. Saat navigasi saja, Skia FPS tetap nol dan UI FPS mengikuti refresh rate browser.

Tes interaksi murni dapat dijalankan dengan:

```sh
node --test crates/rs-editor-demo/tests/interaction.test.mjs
```

Untuk tes payload/geometri asli, jalankan host terpisah agar seleksi sesi pengguna tidak terganggu:

```sh
cargo run -p rs-editor-demo -- 127.0.0.1:4174
```

Di terminal lain:

```sh
RS_EDITOR_DEMO_URL=http://127.0.0.1:4174 node --test crates/rs-editor-demo/tests/scene.test.mjs
```

Tes membandingkan hit lokal dengan Skia pada tiga skala layar dan memulihkan seleksi awal setelah selesai. Browser HUD juga mengekspos jumlah request dan sampel melalui atribut `data-*` pada `.performance-hud` untuk pemeriksaan pengembang.
