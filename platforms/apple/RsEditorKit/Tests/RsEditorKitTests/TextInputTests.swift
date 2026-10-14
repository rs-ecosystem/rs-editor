#if os(macOS)
import AppKit
import XCTest
@testable import RsEditorKit

/// The view as input methods and VoiceOver see it, without a keyboard or a
/// screen reader: the calls AppKit makes, made directly.
@MainActor
final class TextInputTests: XCTestCase {
    private func editor(_ html: String) -> (RsEditorView, NSWindow) {
        let view = RsEditorView(content: html)
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 600, height: 400),
            styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = view
        window.makeFirstResponder(view)
        return (view, window)
    }

    func testJapaneseInputComposesThenCommits() {
        let (view, window) = editor("<p>halo</p>")
        defer { window.close() }
        view.run("setTextSelection", ["from": 5, "to": 5])
        // The input method marks "にほ", then "にほん", then commits "日本".
        view.setMarkedText("にほ", selectedRange: NSRange(location: 2, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        XCTAssertTrue(view.hasMarkedText())
        XCTAssertEqual(view.markedRange(), NSRange(location: 4, length: 2))
        view.setMarkedText("にほん", selectedRange: NSRange(location: 3, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        XCTAssertEqual(view.markedRange(), NSRange(location: 4, length: 3))
        XCTAssertEqual(view.text, "haloにほん")
        view.insertText("日本", replacementRange: NSRange(location: NSNotFound, length: 0))
        XCTAssertFalse(view.hasMarkedText())
        XCTAssertEqual(view.text, "halo日本")
        XCTAssertEqual(view.selectedRange(), NSRange(location: 6, length: 0))
        // One undo step takes the composed word away.
        view.run("undo")
        XCTAssertEqual(view.text, "halo")
    }

    func testReconversionReplacesARange() {
        let (view, window) = editor("<p>halo dunia</p>")
        defer { window.close() }
        // Reconversion (or the Mac's text replacement) names the range to replace.
        view.insertText("bumi", replacementRange: NSRange(location: 5, length: 5))
        XCTAssertEqual(view.text, "halo bumi")
        let sub = view.attributedSubstring(forProposedRange: NSRange(location: 0, length: 4), actualRange: nil)
        XCTAssertEqual(sub?.string, "halo")
    }

    func testCandidateWindowSitsAtTheCaret() {
        let (view, window) = editor("<p>halo</p>")
        defer { window.close() }
        view.run("setTextSelection", ["from": 3, "to": 3])
        let rect = view.firstRect(forCharacterRange: NSRange(location: 2, length: 0), actualRange: nil)
        XCTAssertGreaterThan(rect.height, 10, "a line tall")
        let screen = window.convertToScreen(view.convert(view.bounds, to: nil))
        XCTAssertTrue(screen.insetBy(dx: -1, dy: -1).contains(rect.origin), "inside the view on screen")
        // characterIndex(for:) finds the same place again.
        let index = view.characterIndex(for: NSPoint(x: rect.minX + 1, y: rect.midY))
        XCTAssertEqual(index, 2)
    }

    func testCommandKeysAndEmacsBindings() {
        let (view, window) = editor("<p>halo dunia</p>")
        defer { window.close() }
        view.run("setTextSelection", ["from": 6, "to": 6])
        // Control-A: the start of the line, through doCommand(by:).
        view.doCommand(by: #selector(NSResponder.moveToBeginningOfLine(_:)))
        XCTAssertEqual(view.selectedRange(), NSRange(location: 0, length: 0))
        view.doCommand(by: #selector(NSResponder.insertNewline(_:)))
        XCTAssertEqual(view.json.components(separatedBy: "\"paragraph\"").count - 1, 2)
    }

    func testVoiceOverSeesTheDocument() {
        let (view, window) = editor("<h1>Judul</h1><p>halo</p>")
        defer { window.close() }
        window.makeKeyAndOrderFront(nil)
        view.display()
        let children = view.accessibilityChildren() ?? []
        XCTAssertFalse(children.isEmpty, "AccessKit answers for the view")
        let area = children.first as? NSAccessibilityProtocol
        XCTAssertEqual(area?.accessibilityRole(), .textArea)
        // VoiceOver reads a multiline text area through its ranges.
        let count = area?.accessibilityNumberOfCharacters() ?? 0
        let text = area?.accessibilityString(for: NSRange(location: 0, length: count)) ?? ""
        XCTAssertTrue(text.contains("Judul") && text.contains("halo"), text)
        XCTAssertEqual(area?.accessibilityRange(forLine: 0).location, 0)
        let heading = area?.accessibilityChildren()?.first as? NSAccessibilityProtocol
        XCTAssertEqual(heading?.accessibilityRole()?.rawValue, "Heading")
    }

    func testDroppedImagesAreStoredAndInserted() async throws {
        let (view, window) = editor("<p>halo</p>")
        defer { window.close() }
        var stored: Data?
        view.storeImage = { data in
            stored = data
            return "https://example.com/foto.png"
        }
        let png = try XCTUnwrap(NSImage(size: NSSize(width: 4, height: 4), flipped: false) { rect in
            NSColor.red.setFill()
            rect.fill()
            return true
        }.tiffRepresentation)
        view.insertImage(data: png)
        for _ in 0..<50 where !view.json.contains("foto.png") {
            try await Task.sleep(nanoseconds: 20_000_000)
        }
        XCTAssertNotNil(stored)
        XCTAssertTrue(view.json.contains("https://example.com/foto.png"), view.json)
    }
}
#endif
