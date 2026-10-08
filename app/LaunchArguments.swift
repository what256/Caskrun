import Foundation

/// A line is one literal Windows argument. No shell parsing or whitespace stripping.
enum LaunchArgumentDraft {
    static func parse(_ text: String) -> [String] {
        text.components(separatedBy: .newlines).filter { !$0.isEmpty }
    }

    static func validation(_ arguments: [String], saved: [String] = []) -> String? {
        let all = saved + arguments
        if all.count > 64 { return "Use at most 64 saved and temporary arguments together." }
        if all.contains(where: { $0.contains("\0") }) { return "Arguments cannot contain a null character." }
        if all.contains(where: { $0.utf8.count > 4096 }) { return "Each argument must be 4096 bytes or shorter." }
        if all.reduce(0, { $0 + $1.utf8.count }) > 16 * 1024 { return "Arguments must total 16 KiB or less." }
        return nil
    }
}
