import AppKit
import Foundation

@main
struct SwitchCollectionTests {
    @MainActor static func main() throws {
        var checks = 0
        func check(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
            guard try condition() else { throw SwitchCollectionError.invalid("Test failed: " + message) }
            checks += 1
        }
        func refuses(_ message: String, _ body: () throws -> Void) throws {
            do { try body() } catch { checks += 1; return }
            throw SwitchCollectionError.invalid("Test failed: " + message)
        }
        guard let base = ProcessInfo.processInfo.environment["CASKRUN_TEST_ROOT"] else {
            throw SwitchCollectionError.invalid("CASKRUN_TEST_ROOT is required.")
        }
        let root = URL(fileURLWithPath: base, isDirectory: true).appendingPathComponent("switch-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let gamesDir = root.appendingPathComponent("games", isDirectory: true)
        try FileManager.default.createDirectory(at: gamesDir, withIntermediateDirectories: true)
        let game = gamesDir.appendingPathComponent("A Game.nsp")
        try Data([1,2,3]).write(to: game)
        let empty = gamesDir.appendingPathComponent("empty.xci")
        try Data().write(to: empty)
        let invalid = gamesDir.appendingPathComponent("wrong.exe")
        try Data([1]).write(to: invalid)
        let linked = gamesDir.appendingPathComponent("linked.nro")
        try FileManager.default.createSymbolicLink(at: linked, withDestinationURL: game)
        try check(SwitchCollectionStorage.gamePath(game) == game, "game path preserves spaces")
        try refuses("empty game rejected") { _ = try SwitchCollectionStorage.gamePath(empty) }
        try refuses("wrong game type rejected") { _ = try SwitchCollectionStorage.gamePath(invalid) }
        try refuses("symbolic link rejected") { _ = try SwitchCollectionStorage.gamePath(linked) }
        try refuses("unrecognized app rejected") { _ = try SwitchCollectionStorage.enginePath(game) }
        let model = SwitchCollectionModel(storageDirectory: root.appendingPathComponent("data"))
        let entry = try model.importGame(name: "A Game", url: game)
        try check(model.games == [entry], "import persists entry")
        try refuses("duplicate import") { _ = try model.importGame(name: "Duplicate", url: game) }
        let restored = SwitchCollectionModel(storageDirectory: root.appendingPathComponent("data"))
        try check(restored.games == [entry], "private metadata survives reload")
        try restored.remove(entry)
        try check(restored.games.isEmpty, "entry removed")
        try check(FileManager.default.fileExists(atPath: game.path), "removal preserves game")
        try check(try SwitchLaunchPolicy.arguments(gamePath: game.path, existingInstance: false) == [game.path], "single launch argument preserves path")
        try refuses("existing process rejected instead of dropping argument") {
            _ = try SwitchLaunchPolicy.arguments(gamePath: game.path, existingInstance: true)
        }
        let duplicate = SwitchCollectionStorage.Document(version: 1, games: [entry, entry], enginePath: nil)
        try refuses("duplicate metadata") { _ = try SwitchCollectionStorage.decode(JSONEncoder().encode(duplicate)) }
        try refuses("bounded metadata") { _ = try SwitchCollectionStorage.decode(Data(repeating: 32, count: SwitchCollectionStorage.maxBytes + 1)) }
        let unsupported = SwitchCollectionStorage.Document(version: 2, games: [], enginePath: nil)
        try refuses("unknown metadata version") { _ = try SwitchCollectionStorage.decode(JSONEncoder().encode(unsupported)) }
        if let path = ProcessInfo.processInfo.environment["CASKRUN_TEST_SUYU"] {
            let actual = URL(fileURLWithPath: path)
            try check(try SwitchCollectionStorage.enginePath(actual) == actual.standardizedFileURL, "official Suyu bundle accepted")
            try model.configureEngine(actual)
            let reloaded = SwitchCollectionModel(storageDirectory: root.appendingPathComponent("data"))
            try check(reloaded.enginePath == actual.path, "engine selection survives reload")
        }
        print("\(checks) Switch collection checks passed")
    }
}
