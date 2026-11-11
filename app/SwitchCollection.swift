import AppKit
import Combine
import Darwin
import Foundation

struct SwitchCollectionGame: Codable, Identifiable, Equatable {
    let id: String
    let name: String
    let path: String
}

enum SwitchCollectionError: LocalizedError {
    case invalid(String)
    var errorDescription: String? { if case .invalid(let message) = self { return message }; return nil }
}

// Only links and an engine location are stored. The user's games and emulator
// remain external and are never copied, changed, or removed.
enum SwitchCollectionStorage {
    static let maxBytes = 512 * 1024
    static let maxGames = 256
    static let bundleID = "com.suyu-emu.suyu"
    struct Document: Codable {
        let version: Int
        let games: [SwitchCollectionGame]
        let enginePath: String?
    }

    static func gamePath(_ url: URL) throws -> URL {
        let path = try PS5CollectionStorage.checkedPath(url)
        guard ["nsp", "xci", "nro"].contains(path.pathExtension.lowercased()) else {
            throw SwitchCollectionError.invalid("Choose your own NSP, XCI or NRO game file.")
        }
        let fd = try PS5CollectionStorage.fileDescriptor(path)
        defer { Darwin.close(fd) }
        var s = stat()
        guard fstat(fd, &s) == 0, s.st_size > 0 else {
            throw SwitchCollectionError.invalid("The selected game file is empty or inaccessible.")
        }
        return path
    }

    static func enginePath(_ url: URL) throws -> URL {
        let path = try PS5CollectionStorage.checkedPath(url)
        guard path.pathExtension.lowercased() == "app" else {
            throw SwitchCollectionError.invalid("Choose the Suyu macOS application.")
        }
        let directory = try PS5CollectionStorage.fileDescriptor(path, directory: true)
        defer { Darwin.close(directory) }
        let info = try PS5CollectionStorage.boundedRead(path.appendingPathComponent("Contents/Info.plist"), maximum: 256 * 1024)
        guard let plist = try PropertyListSerialization.propertyList(from: info, format: nil) as? [String: Any],
              plist["CFBundleIdentifier"] as? String == bundleID,
              let recordedExecutable = plist["CFBundleExecutable"] as? String else {
            throw SwitchCollectionError.invalid("Choose a Suyu macOS app with the expected bundle identifier.")
        }
        // Suyu v14.1 ships an empty CFBundleExecutable; Launch Services falls
        // back to the bundle name. Only accept that exact known fallback.
        let executable = recordedExecutable.isEmpty ? "suyu" : recordedExecutable
        guard executable == "suyu" else { throw SwitchCollectionError.invalid("The selected Suyu app has an unexpected executable.") }
        let binary = path.appendingPathComponent("Contents/MacOS", isDirectory: true).appendingPathComponent(executable)
        guard access(binary.path, X_OK) == 0,
              try PS5CollectionStorage.architectures(binary, requiredFileType: 2).contains("arm64") else {
            throw SwitchCollectionError.invalid("Choose a Suyu build for Apple Silicon.")
        }
        return path
    }

    static func decode(_ data: Data) throws -> Document {
        guard data.count <= maxBytes else { throw SwitchCollectionError.invalid("The Switch collection is too large.") }
        let doc = try JSONDecoder().decode(Document.self, from: data)
        guard doc.version == 1, doc.games.count <= maxGames else { throw SwitchCollectionError.invalid("Unsupported Switch collection.") }
        var ids = Set<String>(), paths = Set<String>()
        for game in doc.games {
            guard UUID(uuidString: game.id) != nil, ids.insert(game.id.lowercased()).inserted,
                  try PS5CollectionStorage.checkedName(game.name) == game.name,
                  game.path.hasPrefix("/"), try PS5CollectionStorage.checkedPath(URL(fileURLWithPath: game.path)).path == game.path,
                  ["nsp", "xci", "nro"].contains(URL(fileURLWithPath: game.path).pathExtension.lowercased()),
                  paths.insert(game.path).inserted else {
                throw SwitchCollectionError.invalid("The Switch collection contains an invalid or duplicate entry.")
            }
        }
        if let path = doc.enginePath {
            guard path.hasPrefix("/"), try PS5CollectionStorage.checkedPath(URL(fileURLWithPath: path)).path == path,
                  URL(fileURLWithPath: path).pathExtension.lowercased() == "app" else {
                throw SwitchCollectionError.invalid("The saved Switch engine path is invalid.")
            }
        }
        return doc
    }

    static func write(_ document: Document, directory: URL) throws {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(document); _ = try decode(data)
        try PS5CollectionStorage.ensureDirectory(directory)
        let root = try PS5CollectionStorage.fileDescriptor(directory, directory: true)
        defer { Darwin.close(root) }
        let name = "switch-collection.json"
        var previous = stat()
        if fstatat(root, name, &previous, AT_SYMLINK_NOFOLLOW) == 0 {
            guard (previous.st_mode & S_IFMT) == S_IFREG, previous.st_nlink == 1 else {
                throw SwitchCollectionError.invalid("The Switch collection is not a private regular file.")
            }
        } else if errno != ENOENT { throw SwitchCollectionError.invalid("The Switch collection cannot be checked.") }
        let temporary = ".switch-collection-\(UUID().uuidString).tmp"
        let fd = openat(root, temporary, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, mode_t(0o600))
        guard fd >= 0 else { throw SwitchCollectionError.invalid("The Switch collection could not be saved.") }
        defer { Darwin.close(fd); unlinkat(root, temporary, 0) }
        let complete = data.withUnsafeBytes { bytes -> Bool in
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.write(fd, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if count < 0, errno == EINTR { continue }
                guard count > 0 else { return false }; offset += count
            }
            return true
        }
        guard complete, fsync(fd) == 0, renameat(root, temporary, root, name) == 0 else {
            throw SwitchCollectionError.invalid("The Switch collection could not be saved completely.")
        }
    }
}

// Launch Services may activate an existing process without forwarding launch
// arguments. Never report a new game opened in that situation.
enum SwitchLaunchPolicy {
    static func arguments(gamePath: String, existingInstance: Bool) throws -> [String] {
        guard !existingInstance else {
            throw SwitchCollectionError.invalid("Suyu is already open. Quit it before opening another game from Caskrun.")
        }
        return [gamePath]
    }
}

@MainActor
final class SwitchCollectionModel: ObservableObject {
    @Published private(set) var games: [SwitchCollectionGame] = []
    @Published private(set) var enginePath: String?
    @Published private(set) var error: String?
    @Published private(set) var launching = false
    @Published private(set) var quitting = false
    private var ownedApplication: NSRunningApplication?
    private let directory: URL
    private var storageValid = false

    init(storageDirectory: URL? = nil) {
        if let storageDirectory { directory = storageDirectory.standardizedFileURL }
        else if let override = ProcessInfo.processInfo.environment["CASKRUN_HOME"], !override.isEmpty {
            directory = URL(fileURLWithPath: override, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)).standardizedFileURL
        } else {
            directory = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/Caskrun", isDirectory: true)
        }
        refresh()
    }

    func refresh() {
        let path = directory.appendingPathComponent("switch-collection.json")
        var attributes = stat()
        if lstat(path.path, &attributes) != 0, errno == ENOENT {
            games = []; enginePath = nil; error = nil; storageValid = true; return
        }
        do {
            guard attributes.st_nlink == 1 else { throw SwitchCollectionError.invalid("The Switch collection is not private.") }
            let doc = try SwitchCollectionStorage.decode(PS5CollectionStorage.boundedRead(path, maximum: SwitchCollectionStorage.maxBytes))
            games = doc.games; enginePath = doc.enginePath; error = nil; storageValid = true
        } catch { self.error = "Switch collection could not be read: \(error.localizedDescription)"; storageValid = false }
    }

    private func save(games: [SwitchCollectionGame], engine: String?) throws {
        guard storageValid else { throw SwitchCollectionError.invalid("Resolve the Switch collection storage error first.") }
        try SwitchCollectionStorage.write(.init(version: 1, games: games, enginePath: engine), directory: directory)
        self.games = games; enginePath = engine; error = nil
    }

    func configureEngine(_ url: URL) throws {
        let path = try SwitchCollectionStorage.enginePath(url)
        try save(games: games, engine: path.path)
    }

    func importGame(name: String, url: URL) throws -> SwitchCollectionGame {
        let clean = try PS5CollectionStorage.checkedName(name)
        let path = try SwitchCollectionStorage.gamePath(url)
        guard games.count < SwitchCollectionStorage.maxGames, !games.contains(where: { $0.path == path.path }) else {
            throw SwitchCollectionError.invalid("This game is already in Collection or the collection is full.")
        }
        let game = SwitchCollectionGame(id: UUID().uuidString, name: clean, path: path.path)
        try save(games: games + [game], engine: enginePath)
        return game
    }

    func remove(_ game: SwitchCollectionGame) throws {
        guard games.contains(game) else { throw SwitchCollectionError.invalid("This game is no longer in Collection.") }
        try save(games: games.filter { $0.id != game.id }, engine: enginePath)
    }

    func launch(_ game: SwitchCollectionGame) async throws {
        guard !quitting else { throw SwitchCollectionError.invalid("Caskrun is closing. Wait until it finishes.") }
        guard !launching, games.contains(game), let enginePath else {
            throw SwitchCollectionError.invalid("Choose Suyu before opening this game.")
        }
        launching = true; defer { launching = false }
        let gameURL = try SwitchCollectionStorage.gamePath(URL(fileURLWithPath: game.path))
        let appURL = try SwitchCollectionStorage.enginePath(URL(fileURLWithPath: enginePath))
        let running = !NSRunningApplication.runningApplications(withBundleIdentifier: SwitchCollectionStorage.bundleID).isEmpty
        let args = try SwitchLaunchPolicy.arguments(gamePath: gameURL.path, existingInstance: running)
        let options = NSWorkspace.OpenConfiguration(); options.arguments = args; options.activates = true
        options.createsNewApplicationInstance = true
        let app = try await NSWorkspace.shared.openApplication(at: appURL, configuration: options)
        // A request for a new instance prevents us adopting a pre-existing one.
        // Confirm its identity before remembering it for a later quit.
        guard app.bundleIdentifier == SwitchCollectionStorage.bundleID,
              app.bundleURL?.standardizedFileURL == appURL else {
            throw SwitchCollectionError.invalid("Suyu opened, but Caskrun could not verify its process ownership.")
        }
        ownedApplication = app
    }

    // Full Quit only touches the exact application returned for our own new
    // launch, never another Suyu session the user started separately.
    func shutdownForQuit() async -> Bool {
        quitting = true
        for _ in 0..<100 where launching {
            try? await Task.sleep(for: .milliseconds(100))
        }
        guard !launching else { error = "Suyu is still starting. Try quitting Caskrun again."; return false }
        guard let app = ownedApplication, !app.isTerminated else { ownedApplication = nil; return true }
        guard app.terminate() else { error = "Suyu could not be asked to quit. Close it and try again."; return false }
        for _ in 0..<100 where !app.isTerminated {
            try? await Task.sleep(for: .milliseconds(100))
        }
        guard app.isTerminated else { error = "Suyu has not finished quitting. Close it and try again."; return false }
        ownedApplication = nil
        return true
    }

    func prepareForQuit() { quitting = true }
    func resumeAfterQuitFailure() { quitting = false }

}
