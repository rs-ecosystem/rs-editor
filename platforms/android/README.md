# Rs Editor untuk Android

Rs Editor sebagai view Android, dengan tambahan wrapper Compose. Inti Rust-nya (`crates/rs-editor-android`) dipanggil lewat binding Kotlin yang dibuat oleh BoltFFI.

| Modul | Isi |
| --- | --- |
| `:rs-editor` | `RsEditorView` (SurfaceView + Skia), input method, menu seleksi, clipboard, TalkBack |
| `:rs-editor-compose` | `RsEditor` dan `rememberRsEditorState` |
| `:sample` | Aplikasi contoh: editor dengan toolbar format |

## Build

Prasyarat: NDK 27, JDK 17, `boltffi` CLI 0.29.3 (`cargo install boltffi_cli --version 0.29.3`), dan target Rust `aarch64-linux-android` (tambah `x86_64-linux-android` untuk emulator).

```bash
platforms/android/package-native.py --arch arm64
```

```bash
cd platforms/android && ./gradlew :sample:installDebug
```

`package-native.py` menulis ke `rs-editor/generated/`, yang di-ignore oleh git:
- `kotlin/` berisi kelas `NativeEditor` dan glue JNI-nya;
- `jniLibs/` berisi `librs_editor_android.so` untuk setiap ABI.

Build-nya dioptimalkan (release) secara default; gunakan `--debug` untuk build debug. Tanpa `--arch`, kedua ABI di `boltffi.toml` akan dibuild.

## API

```kotlin
val editor = RsEditorView(context, contentJsonOrHtml)

editor.chain().focus().toggleBold().toggleHeading(2).run()   // satu langkah undo
editor.can().toggleBulletList().run()                        // apakah bisa? tidak mengubah apa pun
editor.isActive("heading", mapOf("level" to 2))
editor.getAttributes("link")["href"]
editor.json = "..."   // atau editor.html; editor.text, editor.isEditable
editor.addOnChangeListener { refreshToolbar() }
editor.release()      // saat view dibuang
```

Di Compose:

```kotlin
val state = rememberRsEditorState(content)
RsEditor(state, Modifier.weight(1f))
IconToggleButton(
    checked = state.isActive("bold"),
    onCheckedChange = { state.chain().toggleBold().run() },
    enabled = state.can().toggleBold().run(),
) { … }
```

Nama perintah dan argumennya sama dengan extension Rust (dan Tiptap). Perintah yang tidak punya metode sendiri di `RsEditorChain` bisa dipanggil lewat `command(name, args)`.
