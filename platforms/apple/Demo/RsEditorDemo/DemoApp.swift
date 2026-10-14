import PhotosUI
import RsEditorKit
import SwiftUI

@main
struct DemoApp: App {
    @StateObject private var editor = RsEditorState(content: DemoView.sample)

    var body: some Scene {
        WindowGroup {
            DemoView(editor: editor)
        }
        #if os(macOS)
        .defaultSize(width: 860, height: 760)
        .commands { FormatCommands(editor: editor) }
        #endif
    }
}

#if os(macOS)
/// The Format menu of the menu bar, with the usual shortcuts.
struct FormatCommands: Commands {
    @ObservedObject var editor: RsEditorState

    var body: some Commands {
        CommandMenu("Format") {
            Button("Tebal") { editor.run("toggleBold") }.keyboardShortcut("b")
            Button("Miring") { editor.run("toggleItalic") }.keyboardShortcut("i")
            Button("Garis Bawah") { editor.run("toggleUnderline") }.keyboardShortcut("u")
            Button("Coret") { editor.run("toggleStrike") }.keyboardShortcut("s", modifiers: [.command, .shift])
            Button("Kode") { editor.run("toggleCode") }.keyboardShortcut("e")
            Divider()
            Button("Judul 1") { editor.run("toggleHeading", ["level": 1]) }.keyboardShortcut("1", modifiers: [.command, .option])
            Button("Judul 2") { editor.run("toggleHeading", ["level": 2]) }.keyboardShortcut("2", modifiers: [.command, .option])
            Button("Daftar") { editor.run("toggleBulletList") }.keyboardShortcut("8", modifiers: [.command, .shift])
            Button("Daftar Bernomor") { editor.run("toggleOrderedList") }.keyboardShortcut("7", modifiers: [.command, .shift])
            Button("Daftar Tugas") { editor.run("toggleTaskList") }.keyboardShortcut("9", modifiers: [.command, .shift])
            Button("Kutipan") { editor.run("toggleBlockquote") }.keyboardShortcut("b", modifiers: [.command, .shift])
            Divider()
            Button("Tautan…") { editor.view.editLink() }.keyboardShortcut("k")
        }
    }
}
#endif

/// The editor with a toolbar driven by its state, as an app would build one.
struct DemoView: View {
    @ObservedObject var editor: RsEditorState
    @State private var pickingPhoto = false
    @State private var photo: PhotosPickerItem?

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider()
            RsEditor(editor)
            Divider()
            HStack {
                let _ = editor.revision
                Text("\(editor.view.characters) karakter · \(editor.view.words) kata")
                Spacer()
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
        }
        .onAppear {
            // The `/` menu's Image row asks the app for a picture.
            editor.view.onPickImage = { pickImage() }
        }
        .photosPicker(isPresented: $pickingPhoto, selection: $photo, matching: .images)
        .onChange(of: photo) { _, item in
            guard let item else { return }
            photo = nil
            Task {
                if let data = try? await item.loadTransferable(type: Data.self) {
                    editor.view.insertImage(data: data)
                }
            }
        }
    }

    private func pickImage() {
        #if os(macOS)
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.image]
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let url = panel.url, let data = try? Data(contentsOf: url) {
            editor.view.insertImage(data: data)
        }
        #else
        pickingPhoto = true
        #endif
    }

    private var toolbar: some View {
        // Reading `revision` refreshes the buttons after every change.
        let _ = editor.revision
        return ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4) {
                toggle("bold", "bold", "toggleBold")
                toggle("italic", "italic", "toggleItalic")
                toggle("underline", "underline", "toggleUnderline")
                toggle("strikethrough", "strike", "toggleStrike")
                toggle("chevron.left.forwardslash.chevron.right", "code", "toggleCode")
                Divider().frame(height: 20)
                button("H1", active: editor.isActive("heading", ["level": 1])) {
                    editor.run("toggleHeading", ["level": 1])
                }
                button("H2", active: editor.isActive("heading", ["level": 2])) {
                    editor.run("toggleHeading", ["level": 2])
                }
                toggle("list.bullet", "bulletList", "toggleBulletList")
                toggle("list.number", "orderedList", "toggleOrderedList")
                toggle("checklist", "taskList", "toggleTaskList")
                toggle("text.quote", "blockquote", "toggleBlockquote")
                Divider().frame(height: 20)
                icon("link", active: editor.isActive("link")) { editor.view.editLink() }
                icon("photo", active: false) { pickImage() }
                icon("arrow.uturn.backward", active: false) { editor.run("undo") }
                    .disabled(!editor.can("undo"))
                icon("arrow.uturn.forward", active: false) { editor.run("redo") }
                    .disabled(!editor.can("redo"))
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
        }
    }

    private func toggle(_ symbol: String, _ name: String, _ command: String) -> some View {
        icon(symbol, active: editor.isActive(name)) { editor.run(command) }
    }

    private func icon(_ symbol: String, active: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol).frame(width: 28, height: 28)
        }
        .buttonStyle(.plain)
        .background(active ? Color.secondary.opacity(0.2) : .clear, in: RoundedRectangle(cornerRadius: 6))
    }

    private func button(_ title: String, active: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title).font(.system(size: 13, weight: .semibold)).frame(width: 28, height: 28)
        }
        .buttonStyle(.plain)
        .background(active ? Color.secondary.opacity(0.2) : .clear, in: RoundedRectangle(cornerRadius: 6))
    }

    static let sample: String = {
        guard let url = Bundle.main.url(forResource: "sample", withExtension: "json"),
              let text = try? String(contentsOf: url, encoding: .utf8)
        else { return "" }
        return text
    }()
}
