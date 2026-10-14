#if os(macOS)
import AppKit
import QuartzCore
import RsEditorCore

/// The editor as an `NSView`: drawn by the Rust core with Skia on Metal,
/// typed into through `NSTextInputClient` (so input methods, dictation and
/// the emoji picker work), with the mouse, trackpad and keyboard of a Mac.
public final class RsEditorView: NSView, NSTextInputClient, NSMenuItemValidation {
    let engine: EditorEngine
    let images: ImageLoader
    private var displayLink: CADisplayLink?
    private var tracking: NSTrackingArea?
    /// Called after every change to the document or the selection.
    public var onChange: (() -> Void)?
    /// Called when the user inserts an image from the `/` menu: show a picker, then call
    /// `insertImage`. Without it the editor asks for an image address.
    public var onPickImage: (() -> Void)? {
        didSet { engine.editor.setImagePicker(on: onPickImage != nil) }
    }
    private var seenRevision: UInt64 = 0

    /// An editor starting with `content` (Tiptap JSON, HTML, or Markdown when `markdown`).
    public init(content: String = "", markdown: Bool = false, characterLimit: Int = 0) {
        guard let engine = EditorEngine(
            content: content, markdown: markdown, dark: false, scale: 2, touch: false,
            characterLimit: characterLimit)
        else { fatalError("Rs Editor needs Metal") }
        self.engine = engine
        images = ImageLoader(engine: engine)
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        engine.onEffects = { [weak self] effects in self?.perform(effects) }
        engine.wake = { [weak self] in self?.displayLink?.isPaused = false }
        // VoiceOver, through AccessKit (before the view is first shown).
        engine.editor.attachAccessibility(view: UInt64(UInt(bitPattern: Unmanaged.passUnretained(self).toOpaque())))
        for name in [NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification] {
            NotificationCenter.default.addObserver(
                self, selector: #selector(windowKeyChanged(_:)), name: name, object: nil)
        }
    }

    /// VoiceOver moved the caret or edited the document.
    @objc func rsEditorAccessibilityChanged() {
        afterInput()
    }

    @objc private func windowKeyChanged(_ note: Notification) {
        guard let window, note.object as? NSWindow === window else { return }
        engine.editor.setWindowFocused(focused: window.isKeyWindow)
        engine.setNeedsFrame()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    public override func makeBackingLayer() -> CALayer {
        engine.layer
    }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }
    public override var isOpaque: Bool { true }

    // MARK: - Window, size and frames

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        displayLink?.invalidate()
        displayLink = nil
        guard window != nil else {
            engine.detach()
            return
        }
        updateScale()
        engine.attach()
        updateAppearance()
        let link = displayLink(target: self, selector: #selector(step(_:)))
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    public override func setFrameSize(_ size: NSSize) {
        super.setFrameSize(size)
        engine.resize(bounds.size, scale: scale)
        engine.tick()
    }

    public override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        updateScale()
    }

    public override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        updateAppearance()
    }

    private var scale: CGFloat {
        window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2
    }

    private func updateScale() {
        engine.resize(bounds.size, scale: scale)
    }

    private func updateAppearance() {
        let dark = effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        engine.editor.setDark(dark: dark)
        engine.setNeedsFrame()
    }

    @objc private func step(_ link: CADisplayLink) {
        // Idle (no animation, no caret blinking): the link sleeps until
        // the next input or change.
        link.isPaused = !engine.tick(now: CACurrentMediaTime())
    }

    /// After input: a frame, the effects, and `onChange`.
    func afterInput() {
        engine.afterInput()
        let now = engine.editor.revision()
        if now != seenRevision {
            seenRevision = now
            inputContext?.invalidateCharacterCoordinates()
            onChange?()
        }
    }

    // MARK: - Focus

    public override func becomeFirstResponder() -> Bool {
        engine.editor.setFocused(focused: true)
        afterInput()
        return true
    }

    public override func resignFirstResponder() -> Bool {
        engine.editor.setFocused(focused: false)
        afterInput()
        return true
    }

    // MARK: - Mouse and trackpad

    public override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(
            rect: .zero,
            options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect, .cursorUpdate],
            owner: self)
        addTrackingArea(area)
        tracking = area
    }

    private func point(_ event: NSEvent) -> CGPoint {
        convert(event.locationInWindow, from: nil)
    }

    private func pointer(_ kind: Int32, _ event: NSEvent) {
        let at = point(event)
        _ = engine.editor.pointer(
            kind: kind, x: Float(at.x), y: Float(at.y),
            clicks: UInt32(max(event.clickCount, 1)), modifiers: modifiers(event.modifierFlags).rawValue)
        afterInput()
    }

    public override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        pointer(0, event)
    }

    public override func mouseDragged(with event: NSEvent) { pointer(1, event) }
    public override func mouseMoved(with event: NSEvent) { pointer(1, event) }
    public override func mouseUp(with event: NSEvent) { pointer(2, event) }

    public override func mouseExited(with event: NSEvent) {
        _ = engine.editor.pointer(kind: 3, x: 0, y: 0, clicks: 1, modifiers: 0)
        afterInput()
    }

    public override func cursorUpdate(with event: NSEvent) {
        NSCursor.iBeam.set()
    }

    public override func scrollWheel(with event: NSEvent) {
        let at = point(event)
        // Trackpads and Magic Mice report points and their own momentum;
        // wheels report lines, which glide.
        let precise = event.hasPreciseScrollingDeltas
        let dy = precise ? -event.scrollingDeltaY : -event.scrollingDeltaY * 40
        guard dy != 0 else { return }
        engine.editor.wheel(x: Float(at.x), y: Float(at.y), dy: Float(dy), smooth: !precise)
        afterInput()
    }

    public override func menu(for event: NSEvent) -> NSMenu? {
        // Without a selection, the click places the caret first.
        if engine.editor.selection().contains("\"empty\":true") {
            pointer(0, event)
            pointer(2, event)
        }
        let menu = NSMenu()
        menu.addItem(withTitle: menuLabel("cut"), action: #selector(cut(_:)), keyEquivalent: "")
        menu.addItem(withTitle: menuLabel("copy"), action: #selector(copy(_:)), keyEquivalent: "")
        menu.addItem(withTitle: menuLabel("paste"), action: #selector(paste(_:)), keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(withTitle: menuLabel("selectAll"), action: #selector(selectAll(_:)), keyEquivalent: "")
        return menu
    }

    // MARK: - Keys

    public override func keyDown(with event: NSEvent) {
        // An input method composing gets every key first.
        if hasMarkedText() {
            inputContext?.handleEvent(event)
            afterInput()
            return
        }
        let flags = modifiers(event.modifierFlags)
        if let key = Self.keyName(event) {
            let command = flags.contains(.command) || flags.contains(.control)
            if command, let action = Self.clipboardAction(key, flags) {
                // Without an Edit menu, Command-C/X/V/Z arrive here.
                NSApp.sendAction(action, to: self, from: self)
                return
            }
            if (command || Self.isSpecial(key)) && engine.editor.keyDown(key: key, modifiers: flags.rawValue) {
                afterInput()
                return
            }
        }
        inputContext?.handleEvent(event)
        afterInput()
    }

    public override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard window?.firstResponder === self, let key = Self.keyName(event) else {
            return super.performKeyEquivalent(with: event)
        }
        let flags = modifiers(event.modifierFlags)
        // Formatting shortcuts (Command-B, -I, -K…) the app's menu does not take.
        if Self.clipboardAction(key, flags) == nil, flags.contains(.command),
           engine.editor.keyDown(key: key, modifiers: flags.rawValue) {
            afterInput()
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    private func modifiers(_ flags: NSEvent.ModifierFlags) -> EditorModifiers {
        var result: EditorModifiers = []
        if flags.contains(.shift) { result.insert(.shift) }
        if flags.contains(.control) { result.insert(.control) }
        if flags.contains(.option) { result.insert(.option) }
        if flags.contains(.command) { result.insert(.command) }
        return result
    }

    /// The W3C key name of a key event (`ArrowLeft`, `Backspace`, `b`).
    static func keyName(_ event: NSEvent) -> String? {
        switch event.keyCode {
        case 36, 76: return "Enter"
        case 48: return "Tab"
        case 51: return "Backspace"
        case 53: return "Escape"
        case 117: return "Delete"
        case 115: return "Home"
        case 119: return "End"
        case 116: return "PageUp"
        case 121: return "PageDown"
        case 123: return "ArrowLeft"
        case 124: return "ArrowRight"
        case 125: return "ArrowDown"
        case 126: return "ArrowUp"
        default: break
        }
        guard let characters = event.charactersIgnoringModifiers, !characters.isEmpty else { return nil }
        // Shortcuts name the key, not the character Shift makes of it.
        return event.modifierFlags.contains(.shift) ? characters.lowercased() : characters
    }

    static func isSpecial(_ key: String) -> Bool {
        key.count > 1
    }

    static func clipboardAction(_ key: String, _ flags: EditorModifiers) -> Selector? {
        guard flags.contains(.command), !flags.contains(.control), !flags.contains(.option) else { return nil }
        switch key {
        case "c": return #selector(copy(_:))
        case "x": return #selector(cut(_:))
        case "v": return #selector(paste(_:))
        case "a": return #selector(selectAll(_:))
        case "z": return flags.contains(.shift) ? #selector(redo(_:)) : #selector(undo(_:))
        default: return nil
        }
    }

    // MARK: - Edit menu actions

    @objc public func copy(_ sender: Any?) {
        write(engine.editor.copy(cut: false))
    }

    @objc public func cut(_ sender: Any?) {
        write(engine.editor.copy(cut: true))
        afterInput()
    }

    @objc public func paste(_ sender: Any?) {
        let board = NSPasteboard.general
        if let html = board.string(forType: .html), engine.editor.pasteHtml(html: html) {
            afterInput()
            return
        }
        if let text = board.string(forType: .string) {
            _ = engine.editor.paste(text: text)
        }
        afterInput()
    }

    @objc public override func selectAll(_ sender: Any?) {
        _ = engine.editor.keyDown(key: "a", modifiers: EditorModifiers.command.rawValue)
        afterInput()
    }

    @objc public func undo(_ sender: Any?) {
        _ = engine.editor.run(name: "undo", argsJson: "null")
        afterInput()
    }

    @objc public func redo(_ sender: Any?) {
        _ = engine.editor.run(name: "redo", argsJson: "null")
        afterInput()
    }

    public func validateMenuItem(_ item: NSMenuItem) -> Bool {
        let empty = engine.editor.selection().contains("\"empty\":true")
        switch item.action {
        case #selector(copy(_:)): return !empty
        case #selector(cut(_:)): return !empty && engine.editor.isEditable()
        case #selector(paste(_:)): return engine.editor.isEditable()
        case #selector(undo(_:)): return engine.editor.can(name: "undo", argsJson: "null")
        case #selector(redo(_:)): return engine.editor.can(name: "redo", argsJson: "null")
        default: return true
        }
    }

    /// Puts `{text, html}` JSON on the pasteboard.
    private func write(_ json: String) {
        guard let data = json.data(using: .utf8),
              let content = try? JSONSerialization.jsonObject(with: data) as? [String: String]
        else { return }
        write(text: content["text"] ?? "", html: content["html"] ?? "")
    }

    private func write(text: String, html: String) {
        guard !text.isEmpty || !html.isEmpty else { return }
        let board = NSPasteboard.general
        board.clearContents()
        if !html.isEmpty {
            board.setString(html, forType: .html)
        }
        board.setString(text, forType: .string)
    }

    // MARK: - NSTextInputClient

    public func insertText(_ string: Any, replacementRange: NSRange) {
        let text = Self.plain(string)
        let state = engine.imeState()
        if replacementRange.location != NSNotFound {
            _ = engine.editor.replaceText(
                start: Int32(replacementRange.location), end: Int32(NSMaxRange(replacementRange)),
                text: text, composing: false)
        } else if let marked = state.composition {
            _ = engine.editor.replaceText(
                start: Int32(marked.location), end: Int32(NSMaxRange(marked)), text: text, composing: false)
        } else {
            _ = engine.editor.insertText(text: text)
        }
        afterInput()
    }

    public func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        let text = Self.plain(string)
        let state = engine.imeState()
        if replacementRange.location != NSNotFound {
            _ = engine.editor.replaceText(
                start: Int32(replacementRange.location), end: Int32(NSMaxRange(replacementRange)),
                text: text, composing: true)
        } else if let marked = state.composition {
            _ = engine.editor.replaceText(
                start: Int32(marked.location), end: Int32(NSMaxRange(marked)), text: text, composing: true)
        } else {
            engine.editor.setMarkedText(text: text)
        }
        afterInput()
    }

    public func unmarkText() {
        engine.editor.unmarkText()
        afterInput()
    }

    public func selectedRange() -> NSRange {
        let state = engine.imeState()
        return state.start == -1 ? NSRange(location: NSNotFound, length: 0) : state.selectedRange
    }

    public func markedRange() -> NSRange {
        engine.imeState().composition ?? NSRange(location: NSNotFound, length: 0)
    }

    public func hasMarkedText() -> Bool {
        engine.imeState().composition != nil
    }

    public func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        let state = engine.imeState()
        let text = state.text as NSString
        let clipped = NSIntersectionRange(range, NSRange(location: 0, length: text.length))
        guard state.start != -1, clipped.location != NSNotFound else { return nil }
        actualRange?.pointee = clipped
        return NSAttributedString(string: text.substring(with: clipped))
    }

    public func validAttributesForMarkedText() -> [NSAttributedString.Key] {
        []
    }

    public func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        let local = Self.firstLineRect(engine.editor, range) ?? engine.imeState().caret ?? .zero
        actualRange?.pointee = range
        guard let window else { return .zero }
        return window.convertToScreen(convert(local, to: nil))
    }

    public func characterIndex(for point: NSPoint) -> Int {
        guard let window else { return NSNotFound }
        let local = convert(window.convertPoint(fromScreen: point), from: nil)
        let offset = engine.editor.imeOffsetAt(x: Float(local.x), y: Float(local.y))
        return offset < 0 ? NSNotFound : Int(offset)
    }

    public override func doCommand(by selector: Selector) {
        // Keys the editor did not take (emacs bindings such as Control-A).
        let commands: [Selector: (String, EditorModifiers)] = [
            #selector(insertNewline(_:)): ("Enter", []),
            #selector(insertLineBreak(_:)): ("Enter", .shift),
            #selector(insertTab(_:)): ("Tab", []),
            #selector(insertBacktab(_:)): ("Tab", .shift),
            #selector(deleteBackward(_:)): ("Backspace", []),
            #selector(deleteForward(_:)): ("Delete", []),
            #selector(deleteWordBackward(_:)): ("Backspace", .option),
            #selector(deleteWordForward(_:)): ("Delete", .option),
            #selector(moveLeft(_:)): ("ArrowLeft", []),
            #selector(moveRight(_:)): ("ArrowRight", []),
            #selector(moveUp(_:)): ("ArrowUp", []),
            #selector(moveDown(_:)): ("ArrowDown", []),
            #selector(moveBackward(_:)): ("ArrowLeft", []),
            #selector(moveForward(_:)): ("ArrowRight", []),
            #selector(moveToBeginningOfParagraph(_:)): ("ArrowLeft", .command),
            #selector(moveToEndOfParagraph(_:)): ("ArrowRight", .command),
            #selector(moveToBeginningOfLine(_:)): ("ArrowLeft", .command),
            #selector(moveToEndOfLine(_:)): ("ArrowRight", .command),
            #selector(cancelOperation(_:)): ("Escape", []),
        ]
        if let (key, flags) = commands[selector] {
            _ = engine.editor.keyDown(key: key, modifiers: flags.rawValue)
            afterInput()
        } else if selector == #selector(deleteToEndOfParagraph(_:)) {
            _ = engine.editor.keyDown(key: "ArrowRight", modifiers: (EditorModifiers.command.union(.shift)).rawValue)
            write(engine.editor.copy(cut: true))
            afterInput()
        }
    }

    static func plain(_ string: Any) -> String {
        (string as? NSAttributedString)?.string ?? (string as? String) ?? ""
    }

    /// The part of `range` on its first line, in view points.
    static func firstLineRect(_ editor: NativeEditor, _ range: NSRange) -> CGRect? {
        let to = range.length == 0 ? range.location + 1 : NSMaxRange(range)
        let json = editor.imeCursorInfo(from: Int32(range.location), to: Int32(to))
        guard let data = json.data(using: .utf8),
              let info = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        let boxes = (info["bounds"] as? [Any] ?? []).compactMap(effectRect)
        if let first = boxes.first {
            let line = boxes.filter { abs($0.midY - first.midY) < first.height / 2 }
            var rect = line.reduce(first) { $0.union($1) }
            if range.length == 0 {
                rect.size.width = 0
            }
            return rect
        }
        if let caret = info["caret"] as? [NSNumber], caret.count == 4 {
            return CGRect(
                x: caret[0].doubleValue, y: caret[1].doubleValue, width: 0,
                height: caret[3].doubleValue - caret[1].doubleValue)
        }
        return nil
    }

    // MARK: - Effects

    private func perform(_ effects: [[String: Any]]) {
        for effect in effects {
            switch effect["type"] as? String {
            case "clipboard":
                write(text: effect["text"] as? String ?? "", html: effect["html"] as? String ?? "")
            case "openUrl":
                if let url = (effect["url"] as? String).flatMap({ URL(string: $0, relativeTo: images.baseURL) }) {
                    NSWorkspace.shared.open(url)
                }
            case "cursor":
                switch effect["cursor"] as? String {
                case "pointer": NSCursor.pointingHand.set()
                case "grab": NSCursor.openHand.set()
                case "grabbing": NSCursor.closedHand.set()
                case "default": NSCursor.arrow.set()
                default: NSCursor.iBeam.set()
                }
            case "missingImages":
                images.load(effect["sources"] as? [String] ?? [])
            case "pickImage":
                onPickImage?()
            case "limitReached":
                NSSound.beep()
            case "haptic":
                NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .now)
            default:
                break
            }
        }
    }
}
#endif
