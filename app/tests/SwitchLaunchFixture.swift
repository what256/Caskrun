// Original disposable app used only to verify Launch Services and shutdown.
import AppKit
import Foundation
@main struct SwitchLaunchFixture {
    static func main() {
        guard CommandLine.arguments.count == 2 else { return }
        let game = URL(fileURLWithPath: CommandLine.arguments[1])
        let record = game.deletingLastPathComponent().appendingPathComponent("launch-arguments.json")
        try? JSONEncoder().encode(CommandLine.arguments).write(to: record, options: .atomic)
        NSApplication.shared.setActivationPolicy(.accessory)
        NSApplication.shared.run()
    }
}
