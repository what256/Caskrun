import Foundation

@main
struct SteamStartupTests {
    static func main() {
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
        expect(SteamStartup.Progress(elapsed: .seconds(15)).title == "Steam is starting or updating",
               "slow startup explains the updater instead of encouraging repeated opens")
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
        print("Steam startup: \(checks) checks passed")
    }
}
