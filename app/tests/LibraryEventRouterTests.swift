import AppKit
import CoreGraphics
import Foundation

@main
struct LibraryEventRouterTests {
    @MainActor
    static func main() throws {
        // Local event fixtures only: no window, event posting, screen capture or
        // Windows process. Exercise the actual router rather than its pure
        // sequence, which cannot detect a mouse-only AppKit property access.
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let router = LibraryClickRouter()
        var checks = 0
        func expect(_ value: Bool, _ message: String) {
            precondition(value, message)
            checks += 1
        }
        guard let key = NSEvent.keyEvent(with: .keyDown, location: .zero,
                                         modifierFlags: [], timestamp: 1, windowNumber: 0,
                                         context: nil, characters: "a", charactersIgnoringModifiers: "a",
                                         isARepeat: false, keyCode: 0),
              let scrollCG = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1,
                                     wheel1: -12, wheel2: 0, wheel3: 0),
              let scroll = NSEvent(cgEvent: scrollCG) else {
            fatalError("Could not create local AppKit event fixtures")
        }
        expect(key.type == .keyDown, "real key fixture has its non-mouse type")
        expect(router.route(key) === key, "key event passes through the actual router without mouse-property access")
        expect(scroll.type == .scrollWheel, "real scroll fixture has its non-mouse type")
        expect(router.route(scroll) === scroll, "wheel event reaches its original control without an AppKit exception")
        expect(scroll.scrollingDeltaY != 0, "passed wheel retains a nonzero native scroll delta")
        expect(router.route(scroll) === scroll, "repeated wheel events remain unconsumed")
        print("\(checks) real AppKit event-router checks passed")
    }
}
