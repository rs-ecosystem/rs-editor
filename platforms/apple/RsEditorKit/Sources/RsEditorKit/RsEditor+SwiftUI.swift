import SwiftUI

/// The editor's state for SwiftUI: owns the view, and publishes `revision`
/// so toolbars refresh when the document or the selection changes.
@MainActor
public final class RsEditorState: ObservableObject {
    public let view: RsEditorView
    @Published public private(set) var revision: UInt64 = 0

    public init(content: String = "", markdown: Bool = false, characterLimit: Int = 0) {
        view = RsEditorView(content: content, markdown: markdown, characterLimit: characterLimit)
        view.onChange = { [weak self] in
            guard let self else { return }
            self.revision = self.view.revision
        }
    }

    public func isActive(_ name: String, _ attrs: [String: Any] = [:]) -> Bool {
        view.isActive(name, attrs)
    }

    public func can(_ name: String, _ args: Any? = nil) -> Bool {
        view.can(name, args)
    }

    /// Runs one command, keeping the editor focused (as a toolbar button should).
    @discardableResult
    public func run(_ name: String, _ args: Any? = nil) -> Bool {
        view.chain([("focus", nil), (name, args)])
    }
}

#if os(macOS)
/// The editor in SwiftUI.
public struct RsEditor: NSViewRepresentable {
    @ObservedObject var state: RsEditorState

    public init(_ state: RsEditorState) {
        self.state = state
    }

    public func makeNSView(context: Context) -> RsEditorView { state.view }
    public func updateNSView(_ view: RsEditorView, context: Context) {}
}
#else
/// The editor in SwiftUI.
public struct RsEditor: UIViewRepresentable {
    @ObservedObject var state: RsEditorState

    public init(_ state: RsEditorState) {
        self.state = state
    }

    public func makeUIView(context: Context) -> RsEditorView { state.view }
    public func updateUIView(_ view: RsEditorView, context: Context) {}
}
#endif
