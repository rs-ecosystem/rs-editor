# Demo interaktif OpenRich

Demo ini menjalankan core editor dan renderer Skia di proses Rust lokal, lalu menyediakan host browser yang dapat dipakai dengan mouse, keyboard, clipboard, dan IME sistem.

## Menjalankan

Prasyarat: Rust stable sesuai `rust-version` workspace, Ninja, dan toolchain native Skia. Pada mesin macOS ARM64 yang menjadi baseline, jalankan:

```sh
cargo run -p openrich-demo
```

Buka [http://127.0.0.1:4173](http://127.0.0.1:4173). Untuk port lain, berikan alamat sebagai argumen, misalnya `cargo run -p openrich-demo -- 127.0.0.1:4174`. Server hanya bind ke alamat yang dipilih; default-nya loopback lokal.

Untuk membuka playground di jendela macOS native (WKWebView, termasuk input method dan accessibility WebKit), jalankan `./scripts/run-macos-demo.sh`. Launcher membangun server Rust, membuka jendela OpenRich, dan menghentikan server saat jendela ditutup. Tutup server/browser lama sebelum menjalankannya karena kedua mode memakai port 4173.

## Yang dapat dicoba

1. Ketik pada judul atau paragraf; tekan Enter untuk memisahkan paragraf.
2. Pilih teks dengan mouse atau keyboard, lalu gunakan tombol format atau shortcut Bold/Italic/Underline.
3. Coba undo dan redo, backspace/delete, copy, cut, dan paste teks biasa.
4. Gunakan IME OS untuk mengetik aksara Asia. Preedit dikelola browser, lalu dikirim ke core sebagai satu commit dan satu undo.
5. Uji emoji, combining marks, aksara kanan-ke-kiri, dan resize jendela.
6. Gunakan **Simpan JSON** untuk mengunduh dokumen, lalu **Buka JSON** untuk memuatnya kembali. Schema v1 akan dimigrasikan saat dibuka.

Tampilan visual berasal dari `openrich-render-skia`; lapisan DOM transparan menyajikan semantic HTML dan menerima input/accessibility browser. Layout Skia di-render menjadi frame RGBA software pada server lokal (fallback PNG untuk payload raster di atas 8 MiB). Frame teks dan geometri Skia dikirim bersamaan; browser menyimpan geometri untuk hit testing, navigasi panah, serta overlay caret/selection tanpa request server pada tiap gerakan. Tidak ada dokumen yang dikirim ke layanan eksternal.

## Indikator performa

Panel di bawah editor menunjukkan UI FPS (frekuensi callback `requestAnimationFrame` browser), Skia FPS (frame raster baru yang diterima/ditampilkan per detik), waktu raster Skia terakhir, total waktu scene, dan P95 input→draw overlay. Arahkan pointer ke nilai scene untuk rincian layout, geometri, raster, serta persiapan transfer.

Skia merender saat isi, format, atau ukuran berubah. Nilai **Skia 0 FPS saat diam atau hanya memindahkan caret adalah normal**. UI tetap merespons melalui overlay lokal. Angka FPS ini bukan benchmark maksimum backend GPU Skia; demo memakai raster software. Input→draw mengukur hingga drawing dalam callback rAF, bukan latensi fisik layar. Lihat [catatan performa](performance.md) untuk hasil dan cara mengulang uji.

## Acceptance checklist

Daftar ini untuk uji manual maintainer/pengguna; checklist belum menyatakan seluruh OS/browser sudah tersertifikasi.

- [ ] Ketik teks Latin dan emoji, lalu pastikan grapheme tidak terbelah saat backspace.
- [ ] Seleksi teks, format tebal/miring/garis bawah, lalu verifikasi tampilan Skia dan ulangi dengan undo/redo.
- [ ] Pisahkan dan gabungkan paragraf dengan Enter dan backspace di awal paragraf.
- [ ] Copy/cut/paste teks biasa dan pilih teks lintas paragraf sebelum menggantinya.
- [ ] Mulai komposisi IME, ubah preedit beberapa kali, commit, lalu satu undo menghapus seluruh komposisi.
- [ ] Simpan dokumen sebagai JSON, muat kembali file itu, dan pastikan isi serta format tetap sama.
- [ ] Buka halaman dengan VoiceOver atau screen reader browser; pastikan heading dan paragraf terbaca, editor mendapat nama aksesibel, dan tombol format memiliki label.
- [ ] Resize browser dan gunakan zoom/DPI layar; pastikan frame Skia menyesuaikan ukuran.

## Batas demo saat ini

Demo menyediakan playground browser dan app shell macOS native berbasis WKWebView; keduanya memakai binary Rust lokal yang sama. Editing interaktif difokuskan pada paragraf dan heading tingkat atas; list/blockquote dapat dilayout tetapi belum memiliki selection/editing pada child node. Screen reader dan IME telah diberi semantic/browser hooks, tetapi checklist manual per kombinasi OS/browser masih harus dilengkapi sebelum klaim dukungan aksesibilitas lintas platform.

### Format paragraf dan touch

Toolbar paragraf menyediakan daftar bernomor, bullet, checklist, dan perataan kiri/tengah/kanan/justify. Format disimpan pada node `formatted` beserta konten inline dan ID stabil, serta mengikuti undo/redo. Enter melanjutkan daftar; Enter pada item kosong mengakhiri daftar. Kotak checklist dapat diklik atau diubah lewat tombol centang. Perataan mengikuti Skia Paragraph, termasuk geometri seleksi.

Di area editor, satu ketukan memindahkan caret, dua ketukan memilih kata, dan tekan-tahan 450 ms lalu drag memperluas seleksi. Geser biasa menggulir halaman. Dua pegangan seleksi dapat diseret untuk mengubah batasnya. Input IME direkonsiliasi dengan snapshot teks agar perpindahan seleksi tidak menyisipkan ulang kata. Validasi pada perangkat touch fisik tetap diperlukan.
