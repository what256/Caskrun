import AppKit
import Combine
import Darwin
import Foundation
import ImageIO

struct PS5CollectionGame: Codable, Identifiable, Sendable, Equatable {
    let id: String
    let name: String
    let path: String
    let createdAt: UInt64
    var artworkPath: String? = nil
}

struct PS5EngineConfiguration: Codable, Sendable, Equatable {
    let path: String
    let version: String
}

enum PS5CollectionError: LocalizedError {
    case invalid(String)
    var errorDescription: String? {
        switch self { case .invalid(let message): return message }
    }
}

// Private metadata only. Original game files and the selected engine stay in place.
enum PS5CollectionStorage {
    static let maximumBytes = 512 * 1024
    static let maximumGames = 256
    static let noFollowAny: Int32 = 0x2000_0000

    struct Document: Codable {
        let version: Int
        let games: [PS5CollectionGame]
        let engine: PS5EngineConfiguration?
    }

    static func checkedPath(_ url: URL) throws -> URL {
        guard url.isFileURL, url.host == nil || url.host == "localhost",
              !url.path.isEmpty, url.path.utf8.count <= 4096, !url.path.contains("\0"),
              url.path.hasPrefix("/") else {
            throw PS5CollectionError.invalid("Choose a local file or folder.")
        }
        return url.standardizedFileURL
    }

    static func checkedName(_ name: String) throws -> String {
        let clean = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty, clean.utf8.count <= 240,
              !clean.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else {
            throw PS5CollectionError.invalid("Give the game a shorter name without line breaks or control characters.")
        }
        return clean
    }

    static func fileDescriptor(_ url: URL, directory: Bool = false) throws -> Int32 {
        let checked = try checkedPath(url)
        let fd = Darwin.open(checked.path, O_RDONLY | O_CLOEXEC | O_NONBLOCK | noFollowAny | (directory ? O_DIRECTORY : 0))
        guard fd >= 0 else { throw PS5CollectionError.invalid("The selected item is missing, inaccessible or a symbolic link.") }
        var attributes = stat()
        guard fstat(fd, &attributes) == 0,
              (attributes.st_mode & S_IFMT) == (directory ? S_IFDIR : S_IFREG) else {
            Darwin.close(fd)
            throw PS5CollectionError.invalid("Choose a regular local file or folder.")
        }
        return fd
    }

    static func boundedRead(_ url: URL, maximum: Int) throws -> Data {
        let fd = try fileDescriptor(url)
        defer { Darwin.close(fd) }
        var before = stat()
        guard fstat(fd, &before) == 0, before.st_size > 0, before.st_size <= maximum else {
            throw PS5CollectionError.invalid("The file is empty or exceeds the allowed size.")
        }
        let bytes = try read(fd, offset: 0, count: Int(before.st_size))
        var after = stat()
        guard fstat(fd, &after) == 0, before.st_size == after.st_size,
              before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
              before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
              before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
              before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec else {
            throw PS5CollectionError.invalid("The file changed while it was being read. Try again.")
        }
        return bytes
    }

    static func read(_ fd: Int32, offset: Int64, count: Int) throws -> Data {
        var data = Data(count: count)
        let complete = data.withUnsafeMutableBytes { bytes -> Bool in
            var consumed = 0
            while consumed < count {
                let size = pread(fd, bytes.baseAddress!.advanced(by: consumed), count - consumed, off_t(offset) + off_t(consumed))
                if size < 0, errno == EINTR { continue }
                guard size > 0 else { return false }
                consumed += size
            }
            return true
        }
        guard complete else { throw PS5CollectionError.invalid("The selected file could not be read completely.") }
        return data
    }

    private static func canonicalPath(_ fd: Int32) throws -> URL {
        var buffer = [CChar](repeating: 0, count: Int(MAXPATHLEN))
        guard fcntl(fd, F_GETPATH, &buffer) == 0 else {
            throw PS5CollectionError.invalid("The selected game path could not be resolved.")
        }
        return try checkedPath(URL(fileURLWithPath: String(cString: buffer)))
    }

    static func ensureDirectory(_ url: URL) throws {
        let checked = try checkedPath(url)
        // Check ancestors before creating anything; the final open refuses links again.
        var ancestor = checked
        var missing: [URL] = []
        while true {
            var attributes = stat()
            if lstat(ancestor.path, &attributes) == 0 {
                let fd = try fileDescriptor(ancestor, directory: true)
                Darwin.close(fd)
                break
            }
            guard errno == ENOENT, ancestor.path != "/" else {
                throw PS5CollectionError.invalid("Caskrun's PS5 storage folder is inaccessible.")
            }
            missing.append(ancestor)
            ancestor.deleteLastPathComponent()
        }
        for directory in missing.reversed() {
            guard mkdir(directory.path, mode_t(0o700)) == 0 || errno == EEXIST else {
                throw PS5CollectionError.invalid("Caskrun's PS5 storage folder could not be created.")
            }
            let fd = try fileDescriptor(directory, directory: true)
            Darwin.close(fd)
        }
    }

    static func decode(_ data: Data) throws -> Document {
        guard data.count <= maximumBytes else { throw PS5CollectionError.invalid("The PS5 collection is too large.") }
        let document = try JSONDecoder().decode(Document.self, from: data)
        guard document.version == 1 else { throw PS5CollectionError.invalid("This PS5 collection format is not supported by this Caskrun version.") }
        guard document.games.count <= maximumGames else { throw PS5CollectionError.invalid("The PS5 collection contains too many games.") }
        var ids = Set<String>(), paths = Set<String>()
        for game in document.games {
            guard UUID(uuidString: game.id) != nil, ids.insert(game.id.lowercased()).inserted,
                  try checkedName(game.name) == game.name,
                  game.path.hasPrefix("/"), try checkedPath(URL(fileURLWithPath: game.path)).path == game.path,
                  paths.insert(game.path).inserted else {
                throw PS5CollectionError.invalid("The PS5 collection contains an invalid or duplicate entry.")
            }
            if let artwork = game.artworkPath {
                let original = URL(fileURLWithPath: game.path)
                var folders = [original]
                if ["elf", "zar"].contains(original.pathExtension.lowercased()) { folders.append(original.deletingLastPathComponent()) }
                let allowed = folders.flatMap { [$0.appendingPathComponent("sce_sys/icon0.png").path, $0.appendingPathComponent("icon0.png").path] }
                guard artwork.hasPrefix("/"), try checkedPath(URL(fileURLWithPath: artwork)).path == artwork,
                      allowed.contains(artwork) else {
                    throw PS5CollectionError.invalid("The PS5 collection contains an invalid picture path.")
                }
            }
        }
        if let engine = document.engine {
            guard engine.path.hasPrefix("/"), try checkedPath(URL(fileURLWithPath: engine.path)).path == engine.path,
                  URL(fileURLWithPath: engine.path).lastPathComponent == "kyty_emulator",
                  !engine.version.isEmpty, engine.version.utf8.count <= 128,
                  !engine.version.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else {
                throw PS5CollectionError.invalid("The recorded PS5 engine is invalid.")
            }
        }
        return document
    }

    static func write(_ document: Document, directory: URL) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(document)
        _ = try decode(data)
        try ensureDirectory(directory)
        let root = try fileDescriptor(directory, directory: true)
        defer { Darwin.close(root) }
        let name = "ps5-collection.json"
        var existing = stat()
        if fstatat(root, name, &existing, AT_SYMLINK_NOFOLLOW) == 0 {
            guard (existing.st_mode & S_IFMT) == S_IFREG, existing.st_nlink == 1 else {
                throw PS5CollectionError.invalid("Caskrun's PS5 collection file is not a private regular file.")
            }
        } else if errno != ENOENT {
            throw PS5CollectionError.invalid("Caskrun's PS5 collection file cannot be checked.")
        }
        let temporary = ".ps5-collection-\(UUID().uuidString).tmp"
        let fd = openat(root, temporary, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, mode_t(0o600))
        guard fd >= 0 else { throw PS5CollectionError.invalid("The PS5 collection could not be saved.") }
        defer { Darwin.close(fd); unlinkat(root, temporary, 0) }
        let complete = data.withUnsafeBytes { bytes -> Bool in
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.write(fd, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if count < 0, errno == EINTR { continue }
                guard count > 0 else { return false }
                offset += count
            }
            return true
        }
        guard complete, fsync(fd) == 0, renameat(root, temporary, root, name) == 0 else {
            throw PS5CollectionError.invalid("The PS5 collection could not be saved completely.")
        }
    }

    static func importPath(_ url: URL, includeArtwork: Bool = true) throws -> (URL, URL?) {
        var path = try checkedPath(url)
        var attributes = stat()
        guard lstat(path.path, &attributes) == 0 else { throw PS5CollectionError.invalid("The selected game cannot be found.") }
        let folder: URL?
        if (attributes.st_mode & S_IFMT) == S_IFDIR {
            let fd = try fileDescriptor(path, directory: true)
            defer { Darwin.close(fd) }
            path = try canonicalPath(fd)
            let boot = try fileDescriptor(path.appendingPathComponent("eboot.bin"))
            defer { Darwin.close(boot) }
            var bootAttributes = stat()
            guard fstat(boot, &bootAttributes) == 0, bootAttributes.st_size > 0 else {
                throw PS5CollectionError.invalid("Choose a game folder containing a nonempty eboot.bin.")
            }
            folder = path
        } else {
            guard ["elf", "zar"].contains(path.pathExtension.lowercased()) else {
                throw PS5CollectionError.invalid("Choose a PS5 game folder, ELF file or ZArchive (.zar) dump.")
            }
            let fd = try fileDescriptor(path)
            defer { Darwin.close(fd) }
            path = try canonicalPath(fd)
            var fileAttributes = stat()
            guard fstat(fd, &fileAttributes) == 0, fileAttributes.st_size > 0 else {
                throw PS5CollectionError.invalid("The selected game file is empty.")
            }
            if path.pathExtension.lowercased() == "elf" {
                guard fileAttributes.st_size >= 64 else {
                    throw PS5CollectionError.invalid("The selected ELF executable is incomplete.")
                }
                let header = try read(fd, offset: 0, count: 64)
                guard header.prefix(7) == Data([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1]),
                      header[18] == 0x3e, header[19] == 0 else {
                    throw PS5CollectionError.invalid("Choose a 64-bit x86 PS5 ELF executable.")
                }
            }
            folder = nil
        }
        // Optional pictures never delay import. A selected file grants no sibling
        // folder access; only an explicitly selected folder gets picture discovery.
        return (path, includeArtwork ? folder?.appendingPathComponent("sce_sys/icon0.png") : nil)
    }

    static func architectures(_ path: URL, requiredFileType: UInt32) throws -> Set<String> {
        let fd = try fileDescriptor(path)
        defer { Darwin.close(fd) }
        var attributes = stat()
        guard fstat(fd, &attributes) == 0, attributes.st_size >= 32 else {
            throw PS5CollectionError.invalid("Choose a complete macOS Mach-O binary.")
        }
        func uint32(_ bytes: Data, _ offset: Int, little: Bool) -> UInt32 {
            let values = Array(bytes[offset..<offset + 4])
            return values.enumerated().reduce(0) { result, item in
                result | UInt32(item.element) << UInt32((little ? item.offset : 3 - item.offset) * 8)
            }
        }
        func slice(_ offset: Int64, _ size: Int64) throws -> String? {
            guard offset >= 0, size >= 32, offset <= attributes.st_size - size else {
                throw PS5CollectionError.invalid("The macOS binary contains an invalid slice.")
            }
            let header = try read(fd, offset: offset, count: 32)
            let magic = Array(header.prefix(4))
            let little: Bool
            if magic == [0xcf, 0xfa, 0xed, 0xfe] { little = true }
            else if magic == [0xfe, 0xed, 0xfa, 0xcf] { little = false }
            else { throw PS5CollectionError.invalid("Choose a 64-bit macOS Mach-O binary.") }
            let cpu = uint32(header, 4, little: little)
            let commands = uint32(header, 16, little: little)
            let commandBytes = uint32(header, 20, little: little)
            guard uint32(header, 12, little: little) == requiredFileType,
                  commands > 0, commands <= 8192, commandBytes >= commands * 8,
                  Int64(commandBytes) <= size - 32 else {
                throw PS5CollectionError.invalid("The selected macOS binary is incomplete or has the wrong type.")
            }
            if cpu == 0x0100_000c { return "arm64" }
            if cpu == 0x0100_0007 { return "x86_64" }
            return nil
        }
        let header = try read(fd, offset: 0, count: 32)
        let magic = Array(header.prefix(4))
        if magic == [0xcf, 0xfa, 0xed, 0xfe] || magic == [0xfe, 0xed, 0xfa, 0xcf] {
            return Set(try slice(0, attributes.st_size).map { [$0] } ?? [])
        }
        guard magic == [0xca, 0xfe, 0xba, 0xbe] else {
            throw PS5CollectionError.invalid("Choose a macOS kyty_emulator binary, not a Windows or Linux build.")
        }
        let count = uint32(header, 4, little: false)
        guard count > 0, count <= 32, Int64(8 + count * 20) <= attributes.st_size else {
            throw PS5CollectionError.invalid("The universal macOS binary is invalid.")
        }
        let table = try read(fd, offset: 8, count: Int(count) * 20)
        var result = Set<String>()
        for index in 0..<Int(count) {
            let offset = Int64(uint32(table, index * 20 + 8, little: false))
            let size = Int64(uint32(table, index * 20 + 12, little: false))
            if let cpu = try slice(offset, size) { result.insert(cpu) }
        }
        return result
    }

    static func checkedEngine(_ url: URL) throws -> PS5EngineConfiguration {
        var path = try checkedPath(url)
        var attributes = stat()
        if lstat(path.path, &attributes) == 0, (attributes.st_mode & S_IFMT) == S_IFDIR {
            let directoryFD = try fileDescriptor(path, directory: true)
            defer { Darwin.close(directoryFD) }
            path = try canonicalPath(directoryFD)
            let candidates = [path.appendingPathComponent("kyty_emulator"),
                              path.appendingPathComponent("Contents/MacOS/kyty_emulator")]
            guard let executable = candidates.first(where: { candidate in
                var entry = stat()
                return lstat(candidate.path, &entry) == 0 && (entry.st_mode & S_IFMT) == S_IFREG
            }) else {
                throw PS5CollectionError.invalid("Choose a complete KytyPS5 Mac package containing kyty_emulator.")
            }
            path = executable
        }
        guard path.lastPathComponent == "kyty_emulator", access(path.path, X_OK) == 0 else {
            throw PS5CollectionError.invalid("Choose the executable named kyty_emulator from a KytyPS5 Mac release.")
        }
        let architectures = try architectures(path, requiredFileType: 2)
        guard !architectures.isEmpty else { throw PS5CollectionError.invalid("The PS5 engine must support arm64 or x86-64 macOS.") }
        if !architectures.contains("arm64"),
           access("/Library/Apple/usr/libexec/oah/libRosettaRuntime", R_OK) != 0 {
            throw PS5CollectionError.invalid("This x86-64 PS5 engine requires Rosetta 2. Install Rosetta before attaching it.")
        }
        let parent = path.deletingLastPathComponent()
        let selectedArchitecture = architectures.contains("arm64") ? "arm64" : "x86_64"
        let candidates = [parent.appendingPathComponent("libMoltenVK.dylib"),
                          parent.deletingLastPathComponent().appendingPathComponent("Frameworks/libMoltenVK.dylib")]
        guard candidates.contains(where: { candidate in
            guard let supported = try? Self.architectures(candidate, requiredFileType: 6) else { return false }
            return supported.contains(selectedArchitecture)
        }) else {
            throw PS5CollectionError.invalid("This engine needs a matching libMoltenVK.dylib beside it or in its app's Frameworks folder. Choose a complete KytyPS5 Mac release.")
        }
        // Do not execute an arbitrary selected binary to ask its version.
        return PS5EngineConfiguration(path: path.path, version: "Not recorded")
    }

    static func artworkPNG(_ paths: [URL]) -> Data? {
        for path in paths {
            guard !Task.isCancelled else { return nil }
            guard let data = try? boundedRead(path, maximum: 8 * 1024 * 1024),
                  let source = CGImageSourceCreateWithData(data as CFData, nil),
                  CGImageSourceGetCount(source) == 1,
                  let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
                  let width = props[kCGImagePropertyPixelWidth] as? NSNumber,
                  let height = props[kCGImagePropertyPixelHeight] as? NSNumber,
                  width.intValue > 0, height.intValue > 0, width.intValue <= 4096, height.intValue <= 4096,
                  let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                    kCGImageSourceCreateThumbnailFromImageAlways: true,
                    kCGImageSourceThumbnailMaxPixelSize: 512,
                    kCGImageSourceCreateThumbnailWithTransform: true
                  ] as CFDictionary) else { continue }
            guard !Task.isCancelled else { return nil }
            let result = NSMutableData()
            guard let destination = CGImageDestinationCreateWithData(result, "public.png" as CFString, 1, nil) else { continue }
            CGImageDestinationAddImage(destination, image, nil)
            if CGImageDestinationFinalize(destination), result.length <= 2 * 1024 * 1024 {
                return result as Data
            }
        }
        return nil
    }
}

// At most two optional picture readers. Cancelling a view removes its queued
// request; cancellation cannot interrupt an OS file-access decision already open.
private actor PS5ArtworkReader {
    static let shared = PS5ArtworkReader()
    private var active = 0
    private var waiting: [(UUID, CheckedContinuation<Bool, Never>)] = []

    func read(_ paths: [URL]) async -> Data? {
        guard !Task.isCancelled else { return nil }
        let id = UUID()
        let admitted = await withTaskCancellationHandler(operation: {
            if active < 2 { active += 1; return true }
            return await withCheckedContinuation { waiting.append((id, $0)) }
        }, onCancel: { Task { await self.cancel(id) } })
        guard admitted else { return nil }
        defer {
            if waiting.isEmpty { active -= 1 }
            else { waiting.removeFirst().1.resume(returning: true) }
        }
        guard !Task.isCancelled else { return nil }
        let worker = Task.detached(priority: .utility) { PS5CollectionStorage.artworkPNG(paths) }
        return await withTaskCancellationHandler(operation: { await worker.value }, onCancel: { worker.cancel() })
    }

    private func cancel(_ id: UUID) {
        guard let index = waiting.firstIndex(where: { $0.0 == id }) else { return }
        waiting.remove(at: index).1.resume(returning: false)
    }
}

private final class PS5OwnedSession: @unchecked Sendable {
    let process: Process
    let pipe = Pipe()
    let logURL: URL
    private let lock = NSLock()
    private var log: FileHandle?
    private var written = 0
    private var closed = false
    private var stopRequested = false
    private let readFD: Int32
    private static let maximumLogBytes = 4 * 1024 * 1024

    init(process: Process, logURL: URL) throws {
        self.process = process
        self.logURL = logURL
        readFD = pipe.fileHandleForReading.fileDescriptor
        let fd = Darwin.open(logURL.path, O_WRONLY | O_CREAT | O_CLOEXEC | PS5CollectionStorage.noFollowAny | O_NONBLOCK, mode_t(0o600))
        guard fd >= 0 else { throw PS5CollectionError.invalid("The PS5 session log could not be created.") }
        var attributes = stat()
        guard fstat(fd, &attributes) == 0, (attributes.st_mode & S_IFMT) == S_IFREG, attributes.st_nlink == 1,
              attributes.st_uid == geteuid(), fchmod(fd, mode_t(0o600)) == 0, ftruncate(fd, 0) == 0 else {
            Darwin.close(fd)
            throw PS5CollectionError.invalid("The PS5 session log is not a private regular file.")
        }
        log = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        let flags = fcntl(readFD, F_GETFL)
        guard flags >= 0, fcntl(readFD, F_SETFL, flags | O_NONBLOCK) == 0 else {
            try? log?.close()
            throw PS5CollectionError.invalid("The PS5 session output could not be configured.")
        }
        process.standardOutput = pipe
        process.standardError = pipe
        pipe.fileHandleForReading.readabilityHandler = { [weak self] _ in self?.readAvailable() }
    }

    private func readAvailable() {
        lock.lock()
        defer { lock.unlock() }
        guard !closed else { return }
        var bytes = Data(count: 64 * 1024)
        let readCount = bytes.withUnsafeMutableBytes { Darwin.read(readFD, $0.baseAddress, $0.count) }
        if readCount == 0 { pipe.fileHandleForReading.readabilityHandler = nil; return }
        guard readCount > 0 else { return }
        let count = min(readCount, Self.maximumLogBytes - written)
        guard count > 0, let log else { return }
        do { try log.write(contentsOf: bytes.prefix(count)); written += count }
        catch { try? log.close(); self.log = nil }
    }

    func requestStop() {
        lock.lock()
        stopRequested = true
        lock.unlock()
        if process.isRunning { process.terminate() }
    }

    var wasStoppedByCaskrun: Bool {
        lock.lock()
        defer { lock.unlock() }
        return stopRequested
    }

    func close() {
        lock.lock()
        defer { lock.unlock() }
        guard !closed else { return }
        closed = true
        pipe.fileHandleForReading.readabilityHandler = nil
        try? log?.close()
        log = nil
        try? pipe.fileHandleForReading.close()
        try? pipe.fileHandleForWriting.close()
    }
}

@MainActor
final class PS5CollectionModel: ObservableObject {
    @Published private(set) var games: [PS5CollectionGame] = []
    @Published private(set) var engine: PS5EngineConfiguration?
    @Published private(set) var error: String?
    @Published private(set) var busy = false
    @Published private(set) var runningGameIDs = Set<String>()
    private let storageDirectory: URL
    private var storageValid = false
    private var quitting = false
    private var sessions: [String: PS5OwnedSession] = [:]

    init(storageDirectory: URL? = nil) {
        if let storageDirectory { self.storageDirectory = storageDirectory.standardizedFileURL }
        else if let override = ProcessInfo.processInfo.environment["CASKRUN_HOME"], !override.isEmpty {
            self.storageDirectory = URL(fileURLWithPath: override, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)).standardizedFileURL
        } else {
            let home = ProcessInfo.processInfo.environment["HOME"].map { URL(fileURLWithPath: $0, isDirectory: true) } ?? FileManager.default.homeDirectoryForCurrentUser
            self.storageDirectory = home.appendingPathComponent("Library/Application Support/Caskrun", isDirectory: true)
        }
        refresh()
    }

    func refresh() {
        guard !busy else { return }
        let path = storageDirectory.appendingPathComponent("ps5-collection.json")
        var attributes = stat()
        if lstat(path.path, &attributes) != 0, errno == ENOENT {
            games = []; engine = nil; error = nil; storageValid = true
            return
        }
        do {
            guard attributes.st_nlink == 1 else { throw PS5CollectionError.invalid("The PS5 collection file is not private.") }
            let document = try PS5CollectionStorage.decode(PS5CollectionStorage.boundedRead(path, maximum: PS5CollectionStorage.maximumBytes))
            games = document.games
            engine = document.engine
            error = nil
            storageValid = true
        } catch {
            self.error = "PS5 collection could not be read: \(error.localizedDescription)"
            storageValid = false
        }
    }

    private func requireAvailable() throws {
        guard !busy, !quitting else { throw PS5CollectionError.invalid("Wait for the current PS5 operation to finish.") }
        guard storageValid else { throw PS5CollectionError.invalid("Resolve the PS5 collection storage error before making changes.") }
    }

    private func save(games: [PS5CollectionGame], engine: PS5EngineConfiguration?) throws {
        try PS5CollectionStorage.write(.init(version: 1, games: games, engine: engine), directory: storageDirectory)
        self.games = games
        self.engine = engine
        error = nil
    }

    @discardableResult
    func importGame(name: String, path: URL) async throws -> PS5CollectionGame {
        try requireAvailable()
        let name = try PS5CollectionStorage.checkedName(name)
        guard games.count < PS5CollectionStorage.maximumGames else { throw PS5CollectionError.invalid("The PS5 collection is full.") }
        busy = true
        defer { busy = false }
        let (selected, artwork) = try await Task.detached(priority: .utility) { try PS5CollectionStorage.importPath(path) }.value
        guard !games.contains(where: { $0.path == selected.path }) else { throw PS5CollectionError.invalid("This game is already in Collection.") }
        let game = PS5CollectionGame(id: UUID().uuidString, name: name, path: selected.path,
                                     createdAt: UInt64(Date().timeIntervalSince1970), artworkPath: artwork?.path)
        try save(games: games + [game], engine: engine)
        return game
    }

    func configureEngine(path: URL) async throws {
        try requireAvailable()
        guard sessions.isEmpty else { throw PS5CollectionError.invalid("Stop your PS5 session before changing its engine.") }
        busy = true
        defer { busy = false }
        let configuration = try await Task.detached(priority: .utility) { try PS5CollectionStorage.checkedEngine(path) }.value
        try save(games: games, engine: configuration)
    }

    func removeGame(_ game: PS5CollectionGame) throws {
        try requireAvailable()
        guard sessions[game.id] == nil else { throw PS5CollectionError.invalid("Stop this PS5 game before removing its Collection entry.") }
        guard games.contains(game) else { throw PS5CollectionError.invalid("The PS5 game is no longer in Collection.") }
        try save(games: games.filter { $0.id != game.id }, engine: engine)
    }

    func launch(_ game: PS5CollectionGame) throws {
        try requireAvailable()
        guard games.contains(game) else { throw PS5CollectionError.invalid("The PS5 game is no longer in Collection.") }
        if let session = sessions[game.id], session.process.isRunning {
            _ = focus(game)
            return
        }
        guard let engine else { throw PS5CollectionError.invalid("Choose your KytyPS5 Mac engine before opening this game.") }
        _ = try PS5CollectionStorage.checkedEngine(URL(fileURLWithPath: engine.path))
        _ = try PS5CollectionStorage.importPath(URL(fileURLWithPath: game.path), includeArtwork: false)
        let logDirectory = storageDirectory.appendingPathComponent("ps5-sessions", isDirectory: true)
        try PS5CollectionStorage.ensureDirectory(logDirectory)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: engine.path)
        process.arguments = ["--game", game.path]
        process.currentDirectoryURL = URL(fileURLWithPath: engine.path).deletingLastPathComponent()
        let session = try PS5OwnedSession(process: process, logURL: logDirectory.appendingPathComponent("\(game.id).log"))
        process.terminationHandler = { [weak self, weak session] _ in
            session?.close()
            Task { @MainActor in
                guard let self, let session, self.sessions[game.id] === session else { return }
                self.sessions.removeValue(forKey: game.id)
                self.runningGameIDs.remove(game.id)
                if !session.wasStoppedByCaskrun,
                   session.process.terminationReason == .uncaughtSignal || session.process.terminationStatus != 0 {
                    self.error = "The experimental PS5 engine stopped. Its session log may explain why."
                }
            }
        }
        do {
            try process.run()
            try? session.pipe.fileHandleForWriting.close()
            sessions[game.id] = session
            runningGameIDs.insert(game.id)
            error = nil
        } catch {
            session.close()
            throw PS5CollectionError.invalid("The PS5 engine could not start: \(error.localizedDescription)")
        }
    }

    func stop(_ game: PS5CollectionGame) async throws {
        guard let session = sessions[game.id] else { return }
        session.requestStop()
        let deadline = ProcessInfo.processInfo.systemUptime + 3
        while session.process.isRunning, ProcessInfo.processInfo.systemUptime < deadline {
            try await Task.sleep(nanoseconds: 40_000_000)
        }
        guard !session.process.isRunning else {
            throw PS5CollectionError.invalid("The PS5 engine has not stopped yet. Close its window before quitting Caskrun.")
        }
        session.close()
        if sessions[game.id] === session {
            sessions.removeValue(forKey: game.id)
            runningGameIDs.remove(game.id)
        }
    }

    func shutdownForQuit() async -> Bool {
        guard !busy else { error = "Wait for the PS5 operation to finish before quitting."; return false }
        quitting = true
        do {
            let owned = sessions
            for session in owned.values where session.process.isRunning { session.requestStop() }
            let deadline = ProcessInfo.processInfo.systemUptime + 3
            while owned.values.contains(where: { $0.process.isRunning }), ProcessInfo.processInfo.systemUptime < deadline {
                try await Task.sleep(nanoseconds: 40_000_000)
            }
            for (id, session) in owned where !session.process.isRunning {
                session.close()
                if sessions[id] === session { sessions.removeValue(forKey: id); runningGameIDs.remove(id) }
            }
            guard !owned.values.contains(where: { $0.process.isRunning }) else {
                throw PS5CollectionError.invalid("The PS5 engine has not stopped yet. Close its window before quitting Caskrun.")
            }
            return true
        } catch {
            self.error = error.localizedDescription
            quitting = false
            return false
        }
    }

    func focus(_ game: PS5CollectionGame) -> Bool {
        guard let session = sessions[game.id], session.process.isRunning,
              let app = NSRunningApplication(processIdentifier: session.process.processIdentifier) else { return false }
        return app.activate(options: [.activateAllWindows])
    }

    func logsURL(for game: PS5CollectionGame) -> URL? {
        let url = storageDirectory.appendingPathComponent("ps5-sessions/\(game.id).log")
        guard UUID(uuidString: game.id) != nil, let fd = try? PS5CollectionStorage.fileDescriptor(url) else { return nil }
        Darwin.close(fd)
        return url
    }

    func gameArtwork(_ game: PS5CollectionGame) async -> NSImage? {
        guard games.contains(game), let path = game.artworkPath else { return nil }
        let selected = URL(fileURLWithPath: game.path)
        let folderCandidates = [selected.appendingPathComponent("sce_sys/icon0.png"), selected.appendingPathComponent("icon0.png")]
        // Older entries may contain a specifically recorded adjacent picture.
        // Preserve that reference without expanding it into a sibling-folder scan.
        let candidates = folderCandidates.contains(where: { $0.path == path }) ? folderCandidates : [URL(fileURLWithPath: path)]
        guard let data = await PS5ArtworkReader.shared.read(candidates), !Task.isCancelled,
              games.contains(game) else { return nil }
        return NSImage(data: data)
    }
}
