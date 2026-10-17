import Foundation
import CoreGraphics
import Darwin

@main
struct SteamStartupTests {
    static func main() throws {
        var checks = 0
        func expect(_ value: @autoclosure () -> Bool, _ message: String) {
            precondition(value(), message)
            checks += 1
        }
        let bottle = "11111111-1111-4111-8111-111111111111"
        let other = "22222222-2222-4222-8222-222222222222"
        let ownID = "dev.caskrun.windows.11111111111141118111111111111111." + String(repeating: "a", count: 32)
        func app(_ identifier: String? = nil, name: String = "Steam", executable: String = "WindowsApp",
                 pid: Int32 = 10, terminated: Bool = false) -> SteamStartup.Application {
            .init(pid: pid, bundleID: identifier ?? ownID, name: name,
                  executableName: executable, terminated: terminated)
        }
        func window(pid: Int32 = 10, layer: Int = 0, onScreen: Bool = true,
                    width: Double = 706, height: Double = 438, alpha: Double = 1) -> SteamStartup.Window {
            .init(ownerPID: pid, layer: layer, onScreen: onScreen, width: width, height: height, alpha: alpha)
        }
        func visible(_ applications: [SteamStartup.Application], _ windows: [SteamStartup.Window]) -> Bool {
            SteamStartup.hasVisibleClientWindow(applications: applications, windows: windows,
                                                bottleID: bottle, displayName: "Steam")
        }
        expect(!visible([app()], []), "a launched process alone never hides Caskrun")
        expect(visible([app()], [window()]), "the owned Steam sign-in window permits the handoff")
        expect(!visible([], [window()]), "an unowned window cannot finish startup")
        expect(!visible([app()], [window(pid: 11)]), "a different process cannot finish startup")
        expect(!visible([app("com.valvesoftware.steam")], [window()]), "native Steam is unrelated")
        expect(!SteamStartup.ownsSteamApplication(app(), bottleID: other, displayName: "Steam"),
               "another bottle cannot satisfy startup")
        expect(!SteamStartup.ownsSteamApplication(app(), bottleID: "invalid", displayName: "Steam"),
               "invalid bottle identifiers cannot broaden matching")
        expect(!visible([app(ownID + ".child")], [window()]), "bundle identity must match the exact alias format")
        expect(!visible([app(ownID.dropLast().description + "z")], [window()]), "alias key must be hexadecimal")
        expect(!visible([app(name: "Wine")], [window()]), "other owned helpers do not count as Steam")
        expect(!visible([app(executable: "wine")], [window()]), "unaliased Wine does not count")
        expect(SteamStartup.hasVisibleClientWindow(applications: [app(name: "Example Steam")], windows: [window()],
                                                   bottleID: bottle, displayName: "Example Steam"),
               "the core's renamed Steam alias is matched using its saved launcher label")
        expect(!visible([app(name: "Example Steam")], [window()]),
               "another alias name does not satisfy the requested Steam launcher")
        expect(SteamStartup.hasVisibleClientWindow(applications: [app()], windows: [window()],
                                                   bottleID: bottle, displayName: " \tSteam.exe\u{2003}"),
               "a saved executable suffix and Unicode whitespace resolve to the actual Steam alias")
        expect(SteamStartup.normalizedDisplayName("Example Steam .exe") == "Example Steam",
               "whitespace before the removed suffix matches the core's second trim")
        expect(SteamStartup.normalizedDisplayName("Steam.EXE") == "Steam.EXE",
               "the core strips only a lowercase suffix")
        expect(SteamStartup.normalizedDisplayName("Steam.exe.exe") == "Steam.exe",
               "the core strips exactly one suffix")
        expect(SteamStartup.hasVisibleClientWindow(applications: [app(name: "Steam.exe")], windows: [window()],
                                                   bottleID: bottle, displayName: "Steam.exe.exe"),
               "the already-normalized OS name is not stripped a second time")
        expect(SteamStartup.normalizedDisplayName(".exe") == nil,
               "an empty normalized name is refused")
        expect(SteamStartup.normalizedDisplayName("Ste\n am") == nil,
               "embedded control characters are refused")
        expect(SteamStartup.normalizedDisplayName("Ste\u{202e}am") == nil,
               "bidi controls are refused")
        expect(SteamStartup.normalizedDisplayName(String(repeating: "x", count: 121)) == nil,
               "overlong labels match the core's refusal")
        expect(!visible([app(terminated: true)], [window()]), "terminated aliases are stale evidence")
        expect(!visible([app()], [window(onScreen: false)]), "a hidden window is not ready")
        expect(!visible([app()], [window(layer: 8)]), "a popup or menu is not the main window")
        expect(!visible([app()], [window(width: 560, height: 240)]), "small updater windows keep the progress visible")
        expect(!visible([app()], [window(alpha: 0)]), "transparent windows are not ready")
        expect(!visible([app()], [window(width: .infinity)]), "invalid geometry cannot finish startup")
        expect(visible([app(pid: 11), app()], [window()]), "a visible helper may own the main window")
        expect(SteamStartup.isSteamLauncher(name: "Renamed", path: "/example/client.exe", registered: true),
               "the core's exact saved Steam-client record handles custom names")
        expect(SteamStartup.isSteamLauncher(name: "Steam", path: "/example/prefix/drive_c/Program Files (x86)/Steam/Steam.exe", registered: false),
               "conventional older Steam records have a narrow fallback")
        expect(SteamStartup.isSteamLauncher(name: " Steam.exe ", path: "/example/prefix/drive_c/Program Files (x86)/Steam/Steam.exe", registered: false),
               "older client recognition uses the same normalized native label")
        expect(!SteamStartup.isSteamLauncher(name: "Steam", path: "/example/Steam.exe", registered: false),
               "an arbitrary program named Steam is not reclassified")
        expect(!SteamStartup.isSteamLauncher(name: "Other", path: "/example/prefix/drive_c/Program Files/Steam/Steam.exe", registered: false),
               "fallback requires both Steam name and conventional location")
        expect(SteamStartup.Progress(elapsed: .seconds(1)).title == "Starting Steam", "startup has immediate feedback")
        expect(SteamStartup.Progress(elapsed: .seconds(15)).title == "Waiting for Steam",
               "elapsed time alone never claims an update is happening")
        expect(SteamStartup.timeout == .seconds(180), "startup wait has a finite deadline")
        var monitor = SteamStartup.Monitor()
        let initial = monitor.begin()
        expect(monitor.isCurrent(initial), "a fresh monitor can complete its own handoff")
        monitor.stop()
        expect(!monitor.isCurrent(initial) && !monitor.isWaiting,
               "Stop waiting invalidates pending observations without a process operation")
        let next = monitor.begin()
        expect(next != initial && monitor.isCurrent(next) && !monitor.isCurrent(initial),
               "a later open cannot accept an earlier monitor's completion")
        monitor.finish(initial)
        expect(monitor.isCurrent(next), "an old monitor's defer cannot finish the newer monitor")
        monitor.finish(next)
        expect(!monitor.isWaiting, "a completed monitor releases the waiting state")
        expect(SteamStartup.stoppedWaitingMessage.contains("Steam was left running"),
               "stopping observation does not claim Steam stopped or setup failed")
        expect(SteamStartup.MonitorError.timedOut != .stopped,
               "a timeout has its own monitor outcome rather than an installer failure")
        expect(SteamStartup.MonitorError.timedOut.message == SteamStartup.timeoutMessage &&
               SteamStartup.MonitorError.stopped.message == SteamStartup.stoppedWaitingMessage,
               "both monitor outcomes carry startup-only feedback after successful setup")
        let clientIDs = ["steam", "epic-games-store", "gog-galaxy", "battle-net", "ea-app",
                         "ubisoft-connect", "rockstar-games-launcher", "itch-io-app"]
        expect(Set(clientIDs.compactMap(SteamStartup.Client.catalog)) == Set(SteamStartup.Client.allCases),
               "every current catalog store has neutral startup feedback")
        expect(SteamStartup.Client.catalog("arbitrary") == nil, "unrecognized catalog entries are not guessed")
        expect(SteamStartup.Progress(elapsed: .seconds(30), client: .gog).title == "Waiting for GOG Galaxy",
               "other stores never inherit Steam updater messages")
        expect(SteamStartup.Progress(elapsed: .seconds(30), client: .gog, update: .installing).update == nil,
               "Steam-specific evidence is never applied to another store")
        expect(SteamStartup.MonitorError(outcome: .stopped, client: .epic).message.contains("Epic Games") &&
               !SteamStartup.MonitorError(outcome: .stopped, client: .epic).message.contains("Steam"),
               "a stopped catalog monitor reports its actual store")
        expect(SteamStartup.MonitorError(outcome: .timedOut, client: .ea).message.contains("left its apps running"),
               "a catalog timeout never claims to have stopped the store")
        expect(SteamStartup.clientForLauncher(name: "Renamed", path: "/example/prefix/drive_c/Program Files/GOG Galaxy/GalaxyClient.exe", registeredSteam: false) == .gog,
               "conventional store paths support renamed saved launchers")
        expect(SteamStartup.clientForLauncher(name: "Epic Games", path: "/example/EpicGamesLauncher.exe", registeredSteam: false) == nil,
               "arbitrary executable names cannot acquire a store monitor")
        expect(SteamStartup.clientForLauncher(name: "itch.io", path: "/example/prefix/drive_c/users/example/AppData/Local/itch/itch-setup.exe", registeredSteam: false) == .itch,
               "the catalog's stable itch bootstrap is recognized inside its Windows profile")
        expect(SteamStartup.clientForLauncher(name: "itch.io", path: "/example/AppData/Local/itch/itch-setup.exe", registeredSteam: false) == nil,
               "an arbitrary itch-like path is not a catalog client")
        var info: [String: Any] = [kCGWindowOwnerPID as String: 10, kCGWindowLayer as String: 0,
                                  kCGWindowBounds as String: ["Width": 706, "Height": 438], kCGWindowAlpha as String: 1]
        expect(SteamStartup.onScreenWindow(info)?.onScreen == true,
               "an already on-screen-filtered query can omit the redundant on-screen key")
        info[kCGWindowIsOnscreen as String] = false
        expect(SteamStartup.onScreenWindow(info)?.onScreen == false, "explicit hidden metadata is respected")
        info[kCGWindowIsOnscreen as String] = "invalid"
        expect(SteamStartup.onScreenWindow(info) == nil, "malformed on-screen metadata is refused")
        info.removeValue(forKey: kCGWindowIsOnscreen as String)
        info.removeValue(forKey: kCGWindowAlpha as String)
        expect(SteamStartup.onScreenWindow(info) == nil, "required alpha metadata is not guessed")

        typealias Update = SteamStartup.Update
        expect(Update.parse("[2026-01-01 12:00:01] Downloading update (1,024 of 238,534 KB)") ==
               .downloading(receivedKB: 1_024, totalKB: 238_534), "publisher counts are parsed without displaying raw log text")
        expect(Update.parse("Downloading update (238,534 of 238,534 KB)...") ==
               .downloading(receivedKB: 238_534, totalKB: 238_534), "publisher ellipsis form is supported")
        expect(Update.parse("Downloading update (1 of 2 KB)…") == .downloading(receivedKB: 1, totalKB: 2),
               "a typographic ellipsis remains a fixed known form")
        expect(Update.parse("Download complete.") == .installing, "download completion is not client-window readiness")
        expect(Update.parse("Installing update...") == .installing, "installation has its own phase")
        expect(Update.parse("Extracting package...") == .extracting, "extraction has its own phase")
        expect(Update.parse("Update complete, launching Steam...") == .opening, "update completion still waits for a window")
        expect(Update.parse("Verifying installation...") == .checking, "verification is distinct from downloading")
        expect(Update.parse("Nothing to do") == .opening, "a verified up-to-date client transitions to opening")
        expect(Update.parse("Verification complete") == .opening, "completed verification clears the checking phase")
        for malformed in ["Downloading update (2 of 1 KB)", "Downloading update (0 of 0 KB)",
                          "Downloading update (-1 of 10 KB)", "Downloading update (1,00 of 100 KB)",
                          "Downloading update (1 of 1000000001 KB)", "Downloading update (1 of 18446744073709551615 KB)",
                          "Downloading update (1 of 2 KB) private text", "[not a timestamp] Installing update...",
                          "user@example.invalid: Installing update...", String(repeating: "x", count: 2049)] {
            expect(Update.parse(malformed) == nil, "unknown or invalid publisher lines cannot become progress")
        }
        expect(SteamStartup.Progress(elapsed: .zero, update: .downloading(receivedKB: 1, totalKB: 4)).title == "Updating Steam · 25%",
               "only verified numeric progress produces a percentage")
        expect(SteamStartup.Progress(elapsed: .zero, update: .downloading(receivedKB: 4, totalKB: 0)).update == nil,
               "invalid typed counters cannot cause division by zero")

        let manager = FileManager.default
        let fixture = URL(fileURLWithPath: manager.currentDirectoryPath).resolvingSymlinksInPath()
            .appendingPathComponent(".build/steam-startup-fixture-" + UUID().uuidString, isDirectory: true)
        try manager.createDirectory(at: fixture, withIntermediateDirectories: true)
        defer { try? manager.removeItem(at: fixture) }
        let prefix = fixture.appendingPathComponent("prefix", isDirectory: true)
        let steam = prefix.appendingPathComponent("drive_c/Program Files (x86)/Steam", isDirectory: true)
        let logs = steam.appendingPathComponent("logs", isDirectory: true)
        try manager.createDirectory(at: logs, withIntermediateDirectories: true)
        let log = logs.appendingPathComponent("bootstrap_log.txt")
        let launcher = steam.appendingPathComponent("Steam.exe").path
        func capture() -> SteamStartup.UpdateLog? {
            SteamStartup.UpdateLog.capture(prefixPath: prefix.path, launcherPath: launcher)
        }
        func append(_ text: String) throws {
            let file = try FileHandle(forWritingTo: log)
            defer { try? file.close() }
            try file.seekToEnd()
            try file.write(contentsOf: Data(text.utf8))
        }
        try Data("Downloading update (10 of 20 KB)\nInstalling update...\n".utf8).write(to: log)
        var cursor = capture()!
        expect(cursor.poll() == nil, "historical phases are never replayed on Open")
        try append("Downloading update (2 of 20 KB)\n")
        expect(cursor.poll() == .downloading(receivedKB: 2, totalKB: 20), "new current-session counters are observed")
        try append("Extracting pack")
        expect(cursor.poll() == .downloading(receivedKB: 2, totalKB: 20), "a partial line never becomes an event")
        try append("age...\n")
        expect(cursor.poll() == .extracting, "a split complete publisher line is assembled once")
        try append("private account text\n")
        expect(cursor.poll() == .extracting, "unknown lines do not escape the parser")
        try append("Update complete, launching Steam...\nVerifying installation...\nNothing to do\n")
        expect(cursor.poll() == .opening, "the latest complete current event wins, without claiming a window")
        try Data("Installing update...".utf8).write(to: log)
        var partialBaseline = capture()!
        try append("\nExtracting package...\n")
        expect(partialBaseline.poll() == .extracting, "an old incomplete baseline line is discarded")
        try manager.removeItem(at: log)
        var missing = capture()!
        try Data("Downloading update (3 of 20 KB)\n".utf8).write(to: log)
        try manager.setAttributes([.modificationDate: Date().addingTimeInterval(1)], ofItemAtPath: log.path)
        expect(missing.poll() == .downloading(receivedKB: 3, totalKB: 20), "a newly created log can provide fresh evidence")
        try manager.moveItem(at: log, to: logs.appendingPathComponent("old.log"))
        try Data("Installing update...\n".utf8).write(to: log)
        try manager.setAttributes([.modificationDate: Date(timeIntervalSince1970: 1)], ofItemAtPath: log.path)
        expect(missing.poll() == .downloading(receivedKB: 3, totalKB: 20), "an old replacement log cannot overwrite current progress")
        try append("Extracting package...\n")
        expect(missing.poll() == .extracting, "a later append does not replay the replacement's earlier lines")
        var bounded = capture()!
        try append(String(repeating: "x", count: 40_000) + "\nDownloading update (5 of 20 KB)\n")
        expect(bounded.poll() == .downloading(receivedKB: 5, totalKB: 20), "bounded tail reading discards a large partial leading line")
        expect(SteamStartup.UpdateLog.capture(prefixPath: prefix.path, launcherPath: fixture.appendingPathComponent("Steam.exe").path) == nil,
               "the probe never reads logs outside the verified bottle's Steam directory")
        expect(SteamStartup.UpdateLog.capture(prefixPath: prefix.path + "/.", launcherPath: launcher) == nil,
               "noncanonical prefix paths are refused")
        let external = fixture.appendingPathComponent("external.log")
        try Data("Installing update...\n".utf8).write(to: external)
        try manager.removeItem(at: log)
        try manager.createSymbolicLink(atPath: log.path, withDestinationPath: external.path)
        expect(capture() == nil, "a final log symlink is refused")
        expect(bounded.poll() == .downloading(receivedKB: 5, totalKB: 20), "polling never follows a newly substituted symlink")
        try manager.removeItem(at: log)
        try manager.removeItem(at: logs)
        let externalDirectory = fixture.appendingPathComponent("external-directory", isDirectory: true)
        try manager.createDirectory(at: externalDirectory, withIntermediateDirectories: false)
        try Data("Installing update...\n".utf8).write(to: externalDirectory.appendingPathComponent("bootstrap_log.txt"))
        try manager.createSymbolicLink(atPath: logs.path, withDestinationPath: externalDirectory.path)
        expect(capture() == nil, "a log-directory symlink is refused")
        try manager.removeItem(at: logs)
        try manager.createDirectory(at: logs, withIntermediateDirectories: false)
        expect(mkfifo(log.path, 0o600) == 0, "an owned nonblocking log fixture was created")
        expect(capture() == nil, "a named pipe is refused without waiting for a writer")
        try manager.removeItem(at: log)
        try manager.linkItem(at: external, to: log)
        expect(capture() == nil, "a multiply linked log cannot cross the bottle boundary")
        print("Steam startup: \(checks) checks passed")
    }
}
