# Audit input mobile — 11 Oktober 2026

## Bukti dan ruang lingkup

Audit mencakup rekaman Android berdurasi 25 detik yang diberikan pengguna, kode adapter browser, editor Rust, renderer Skia, tes unit, tes API pada server terpisah, dan pemeriksaan UI desktop melalui browser. Audit ini **bukan** sertifikasi bahwa semua keyboard/perangkat mobile bebas bug. Keyboard Android pengguna tidak dikendalikan langsung dari lingkungan ini.

## Temuan dan perbaikan

| Kasus | Penyebab | Perubahan | Bukti verifikasi |
|---|---|---|---|
| Bold hilang pada ketikan IME | Rekonversi keyboard mengganti rentang teks lewat `set_selection`, yang membuang override format | Komposisi menangkap mark; `composition_replace` menerapkan rentang dan mark secara atomik | Tes Rust dan API bold on/off, reconversion, history |
| Format hilang tanpa perpindahan caret | Perubahan affinity dianggap perubahan posisi logis | Affinity saja mempertahankan active marks dan composition | Tes Rust |
| Bold tampak hilang selama preedit | Host teks native baru tidak mewarisi gaya renderer | Gaya host insertion diambil dari active marks/heading dan diperbarui saat native input | Tes kebijakan gaya; perlu validasi fisik keyboard |
| Highlight biru hilang selama IME | CSS dan painter sengaja menonaktifkan overlay selama composition | Skia menggambar teks dan highlight selama preedit maupun committed; perubahan native selection diteruskan ke overlay; pegangan custom disembunyikan ketika native selection aktif | Pemeriksaan kode, tes geometri; perlu validasi fisik |
| Highlight berkedip saat scene baru diminta | Geometri dibuang sebelum pengganti datang | Pertahankan frame/geometri sebelumnya dan seleksi yang cocok dengannya | Pemeriksaan UI dan tes scene |
| Klik/panah memakai layout dokumen lama | Adanya objek layout dianggap cukup walaupun dokumen berubah | Input geometri menunggu scene dengan identitas dokumen yang sesuai | Tes kebijakan identitas scene, tes API |
| Seleksi berubah oleh balasan request lama | Preservasi seleksi membaca ulang DOM yang dapat dikolapskan keyboard | Balasan lama mempertahankan seleksi lokal; event selectionchange programatik diberi guard | Pemeriksaan kode dan UI seleksi |
| Kata terduplikasi oleh reconversion | `compositionend.data` berisi seluruh kata sekitar caret | Hitung diff grapheme native terhadap snapshot; perubahan seleksi tanpa perubahan teks tidak mengedit dokumen | Tes JS dan API, emoji/RTL/no-op/duplicate commit |
| Karakter berulang sah dapat tertelan | Suppression trailing event memakai jendela waktu 100 ms | Suppression berakhir pada event turn berikutnya | Pemeriksaan adapter; perlu validasi urutan event fisik |
| Double tap pada ujung kata gagal | Posisi hit test bisa tepat di akhir teks atau batas kata | Pemilihan kata mempertimbangkan affinity dan batas akhir | Tes JS |
| Toolbar/Save digunakan ketika IME belum selesai | Komposisi belum committed saat command/save berjalan | Toolbar, undo/redo dan save menunggu compositionend serta antrean commit | Pemeriksaan adapter; perlu validasi blur Gboard/Samsung/iOS |
| Tombol undo/redo mengambil fokus pada touch | Hanya mousedown yang dicegah | Pointerdown menjaga seleksi | Pemeriksaan adapter |
| Dua perangkat mengedit state yang berbeda | Demo membagikan satu dokumen tanpa pengecekan basis edit | Request membawa expected_document; konflik 409 menyinkronkan ulang dan membatalkan antrean lama | Tes API konflik dan dokumen tidak termutasi |
| Checkbox/teks rata tengah memiliki geometri berbeda | Risiko ketidaksamaan marker, alignment dan DPI | Checklist vector, area tap terpisah, tes hit/caret yang sama dengan teks | Tes Rust DPI 1/2/3 |
| Heading format menerima level tidak valid | Format baru belum diperiksa validator | Level heading pada formatted block divalidasi | Pemeriksaan validator; seluruh tes dokumen lulus |

## Hasil otomatis

- 37 tes unit Rust lulus di seluruh workspace.
- 21 tes JavaScript kebijakan input dan geometri lulus.
- 2 tes integrasi terpisah lulus: rangkaian API mobile dan kecocokan scene/hit test pada DPI 1/2/3.
- Format Rust, workspace check semua target, build demo, dan pemeriksaan sintaks modul browser lulus.
- Pemeriksaan UI browser terpisah: aktifkan bold pada caret, ketik ` TEST`, perluas seleksi empat karakter; JSON mempertahankan mark bold dan screenshot memperlihatkan highlight biru.

Tes API tidak mensimulasikan event nyata keyboard Android. Tes kebijakan gesture tidak menggantikan pengujian gesture pada perangkat fisik.

## Kasus yang wajib diverifikasi pada HP

1. Toggle bold on/off pada paragraf kosong lalu ketik beberapa kata tanpa menutup keyboard.
2. Pilih sebagian kata bold, ganti dengan kata lain, gunakan saran/autocorrect keyboard.
3. Tekan-tahan 450 ms, drag, lepaskan, ubah kedua pegangan; highlight harus bertahan.
4. Pilih teks saat kata sebelumnya masih preedit, gunakan toolbar, kemudian lanjut mengetik.
5. Ketik huruf yang sama berulang dengan cepat setelah compositionend.
6. Emoji ZWJ, aksen gabungan, RTL, hapus/backspace, undo dan redo.
7. Simpan saat kata masih preedit; JSON harus berisi kata terakhir.
8. Checklist: centang/uncentang, Enter melanjutkan item unchecked, Enter pada item kosong mengakhiri list.
9. Putar orientasi/resize saat seleksi dan keyboard aktif.
10. Buka dua perangkat; setelah satu mengubah dokumen, edit dari perangkat lama harus ditolak dan disinkronkan ulang.

## Batas yang masih diketahui

- Preedit menggunakan DOM native; teks committed menggunakan Skia. Metrik font/fallback dan dekorasi preedit masih dapat berbeda, terutama mixed marks, RTL dan wrapping. Penyatuan layout preedit dengan engine perlu milestone tersendiri.
- Komposisi yang mengubah beberapa paragraf sekaligus ditolak dan dikembalikan ke dokumen canonical, bukan ditebak menjadi sejumlah edit terpisah.
- Antrean ketikan cepat yang berpindah dari beforeinput biasa ke IME, native context menu/selection handles, perubahan orientasi, dan keyboard yang tidak mengirim compositionend saat blur masih memerlukan validasi perangkat nyata.
- Server tetap demo dokumen bersama, bukan sistem kolaborasi, pemulihan offline atau penyimpanan durable. Konflik dicegah; tidak ada merge otomatis.
- Audit tidak menghapus duplikasi yang sudah tersimpan sebelum perbaikan.

## Reproduksi tes

Jalankan server audit tersendiri; jangan memakai port demo pengguna untuk tes yang memuat fixture.

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo test --workspace --locked --offline
cargo build -p openrich-demo --locked --offline
node --input-type=module --check < crates/openrich-demo/web/app.js
node --test crates/openrich-demo/tests/input.test.mjs crates/openrich-demo/tests/interaction.test.mjs
target/debug/openrich-demo 127.0.0.1:4174
# Terminal lain, secara berurutan:
OPENRICH_DEMO_URL=http://127.0.0.1:4174 node --test crates/openrich-demo/tests/mobile-api.test.mjs
OPENRICH_DEMO_URL=http://127.0.0.1:4174 node --test crates/openrich-demo/tests/scene.test.mjs
```

## Koreksi setelah laporan perangkat

Perbaikan overlay sebelumnya menimbulkan highlight ganda native Android dan Skia. Komposisi sekarang hanya menampilkan seleksi native. Penggantian rentang bold kini mewarisi mark karakter pertama yang dipilih, bukan karakter sebelum seleksi; tes mencakup kedua arah seleksi, awal paragraf, dan komposisi. Gaya host preedit diterapkan dengan prioritas agar span yang dibuat keyboard tidak menghapus bold. Validasi Android fisik masih diperlukan.

### Refresh gaya selama keyboard terbuka

MutationObserver untuk perubahan teks/child node dan refresh sebelum frame berikutnya memperbarui gaya insertion host setelah IME mengganti node. Observer tidak mengamati atribut sehingga penulisan style tidak memicu loop. Mark selama preedit tetap memakai snapshot awal komposisi, bukan balasan state lama. Font synthesis diizinkan khusus preedit untuk fallback Android. Perubahan ini belum diverifikasi pada HP fisik.

## Investigasi langsung ADB / Chrome Android

- Perangkat POCO model 23049PCD8G tersambung lewat wireless ADB; keyboard `com.preff.kb.xm/com.preff.kb.LatinIME`.
- Screenshot nyata menangkap perbedaan tampilan seluruh dokumen ketika composition aktif, highlight hilang dengan native handles, dan native handles bertumpuk dengan custom handles.
- Perintah sentuh shell ditolak perangkat (`INJECT_EVENTS`) bahkan setelah reconnect. Uji komposisi dilakukan melalui Chrome DevTools Protocol `Input.imeSetComposition` di browser HP; ini tidak setara dengan seluruh urutan event keyboard fisik.
- Skia kini menerima preview dokumen lewat POST `/api/preview-scene`; tidak memutasi dokumen/history editor. Snapshot dokumen dan mark digunakan untuk menyusun preview grapheme. DOM IME tidak diganti selama komposisi.
- Preview memakai fingerprint dokumen yang tidak bergantung urutan field JSON. Tes menangkap perbedaan urutan field dari JS dan Rust.
- Canvas menutupi teks serta background preedit native; overlay Skia berada di atas canvas. Ini menggantikan kebijakan native-only pada percobaan sebelumnya.
- Selectionchange yang tertahan guard dijadwalkan ulang; native selection/context menu menonaktifkan pegangan custom.
- Screenshot setelah koreksi menunjukkan `TestIME`/`BoldIME` terlihat tebal ketika status masih “Menulis dengan IME…”. Hasil ketikan keyboard nyata setelah koreksi masih menunggu validasi.

### Koreksi setelah uji keyboard nyata

Pengguna mengonfirmasi masalah bold tidak tampak lagi, tetapi melaporkan delay typing, seleksi hilang, dan keyboard tertutup saat menyentuh teks. Screenshot dan trace Chrome menunjukkan native range berisi `test`, sementara overlay masih memakai caret lokal. Painter kini membaca native range langsung ketika native selection aktif. Sentuhan saat composing tidak lagi memanggil blur. Event selection yang tertahan guard dijadwalkan ulang.

Transfer LAN sekarang PNG (sekitar 40 KB pada dokumen/perangkat uji, dibanding RGBA sekitar 2 MB); pengukuran satu load HP 117 ms termasuk render/transfer, bukan klaim 60 FPS. Preedit request digabung saat satu frame masih berjalan, frame lengkap dalam komposisi yang sama boleh tampil sebelum versi terbaru datang. Event compositionupdate tidak lagi menambah antrean API state untuk setiap karakter. Tes endpoint memastikan PNG kurang dari 100 KB untuk fixture.

Screenshot pada HP memperlihatkan highlight `test` setelah koreksi. Validasi ketikan/sentuhan keyboard nyata pada versi terakhir masih menunggu pengguna; jangan menyatakan semua masalah telah selesai.

### Keselarasan pegangan native — 12 Oktober 2026

Pengguna mengonfirmasi bold, ketikan, fokus keyboard dan highlight sudah aman pada versi dengan PNG/coalescing; laporan tersisa adalah indikator kanan tidak akurat. Screenshot `account` dan Range.getClientRects memperlihatkan font fallback Android berbeda dari Arial macOS.

Renderer dan adapter kini memakai byte font Liberation Sans/Mono 2.1.5 yang sama (regular/bold/italic/bold italic), dibundel dengan lisensi dan hash di assets/fonts. Paragraph Skia memakai ukuran fractional tanpa hinting/rounding hack. Browser menunggu font selesai dimuat sebelum editor aktif. System fallback tetap diperlukan untuk glyph yang tidak tercakup; keselarasan emoji/RTL/fallback bukan bagian dari bukti Latin ini.

Pengukuran `account` di HP: native right 129.210227 CSS px, Skia right 129.217175 CSS px, selisih -0.006948 px; selisih bottom -0.363634 px. Tes `android-font.test.mjs` berjalan langsung melalui Chrome CDP pada HP dan memeriksa endpoint kata Latin tanpa mengubah DOM/dokumen/seleksi. Batas horizontal <0.5 CSS px, vertikal <1.25 CSS px. Tes lulus.

```sh
adb forward tcp:9222 localabstract:chrome_devtools_remote
OPENRICH_ANDROID_CDP=http://127.0.0.1:9222 OPENRICH_ANDROID_URL=http://192.168.100.20:4173/ node --test crates/openrich-demo/tests/android-font.test.mjs
```

Verifikasi akhir: 37 tes Rust, 21 tes kebijakan JavaScript, 2 tes integrasi API/scene, dan 1 tes geometri browser Android lulus. Pemeriksaan manual dragging pegangan setelah font berubah tetap perlu dikonfirmasi pengguna; endpoint geometri sudah diverifikasi di HP.

### Caret kembali ke kata sebelumnya

Kasus pengguna: dari `Selamat` offset 3 pindah ke `datang` offset 10, tetapi caret kembali. Urutan adapter dapat direproduksi di Chrome HP: compositionstart, perubahan caret native, compositionend tanpa perubahan teks. Handler lama mengganti DOM lalu memulihkan selection awal; selectionchange selama mutationPending juga dibuang.

Perbaikan mempertahankan final native selection, tidak mengganti DOM pada no-op composition, mempertahankan selection saat balasan cancel/replace tiba, dan menerima perpindahan native saat mutation request berjalan. Touch baru mereset guard seleksi programatik.

Tes `android-caret.test.mjs` memakai tab dan dokumen uji terpisah pada HP. Event komposisi disusun secara deterministik (bukan klaim mengetik langsung melalui keyboard), request action ditunda 80 ms. Caret no-op tetap offset 10 sebelum/sesudah balasan; caret setelah commit dan perpindahan berikutnya tetap offset 11. Tes lulus; 21 tes JS dan 2 integrasi API/scene juga lulus. Validasi ulang gesture keyboard nyata oleh pengguna masih diperlukan.
