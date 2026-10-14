#if os(iOS)
import QuartzCore
import RsEditorCore
import UIKit

/// The editor as a `UIView`: drawn by the Rust core with Skia on Metal,
/// typed into through `UITextInput` (keyboards, autocorrection, dictation,
/// input methods). Touch gestures, selection handles and menus are the
/// core's, as on Android; the view shows the system edit menu, loupe and
/// haptics for them.
public final class RsEditorView: UIView, UITextInput, UIEditMenuInteractionDelegate {
    let engine: EditorEngine
    let images: ImageLoader
    private var displayLink: CADisplayLink?
    /// Called after every change to the document or the selection.
    public var onChange: (() -> Void)?
    /// Called when the user inserts an image from the `/` menu: show a picker, then call
    /// `insertImage`. Without it the editor asks for an image address.
    public var onPickImage: (() -> Void)? {
        didSet { engine.editor.setImagePicker(on: onPickImage != nil) }
    }
    private var seenRevision: UInt64 = 0
    private var seenStart = -1
    private var editMenu: UIEditMenuInteraction?
    /// The selection menu the core asked for: its rect and items.
    private var menuRequest: (rect: CGRect, items: [String])?
    private var loupe: UITextLoupeSession?
    /// UITextInput calls in progress: UIKit knows about their changes.
    private var keyboardEditing = 0

    public override class var layerClass: AnyClass { CAMetalLayer.self }

    /// An editor starting with `content` (Tiptap JSON, HTML, or Markdown when `markdown`).
    public init(content: String = "", markdown: Bool = false, characterLimit: Int = 0) {
        guard let engine = EditorEngine(
            content: content, markdown: markdown, dark: false, scale: UIScreen.main.scale, touch: true,
            characterLimit: characterLimit)
        else { fatalError("Rs Editor needs Metal") }
        self.engine = engine
        images = ImageLoader(engine: engine)
        super.init(frame: .zero)
        isOpaque = true
        isMultipleTouchEnabled = false
        engine.onEffects = { [weak self] effects in self?.perform(effects) }
        engine.wake = { [weak self] in self?.displayLink?.isPaused = false }
        let menu = UIEditMenuInteraction(delegate: self)
        addInteraction(menu)
        editMenu = menu
        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (view: RsEditorView, _) in
            view.updateAppearance()
        }
        NotificationCenter.default.addObserver(
            self, selector: #selector(keyboardFrameChanged(_:)),
            name: UIResponder.keyboardWillChangeFrameNotification, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(keyboardFrameChanged(_:)),
            name: UIResponder.keyboardWillHideNotification, object: nil)
        // VoiceOver, through AccessKit (before the view is first shown).
        engine.editor.attachAccessibility(view: UInt64(UInt(bitPattern: Unmanaged.passUnretained(self).toOpaque())))
    }

    /// VoiceOver moved the caret or edited the document.
    @objc func rsEditorAccessibilityChanged() {
        afterInput()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    private var metalLayer: CAMetalLayer { layer as! CAMetalLayer }

    // MARK: - Window, size and frames

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        displayLink?.invalidate()
        displayLink = nil
        guard let window else {
            engine.detach()
            return
        }
        adoptLayer()
        engine.resize(bounds.size, scale: window.screen.scale)
        engine.attach()
        updateAppearance()
        let link = CADisplayLink(target: self, selector: #selector(step(_:)))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    /// The engine draws into this view's own Metal layer.
    private func adoptLayer() {
        if engine.layer !== metalLayer {
            engine.useLayer(metalLayer)
        }
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        engine.resize(bounds.size, scale: window?.screen.scale ?? UIScreen.main.scale)
        engine.tick()
    }

    private func updateAppearance() {
        engine.editor.setDark(dark: traitCollection.userInterfaceStyle == .dark)
        engine.setNeedsFrame()
    }

    @objc private func step(_ link: CADisplayLink) {
        link.isPaused = !engine.tick(now: CACurrentMediaTime())
    }

    /// After input: a frame, the effects, `onChange`, and the keyboard told
    /// about changes it did not make.
    func afterInput() {
        engine.afterInput()
        let now = engine.editor.revision()
        guard now != seenRevision else { return }
        seenRevision = now
        if keyboardEditing == 0 {
            let start = engine.imeState().start
            if (start == -2) != (seenStart == -2) {
                // Into or out of the link field: its keyboard (URL, Go).
                reloadInputViews()
            }
            if start != seenStart {
                // Another block: the keyboard reads its text afresh.
                inputDelegate?.textWillChange(self)
                inputDelegate?.textDidChange(self)
            } else {
                inputDelegate?.selectionWillChange(self)
                inputDelegate?.selectionDidChange(self)
            }
            seenStart = start
        }
        onChange?()
    }

    private func keyboardDriven(_ change: () -> Void) {
        keyboardEditing += 1
        change()
        afterInput()
        keyboardEditing -= 1
        seenStart = engine.imeState().start
    }

    @objc private func keyboardFrameChanged(_ note: Notification) {
        guard let window,
              let end = note.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect
        else { return }
        let hiding = note.name == UIResponder.keyboardWillHideNotification
        let keyboard = convert(window.convert(end, from: nil), from: window)
        let covered = hiding ? 0 : max(0, bounds.maxY - keyboard.minY)
        engine.editor.setObscuredBottom(points: Float(covered))
        afterInput()
    }

    // MARK: - Focus

    /// Takes the keyboard focus (`chain([("focus", nil), …])`), showing the keyboard.
    public func focusEditor() {
        if !isFirstResponder {
            _ = becomeFirstResponder()
        }
    }

    public override var canBecomeFirstResponder: Bool { true }

    public override func becomeFirstResponder() -> Bool {
        let became = super.becomeFirstResponder()
        if became {
            engine.editor.setFocused(focused: true)
            afterInput()
        }
        return became
    }

    public override func resignFirstResponder() -> Bool {
        let resigned = super.resignFirstResponder()
        if resigned {
            engine.editor.setFocused(focused: false)
            afterInput()
        }
        return resigned
    }

    // MARK: - Touch

    private var finger: UITouch?

    private func touch(_ action: Int32, _ touch: UITouch) {
        let at = touch.location(in: self)
        engine.editor.touch(action: action, x: Float(at.x), y: Float(at.y))
        afterInput()
    }

    public override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard finger == nil, let first = touches.first else { return }
        finger = first
        touch(0, first)
    }

    public override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let finger, touches.contains(finger) else { return }
        touch(1, finger)
    }

    public override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let finger, touches.contains(finger) else { return }
        self.finger = nil
        touch(2, finger)
    }

    public override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let finger, touches.contains(finger) else { return }
        self.finger = nil
        touch(3, finger)
    }

    // MARK: - Hardware keyboards

    public override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var handled = false
        if markedTextRange == nil {
            for press in presses {
                guard let key = press.key, let name = Self.keyName(key) else { continue }
                let flags = Self.modifiers(key.modifierFlags)
                let command = flags.contains(.command) || flags.contains(.control)
                if (command || name.count > 1) && engine.editor.keyDown(key: name, modifiers: flags.rawValue) {
                    handled = true
                }
            }
        }
        if handled {
            afterInput()
        } else {
            super.pressesBegan(presses, with: event)
        }
    }

    static func keyName(_ key: UIKey) -> String? {
        switch key.keyCode {
        case .keyboardReturnOrEnter, .keypadEnter: return "Enter"
        case .keyboardTab: return "Tab"
        case .keyboardDeleteOrBackspace: return "Backspace"
        case .keyboardDeleteForward: return "Delete"
        case .keyboardEscape: return "Escape"
        case .keyboardHome: return "Home"
        case .keyboardEnd: return "End"
        case .keyboardPageUp: return "PageUp"
        case .keyboardPageDown: return "PageDown"
        case .keyboardLeftArrow: return "ArrowLeft"
        case .keyboardRightArrow: return "ArrowRight"
        case .keyboardUpArrow: return "ArrowUp"
        case .keyboardDownArrow: return "ArrowDown"
        default:
            let characters = key.charactersIgnoringModifiers
            return characters.isEmpty ? nil : characters.lowercased()
        }
    }

    static func modifiers(_ flags: UIKeyModifierFlags) -> EditorModifiers {
        var result: EditorModifiers = []
        if flags.contains(.shift) { result.insert(.shift) }
        if flags.contains(.control) { result.insert(.control) }
        if flags.contains(.alternate) { result.insert(.option) }
        if flags.contains(.command) { result.insert(.command) }
        return result
    }

    // MARK: - Edit actions

    public override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        let empty = engine.editor.selection().contains("\"empty\":true")
        switch action {
        case #selector(copy(_:)): return !empty
        case #selector(cut(_:)): return !empty && engine.editor.isEditable()
        case #selector(paste(_:)): return engine.editor.isEditable() && UIPasteboard.general.hasStrings
        case #selector(selectAll(_:)): return true
        default: return super.canPerformAction(action, withSender: sender)
        }
    }

    public override func copy(_ sender: Any?) {
        menuAction("copy")
    }

    public override func cut(_ sender: Any?) {
        menuAction("cut")
    }

    public override func paste(_ sender: Any?) {
        menuAction("paste")
    }

    public override func selectAll(_ sender: Any?) {
        menuAction("selectAll")
    }

    private func menuAction(_ item: String) {
        var text: String?
        var html: String?
        if item == "paste" {
            let board = UIPasteboard.general
            html = board.value(forPasteboardType: "public.html") as? String
            text = board.string
        }
        let copied = engine.editor.menuAction(item: item, text: text, html: html)
        if !copied.isEmpty { write(copied) }
        afterInput()
    }

    private func write(_ json: String) {
        guard let data = json.data(using: .utf8),
              let content = try? JSONSerialization.jsonObject(with: data) as? [String: String]
        else { return }
        write(text: content["text"] ?? "", html: content["html"] ?? "")
    }

    private func write(text: String, html: String) {
        guard !text.isEmpty || !html.isEmpty else { return }
        var item: [String: Any] = ["public.utf8-plain-text": text]
        if !html.isEmpty { item["public.html"] = html }
        UIPasteboard.general.items = [item]
    }

    // MARK: - Edit menu

    public func editMenuInteraction(
        _ interaction: UIEditMenuInteraction, menuFor configuration: UIEditMenuConfiguration,
        suggestedActions: [UIMenuElement]
    ) -> UIMenu? {
        guard let request = menuRequest else { return nil }
        let actions = request.items.map { item in
            UIAction(title: menuLabel(item)) { [weak self] _ in self?.menuAction(item) }
        }
        return UIMenu(children: actions)
    }

    public func editMenuInteraction(
        _ interaction: UIEditMenuInteraction, targetRectFor configuration: UIEditMenuConfiguration
    ) -> CGRect {
        menuRequest?.rect ?? .zero
    }

    public func editMenuInteraction(
        _ interaction: UIEditMenuInteraction, willDismissMenuFor configuration: UIEditMenuConfiguration,
        animator: UIEditMenuInteractionAnimating
    ) {
        guard menuRequest != nil else { return }
        menuRequest = nil
        engine.editor.menuDismissed()
        afterInput()
    }

    // MARK: - Effects

    private func perform(_ effects: [[String: Any]]) {
        for effect in effects {
            switch effect["type"] as? String {
            case "menu":
                showMenu(effect["menu"] as? [String: Any])
            case "magnifier":
                magnify(at: effect["at"] as? [NSNumber])
            case "showKeyboard":
                if !isFirstResponder { _ = becomeFirstResponder() }
            case "haptic":
                if effect["kind"] as? String == "longPress" {
                    UIImpactFeedbackGenerator(style: .medium).impactOccurred()
                } else {
                    UISelectionFeedbackGenerator().selectionChanged()
                }
            case "limitReached":
                UINotificationFeedbackGenerator().notificationOccurred(.error)
            case "clipboard":
                write(text: effect["text"] as? String ?? "", html: effect["html"] as? String ?? "")
            case "openUrl":
                if let url = (effect["url"] as? String).flatMap({ URL(string: $0, relativeTo: images.baseURL) }) {
                    UIApplication.shared.open(url)
                }
            case "missingImages":
                images.load(effect["sources"] as? [String] ?? [])
            case "pickImage":
                onPickImage?()
            default:
                break
            }
        }
    }

    private func showMenu(_ menu: [String: Any]?) {
        guard let editMenu else { return }
        guard let menu, let rect = effectRect(menu["rect"]), let items = menu["items"] as? [String], !items.isEmpty
        else {
            if menuRequest != nil {
                menuRequest = nil
                editMenu.dismissMenu()
            }
            return
        }
        menuRequest = (rect, items)
        let configuration = UIEditMenuConfiguration(identifier: nil, sourcePoint: CGPoint(x: rect.midX, y: rect.minY))
        editMenu.presentEditMenu(with: configuration)
    }

    private func magnify(at point: [NSNumber]?) {
        guard let point, point.count == 2 else {
            loupe?.invalidate()
            loupe = nil
            return
        }
        let at = CGPoint(x: point[0].doubleValue, y: point[1].doubleValue)
        let caret = engine.imeState().caret ?? CGRect(x: at.x, y: at.y - 10, width: 2, height: 20)
        if let loupe {
            loupe.move(to: at, withCaretRect: caret, trackingCaret: true)
        } else {
            loupe = UITextLoupeSession.begin(at: at, fromSelectionWidgetView: nil, in: self)
        }
    }

    // MARK: - UIKeyInput

    public var hasText: Bool { !engine.editor.isEmpty() }

    public func insertText(_ text: String) {
        keyboardDriven {
            if text == "\n" {
                _ = engine.editor.keyDown(key: "Enter", modifiers: 0)
            } else if let marked = engine.imeState().composition {
                _ = engine.editor.replaceText(
                    start: Int32(marked.location), end: Int32(NSMaxRange(marked)), text: text, composing: false)
            } else {
                _ = engine.editor.insertText(text: text)
            }
        }
    }

    public func deleteBackward() {
        keyboardDriven {
            _ = engine.editor.keyDown(key: "Backspace", modifiers: 0)
        }
    }

    // MARK: - UITextInput: text and selection (offsets in the IME block)

    public weak var inputDelegate: UITextInputDelegate?
    public var markedTextStyle: [NSAttributedString.Key: Any]?
    public lazy var tokenizer: UITextInputTokenizer = UITextInputStringTokenizer(textInput: self)

    public var autocorrectionType: UITextAutocorrectionType = .default
    public var autocapitalizationType: UITextAutocapitalizationType = .sentences
    /// A keyboard for addresses while the link field takes input.
    public var keyboardType: UIKeyboardType {
        get { engine.imeState().start == -2 ? .URL : .default }
        set {}
    }
    public var returnKeyType: UIReturnKeyType {
        get { engine.imeState().start == -2 ? .go : .default }
        set {}
    }

    public func text(in range: UITextRange) -> String? {
        guard let range = range as? EditorTextRange else { return nil }
        let text = engine.imeState().text as NSString
        let clipped = NSIntersectionRange(range.nsRange, NSRange(location: 0, length: text.length))
        return text.substring(with: clipped)
    }

    public func replace(_ range: UITextRange, withText text: String) {
        guard let range = range as? EditorTextRange else { return }
        keyboardDriven {
            _ = engine.editor.replaceText(
                start: Int32(range.start.offset), end: Int32(range.end.offset), text: text, composing: false)
        }
    }

    public var selectedTextRange: UITextRange? {
        get {
            let state = engine.imeState()
            guard state.start != -1 else { return nil }
            return EditorTextRange(state.selectedRange)
        }
        set {
            guard let range = newValue as? EditorTextRange else { return }
            keyboardDriven {
                engine.editor.setSelection(anchor: Int32(range.start.offset), head: Int32(range.end.offset))
            }
        }
    }

    public var markedTextRange: UITextRange? {
        engine.imeState().composition.map(EditorTextRange.init)
    }

    public func setMarkedText(_ markedText: String?, selectedRange: NSRange) {
        let text = markedText ?? ""
        keyboardDriven {
            let state = engine.imeState()
            if let marked = state.composition {
                _ = engine.editor.replaceText(
                    start: Int32(marked.location), end: Int32(NSMaxRange(marked)), text: text, composing: true)
            } else {
                engine.editor.setMarkedText(text: text)
            }
        }
    }

    public func unmarkText() {
        keyboardDriven {
            engine.editor.unmarkText()
        }
    }

    public var beginningOfDocument: UITextPosition { EditorTextPosition(0) }

    public var endOfDocument: UITextPosition { EditorTextPosition(engine.imeState().length) }

    public func textRange(from fromPosition: UITextPosition, to toPosition: UITextPosition) -> UITextRange? {
        guard let from = fromPosition as? EditorTextPosition, let to = toPosition as? EditorTextPosition else {
            return nil
        }
        return EditorTextRange(from: from.offset, to: to.offset)
    }

    public func position(from position: UITextPosition, offset: Int) -> UITextPosition? {
        guard let position = position as? EditorTextPosition else { return nil }
        let target = position.offset + offset
        guard target >= 0, target <= engine.imeState().length else { return nil }
        return EditorTextPosition(target)
    }

    public func position(
        from position: UITextPosition, in direction: UITextLayoutDirection, offset: Int
    ) -> UITextPosition? {
        switch direction {
        case .left, .up: return self.position(from: position, offset: -offset)
        default: return self.position(from: position, offset: offset)
        }
    }

    public func compare(_ position: UITextPosition, to other: UITextPosition) -> ComparisonResult {
        let a = (position as? EditorTextPosition)?.offset ?? 0
        let b = (other as? EditorTextPosition)?.offset ?? 0
        return a < b ? .orderedAscending : a > b ? .orderedDescending : .orderedSame
    }

    public func offset(from: UITextPosition, to toPosition: UITextPosition) -> Int {
        ((toPosition as? EditorTextPosition)?.offset ?? 0) - ((from as? EditorTextPosition)?.offset ?? 0)
    }

    public func position(within range: UITextRange, farthestIn direction: UITextLayoutDirection) -> UITextPosition? {
        guard let range = range as? EditorTextRange else { return nil }
        switch direction {
        case .left, .up: return range.start
        default: return range.end
        }
    }

    public func characterRange(byExtending position: UITextPosition, in direction: UITextLayoutDirection) -> UITextRange? {
        guard let position = position as? EditorTextPosition else { return nil }
        switch direction {
        case .left, .up: return EditorTextRange(from: max(0, position.offset - 1), to: position.offset)
        default: return EditorTextRange(from: position.offset, to: min(engine.imeState().length, position.offset + 1))
        }
    }

    public func baseWritingDirection(for position: UITextPosition, in direction: UITextStorageDirection) -> NSWritingDirection {
        .natural
    }

    public func setBaseWritingDirection(_ writingDirection: NSWritingDirection, for range: UITextRange) {}

    // MARK: - UITextInput: geometry (points in this view)

    public func firstRect(for range: UITextRange) -> CGRect {
        guard let range = range as? EditorTextRange else { return .zero }
        return Self.firstLineRect(engine.editor, range.nsRange) ?? .zero
    }

    public func caretRect(for position: UITextPosition) -> CGRect {
        guard let position = position as? EditorTextPosition else { return .zero }
        let rect = Self.firstLineRect(engine.editor, NSRange(location: position.offset, length: 0))
        return rect.map { CGRect(x: $0.minX, y: $0.minY, width: 2, height: $0.height) } ?? .zero
    }

    public func selectionRects(for range: UITextRange) -> [UITextSelectionRect] {
        // The core draws the selection and its handles.
        []
    }

    public func closestPosition(to point: CGPoint) -> UITextPosition? {
        let offset = engine.editor.imeOffsetAt(x: Float(point.x), y: Float(point.y))
        return offset < 0 ? nil : EditorTextPosition(Int(offset))
    }

    public func closestPosition(to point: CGPoint, within range: UITextRange) -> UITextPosition? {
        guard let range = range as? EditorTextRange,
              let position = closestPosition(to: point) as? EditorTextPosition
        else { return nil }
        return EditorTextPosition(min(max(position.offset, range.start.offset), range.end.offset))
    }

    public func characterRange(at point: CGPoint) -> UITextRange? {
        guard let position = closestPosition(to: point) as? EditorTextPosition else { return nil }
        return EditorTextRange(from: position.offset, to: min(position.offset + 1, engine.imeState().length))
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
            if range.length == 0 { rect.size.width = 0 }
            return rect
        }
        if let caret = info["caret"] as? [NSNumber], caret.count == 4 {
            return CGRect(
                x: caret[0].doubleValue, y: caret[1].doubleValue, width: 0,
                height: caret[3].doubleValue - caret[1].doubleValue)
        }
        return nil
    }
}

/// An offset in the IME block (UTF-16 units).
final class EditorTextPosition: UITextPosition {
    let offset: Int

    init(_ offset: Int) {
        self.offset = offset
    }
}

final class EditorTextRange: UITextRange {
    private let from: EditorTextPosition
    private let to: EditorTextPosition

    init(from: Int, to: Int) {
        self.from = EditorTextPosition(min(from, to))
        self.to = EditorTextPosition(max(from, to))
    }

    convenience init(_ range: NSRange) {
        self.init(from: range.location, to: NSMaxRange(range))
    }

    override var start: EditorTextPosition { from }
    override var end: EditorTextPosition { to }
    override var isEmpty: Bool { from.offset == to.offset }

    var nsRange: NSRange {
        NSRange(location: from.offset, length: to.offset - from.offset)
    }
}
#endif
