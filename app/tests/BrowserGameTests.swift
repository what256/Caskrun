import AppKit
import Foundation

@main
struct BrowserGameTests {
    static func main() throws {
        var checks = 0
        func expect(_ value: Bool, _ message: String) {
            precondition(value, message)
            checks += 1
        }
        expect(BrowserGame.checkedURL("https://games.example.org/")?.host == "games.example.org", "ordinary HTTPS game")
        expect(BrowserGame.checkedURL(" https://Games.EXAMPLE.org/play?q=one%20two#level ") != nil, "trimmed link and encoded spaces")
        for bad in ["file:///game", "http://games.example.org/", "javascript:alert(1)", "https://" + "name:secret@games.example.org/", "https://games.example.org:443/", "https://127.0.0.1/", "https://[::1]/", "https://localhost/", "https://game.local/", "https://game.test/", "https://game.example/", "https://games.example.org./", "https://" + "games.example.org\\@evil.org/", "https://games.example.org/%00", "https://games.example.org/%0a", "https://games.example.org/%5c", "https://games.example.org/%", "https://games.example.org/a b", "https://gáme.example.org/", "https://games.example.org/\"bad\""] {
            expect(BrowserGame.checkedURL(bad) == nil, "unsafe link refused: \(bad)")
        }
        expect(BrowserGame.checkedURL("https://games.example.org/" + String(repeating: "x", count: 2048)) == nil, "bounded URL")
        for suffix in ["?token=private", "?ACCESS_TOKEN=private", "?%74%6f%6b%65%6e=private", "?access%5ftoken=private", "?session[id]=private", "?email=person", "?api-key=private", "?auth_token=private", "?AUTH%5fTOKEN=private", "?bearer_token=private", "?authorization_code=private", "?code=private", "?client_secret=private", "?jwt=private", "?oauth_token=private", "?oauth_token_secret=private", "?oauth_verifier=private", "#id_token=private", "#auth[token]=private", "#client-secret=private", "#/play?authorization=private", "?game=1;Password=private"] {
            expect(BrowserGame.checkedURL("https://games.example.org/" + suffix) == nil, "account link refused: \(suffix)")
        }
        let game = try JSONDecoder().decode(BrowserGame.self, from: Data(#"{"id":"example","name":"Browser Game","url":"https://games.example.org/","created_at":1}"#.utf8))
        expect(game.name == "Browser Game" && game.created_at == 1, "backend DTO decode")
        expect(LibraryScope.allCases.map(\.title) == ["All", "Steam", "Epic", "GOG", "Apps", "Browser"], "compact scopes")
        checks += libraryInteractionChecks()
        print("\(checks) browser game and Library interaction checks passed")
    }

    // Run with:
    // swiftc -target arm64-apple-macos14.0 -parse-as-library app/GameLibraryViews.swift app/GameCompatibility.swift app/tests/BrowserGameTests.swift -o .build/browser-game-tests
    // .build/browser-game-tests
    private static func libraryInteractionChecks() -> Int {
        var checks = 0
        func expect(_ value: Bool, _ message: String) {
            precondition(value, message)
            checks += 1
        }
        func remembered() -> LibraryClickSequence {
            var sequence = LibraryClickSequence()
            sequence.remember(id: "original", windowNumber: 7, point: NSPoint(x: 100, y: 100), timestamp: 10)
            return sequence
        }
        func down(_ sequence: inout LibraryClickSequence, window: Int = 7, x: CGFloat = 100,
                  timestamp: TimeInterval = 10.2, count: Int = 2) -> LibraryClickSequence.Outcome {
            sequence.route(type: .leftMouseDown, windowNumber: window, point: NSPoint(x: x, y: 100),
                           timestamp: timestamp, clickCount: count, doubleClickInterval: 0.5)
        }
        var sequence = remembered()
        expect(sequence.hasTarget, "first click is remembered immediately")
        expect(down(&sequence) == .open("original"), "reflow cannot replace the original tile identity")
        expect(!sequence.hasTarget, "launch consumes the target")
        expect(sequence.route(type: .leftMouseUp, windowNumber: 7, point: .zero, timestamp: 10.21,
                              clickCount: 2, doubleClickInterval: 0.5) == .consume,
               "second mouse-up cannot activate a reflowed control")
        expect(sequence.route(type: .leftMouseUp, windowNumber: 7, point: .zero, timestamp: 10.22,
                              clickCount: 2, doubleClickInterval: 0.5) == .pass,
               "mouse-up is consumed once")
        for (window, x, timestamp, count, message): (Int, CGFloat, Double, Int, String) in [
            (8, 100, 10.2, 2, "different window"), (7, 100, 10.501, 2, "expired interval"),
            (7, 100, 9.9, 2, "negative interval"), (7, 108.01, 10.2, 2, "moved pointer"),
            (7, 100, 10.2, 1, "new single click"), (7, 100, 10.2, 3, "third click")
        ] {
            sequence = remembered()
            expect(down(&sequence, window: window, x: x, timestamp: timestamp, count: count) == .pass,
                   "does not open after \(message)")
            expect(!sequence.hasTarget, "clears target after \(message)")
        }
        sequence = remembered()
        expect(down(&sequence, x: 108, timestamp: 10.5) == .open("original"), "system interval and pointer tolerance boundaries")
        sequence.cancel()
        expect(sequence.route(type: .leftMouseUp, windowNumber: 7, point: .zero, timestamp: 10.51,
                              clickCount: 2, doubleClickInterval: 0.5) == .consume,
               "navigation after launch still swallows the matching mouse-up")
        for type in [NSEvent.EventType.rightMouseDown, .scrollWheel, .keyDown] {
            sequence = remembered()
            expect(sequence.route(type: type, windowNumber: 7, point: .zero, timestamp: 10.1,
                                  clickCount: 0, doubleClickInterval: 0.5) == .pass, "interruption reaches its control")
            expect(down(&sequence) == .pass, "interruption cancels pending launch")
        }
        sequence = remembered()
        sequence.cancel()
        expect(down(&sequence) == .pass, "scope/query/disappearance cancellation prevents launch")
        sequence = remembered()
        expect(sequence.route(type: .leftMouseDown, windowNumber: 7, point: NSPoint(x: 100, y: 100), timestamp: 10.2,
                              clickCount: 2, doubleClickInterval: 0.5, isBlocked: true) == .pass,
               "modal or attached sheet receives its own double-click")
        expect(!sequence.hasTarget, "modal presentation cancels pending Library launch")
        sequence = remembered()
        _ = down(&sequence)
        expect(sequence.route(type: .leftMouseUp, windowNumber: 8, point: .zero, timestamp: 10.21,
                              clickCount: 2, doubleClickInterval: 0.5) == .pass,
               "another window's mouse-up is never swallowed")
        expect(sequence.route(type: .leftMouseUp, windowNumber: 7, point: .zero, timestamp: 10.22,
                              clickCount: 2, doubleClickInterval: 0.5, isBlocked: true) == .pass,
               "new modal mouse-up is never swallowed")
        for source in [LibraryScope.steam, .epic, .gog, .apps, .browser] {
            let item = GameLibraryEntry(id: "sample", name: "Sample", source: source, subtitle: "")
            let action = source == .browser ? "Play in browser" : source == .apps ? "Open app" : (source == .epic || source == .gog) ? "Open store" : "Play"
            expect(item.actionTitle == action, "shared source-specific action title")
            if source == .epic || source == .gog {
                expect(item.cardHelp.contains("open its store") && !item.cardHelp.contains("to play"), "publisher tooltip describes store routing")
            }
            var unavailable = item
            unavailable.canPlay = false
            expect(!unavailable.cardHelp.contains("Double-click"), "unavailable action is not advertised")
        }
        return checks
    }
}
