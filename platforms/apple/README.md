# Rs Editor untuk iOS dan macOS

Rs Editor sebagai view native: `NSView` di macOS dan `UIView` di iOS. Inti Rust-nya (`crates/rs-editor-apple`) menggambar dengan Skia di atas Metal dan dipanggil lewat binding Swift buatan BoltFFI.

| Folder | Isi |
| --- | --- |
| `RsEditorKit` | Paket Swift: `RsEditorView`, `RsEditorState` dan `RsEditor` (SwiftUI) |
| `Demo` | Aplikasi contoh macOS dan iOS (xcodegen) dengan toolbar format |
| `generated` | Hasil `package.sh` (di-ignore git): `RsEditorCore.xcframework` dan binding Swift-nya |

## Build

Prasyarat: Xcode 26 dengan runtime simulator iOS, `boltffi` CLI 0.29.3, xcodegen, serta target Rust `aarch64-apple-ios` dan `aarch64-apple-ios-sim`. Peluncuran pertama di runtime simulator yang baru membangun cache dyld; sisakan beberapa GB ruang disk.

```bash
platforms/apple/package.sh
```

```bash
cd platforms/apple/Demo && xcodegen && open RsEditorDemo.xcodeproj
```

Skia memakai binary prebuilt `ganesh-jpegd-jpege-metal-pdf-textlayout` untuk macOS, iOS, dan iOS simulator.

## API

```swift
let editor = RsEditorView(content: jsonOrHtml)

editor.chain([("toggleBold", nil), ("toggleHeading", ["level": 2])])   // satu langkah undo
editor.can("toggleBulletList")
editor.isActive("heading", ["level": 2])
editor.attributes(of: "link")["href"]
editor.json = "..."      // atau html, markdown; text, isEmpty, isEditable
editor.editLink()        // kolom alamat tautan, untuk tombol di toolbar aplikasi
editor.placeholder = "Mulai menulis"
editor.onPickImage = { … }                     // lalu editor.insertImage(src:)
editor.loadImage = { src in try? await myServer.data(src) }   // gambar yang butuh login
editor.onChange = { refreshToolbar() }
```

Di SwiftUI:

```swift
@StateObject var editor = RsEditorState(content: json)

RsEditor(editor)
Button("B") { editor.run("toggleBold") }
    .background(editor.isActive("bold") ? Color.secondary.opacity(0.2) : .clear)
```

## Platform

- **macOS:** mouse dan trackpad (scroll berinersia dari sistem), `NSTextInputClient` (IME, dikte, emoji picker), menu Edit (Undo, Salin, Tempel…), menu klik kanan, pasteboard HTML dan teks, kursor, dan VoiceOver lewat AccessKit.
- **iOS:** `UITextInput` (keyboard, autocorrect, IME), gesture sentuh dari inti Rust (sama dengan Android), edit menu `UIEditMenuInteraction`, loupe `UITextLoupeSession`, haptik, keyboard fisik, dan VoiceOver lewat AccessKit.
- Gambar dimuat dengan URLSession dan didekode ImageIO (orientasi EXIF diterapkan, maks. 2048 px).
