import AppKit
import CoreGraphics

/// A spawned updater is not proof that Steam's main interface opened. These
/// checks use only process identity and public window geometry; they neither
/// read window titles nor request Accessibility or Screen Recording access.
enum SteamStartup {
    static let timeout: Duration = .seconds(180)
    static let stoppedWaitingMessage = "Stopped waiting for Steam’s window. Steam was left running and may still be updating. You can keep using Caskrun."

    static let timeoutMessage = "Steam has not opened its main window yet. It may still be updating. Caskrun kept this window open; check this bottle’s logs before trying Open Steam again."

    enum MonitorError: Error, Equatable {
        case stopped, timedOut

        var message: String {
            switch self {
            case .stopped: SteamStartup.stoppedWaitingMessage
            case .timedOut: SteamStartup.timeoutMessage
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
        var title: String { elapsed < .seconds(15) ? "Starting Steam" : "Steam is starting or updating" }
        var detail: String {
            "Waiting for Steam’s main window. Updates can take a few minutes; there’s no need to open it again."
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

    @MainActor
    static func hasVisibleClientWindow(bottleID: String, displayName: String) -> Bool {
        let applications = NSWorkspace.shared.runningApplications.map {
            Application(pid: $0.processIdentifier, bundleID: $0.bundleIdentifier,
                        name: $0.localizedName, executableName: $0.executableURL?.lastPathComponent,
                        terminated: $0.isTerminated)
        }
        guard applications.contains(where: {
            ownsSteamApplication($0, bottleID: bottleID, displayName: displayName)
        }), let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements],
                                                 kCGNullWindowID) as? [[String: Any]] else { return false }
        let windows: [Window] = info.compactMap { item in
            guard let pid = item[kCGWindowOwnerPID as String] as? NSNumber,
                  let layer = item[kCGWindowLayer as String] as? NSNumber,
                  let onScreen = item[kCGWindowIsOnscreen as String] as? NSNumber,
                  let bounds = item[kCGWindowBounds as String] as? [String: Any],
                  let width = bounds["Width"] as? NSNumber, let height = bounds["Height"] as? NSNumber,
                  let alpha = item[kCGWindowAlpha as String] as? NSNumber else { return nil }
            return Window(ownerPID: pid.int32Value, layer: layer.intValue, onScreen: onScreen.boolValue,
                          width: width.doubleValue, height: height.doubleValue, alpha: alpha.doubleValue)
        }
        return hasVisibleClientWindow(applications: applications, windows: windows,
                                      bottleID: bottleID, displayName: displayName)
    }
}
