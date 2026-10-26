import AppKit
import CoreGraphics
import Darwin
import OSLog

/// A spawned updater is not proof that Steam's main interface opened. These
/// checks use only process identity and public window geometry; they neither
/// read window titles nor request Accessibility or Screen Recording access.
enum SteamStartup {
    static let timeout: Duration = .seconds(180)
    static let stoppedWaitingMessage = "Stopped waiting for Steam’s window. Steam was left running and may still be updating. You can keep using Caskrun."

    static let timeoutMessage = "Steam has not opened its main window yet. It may still be updating. Caskrun kept this window open; check this bottle’s logs before trying Open Steam again."

    enum Client: String, CaseIterable, Sendable, Hashable {
        case steam, epic, gog, battleNet, ea, ubisoft, rockstar, itch

        var name: String {
            switch self {
            case .steam: "Steam"
            case .epic: "Epic Games"
            case .gog: "GOG Galaxy"
            case .battleNet: "Battle.net"
            case .ea: "EA app"
            case .ubisoft: "Ubisoft Connect"
            case .rockstar: "Rockstar Games Launcher"
            case .itch: "itch.io"
            }
        }

        static func catalog(_ id: String) -> Client? {
            switch id {
            case "steam": .steam
            case "epic-games-store": .epic
            case "gog-galaxy": .gog
            case "battle-net": .battleNet
            case "ea-app": .ea
            case "ubisoft-connect": .ubisoft
            case "rockstar-games-launcher": .rockstar
            case "itch-io-app": .itch
            default: nil
            }
        }
    }

    struct MonitorError: Error, Equatable {
        enum Outcome: Equatable { case stopped, timedOut }
        let outcome: Outcome
        let client: Client
        static let stopped = MonitorError(outcome: .stopped, client: .steam)
        static let timedOut = MonitorError(outcome: .timedOut, client: .steam)

        var message: String {
            if client == .steam {
                return outcome == .stopped ? SteamStartup.stoppedWaitingMessage : SteamStartup.timeoutMessage
            }
            switch outcome {
            case .stopped:
                return "Stopped waiting for \(client.name)’s window. Caskrun left its apps running; you can keep using Caskrun."
            case .timedOut:
                return "\(client.name) has not opened its main window yet. Its setup or sign-in may need attention. Caskrun left its apps running; check this bottle’s logs before opening it again."
            }
        }
    }

    /// Invalidating a monitor never cancels the Core launch or owns its process.
    /// A late observation from an earlier wait cannot finish a newer handoff.
    struct Monitor {
        private(set) var generation: UInt64 = 0
        private(set) var isWaiting = false

        mutating func begin() -> UInt64 {
            generation &+= 1
            isWaiting = true
            return generation
        }

        func isCurrent(_ token: UInt64) -> Bool { isWaiting && token == generation }

        mutating func stop() {
            generation &+= 1
            isWaiting = false
        }

        mutating func finish(_ token: UInt64) {
            if token == generation { isWaiting = false }
        }
    }

    struct Application: Sendable {
        let pid: Int32
        let bundleID: String?
        let name: String?
        let executableName: String?
        let terminated: Bool
    }

    struct Window: Sendable {
        let ownerPID: Int32
        let layer: Int
        let onScreen: Bool
        let width: Double
        let height: Double
        let alpha: Double
    }

    struct Progress: Sendable {
        let elapsed: Duration
        let client: Client
        let update: Update?

        init(elapsed: Duration, client: Client = .steam, update: Update? = nil) {
            self.elapsed = elapsed
            self.client = client
            self.update = client == .steam && update?.isValid != false ? update : nil
        }

        var title: String {
            switch update {
            case .checking: "Checking Steam installation"
            case let .downloading(receivedKB, totalKB): "Updating Steam · \(receivedKB * 100 / totalKB)%"
            case .installing: "Installing Steam update"
            case .extracting: "Extracting Steam update"
            case .opening: "Opening Steam"
            case nil: elapsed < .seconds(15) ? "Starting \(client.name)" : "Waiting for \(client.name)"
            }
        }
        var detail: String {
            switch update {
            case let .downloading(receivedKB, totalKB):
                "Downloading \(receivedKB.formatted()) of \(totalKB.formatted()) KB. Steam will open when its update finishes."
            case .installing, .extracting:
                "Steam is applying its update. There’s no need to open it again."
            case .checking:
                "Steam is checking its installed files. Waiting for its main window."
            case .opening:
                "Steam’s updater finished. Waiting for its main window."
            case nil:
                "Waiting for \(client.name)’s window. Setup or sign-in may take a moment; there’s no need to open it again."
            }
        }
    }

    static func clientForLauncher(name: String, path: String, registeredSteam: Bool) -> Client? {
        if isSteamLauncher(name: name, path: path, registered: registeredSteam) { return .steam }
        if isEAClientPath(path) { return .ea }
        let normalized = path.replacingOccurrences(of: "\\", with: "/").lowercased()
        let profileTail = Array(normalized.split(separator: "/").suffix(7))
        if profileTail.count == 7, profileTail[0] == "drive_c", profileTail[1] == "users",
           profileTail[2] != ".", profileTail[2] != "..", profileTail[3] == "appdata",
           profileTail[4] == "local", profileTail[5] == "itch", profileTail[6] == "itch-setup.exe" { return .itch }
        let locations: [(Client, [String])] = [
            (.epic, ["epic games/launcher/portal/binaries/win64/epicgameslauncher.exe",
                     "epic games/launcher/portal/binaries/win32/epicgameslauncher.exe",
                     "epic games/launcher/engine/binaries/win64/epicgameslauncher.exe"]),
            (.gog, ["gog galaxy/galaxyclient.exe"]),
            (.battleNet, ["battle.net/battle.net launcher.exe", "battle.net/battle.net.exe"]),
            (.ubisoft, ["ubisoft/ubisoft game launcher/ubisoftconnect.exe", "ubisoft/ubisoft game launcher/uplay.exe"]),
            (.rockstar, ["rockstar games/launcher/launcher.exe"])
        ]
        return locations.first { _, tails in
            tails.contains { tail in
                normalized.hasSuffix("/drive_c/program files/" + tail) ||
                    normalized.hasSuffix("/drive_c/program files (x86)/" + tail)
            }
        }?.0
    }

    /// A UI routing hint, not proof of a publisher executable. Core separately
    /// checks the file, PE and same-bottle ownership before every launch. Keep
    /// the numbered EA layout in step with publisher_install's version parser,
    /// so a self-update does not silently remove the existing startup feedback.
    static func isEAClientPath(_ path: String, prefixPath: String? = nil) -> Bool {
        func components(_ value: String) -> [String]? {
            guard value.hasPrefix("/"), value.utf8.count <= 4096,
                  !value.contains("\\"),
                  !value.unicodeScalars.contains(where: { $0.properties.generalCategory == .control }) else { return nil }
            let parts = value.dropFirst().split(separator: "/", omittingEmptySubsequences: false).map(String.init)
            guard !parts.isEmpty, parts.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." }) else { return nil }
            return parts
        }
        guard let parts = components(path) else { return false }
        let program: [String]
        if let prefixPath {
            guard let prefix = components(prefixPath), prefix.last == "prefix",
                  parts.count > prefix.count + 1, Array(parts.prefix(prefix.count)) == prefix,
                  parts[prefix.count] == "drive_c" else { return false }
            program = Array(parts.dropFirst(prefix.count + 1)).map { $0.lowercased() }
        } else {
            let lower = parts.map { $0.lowercased() }
            guard let drive = lower.lastIndex(of: "drive_c"), drive > 0,
                  lower[drive - 1] == "prefix" else { return false }
            program = Array(lower.dropFirst(drive + 1))
        }
        guard program.count == 5 || program.count == 6,
              ["program files", "program files (x86)"].contains(program[0]),
              program[1] == "electronic arts", program[2] == "ea desktop",
              program.suffix(2) == ["ea desktop", "eadesktop.exe"] else { return false }
        if program.count == 5 { return true }
        let version = program[3]
        let numbers = version.split(separator: ".", omittingEmptySubsequences: false)
        return version.utf8.count <= 64 && (2...6).contains(numbers.count) && numbers.allSatisfy {
            !$0.isEmpty && $0.utf8.allSatisfy { (48...57).contains($0) } && UInt32($0) != nil
        }
    }

    /// Only the publisher's bootstrap phase vocabulary leaves this parser. No
    /// arbitrary log text, local path, account name or connection data is shown.
    enum Update: Equatable, Sendable {
        case checking, downloading(receivedKB: UInt64, totalKB: UInt64), installing, extracting, opening

        var isValid: Bool {
            if case let .downloading(received, total) = self {
                return total > 0 && total <= 1_000_000_000 && received <= total
            }
            return true
        }

        static func parse(_ line: String) -> Update? {
            guard line.utf8.count <= 2048 else { return nil }
            var event = line.trimmingCharacters(in: .whitespacesAndNewlines)
            // Steam prefixes events with [YYYY-MM-DD HH:MM:SS]. Accept that
            // fixed prefix or a bare event for the bounded parser's fixtures.
            if event.first == "[" {
                guard event.utf8.count >= 22 else { return nil }
                let prefix = Array(event.utf8.prefix(22))
                let punctuation: [Int: UInt8] = [0: 91, 5: 45, 8: 45, 11: 32, 14: 58, 17: 58, 20: 93, 21: 32]
                guard prefix.enumerated().allSatisfy({ index, byte in
                    punctuation[index].map { $0 == byte } ?? (48...57).contains(byte)
                }) else { return nil }
                event = String(event.dropFirst(22)).trimmingCharacters(in: .whitespaces)
            }
            switch event {
            case "Verifying installation...", "Verifying installation…": return .checking
            case "Download complete.", "Installing update...", "Installing update…": return .installing
            case "Extracting package...", "Extracting package…": return .extracting
            case "Update complete, launching Steam...", "Update complete, launching Steam…", "Nothing to do", "Verification complete": return .opening
            default: break
            }
            if event.hasSuffix("...") { event.removeLast(3) }
            else if event.hasSuffix("…") { event.removeLast() }
            let prefix = "Downloading update ("
            guard event.hasPrefix(prefix), event.hasSuffix(" KB)") else { return nil }
            let values = event.dropFirst(prefix.count).dropLast(4).components(separatedBy: " of ")
            guard values.count == 2, let received = number(values[0]), let total = number(values[1]) else { return nil }
            let update = Update.downloading(receivedKB: received, totalKB: total)
            return update.isValid ? update : nil
        }

        private static func number(_ text: String) -> UInt64? {
            let groups = text.split(separator: ",", omittingEmptySubsequences: false)
            guard !groups.isEmpty, (1...3).contains(groups[0].count) || groups.count == 1,
                  groups.dropFirst().allSatisfy({ $0.count == 3 }),
                  groups.allSatisfy({ !$0.isEmpty && $0.utf8.allSatisfy { (48...57).contains($0) } }),
                  text.utf8.count <= 16 else { return nil }
            return UInt64(groups.joined())
        }
    }

    /// A per-Open cursor. It starts at the existing file's end, so a prior
    /// session's download or completed-update line cannot become live progress.
    /// Poll on a utility task; at most 32 KiB is read, including after rotation.
    struct UpdateLog: Sendable {
        private struct Mark: Sendable {
            let device: dev_t
            let inode: ino_t
            let size: off_t
            let modifiedSeconds: Int
            let modifiedNanos: Int
            let changedSeconds: Int
            let changedNanos: Int

            init(_ value: stat) {
                device = value.st_dev; inode = value.st_ino; size = value.st_size
                modifiedSeconds = value.st_mtimespec.tv_sec; modifiedNanos = value.st_mtimespec.tv_nsec
                changedSeconds = value.st_ctimespec.tv_sec; changedNanos = value.st_ctimespec.tv_nsec
            }

            func sameFile(_ other: Mark) -> Bool { device == other.device && inode == other.inode }
            func unchanged(_ other: Mark) -> Bool {
                sameFile(other) && size == other.size && modifiedSeconds == other.modifiedSeconds &&
                    modifiedNanos == other.modifiedNanos && changedSeconds == other.changedSeconds &&
                    changedNanos == other.changedNanos
            }

            var modified: TimeInterval { Double(modifiedSeconds) + Double(modifiedNanos) / 1_000_000_000 }
        }

        private static let readLimit = 32 * 1024
        private static let noFollowAny: Int32 = 0x2000_0000
        private let path: String
        private let beganAt: TimeInterval
        private var mark: Mark?
        private var pending = Data()
        private var discardPartialLine = false
        private var latest: Update?

        static func capture(prefixPath: String, launcherPath: String) -> UpdateLog? {
            guard prefixPath.hasPrefix("/"), !prefixPath.contains("\0"), !launcherPath.contains("\0"),
                  URL(fileURLWithPath: prefixPath).standardizedFileURL.path == prefixPath,
                  URL(fileURLWithPath: launcherPath).standardizedFileURL.path == launcherPath,
                  launcherPath.hasPrefix(prefixPath + "/") else { return nil }
            let relative = String(launcherPath.dropFirst(prefixPath.count)).lowercased()
            guard ["/drive_c/program files (x86)/steam/steam.exe", "/drive_c/program files/steam/steam.exe"].contains(relative) else { return nil }
            let path = URL(fileURLWithPath: launcherPath).deletingLastPathComponent()
                .appendingPathComponent("logs/bootstrap_log.txt").path
            var cursor = UpdateLog(path: path, beganAt: Date().timeIntervalSince1970)
            let fd = openLog(path)
            if fd < 0 { return errno == ENOENT ? cursor : nil }
            defer { Darwin.close(fd) }
            guard let before = inspect(fd) else { return nil }
            cursor.mark = before
            if before.size > 0 {
                var last: UInt8 = 0
                guard pread(fd, &last, 1, before.size - 1) == 1 else { return nil }
                cursor.discardPartialLine = last != 10
            }
            guard let after = inspect(fd), before.unchanged(after) else { return nil }
            return cursor
        }

        mutating func poll() -> Update? {
            let fd = Self.openLog(path)
            guard fd >= 0 else { return latest }
            defer { Darwin.close(fd) }
            guard let before = Self.inspect(fd) else { return latest }
            let continues = mark.map { $0.sameFile(before) && before.size >= $0.size } ?? false
            // An old rotated file or copied historical log is not current
            // evidence. Do not replay it even though it has a different inode.
            if !continues && before.modified < beganAt {
                // Remember the end even when this replacement is historical;
                // a later append must not make its earlier lines look fresh.
                var last: UInt8 = 10
                guard before.size == 0 || pread(fd, &last, 1, before.size - 1) == 1,
                      let after = Self.inspect(fd), before.unchanged(after) else { return latest }
                mark = after
                pending.removeAll(keepingCapacity: false)
                discardPartialLine = last != 10
                return latest
            }
            let wantedOffset: off_t = continues ? (mark?.size ?? 0) : 0
            guard before.size > wantedOffset else { return latest }
            let offset = max(wantedOffset, before.size - off_t(Self.readLimit))
            let count = Int(before.size - offset)
            var bytes = Data(count: count)
            let complete = bytes.withUnsafeMutableBytes { output in
                var consumed = 0
                while consumed < count {
                    let read = pread(fd, output.baseAddress!.advanced(by: consumed), count - consumed, offset + off_t(consumed))
                    if read < 0, errno == EINTR { continue }
                    guard read > 0 else { return false }
                    consumed += read
                }
                return true
            }
            guard complete, let after = Self.inspect(fd), before.unchanged(after) else { return latest }
            if !continues || offset > wantedOffset {
                pending.removeAll(keepingCapacity: false)
                discardPartialLine = offset > 0
            }
            mark = after
            pending.append(bytes)
            var start = pending.startIndex
            while let end = pending[start...].firstIndex(of: 10) {
                let line = pending[start..<end]
                if !discardPartialLine, line.count <= 2048, let text = String(data: line, encoding: .utf8),
                   let update = Update.parse(text) { latest = update }
                discardPartialLine = false
                start = pending.index(after: end)
            }
            if start > pending.startIndex { pending.removeSubrange(..<start) }
            if pending.count > 2048 {
                pending.removeAll(keepingCapacity: false)
                discardPartialLine = true
            }
            return latest
        }

        private static func openLog(_ path: String) -> Int32 {
            Darwin.open(path, O_RDONLY | O_CLOEXEC | O_NONBLOCK | noFollowAny)
        }

        private static func inspect(_ fd: Int32) -> Mark? {
            var value = stat()
            guard fstat(fd, &value) == 0, (value.st_mode & S_IFMT) == S_IFREG,
                  value.st_uid == geteuid(), value.st_nlink == 1,
                  value.st_size >= 0, value.st_size <= 1_073_741_824 else { return nil }
            return Mark(value)
        }
    }

    static func isSteamLauncher(name: String, path: String, registered: Bool) -> Bool {
        if registered { return true }
        // An arbitrary renamed program must not acquire Steam startup behavior.
        // Older snapshots may not yet have a saved Steam-client record.
        let normalized = path.replacingOccurrences(of: "\\", with: "/").lowercased()
        return normalizedDisplayName(name)?.caseInsensitiveCompare("Steam") == .orderedSame &&
            (normalized.hasSuffix("/drive_c/program files (x86)/steam/steam.exe") ||
             normalized.hasSuffix("/drive_c/program files/steam/steam.exe"))
    }

    /// Match native_app_identity::display_name: trim Unicode whitespace,
    /// reject control/bidi characters or more than 120 scalars, and remove one
    /// lowercase .exe suffix. The alias's actual OS name is already normalized.
    static func normalizedDisplayName(_ raw: String) -> String? {
        func trim(_ value: String) -> String {
            let scalars = value.unicodeScalars.drop(while: { $0.properties.isWhitespace })
                .reversed().drop(while: { $0.properties.isWhitespace }).reversed()
            return String(String.UnicodeScalarView(scalars))
        }
        var name = trim(raw)
        guard !name.isEmpty, name.unicodeScalars.count <= 120,
              !name.unicodeScalars.contains(where: {
                  $0.properties.generalCategory == .control ||
                      (0x202a...0x202e).contains($0.value) || (0x2066...0x2069).contains($0.value)
              }) else { return nil }
        if name.hasSuffix(".exe") {
            name.removeLast(4)
            name = trim(name)
        }
        return name.isEmpty ? nil : name
    }

    static func ownsSteamApplication(_ app: Application, bottleID: String, displayName: String) -> Bool {
        guard UUID(uuidString: bottleID) != nil, !app.terminated, app.pid > 0,
              app.executableName == "WindowsApp", let identifier = app.bundleID,
              let name = app.name, let expectedName = normalizedDisplayName(displayName),
              name.caseInsensitiveCompare(expectedName) == .orderedSame else { return false }
        let prefix = "dev.caskrun.windows.\(bottleID.replacingOccurrences(of: "-", with: "").lowercased())."
        guard identifier.hasPrefix(prefix) else { return false }
        let key = identifier.dropFirst(prefix.count)
        return key.count == 32 && key.allSatisfy { "0123456789abcdef".contains($0) }
    }

    static func hasVisibleClientWindow(applications: [Application], windows: [Window],
                                       bottleID: String, displayName: String) -> Bool {
        let owners = Set(applications.filter {
            ownsSteamApplication($0, bottleID: bottleID, displayName: displayName)
        }.map(\.pid))
        return windows.contains {
            owners.contains($0.ownerPID) && $0.onScreen && $0.layer == 0 && $0.alpha > 0 &&
                // Steam's sign-in and library windows qualify; a small updater,
                // tray popup or context menu must not hide Caskrun prematurely.
                $0.width >= 600 && $0.height >= 320 && $0.width.isFinite && $0.height.isFinite
        }
    }

    /// The query supplying this adapter already uses .optionOnScreenOnly.
    /// WindowServer may omit its redundant IsOnscreen key. Required identity,
    /// layer, alpha and bounds remain checked; explicit false is respected.
    static func onScreenWindow(_ item: [String: Any]) -> Window? {
        guard let pid = item[kCGWindowOwnerPID as String] as? NSNumber,
              let layer = item[kCGWindowLayer as String] as? NSNumber,
              let bounds = item[kCGWindowBounds as String] as? [String: Any],
              let width = bounds["Width"] as? NSNumber, let height = bounds["Height"] as? NSNumber,
              let alpha = item[kCGWindowAlpha as String] as? NSNumber else { return nil }
        let onScreen: Bool
        if let raw = item[kCGWindowIsOnscreen as String] {
            guard let value = raw as? NSNumber else { return nil }
            onScreen = value.boolValue
        } else { onScreen = true }
        return Window(ownerPID: pid.int32Value, layer: layer.intValue, onScreen: onScreen,
                      width: width.doubleValue, height: height.doubleValue, alpha: alpha.doubleValue)
    }

    struct Diagnostic: Equatable {
        let aliases: Int
        let queryAvailable: Bool
        let rawWindows: Int
        let droppedMetadata: Int
        let ownedNormalWindows: Int
        let sizeQualifiedWindows: Int
    }

    @MainActor private static var lastDiagnostic: Diagnostic?
    private static let logger = Logger(subsystem: "dev.caskrun.app", category: "store-startup")

    @MainActor
    private static func record(_ snapshot: Diagnostic) {
        guard snapshot != lastDiagnostic else { return }
        lastDiagnostic = snapshot
        logger.info("Startup observation: aliases=\(snapshot.aliases, privacy: .public) queryAvailable=\(snapshot.queryAvailable, privacy: .public) rawWindows=\(snapshot.rawWindows, privacy: .public) droppedMetadata=\(snapshot.droppedMetadata, privacy: .public) ownedNormalWindows=\(snapshot.ownedNormalWindows, privacy: .public) sizeQualifiedWindows=\(snapshot.sizeQualifiedWindows, privacy: .public)")
    }

    @MainActor
    static func hasVisibleClientWindow(bottleID: String, displayName: String) -> Bool {
        let applications = NSWorkspace.shared.runningApplications.map {
            Application(pid: $0.processIdentifier, bundleID: $0.bundleIdentifier,
                        name: $0.localizedName, executableName: $0.executableURL?.lastPathComponent,
                        terminated: $0.isTerminated)
        }
        let owners = Set(applications.filter {
            ownsSteamApplication($0, bottleID: bottleID, displayName: displayName)
        }.map(\.pid))
        guard !owners.isEmpty else {
            record(.init(aliases: 0, queryAvailable: false, rawWindows: 0, droppedMetadata: 0,
                         ownedNormalWindows: 0, sizeQualifiedWindows: 0))
            return false
        }
        guard let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements],
                                                   kCGNullWindowID) as? [[String: Any]] else {
            record(.init(aliases: owners.count, queryAvailable: false, rawWindows: 0, droppedMetadata: 0,
                         ownedNormalWindows: 0, sizeQualifiedWindows: 0))
            return false
        }
        let windows = info.compactMap(onScreenWindow)
        let normal = windows.filter { owners.contains($0.ownerPID) && $0.onScreen && $0.layer == 0 && $0.alpha > 0 }
        let qualified = normal.filter { $0.width >= 600 && $0.height >= 320 && $0.width.isFinite && $0.height.isFinite }
        record(.init(aliases: owners.count, queryAvailable: true, rawWindows: info.count,
                     droppedMetadata: info.count - windows.count, ownedNormalWindows: normal.count,
                     sizeQualifiedWindows: qualified.count))
        return !qualified.isEmpty
    }
}
