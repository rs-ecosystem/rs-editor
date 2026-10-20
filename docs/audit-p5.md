# Audit UX P5

> 2026-10-13. Audit fitur P5: menu blok `/`, popover tautan, bubble menu, drag handle, gambar, tempel tautan, placeholder, batas karakter, clipboard, dan lokalisasi. Ada empat audit kode paralel, ditambah uji langsung di Chrome dan di Android (Gboard). Tujuannya editor enak dipakai, dengan mouse maupun jari.

## Sudah diperbaiki

### Menu blok `/` (`179014a`)

- **Menu hanya terbuka untuk `/` yang baru diketik.** Sebelumnya, caret yang masuk ke teks seperti `/docs` ikut membuka menu, lalu Enter mengubah blok dan menghapus teks. Teks hasil tempel juga tidak lagi membuka menu.
- **Memilih baris menghapus seluruh kata kueri.** Sebelumnya, kalau caret ada di tengah kata, sisa kata (misalnya `ing`) tertinggal.
- **Kueri boleh berspasi** (`/judul 2`, `/bullet list`). Kueri tanpa hasil menampilkan "Tidak ada hasil". Setelah kueri berspasi tidak cocok dengan apa pun, `/` kembali menjadi teks biasa.
- **Tombol keyboard:**
  - Enter dan Tab bekerja normal kalau tidak ada yang dipilih.
  - Shift+Enter dan Shift+Tab tidak memilih baris.
  - Home, End, PageUp, dan PageDown memindah pilihan.
- **Pilihan keyboard tidak direbut mouse yang diam** di atas menu.
- **Ketukan menghentikan fling dokumen** dan memilih baris yang benar-benar ditekan.
- **Undo setelah memilih baris** berjalan sebagai langkah sendiri.
- **Semua baris muat,** dan tepi menu memudar kalau masih ada baris di atas atau di bawah.
- **Slash lebar `／` (IME Jepang/Cina) didukung.** Kode inline tidak membuka menu. Baris list dan kutipan disembunyikan kalau caret sudah berada di dalam list atau kutipan jenis itu.
- **Batas karakter tidak merusak teks lagi.** Komposisi IME yang ditolak tidak lagi menimpa teks pengguna, dan transaksi yang ditolak kini dilaporkan sebagai gagal.

### Blok, gambar, bubble menu (`78f06bb`)

- **Di layar sentuh, handle seleksi dan checkbox menang atas grip** dan popover tautan di dekatnya.
- **Mengetik setelah drop atau setelah klik grip tidak lagi mengganti seluruh blok.** Caret berada di akhir blok yang dipindah.
- **Setelah menyisipkan gambar, caret pindah ke bawahnya.** Mengetik tidak mengganti gambar, dan menyisipkan beberapa gambar tidak saling menimpa.
- **Salin/potong gambar mengisi clipboard** (teks alt atau alamatnya, plus HTML). Menu web dan Android juga menulis clipboard yang hanya berisi HTML.
- **Menempel alamat di blok kode** tidak lagi menghasilkan teks ganda.
- **Drag:**
  - Grip tidak lagi melompat ke seluruh list saat pointer berada di sela antar-item.
  - Posisi blok diperbarui setiap frame, jadi tetap tepat di dokumen panjang.
  - Kedalaman jatuh mengikuti gerak menyamping pointer.
  - Autoscroll berbasis waktu.
  - Wheel saat drag mencari tempat jatuh baru.
  - Item tunggal yang di-drop di tempatnya sendiri tidak mengubah apa pun.
  - Getaran baru muncul saat blok benar-benar terangkat.
- **Bubble menu:**
  - Muncul untuk select-all.
  - Tidak muncul saat memakai jari.
  - Esc menutupnya sampai seleksi berubah.

### Popover tautan, Android, web (`d7bc2ce`)

- **Kolom tautan:**
  - Cmd/Ctrl+V/C/X berfungsi.
  - Seleksi di dalam tautan mengedit seluruh tautan.
  - Rentang tautan mengikuti perubahan dokumen saat kolom terbuka.
  - Read-only menutup kolom.
  - Ring fokus hilang saat editor tidak fokus.
  - Kolom tetap terlihat saat dokumen di-scroll.
  - Jari yang hanya men-scroll tidak menutup kolom.
- **Popover di layar sentuh berada di bawah handle caret.** Lebarnya muat di layar sempit.
- **Tombol buka hanya membuka alamat aman** (http, https, mailto, tel, relatif). Tautan `javascript:` yang tersimpan tidak pernah dijalankan.
- **Android:**
  - Tombol Back menutup menu, popover, dan kolom tautan lebih dulu.
  - Ctrl+C/X/V di keyboard fisik berfungsi.
  - Foto kamera tidak lagi miring (EXIF).
- **Web:**
  - Backspace, Delete, dan Enter dari keyboard layar di mode textarea (Gboard di Firefox) tidak lagi hilang.
  - Menjatuhkan file tidak membuat halaman berpindah.
  - Kolom tautan mematikan autocorrect dan spellcheck.

### Tambahan (batch 4)

- **Gambar yang diketuk di layar sentuh** memunculkan menu Potong/Salin tanpa membuka keyboard.
- **Mod-Shift-Atas/Bawah** memindah blok atau item list yang berisi caret, seperti di Notion. Caret tetap berada di tempatnya di dalam blok.
- **Placeholder:**
  - Judul 4-6, item list ("Daftar"), dan item tugas ("Tugas") memakai nama bloknya.
  - Petunjuk "ketik /" hanya tampil saat menu `/` bisa dipakai. Saat read-only atau editor tidak fokus, yang tampil "Tulis sesuatu…".
- **Alamat berkurung** seperti `…/Rust_(bahasa)` tidak lagi terpotong.
- **Menu konteks web dan label screen reader** mengikuti opsi `locale` dan `setLocale()`.

### Batch 5

- **Blok yang di-drop di posisi teratas atau terbawah tetap terlihat.** View bergulir halus sampai blok itu tampil di atas keyboard (usulan pengguna). Hal yang sama berlaku untuk Mod-Shift-Atas/Bawah.
- **Batas karakter:**
  - Tempel yang melebihi batas dipotong sampai pas, seperti di Tiptap.
  - Edit yang ditolak memberi umpan balik: getar "tolak" di Android dan event `characterLimit` di web.
  - Ketikan yang ditolak kini dilaporkan sebagai gagal.
- **Paragraf kosong otomatis ditambahkan** setelah gambar, list, atau kutipan yang menutup dokumen (seperti TrailingNode di Tiptap), supaya selalu ada baris untuk diketik. Konten yang diset utuh lewat `setContent`, HTML, atau Markdown tidak diubah.
- **Upload gambar di web:**
  - Gambar langsung tampil dari file lokal sementara `uploadImage` menyimpannya.
  - Setelah selesai, alamatnya diganti tanpa memuat ulang, dan pergantian itu tidak menjadi langkah undo sendiri.
  - Kalau gagal, gambar dihapus lagi dan editor mengirim event `uploadError`.
- **Memuat gambar di web:**
  - Paling banyak tiga gambar dimuat bersamaan.
  - Ukuran piksel dibatasi sebesar lebar kolom.
- **Tempel dari Docs/Word** mempertahankan teks dan gambarnya walaupun clipboard juga berisi file.
- **Potong lewat menu web** baru menghapus isi setelah clipboard berhasil ditulis. Kalau gagal, isi tetap ada dan ponsel bergetar.
- **Klik kanan di web** tanpa seleksi memindah caret ke titik klik.
- **Android mengikuti perubahan bahasa sistem.**
- **`localhost:3000` dilengkapi `http://`,** dan `host:port` tidak lagi dianggap skema.
- **Ghost drag dibatasi tingginya 160 px.**

### Batch 6

- **Tujuan drop baru:**
  - Item tugas yang di-drop ke bullet list atau numbered list menjadi item biasa, dan sebaliknya.
  - Paragraf bisa di-drop di antara item list dan menjadi item list tersebut.
  - Item yang dikeluarkan ke samping list sejenis langsung bergabung dengan list itu, sehingga numbered list tetap melanjutkan nomornya. Undo mengembalikannya dalam satu langkah.
- **Screen reader bisa memindah blok:** grip punya aksi "Pindah ke atas" dan "Pindah ke bawah" (TalkBack, VoiceOver). Grip tetap menempel pada blok yang dipindah.
- **Kolom tautan di layar sentuh:**
  - Tekan lama memunculkan menu Tempel dan Pilih semua.
  - Ketuk ganda memilih seluruh alamat, lalu muncul juga Potong dan Salin.
- **Gambar yang sudah dihapus dilepas dari memori** saat gambar berikutnya selesai dimuat.
- **Gambar bisa di-drag di layar sentuh** (laporan pengguna):
  - Sebelumnya, grip dari gambar yang diketuk muncul untuk blok sesudahnya.
  - Tekan lama di gambar langsung mengangkatnya untuk di-drag, tanpa harus mencari grip.

### Batch 7

- **Tujuan drop lengkap:**
  - Digeser lebih ke kanan di bawah item yang belum punya sublist, blok menjadi item pertama sublist baru.
  - Blok bisa di-drop di antara blok di dalam kutipan.
- **Gambar yang selesai dimuat** hanya me-layout ulang blok yang memuatnya, bukan seluruh dokumen.
- **Gambar yang gagal dimuat** dicoba lagi saat diketuk atau diklik.
- **Popover tautan:**
  - Tombol Salin berubah menjadi centang ("Tersalin") selama 1,5 detik.
  - Tab dan Shift+Tab berpindah antara kolom dan tombol. Enter atau Spasi menekan tombol yang terfokus, dan mengetik kembali ke kolom.
  - Alamat yang ditolak disertai pesan "Pakai alamat web, email, atau telepon", juga untuk screen reader.
- **Bubble menu untuk seleksi beberapa baris** berada di atas baris pertama seleksi. Kalau baris pertama sudah ter-scroll keluar, menu pindah ke bawah baris terakhir.
- **Padding kiri bawaan web dan API menjadi 28 px**, sama dengan Android, supaya grip tidak menimpa teks.

### Batch 8

- **Kolom tautan punya magnifier:** tahan jari di kolom, lalu geser untuk memindah caret di bawah magnifier. Saat dilepas, menu kolom muncul.
- **Web: opsi `loadImage`.** Aplikasi bisa memuat gambar lewat proxy atau `Blob` sendiri, untuk gambar dari server tanpa header CORS.
- **Android: GIF dan stiker dari Gboard (`commitContent`)** disisipkan sebagai gambar (frame pertama). Defaultnya disalin ke cache aplikasi; `onKeyboardImage` bisa dipakai untuk mengunggahnya.
- **Drag gambar dengan mouse di web** sudah dicek langsung di Chrome.

### Laporan pengguna sebelumnya

- **Menu `/`:** bisa di-scroll, sorotan menempel pada opsi yang ditekan, dan tidak kembali ke opsi pertama (`42452a9`, `1e80343`).
- **Drag:**
  - Ada ghost dan garis jatuh yang bergerak halus.
  - Tempat jatuh lebih stabil.
  - Grip lebih besar untuk jari, tanpa latar sampai disentuh atau ditarik.
  - Drag bisa dibatalkan dengan kembali ke tempat asal.
  - Grip ikut ghost dan menempel pada blok yang dipindah (`ea73538`, `783b227`).
- **Salin/potong di web** tidak lagi kosong (`00feff1`). Handle seleksi bisa saling melewati seperti di Android (`569d50b`).

## Belum (urut prioritas)

- **SVG di Android.** BitmapFactory tidak bisa membacanya. Pilihannya: modul SVG Skia (fitur `svg` skia-safe, tapi binary prebuilt-nya harus diunduh ulang), atau pustaka SVG di sisi Kotlin.
- **GIF beranimasi** tampil diam (frame pertama) di semua platform.
- **Grip bisa menimpa teks** kalau host memakai padding kiri di bawah 26 px.
