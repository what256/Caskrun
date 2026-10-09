import AppKit
import Darwin
import Foundation

@main
struct PS5CollectionTests {
    @MainActor
    static func main() async throws {
        var checks = 0
        func expect(_ condition: Bool, _ message: String) throws {
            guard condition else { throw PS5CollectionError.invalid("Test failed: \(message)") }
            checks += 1
        }
        func refuses(_ message: String, _ body: () throws -> Void) throws {
            do { try body() } catch { checks += 1; return }
            throw PS5CollectionError.invalid("Test failed: \(message)")
        }
        let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
            .appendingPathComponent(".build/ps5-collection-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        var ownedProcessesStopped = true
        func noFixtureOwners() -> Bool {
            let probe = Process()
            let output = Pipe(), diagnostics = Pipe()
            probe.executableURL = URL(fileURLWithPath: "/usr/sbin/lsof")
            probe.arguments = ["-t", "+D", root.path]
            probe.standardOutput = output
            probe.standardError = diagnostics
            do { try probe.run() } catch { return false }
            probe.waitUntilExit()
            let owners = output.fileHandleForReading.readDataToEndOfFile()
            let warnings = diagnostics.fileHandleForReading.readDataToEndOfFile()
            return [0, 1].contains(probe.terminationStatus) && owners.isEmpty && warnings.isEmpty
        }
        defer {
            if ownedProcessesStopped && noFixtureOwners() {
                try? FileManager.default.removeItem(at: root)
            } else {
                print("PS5 fixtures preserved because process or descriptor ownership is unresolved.")
            }
        }
        let dataDirectory = root.appendingPathComponent("data", isDirectory: true)
        let gameFolder = root.appendingPathComponent("Example Game $(unused)", isDirectory: true)
        try FileManager.default.createDirectory(at: gameFolder, withIntermediateDirectories: true)
        try Data([0x7f, 0x45, 0x4c, 0x46, 0, 0, 0, 0]).write(to: gameFolder.appendingPathComponent("eboot.bin"))
        let model = PS5CollectionModel(storageDirectory: dataDirectory)
        try expect(model.games.isEmpty && model.engine == nil && model.error == nil, "new collection starts empty")
        let game = try await model.importGame(name: "  Example Game  ", path: gameFolder)
        try expect(game.name == "Example Game" && model.games == [game], "explicit import records trimmed name")
        try expect(FileManager.default.fileExists(atPath: gameFolder.appendingPathComponent("eboot.bin").path), "import keeps original game files")
        let documentURL = dataDirectory.appendingPathComponent("ps5-collection.json")
        var attributes = stat()
        try expect(lstat(documentURL.path, &attributes) == 0 && attributes.st_mode & 0o777 == 0o600, "local metadata is private")
        let restored = PS5CollectionModel(storageDirectory: dataDirectory)
        try expect(restored.games == [game] && restored.engine == nil && restored.error == nil, "metadata round trip")
        do {
            _ = try await model.importGame(name: "Duplicate", path: gameFolder.appendingPathComponent("."))
            throw PS5CollectionError.invalid("Test failed: duplicate was imported")
        } catch {
            try expect(model.games.count == 1 && !model.busy, "canonical duplicate refused without mutation")
        }
        for invalid in ["", "\n", "Example\nGame", String(repeating: "é", count: 121)] {
            try refuses("invalid name", { _ = try PS5CollectionStorage.checkedName(invalid) })
        }
        let incomplete = root.appendingPathComponent("incomplete", isDirectory: true)
        try FileManager.default.createDirectory(at: incomplete, withIntermediateDirectories: true)
        try refuses("folder requires eboot", { _ = try PS5CollectionStorage.importPath(incomplete) })
        let badELF = root.appendingPathComponent("invalid.elf")
        try Data("not an ELF".utf8).write(to: badELF)
        try refuses("ELF magic is checked", { _ = try PS5CollectionStorage.importPath(badELF) })
        let validELF = root.appendingPathComponent("Example.elf")
        var elfHeader = Data(repeating: 0, count: 64)
        elfHeader.replaceSubrange(0..<7, with: [0x7f, 0x45, 0x4c, 0x46, 2, 1, 1])
        elfHeader[18] = 0x3e
        try elfHeader.write(to: validELF)
        try expect(try PS5CollectionStorage.importPath(validELF).0 == validELF, "64-bit x86 ELF header accepted without claiming game compatibility")
        elfHeader[4] = 1
        try elfHeader.write(to: validELF)
        try refuses("32-bit ELF refused", { _ = try PS5CollectionStorage.importPath(validELF) })
        let emptyArchive = root.appendingPathComponent("empty.zar")
        try Data().write(to: emptyArchive)
        try refuses("empty archive refused", { _ = try PS5CollectionStorage.importPath(emptyArchive) })
        let linkedGame = root.appendingPathComponent("linked-game", isDirectory: true)
        try FileManager.default.createSymbolicLink(at: linkedGame, withDestinationURL: gameFolder)
        try refuses("linked game folder refused", { _ = try PS5CollectionStorage.importPath(linkedGame) })
        let nested = root.appendingPathComponent("nested", isDirectory: true)
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(at: nested.appendingPathComponent("eboot.bin"), withDestinationURL: gameFolder.appendingPathComponent("eboot.bin"))
        try refuses("linked boot refused", { _ = try PS5CollectionStorage.importPath(nested) })
        let archive = root.appendingPathComponent("Example.zar")
        try Data([1, 2, 3]).write(to: archive)
        let archiveGame = try await model.importGame(name: "Archive", path: archive)
        try expect(archiveGame.path == archive.path, "archive recorded in place; internal format validity remains engine's responsibility")
        let suffixFolder = root.appendingPathComponent("Folder.zar", isDirectory: true)
        try FileManager.default.createDirectory(at: suffixFolder, withIntermediateDirectories: true)
        try Data([1]).write(to: suffixFolder.appendingPathComponent("eboot.bin"))
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 2, pixelsHigh: 2, bitsPerSample: 8,
                                     samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                                     bytesPerRow: 8, bitsPerPixel: 32)!
        memset(bitmap.bitmapData!, 255, 16)
        let iconData = bitmap.representation(using: .png, properties: [:])!
        try iconData.write(to: suffixFolder.appendingPathComponent("icon0.png"))
        let suffixGame = try await model.importGame(name: "Folder", path: suffixFolder)
        let suffixPicture = await model.gameArtwork(suffixGame)
        try expect(suffixGame.artworkPath == suffixFolder.appendingPathComponent("sce_sys/icon0.png").path && suffixPicture != nil,
                   "folder import records a candidate without image IO; async reader falls back to local icon0")
        try expect(PS5CollectionModel(storageDirectory: dataDirectory).games.contains(suffixGame), "suffix folder metadata restores")
        try iconData.write(to: root.appendingPathComponent("icon0.png"))
        elfHeader[4] = 2
        try elfHeader.write(to: validELF)
        let fileGame = try await model.importGame(name: "File", path: validELF)
        let filePicture = await model.gameArtwork(fileGame)
        try expect(fileGame.artworkPath == nil && filePicture == nil,
                   "ELF file import does not discover a valid but unselected sibling picture")
        let noPictureFolder = root.appendingPathComponent("no-picture", isDirectory: true)
        try FileManager.default.createDirectory(at: noPictureFolder, withIntermediateDirectories: true)
        try Data([1]).write(to: noPictureFolder.appendingPathComponent("eboot.bin"))
        let noPictureGame = try await model.importGame(name: "No Picture", path: noPictureFolder)
        let missingPicture = await model.gameArtwork(noPictureGame)
        try expect(noPictureGame.artworkPath != nil && missingPicture == nil && model.games.contains(noPictureGame),
                   "missing optional pictures do not prevent folder import")
        let cancelledPicture = Task { @MainActor in await model.gameArtwork(suffixGame) }
        cancelledPicture.cancel()
        let cancelledResult = await cancelledPicture.value
        try expect(cancelledResult == nil, "cancelled artwork requests return without reading")
        let encoder = JSONEncoder()
        let base = PS5CollectionStorage.Document(version: 1, games: [game], engine: nil)
        let encoded = try encoder.encode(base)
        try expect(try PS5CollectionStorage.decode(encoded).games == [game], "validated document decode")
        for version in [0, 2, -1] {
            try refuses("unsupported document version", {
                _ = try PS5CollectionStorage.decode(encoder.encode(PS5CollectionStorage.Document(version: version, games: [game], engine: nil)))
            })
        }
        try refuses("duplicate ids", { _ = try PS5CollectionStorage.decode(encoder.encode(PS5CollectionStorage.Document(version: 1, games: [game, game], engine: nil))) })
        let relative = PS5CollectionGame(id: UUID().uuidString, name: "Example", path: "relative/game", createdAt: 0)
        try refuses("relative game path", { _ = try PS5CollectionStorage.decode(encoder.encode(PS5CollectionStorage.Document(version: 1, games: [relative], engine: nil))) })
        let foreignArtwork = PS5CollectionGame(id: UUID().uuidString, name: "Example", path: gameFolder.path, createdAt: 0, artworkPath: root.appendingPathComponent("unrelated.png").path)
        try refuses("artwork must stay beside selected game", { _ = try PS5CollectionStorage.decode(encoder.encode(PS5CollectionStorage.Document(version: 1, games: [foreignArtwork], engine: nil))) })
        try refuses("bounded document", { _ = try PS5CollectionStorage.decode(Data(repeating: 32, count: PS5CollectionStorage.maximumBytes + 1)) })
        let corruptDirectory = root.appendingPathComponent("corrupt", isDirectory: true)
        try FileManager.default.createDirectory(at: corruptDirectory, withIntermediateDirectories: true)
        let corruptFile = corruptDirectory.appendingPathComponent("ps5-collection.json")
        let corrupt = Data("{bad JSON".utf8)
        try corrupt.write(to: corruptFile)
        let corruptModel = PS5CollectionModel(storageDirectory: corruptDirectory)
        try expect(corruptModel.error != nil && corruptModel.games.isEmpty, "malformed storage reported")
        do { _ = try await corruptModel.importGame(name: "Example", path: gameFolder) } catch { checks += 1 }
        try expect(try Data(contentsOf: corruptFile) == corrupt, "malformed storage preserved")
        let linkedDirectory = root.appendingPathComponent("linked-data", isDirectory: true)
        try FileManager.default.createDirectory(at: linkedDirectory, withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(at: linkedDirectory.appendingPathComponent("ps5-collection.json"), withDestinationURL: documentURL)
        let linkedModel = PS5CollectionModel(storageDirectory: linkedDirectory)
        try expect(linkedModel.error != nil, "linked metadata refused")
        try refuses("engine shell script refused", {
            let fake = root.appendingPathComponent("kyty_emulator")
            try Data("#!/bin/sh\nexit 0\n".utf8).write(to: fake)
            chmod(fake.path, mode_t(0o700))
            _ = try PS5CollectionStorage.checkedEngine(fake)
        })

        // Original tiny binaries verify argument arrays and owned process lifecycle.
        // No emulator, commercial game, downloaded dependency or user profile is executed.
        let fixtureDirectory = root.appendingPathComponent("engine", isDirectory: true)
        try FileManager.default.createDirectory(at: fixtureDirectory, withIntermediateDirectories: true)
        let engineSource = fixtureDirectory.appendingPathComponent("engine.c")
        let librarySource = fixtureDirectory.appendingPathComponent("library.c")
        let enginePath = fixtureDirectory.appendingPathComponent("kyty_emulator")
        let libraryPath = fixtureDirectory.appendingPathComponent("libMoltenVK.dylib")
        try Data("""
        #include <signal.h>
        #include <stdio.h>
        #include <unistd.h>
        static volatile sig_atomic_t active = 1;
        static void stop(int signal) { active = 0; }
        int main(int argc, char **argv) {
          signal(SIGTERM, stop);
          printf("%d\\n%s\\n%s\\n", argc, argc > 1 ? argv[1] : "", argc > 2 ? argv[2] : "");
          fflush(stdout);
          char bytes[4096] = {0};
          for(int i=0; i<1050; i++) fwrite(bytes, 1, sizeof(bytes), stdout);
          fflush(stdout);
          while(active) usleep(20000);
          return 0;
        }
        """.utf8).write(to: engineSource)
        try Data("int caskrun_original_fixture(void) { return 1; }\n".utf8).write(to: librarySource)
        func compile(_ arguments: [String]) throws {
            let compiler = Process()
            compiler.executableURL = URL(fileURLWithPath: "/usr/bin/clang")
            compiler.arguments = arguments
            try compiler.run()
            compiler.waitUntilExit()
            guard compiler.terminationStatus == 0 else { throw PS5CollectionError.invalid("Fixture compilation failed.") }
        }
        try compile(["-arch", "arm64", engineSource.path, "-o", enginePath.path])
        try refuses("matching graphics dependency required", { _ = try PS5CollectionStorage.checkedEngine(enginePath) })
        try compile(["-arch", "arm64", "-dynamiclib", librarySource.path, "-o", libraryPath.path])
        try await model.configureEngine(path: enginePath)
        try expect(model.engine?.path == enginePath.path && model.engine?.version == "Not recorded", "attach structurally validated external engine without executing it")
        try await model.configureEngine(path: fixtureDirectory)
        try expect(model.engine?.path == enginePath.path, "explicit package folder resolves flat engine and its dependency")
        let bundle = root.appendingPathComponent("ExampleEngine.app", isDirectory: true)
        let bundleExecutables = bundle.appendingPathComponent("Contents/MacOS", isDirectory: true)
        let bundleFrameworks = bundle.appendingPathComponent("Contents/Frameworks", isDirectory: true)
        try FileManager.default.createDirectory(at: bundleExecutables, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: bundleFrameworks, withIntermediateDirectories: true)
        let bundledEngine = bundleExecutables.appendingPathComponent("kyty_emulator")
        try FileManager.default.copyItem(at: enginePath, to: bundledEngine)
        try FileManager.default.copyItem(at: libraryPath, to: bundleFrameworks.appendingPathComponent("libMoltenVK.dylib"))
        try await model.configureEngine(path: bundle)
        try expect(model.engine?.path == bundledEngine.path, "explicit app package resolves engine and Frameworks dependency")
        try await model.configureEngine(path: fixtureDirectory)
        try expect(PS5CollectionModel(storageDirectory: dataDirectory).engine == model.engine, "engine metadata round trip")
        let external = Process()
        external.executableURL = enginePath
        external.standardOutput = FileHandle.nullDevice
        external.standardError = FileHandle.nullDevice
        func stopExternalFixture() async -> Bool {
            if external.isRunning { external.terminate() }
            let deadline = ProcessInfo.processInfo.systemUptime + 3
            while external.isRunning && ProcessInfo.processInfo.systemUptime < deadline {
                try? await Task.sleep(nanoseconds: 40_000_000)
            }
            return !external.isRunning
        }
        ownedProcessesStopped = false
        do {
            try external.run()
            let logsDirectory = dataDirectory.appendingPathComponent("ps5-sessions", isDirectory: true)
            try FileManager.default.createDirectory(at: logsDirectory, withIntermediateDirectories: true)
            let untouched = root.appendingPathComponent("untouched")
            let untouchedData = Data("original".utf8)
            try untouchedData.write(to: untouched)
            let linkedLog = logsDirectory.appendingPathComponent("\(game.id).log")
            guard link(untouched.path, linkedLog.path) == 0 else { throw PS5CollectionError.invalid("Fixture hard link failed.") }
            try refuses("linked log rejected before truncation", { try model.launch(game) })
            try expect(try Data(contentsOf: untouched) == untouchedData && model.runningGameIDs.isEmpty,
                       "log refusal preserves the original file and starts no process")
            try FileManager.default.removeItem(at: linkedLog)
            try model.launch(game)
            try expect(model.runningGameIDs == [game.id], "owned session tracked")
            try model.launch(game)
            try expect(model.runningGameIDs.count == 1, "repeated launch does not duplicate engine")
            try refuses("running game cannot be removed", { try model.removeGame(game) })
            do { try await model.configureEngine(path: enginePath) } catch { checks += 1 }
            let logDeadline = ProcessInfo.processInfo.systemUptime + 3
            var logData = Data()
            while ProcessInfo.processInfo.systemUptime < logDeadline {
                if let log = model.logsURL(for: game), let data = try? Data(contentsOf: log) { logData = data }
                if logData.count == 4 * 1024 * 1024 { break }
                try await Task.sleep(nanoseconds: 40_000_000)
            }
            let expectedHeader = Data("3\n--game\n\(game.path)\n".utf8)
            try expect(logData.starts(with: expectedHeader), "literal selected game argument is passed without shell interpretation")
            try expect(logData.count == 4 * 1024 * 1024, "session output is capped at four MiB")
            try await model.stop(game)
            try expect(model.runningGameIDs.isEmpty && external.isRunning, "stop preserves independent process")
            try expect(model.error == nil, "requested termination is not reported as an engine crash")
            try model.launch(game)
            let shutdown = await model.shutdownForQuit()
            try expect(shutdown && model.runningGameIDs.isEmpty && external.isRunning, "quit stops only model-owned session")
            try refuses("successful shutdown prevents new launch", { try model.launch(game) })
            try restored.removeGame(game)
            try expect(restored.games.isEmpty && FileManager.default.fileExists(atPath: gameFolder.appendingPathComponent("eboot.bin").path), "remove only changes collection metadata")
            let externalStopped = await stopExternalFixture()
            ownedProcessesStopped = shutdown && externalStopped
            try expect(ownedProcessesStopped, "all explicitly owned fixture processes stopped before cleanup")
        } catch {
            let modelStopped = await model.shutdownForQuit()
            let externalStopped = await stopExternalFixture()
            ownedProcessesStopped = modelStopped && externalStopped
            throw error
        }
        print("PS5 collection checks passed: \(checks)")
    }
}
