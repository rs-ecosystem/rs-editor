# @rs-editor/web

Rs Editor di halaman web. Seluruh editor (layout, render Skia di WebGL2, editing, seleksi dan gesture sentuh) berjalan di WebAssembly dari crate Rust yang sama dengan Android dan desktop. Paket ini hanya meneruskan event browser, menyambungkan input teks (EditContext, atau textarea tersembunyi di Safari dan Firefox), dan menampilkan yang diminta Rust: menu seleksi, magnifier, getar, dan keyboard layar.

## Build

Prasyarat: emsdk 6 (`~/emsdk`) dan target Rust `wasm32-unknown-emscripten`.

```bash
crates/rs-editor-web/build.sh
```

Hasilnya `dist/rs-editor-web.js` dan `dist/rs_editor_web.wasm`, yang di-ignore oleh git. Demo ada di `demo/index.html`. Demo ini memuat subset CJK dari CDN secara default; gunakan `?fonts=none` untuk tanpa font tambahan, atau `?fonts=cdn` untuk ikut memuat font emoji. Sajikan folder ini lewat HTTP, misalnya dengan `python3 -m http.server 4174 --directory platforms/web`, lalu buka `/demo/`.

## API

API-nya mengikuti Tiptap:

```js
import { createEditor } from "@rs-editor/web";

const editor = await createEditor({ element, content /* JSON Tiptap atau HTML */ });

editor.chain().focus().toggleBold().toggleHeading({ level: 2 }).run(); // satu langkah undo
editor.can().toggleBulletList().run();     // apakah bisa? tidak mengubah apa pun
editor.commands.undo();
editor.isActive("heading", { level: 2 });
editor.getAttributes("link").href;
editor.getJSON(); editor.getHTML(); editor.getText(); editor.getMarkdown();
editor.setContent("<p>halo</p>");
editor.setContent("# Judul", { contentType: "markdown" });
editor.on("update", ({ editor }) => save(editor.getJSON()));
editor.on("selectionUpdate", refreshToolbar);
editor.destroy();
```

Semua perintah dari extension Rust bisa dipakai sebagai metode chain, dengan argumen yang sama seperti di Tiptap. Tipe TypeScript-nya ada di `src/index.d.ts`.

Saat teks dipilih dengan mouse atau trackpad, editor menampilkan bubble menu (tebal, miring, garis bawah, coret, kode, sorotan) yang digambar di canvas. Matikan dengan `bubbleMenu: false` kalau aplikasi punya menu sendiri. Di HP, seleksi tetap memakai menu Potong/Salin/Tempel.

## Emoji dan font

Emoji tampil tanpa mengunduh apa pun. Browser menggambar setiap emoji dengan font emoji sistem (Apple di Safari, Segoe di Windows, Noto di Android) ke canvas 2D, lalu editor menempelkannya sebagai gambar di tempat emoji itu, dengan ukuran sekitar 1,2 em. Tampilannya sama dengan emoji di aplikasi lain pada perangkat yang sama, sehingga bisa berbeda antar platform. Caret, seleksi, dan hapus tetap memperlakukan satu emoji (termasuk ZWJ, bendera, dan warna kulit) sebagai satu karakter.

Font bawaan mencakup aksara Latin, Arab, dan Devanagari. Untuk CJK, aplikasi memberi daftar `fonts`. Sebuah font hanya dimuat ketika teks berisi karakter dalam rentang Unicode-nya. Preset `notoFonts.japanese`, `korean`, dan `simplifiedChinese` mengambil Noto dari paket fontsource di jsDelivr dalam **subset per rentang Unicode** (8–60 KB per file). Hanya subset yang memuat karakter di dokumen yang diunduh, dan biasanya totalnya ratusan KB, bukan 5–8 MB untuk font utuh. Ini cara yang sama dengan Flutter Web, yang juga merender teks dengan Skia di wasm. Untuk Han, pilih salah satu font Jepang atau Cina saja, karena keduanya menggambar Han dengan gaya bahasanya masing-masing.

```js
import { createEditor, notoFonts } from "@rs-editor/web";

await createEditor({ element, fonts: [notoFonts.japanese, notoFonts.korean] });
// atau muat sendiri:
editor.addFont(await (await fetch("/fonts/MyFont.woff")).arrayBuffer(), "My Font");
```

Format yang didukung adalah TTF, OTF, dan WOFF (bukan WOFF2). Kalau daftar `fonts` memuat font emoji (misalnya `notoFonts.emoji`, 7 MB), emoji digambar dengan font itu, bukan oleh browser. Untuk produksi, font sebaiknya di-host sendiri.

## Markdown

`getMarkdown()` menghasilkan Markdown gaya GitHub. Mark yang tidak punya sintaks Markdown (garis bawah, sorotan, sub/superskrip) ditulis sebagai tag HTML. `setContent(md, { contentType: "markdown" })` dan opsi `contentType: "markdown"` membaca Markdown. Opsi `pasteMarkdown: true` membuat teks polos yang ditempel dibaca sebagai Markdown.

## Sentuh

- Di HP, gesture jari dikenali di Rust, sama seperti di Android:
  - ketuk menaruh caret, ketuk dua kali memilih kata;
  - tekan lama memilih kata, lalu handle bisa ditarik untuk memperluas seleksi;
  - geser untuk scroll dan fling.
- Menu Potong/Salin/Tempel adalah elemen DOM bergaya shadcn/ui.
- Magnifier adalah salinan frame canvas yang diperbesar.
- Getar memakai `navigator.vibrate` (browser Android).
- Keyboard layar muncul saat teks diketuk.
- Jika keyboard menutupi halaman tanpa mengubah ukurannya (Safari iOS), editor membaca `visualViewport` dan menggeser caret supaya tidak tertutup keyboard.
- Dengan mouse, klik kanan membuka menu yang sama di posisi kursor.

## Aksesibilitas

Canvas tidak punya teks yang bisa dibaca screen reader. Karena itu paket ini membuat DOM tak terlihat yang ditumpuk di atas canvas, dari pohon aksesibilitas yang sama dengan yang dipakai screen reader native (lewat AccessKit) di Android dan desktop. Isinya judul, list, checkbox yang bisa diaktifkan, dan teks per baris di posisi aslinya. Navigasi caret dan seleksi dengan keyboard diumumkan lewat region `aria-live`. Matikan dengan `accessibility: false` kalau aplikasi menyediakan aksesibilitasnya sendiri.

Getar di Android: Chrome tidak meneruskan `navigator.vibrate` saat HP dalam mode senyap, dan Safari iOS tidak mendukung getar sama sekali.
