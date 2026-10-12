import SwiftUI
import WebKit

struct OpenRichWebView: NSViewRepresentable {
    let address: String

    func makeNSView(context: Context) -> WKWebView {
        let view = WKWebView(frame: .zero)
        view.setValue(false, forKey: "drawsBackground")
        if let url = URL(string: address) {
            view.load(URLRequest(url: url))
        }
        return view
    }

    func updateNSView(_ view: WKWebView, context: Context) {}
}

@MainActor
final class OpenRichAppDelegate: NSObject, NSApplicationDelegate {
    var serverProcess: Process?

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        if let process = serverProcess, process.isRunning { process.terminate() }
    }
}

@main
struct OpenRichDemoApp: App {
    @NSApplicationDelegateAdaptor(OpenRichAppDelegate.self) private var appDelegate
    @State private var server: Process
    private let address = "http://127.0.0.1:4173"

    init() {
        let arguments = CommandLine.arguments
        guard arguments.count >= 2 else {
            fatalError("Expected the openrich-demo server executable path as argument 1")
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: arguments[1])
        process.arguments = ["127.0.0.1:4173"]
        do {
            try process.run()
        } catch {
            fatalError("Could not start the OpenRich local engine: \(error)")
        }
        Thread.sleep(forTimeInterval: 0.25)
        _server = State(initialValue: process)
        appDelegate.serverProcess = process
    }

    var body: some Scene {
        WindowGroup("OpenRich Editor") {
            OpenRichWebView(address: address)
                .frame(minWidth: 760, minHeight: 560)
                .onDisappear {
                    if server.isRunning { server.terminate() }
                }
        }
        .defaultSize(width: 1000, height: 740)
    }
}
