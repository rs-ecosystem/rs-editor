import Foundation
import ImageIO
import RsEditorCore
import UniformTypeIdentifiers

/// The editor's content and commands, the same on iOS and macOS (and as in
/// the web and Android libraries): Tiptap's names and JSON.
extension RsEditorView {
    /// The document as Tiptap JSON; setting it replaces the document.
    public var json: String {
        get { engine.editor.getJson() }
        set { if engine.editor.setJson(json: newValue) { afterInput() } }
    }

    /// The document as HTML; setting it replaces the document.
    public var html: String {
        get { engine.editor.getHtml() }
        set { if engine.editor.setHtml(html: newValue) { afterInput() } }
    }

    /// The document as Markdown; setting it replaces the document.
    public var markdown: String {
        get { engine.editor.getMarkdown() }
        set { if engine.editor.setMarkdown(markdown: newValue) { afterInput() } }
    }

    /// Plain text, blocks separated by newlines.
    public var text: String { engine.editor.getText() }

    public var isEmpty: Bool { engine.editor.isEmpty() }

    public var isEditable: Bool {
        get { engine.editor.isEditable() }
        set {
            engine.editor.setEditable(editable: newValue)
            afterInput()
        }
    }

    /// Changes whenever the document or the selection may have changed.
    public var revision: UInt64 { engine.editor.revision() }

    /// Characters in the document (graphemes, so an emoji counts once).
    public var characters: Int { count("characters") }

    /// Words in the document.
    public var words: Int { count("words") }

    /// Text in an empty line with the caret; nil restores the hint for the `/` menu.
    public var placeholder: String? {
        get { nil }
        set {
            engine.editor.setPlaceholder(text: newValue ?? "")
            engine.setNeedsFrame()
        }
    }

    /// A menu of blocks when `/` is typed (on by default).
    public func setSlashMenu(_ enabled: Bool) {
        engine.editor.setSlashMenu(enabled: enabled)
    }

    /// A grip beside blocks to select and move them (on by default).
    public func setDragHandle(_ enabled: Bool) {
        engine.editor.setDragHandle(enabled: enabled)
        engine.setNeedsFrame()
    }

    /// A formatting menu over selected text (on by default with a mouse).
    public func setBubbleMenu(_ enabled: Bool) {
        engine.editor.setBubbleMenu(enabled: enabled)
        engine.setNeedsFrame()
    }

    /// The language of the editor's own menus (BCP 47, such as "id").
    public func setLocale(_ locale: String) {
        engine.editor.setLocale(locale: locale)
        engine.setNeedsFrame()
    }

    /// Plain text pasted is read as Markdown.
    public func setPasteMarkdown(_ markdown: Bool) {
        engine.editor.setMarkdownPaste(markdown: markdown)
    }

    /// Resolves relative image addresses.
    public var imageBaseURL: URL? {
        get { images.baseURL }
        set { images.baseURL = newValue }
    }

    /// The app's own image loading (servers needing credentials, uploads in progress):
    /// return the bytes, or nil to load the address as usual.
    public var loadImage: ((String) async -> Data?)? {
        get { images.load }
        set { images.load = newValue }
    }

    /// Stores an image dropped or pasted into the editor (uploads it, say) and returns its
    /// address. Without it the image is kept in a temporary file.
    public var storeImage: ((Data) async -> String?)? {
        get { images.store }
        set { images.store = newValue }
    }

    /// Inserts an image from its bytes (a photo picked, say) at the caret; `storeImage`
    /// stores it.
    public func insertImage(data: Data) {
        insertImageData(data)
    }

    /// Inserts dropped or pasted image bytes at the caret, stored by `storeImage`.
    func insertImageData(_ data: Data) {
        Task { @MainActor [weak self] in
            guard let self else { return }
            let src: String?
            if let store = self.images.store {
                src = await store(data)
            } else {
                src = Self.temporaryImage(data)
            }
            if let src {
                self.chain([("setImage", ["src": src])])
            }
        }
    }

    /// Keeps image bytes in a temporary file; returns its `file:` address.
    static func temporaryImage(_ data: Data) -> String? {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("rs-editor", isDirectory: true)
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let type = CGImageSourceCreateWithData(data as CFData, nil).flatMap(CGImageSourceGetType)
        let suffix = type.flatMap { UTType($0 as String)?.preferredFilenameExtension } ?? "png"
        let file = folder.appendingPathComponent(UUID().uuidString).appendingPathExtension(suffix)
        guard (try? data.write(to: file)) != nil else { return nil }
        return file.absoluteString
    }

    /// Runs one command by name with JSON-able arguments; false when it did not apply.
    @discardableResult
    public func run(_ name: String, _ args: Any? = nil) -> Bool {
        var applied = false
        external { applied = engine.editor.run(name: name, argsJson: Self.json(args)) }
        afterInput()
        return applied
    }

    /// Whether the command would apply.
    public func can(_ name: String, _ args: Any? = nil) -> Bool {
        engine.editor.can(name: name, argsJson: Self.json(args))
    }

    /// Runs commands as one transaction (one undo step): `chain([("toggleBold", nil)])`.
    /// A `focus` step (as in Tiptap's `chain().focus()`) gives the editor the keyboard focus.
    @discardableResult
    public func chain(_ steps: [(String, Any?)]) -> Bool {
        // `focus` is the view's, not a command of the core.
        if steps.contains(where: { $0.0 == "focus" }) {
            focusEditor()
        }
        let list = steps.filter { $0.0 != "focus" }.map { step -> [String: Any] in
            var entry: [String: Any] = ["name": step.0]
            if let args = step.1 { entry["args"] = args }
            return entry
        }
        var applied = false
        external { applied = engine.editor.runChain(stepsJson: Self.json(list)) }
        afterInput()
        return applied
    }

    /// Whether the mark or node `name` (with `attrs`) is active at the selection.
    public func isActive(_ name: String, _ attrs: [String: Any] = [:]) -> Bool {
        engine.editor.isActive(name: name, attrsJson: Self.json(attrs))
    }

    /// Attributes of the mark or node `name` at the selection.
    public func attributes(of name: String) -> [String: Any] {
        let json = engine.editor.getAttributes(name: name)
        return (json.data(using: .utf8).flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any]) ?? [:]
    }

    /// Opens the link address field for the selection (as Command-K does).
    @discardableResult
    public func editLink() -> Bool {
        let opened = engine.editor.editLink()
        afterInput()
        return opened
    }

    /// Inserts an image at the caret.
    @discardableResult
    public func insertImage(src: String, alt: String? = nil) -> Bool {
        var attrs: [String: Any] = ["src": src]
        if let alt { attrs["alt"] = alt }
        return chain([("focus", nil), ("setImage", attrs)])
    }

    private func count(_ key: String) -> Int {
        let json = engine.editor.characterCount()
        let object = json.data(using: .utf8).flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any]
        return (object?[key] as? NSNumber)?.intValue ?? 0
    }

    static func json(_ value: Any?) -> String {
        guard let value, JSONSerialization.isValidJSONObject(value) || value is String || value is NSNumber,
              let data = try? JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed])
        else { return "null" }
        return String(data: data, encoding: .utf8) ?? "null"
    }
}
