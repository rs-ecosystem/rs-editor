import Foundation
import Metal
import QuartzCore
import RsEditorCore

/// The part of the editor view shared by iOS and macOS: the Rust editor,
/// drawn by Skia into a `CAMetalLayer`, and the frame schedule.
///
/// The platform view forwards input to `editor`, calls `tick` from its
/// display link and acts on the effects the editor reports.
final class EditorEngine {
    let editor: NativeEditor
    /// The layer drawn into: the engine's own (macOS backs its view with it), or the
    /// view's (`useLayer`, iOS).
    private(set) var layer = CAMetalLayer()
    private let device: MTLDevice
    private let queue: MTLCommandQueue
    private var attached = false
    /// A frame is wanted on the next display link tick.
    private(set) var needsFrame = true
    /// When an animation or the caret blink wants the next frame.
    private var deadline: CFTimeInterval?
    /// Called with the effects the editor reported (input, frames).
    var onEffects: (([[String: Any]]) -> Void)?
    /// Called when a frame is wanted, to resume a paused display link.
    var wake: (() -> Void)?

    init?(content: String, markdown: Bool, dark: Bool, scale: CGFloat, touch: Bool, characterLimit: Int) {
        guard let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else {
            return nil
        }
        self.device = device
        self.queue = queue
        editor = NativeEditor(
            width: 1, height: 1, scale: Float(scale), dark: dark,
            content: content, markdown: markdown,
            characterLimit: Int32(clamping: characterLimit), touch: touch)
        layer.device = device
        layer.pixelFormat = .bgra8Unorm
        // Skia may read back what it drew (some blend modes).
        layer.framebufferOnly = false
        layer.isOpaque = true
        layer.contentsScale = scale
        layer.presentsWithTransaction = false
    }

    /// Draws into `layer` (a view's own Metal layer) instead.
    func useLayer(_ layer: CAMetalLayer) {
        layer.device = device
        layer.pixelFormat = .bgra8Unorm
        layer.framebufferOnly = false
        layer.isOpaque = true
        layer.contentsScale = self.layer.contentsScale
        self.layer = layer
        needsFrame = true
    }

    /// Starts drawing (the view joined a window).
    func attach() {
        guard !attached else { return }
        attached = editor.attachMetal(device: pointer(device), queue: pointer(queue))
        needsFrame = true
    }

    /// Stops drawing (the view left its window).
    func detach() {
        guard attached else { return }
        editor.detachMetal()
        attached = false
    }

    /// The view's size in points and its pixels per point.
    func resize(_ size: CGSize, scale: CGFloat) {
        let width = max(size.width, 1), height = max(size.height, 1)
        layer.contentsScale = scale
        layer.drawableSize = CGSize(width: (width * scale).rounded(), height: (height * scale).rounded())
        editor.resize(width: Float(width), height: Float(height), scale: Float(scale))
        needsFrame = true
    }

    func setNeedsFrame() {
        needsFrame = true
        wake?()
    }

    /// From the display link: draws when a frame is due. Returns whether
    /// one is still scheduled (the link may pause otherwise).
    @discardableResult
    func tick(now: CFTimeInterval = CACurrentMediaTime()) -> Bool {
        if needsFrame || deadline.map({ now >= $0 }) == true {
            render(now: now)
        }
        return needsFrame || deadline != nil
    }

    private func render(now: CFTimeInterval) {
        needsFrame = false
        deadline = nil
        guard attached, let drawable = layer.nextDrawable() else {
            needsFrame = attached
            return
        }
        let texture = drawable.texture
        if editor.render(texture: pointer(texture), width: Int32(texture.width), height: Int32(texture.height)) {
            // After Skia's work on the same queue.
            let commands = queue.makeCommandBuffer()
            commands?.present(drawable)
            commands?.commit()
        }
        let next = editor.nextFrameIn()
        if next >= 0 {
            deadline = now + Double(next) / 1000
        }
        drainEffects()
    }

    /// After input: draw and hand the effects to the view.
    func afterInput() {
        setNeedsFrame()
        drainEffects()
    }

    func drainEffects() {
        let json = editor.takeEffects()
        guard json != "[]", let data = json.data(using: .utf8),
              let effects = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]],
              !effects.isEmpty
        else { return }
        if effects.contains(where: { $0["type"] as? String == "redraw" }) {
            setNeedsFrame()
        }
        onEffects?(effects)
    }

    /// The IME block (`imeState`), decoded.
    func imeState() -> ImeState {
        ImeState(json: editor.imeState())
    }

    private func pointer(_ object: AnyObject) -> UInt64 {
        UInt64(UInt(bitPattern: Unmanaged.passUnretained(object).toOpaque()))
    }
}

/// The text an input method sees: the textblock around the caret, in
/// UTF-16 offsets (see `NativeEditor.imeState`).
struct ImeState {
    /// Document position of the block; -1 when no text takes input, -2
    /// for the link address field.
    var start = -1
    var text = ""
    var anchor = 0
    var head = 0
    var composition: NSRange?
    /// `[x, y, width, height]` in points.
    var caret: CGRect?

    init(json: String) {
        guard let data = json.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return }
        start = (object["start"] as? NSNumber)?.intValue ?? -1
        text = object["text"] as? String ?? ""
        if let selection = object["selection"] as? [NSNumber], selection.count == 2 {
            anchor = selection[0].intValue
            head = selection[1].intValue
        }
        if let range = object["composition"] as? [NSNumber], range.count == 2 {
            composition = NSRange(location: range[0].intValue, length: range[1].intValue - range[0].intValue)
        }
        if let rect = object["caret"] as? [NSNumber], rect.count == 4 {
            caret = CGRect(
                x: rect[0].doubleValue, y: rect[1].doubleValue,
                width: rect[2].doubleValue, height: rect[3].doubleValue)
        }
    }

    var length: Int { (text as NSString).length }

    var selectedRange: NSRange {
        NSRange(location: min(anchor, head), length: abs(head - anchor))
    }
}

/// Modifier bits of `NativeEditor.pointer` and `keyDown`.
struct EditorModifiers: OptionSet {
    let rawValue: UInt32
    static let shift = EditorModifiers(rawValue: 1)
    static let control = EditorModifiers(rawValue: 2)
    static let option = EditorModifiers(rawValue: 4)
    static let command = EditorModifiers(rawValue: 8)
}

/// Rectangles in effects are `[left, top, right, bottom]` in points.
func effectRect(_ value: Any?) -> CGRect? {
    guard let r = value as? [NSNumber], r.count == 4 else { return nil }
    return CGRect(
        x: r[0].doubleValue, y: r[1].doubleValue,
        width: r[2].doubleValue - r[0].doubleValue, height: r[3].doubleValue - r[1].doubleValue)
}
