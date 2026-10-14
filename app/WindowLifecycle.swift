import AppKit

/// Keep the primary SwiftUI Window scene alive when its red close button or
/// Close Window command is used. A single Window scene otherwise terminates
/// the application when it closes. Explicit application Quit bypasses this
/// delegate method and continues through the app's owned-session shutdown.
final class CaskrunCloseToMenuBarDelegate: NSObject, NSWindowDelegate {
    private weak var window: NSWindow?
    private weak var previousDelegate: NSWindowDelegate?
    private let hideWindow: @MainActor (NSWindow) -> Void

    @MainActor
    init(window: NSWindow, previousDelegate: NSWindowDelegate?,
         hideWindow: @escaping @MainActor (NSWindow) -> Void) {
        self.window = window
        self.previousDelegate = previousDelegate
        self.hideWindow = hideWindow
        super.init()
    }

    @MainActor
    func attach(to window: NSWindow) {
        guard window === self.window else { return }
        if window.delegate !== self {
            previousDelegate = window.delegate
            window.delegate = self
        }
    }

    @MainActor
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard sender === window else {
            return previousDelegate?.windowShouldClose?(sender) ?? true
        }
        // Do not forward this callback to SwiftUI's delegate: closing its
        // primary Window scene is the operation that initiates termination.
        hideWindow(sender)
        return false
    }

    override func responds(to selector: Selector!) -> Bool {
        super.responds(to: selector) || previousDelegate?.responds(to: selector) == true
    }

    override func forwardingTarget(for selector: Selector!) -> Any? {
        if previousDelegate?.responds(to: selector) == true { return previousDelegate }
        return super.forwardingTarget(for: selector)
    }
}
