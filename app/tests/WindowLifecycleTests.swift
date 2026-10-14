import AppKit
import Foundation

@MainActor
private final class OriginalWindowDelegate: NSObject, NSWindowDelegate {
    var closeRequests = 0
    var closed = 0
    var resized = 0

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        closeRequests += 1
        return true
    }

    func windowWillClose(_ notification: Notification) { closed += 1 }
    func windowDidResize(_ notification: Notification) { resized += 1 }
}

@MainActor
private final class ApplicationQuitDelegate: NSObject, NSApplicationDelegate {
    var quitRequests = 0
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        quitRequests += 1
        return .terminateCancel // Keep this isolated test executable alive.
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
}

@main
struct WindowLifecycleTests {
    @MainActor
    static func main() throws {
        let app = NSApplication.shared
        app.setActivationPolicy(.prohibited)
        let appDelegate = ApplicationQuitDelegate()
        app.delegate = appDelegate
        var checks = 0
        func expect(_ condition: @autoclosure () -> Bool, _ message: String) {
            precondition(condition(), message)
            checks += 1
        }

        let window = NSWindow(contentRect: NSRect(x: -10000, y: -10000, width: 100, height: 100),
                              styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let original = OriginalWindowDelegate()
        window.delegate = original
        var hideRequests = 0
        let proxy = CaskrunCloseToMenuBarDelegate(window: window, previousDelegate: original) { target in
            expect(target === window, "only the captured main window is hidden")
            hideRequests += 1
            target.orderOut(nil)
        }
        proxy.attach(to: window)
        proxy.attach(to: window)
        expect(window.delegate === proxy, "registration does not stack delegate proxies")

        // A harmless process owned by this test stands in for an independently
        // running session. No publisher, Wine runtime or account is opened.
        let session = Process()
        session.executableURL = URL(fileURLWithPath: "/bin/sleep")
        session.arguments = ["30"]
        try session.run()
        defer {
            if session.isRunning { session.terminate() }
            session.waitUntilExit()
        }
        let sessionPID = session.processIdentifier

        // performClose is the AppKit path used by the traffic-light close
        // control and Close Window command. It must not destroy the scene.
        for _ in 0..<3 {
            window.performClose(nil)
            expect(window.windowNumber != 0, "the native window is retained across repeated closes")
            expect(!window.isVisible, "close hides the window")
            expect(original.closeRequests == 0, "the framework's destructive close callback is not forwarded")
            expect(original.closed == 0, "no window-closed event reaches the framework")
            expect(session.isRunning && session.processIdentifier == sessionPID,
                   "closing the host window preserves the independently running session")
            expect(appDelegate.quitRequests == 0, "window close never requests application Quit")
        }
        expect(hideRequests == 3, "each red-close or Close Window request is handled once")

        expect(proxy.responds(to: #selector(NSWindowDelegate.windowDidResize(_:))),
               "unrelated framework delegate capabilities remain visible")
        window.delegate?.windowDidResize?(Notification(name: NSWindow.didResizeNotification, object: window))
        expect(original.resized == 1, "unrelated framework callbacks are forwarded")

        let replacement = OriginalWindowDelegate()
        window.delegate = replacement
        proxy.attach(to: window)
        window.delegate?.windowDidResize?(Notification(name: NSWindow.didResizeNotification, object: window))
        expect(replacement.resized == 1 && original.resized == 1,
               "reattachment forwards to the framework's current delegate")
        window.performClose(nil)
        expect(hideRequests == 4 && replacement.closeRequests == 0 && replacement.closed == 0,
               "reattachment preserves close interception")

        let unrelated = NSWindow(contentRect: .zero, styleMask: [.titled, .closable], backing: .buffered, defer: false)
        unrelated.isReleasedWhenClosed = false
        let unrelatedDelegate = OriginalWindowDelegate()
        unrelated.delegate = unrelatedDelegate
        unrelated.performClose(nil)
        expect(unrelatedDelegate.closeRequests == 1 && unrelatedDelegate.closed == 1,
               "other windows keep ordinary close behavior")
        expect(hideRequests == 4, "another window cannot hide the main window")

        app.terminate(nil)
        expect(appDelegate.quitRequests == 1, "explicit Quit still reaches application termination")
        expect(hideRequests == 4, "application Quit is not converted into window hiding")

        // AppKit does not ask windowShouldClose on direct close during Quit.
        window.close()
        expect(replacement.closed == 1 && hideRequests == 4,
               "direct framework close still reaches its cleanup callback")
        expect(session.isRunning, "the window delegate never owns session termination")
        print("\(checks) window lifecycle checks passed")
    }
}
