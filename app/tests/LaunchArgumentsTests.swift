import Foundation

@main
enum LaunchArgumentsTests {
    static func main() {
        precondition(LaunchArgumentDraft.parse(" leading \n\ntrailing \r\n\"quoted\"\n ") == [" leading ", "trailing ", "\"quoted\"", " "])
        precondition(LaunchArgumentDraft.parse("").isEmpty)
        precondition(LaunchArgumentDraft.validation(["two words", "$(literal)", "`literal`"]) == nil)
        precondition(LaunchArgumentDraft.validation(["bad\0value"]) != nil)
        precondition(LaunchArgumentDraft.validation(Array(repeating: "x", count: 64)) == nil)
        precondition(LaunchArgumentDraft.validation(["x"], saved: Array(repeating: "x", count: 64)) != nil)
        precondition(LaunchArgumentDraft.validation([String(repeating: "é", count: 2048)]) == nil)
        precondition(LaunchArgumentDraft.validation([String(repeating: "é", count: 2049)]) != nil)
        precondition(LaunchArgumentDraft.validation(Array(repeating: String(repeating: "x", count: 4096), count: 4)) == nil)
        precondition(LaunchArgumentDraft.validation(["x"], saved: Array(repeating: String(repeating: "x", count: 4096), count: 4)) != nil)
        print("10 literal argument and boundary checks passed.")
    }
}
