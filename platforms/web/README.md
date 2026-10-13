# @rs-editor/web

Rs Editor di halaman web. Seluruh editor (layout, render Skia di WebGL2, editing, seleksi dan gesture sentuh) berjalan di WebAssembly dari crate Rust yang sama dengan Android dan desktop. Paket ini hanya meneruskan event browser, menyambungkan input teks (EditContext, atau textarea tersembunyi di Safari dan Firefox), dan menampilkan yang diminta Rust: menu seleksi, magnifier, getar, dan keyboard layar.

## Build

Prasyarat: emsdk 6 (`~/emsdk`) dan target Rust `wasm32-unknown-emscripten`.

```bash
crates/rs-editor-web/build.sh
```

Hasilnya `dist/rs-editor-web.js` dan `dist/rs_editor_web.wasm`, yang di-ignore oleh git. Demo ada di `demo/index.html`. Sajikan folder ini lewat HTTP, misalnya dengan `python3 -m http.server 4174 --directory platforms/web`, lalu buka `/demo/`.

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
editor.getJSON(); editor.getHTML(); editor.getText();
editor.setContent("<p>halo</p>");
editor.on("update", ({ editor }) => save(editor.getJSON()));
editor.on("selectionUpdate", refreshToolbar);
editor.destroy();
```

Semua perintah dari extension Rust bisa dipakai sebagai metode chain, dengan argumen yang sama seperti di Tiptap. Tipe TypeScript-nya ada di `src/index.d.ts`.

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
