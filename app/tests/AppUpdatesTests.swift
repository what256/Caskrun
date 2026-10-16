import AppKit
import Foundation

@main
struct AppUpdatesTests {
    @MainActor
    static func main() async throws {
        var checks = 0
        func expect(_ condition: Bool, _ message: String) {
            precondition(condition, message)
            checks += 1
        }
        func info(_ feed: String = "https://raw.githubusercontent.com/example/ExampleApp/main/appcast.xml") -> [String: Any] {
            ["SUFeedURL": feed, "SUPublicEDKey": Data(repeating: 0, count: 32).base64EncodedString(),
             "SURequireSignedFeed": true, "SUVerifyUpdateBeforeExtraction": true,
             "SUAllowsAutomaticUpdates": false]
        }
        guard case .success(let configuration) = CaskrunUpdateConfiguration.read(info()) else {
            fatalError("Synthetic public GitHub feed fixture was rejected")
        }
        expect(configuration.repositoryOwner == "example" && configuration.repositoryName == "ExampleApp",
               "release repository comes from the configured feed")
        expect(CaskrunUpdateConfiguration.read([:]) == .failure(.unconfigured),
               "source build has a distinct unconfigured result")
        for name in ["SUPublicEDKey", "SUFeedURL", "SURequireSignedFeed", "SUVerifyUpdateBeforeExtraction", "SUAllowsAutomaticUpdates"] {
            var metadata = info()
            metadata.removeValue(forKey: name)
            expect(CaskrunUpdateConfiguration.read(metadata) == .failure(.invalid),
                   "partial setup and missing signing protections are rejected")
        }
        for key in ["", "not a public key", Data(repeating: 0, count: 31).base64EncodedString(),
                    Data(repeating: 0, count: 33).base64EncodedString()] {
            var metadata = info()
            metadata["SUPublicEDKey"] = key
            expect(CaskrunUpdateConfiguration.read(metadata) == .failure(.invalid),
                   "malformed Ed25519 public keys are rejected")
        }
        for feed in ["http://github.com/example/ExampleApp/releases/latest/download/appcast.xml",
                     "https://example.org/example/ExampleApp/main/appcast.xml",
                     "https://github.com/example/ExampleApp/blob/main/appcast.xml",
                     "https://raw.githubusercontent.com/example/ExampleApp/main/feed.xml",
                     "https://raw.githubusercontent.com/example/ExampleApp/main/appcast.xml?account=example",
                     "https://raw.githubusercontent.com/example/ExampleApp/main/appcast.xml#fragment",
                     "https://raw.githubusercontent.com:8443/example/ExampleApp/main/appcast.xml",
                     "https://raw.githubusercontent.com/example/ExampleApp/main/../appcast.xml",
                     "https://raw.githubusercontent.com/example/ExampleApp/main/%61ppcast.xml",
                     "https://raw.githubusercontent.com/example//ExampleApp/main/appcast.xml"] {
            expect(CaskrunUpdateConfiguration.read(info(feed)) == .failure(.invalid),
                   "noncanonical or external update feed is rejected")
        }
        for feed in ["https://github.com/example/ExampleApp/releases/latest/download/appcast.xml",
                     "https://github.com/example/ExampleApp/releases/download/updates/appcast.xml"] {
            if case .success = CaskrunUpdateConfiguration.read(info(feed)) { checks += 1 }
            else { fatalError("Public GitHub release appcast fixture was rejected") }
        }
        for name in ["ExampleApp.dmg", "ExampleApp.zip", "ExampleApp.tar.xz", "ExampleApp.aar"] {
            expect(configuration.allowsArchive(URL(string: "https://github.com/example/ExampleApp/releases/download/v1/\(name)")),
                   "application archives from configured repository are allowed")
        }
        let deltaURL = URL(string: "https://github.com/example/ExampleApp/releases/download/v1/ExampleApp.delta")
        expect(configuration.allowsArchive(deltaURL, delta: true) && !configuration.allowsArchive(deltaURL),
               "a delta archive is allowed only in Sparkle's signed delta-update entry")
        for archive in ["https://github.com/example/OtherApp/releases/download/v1/ExampleApp.dmg",
                        "https://github.com/other/ExampleApp/releases/download/v1/ExampleApp.dmg",
                        "https://example.org/example/ExampleApp/releases/download/v1/ExampleApp.dmg",
                        "https://github.com/example/ExampleApp/releases/download/v1/ExampleApp.pkg",
                        "https://github.com/example/ExampleApp/releases/download/v1/ExampleApp.zip?token=example",
                        "https://github.com/example/ExampleApp/releases/latest/download/ExampleApp.zip"] {
            expect(!configuration.allowsArchive(URL(string: archive)),
                   "external downloads, installers, authenticated URLs and mutable asset URLs are refused")
        }
        expect(configuration.allowsNotes(nil), "no external release notes is valid")
        expect(configuration.allowsNotes(URL(string: "https://github.com/example/ExampleApp/releases/tag/v1")),
               "release information stays in the configured repository")
        expect(configuration.allowsNotes(URL(string: "https://raw.githubusercontent.com/example/ExampleApp/main/notes.md")),
               "raw release notes stay in the configured repository")
        expect(!configuration.allowsNotes(URL(string: "https://example.org/notes.html")),
               "external release note links are refused")

        func owners(_ status: Int32, _ text: String, allowed: Set<Int32> = []) -> CaskrunUpdateIdleResult {
            CaskrunUpdateFileOwners.classify(status: status, output: Data(text.utf8), allowedOwners: allowed)
        }
        expect(owners(1, "") == .idle, "lsof's documented no-owner result is idle")
        expect(owners(0, "p41\n", allowed: [41]) == .idle, "only this native app's own executable owner is allowed")
        expect(owners(0, "p41\nf3\nftxt\nfmem\n", allowed: [41]) == .idle,
               "lsof's mandatory descriptor fields retain their preceding owner")
        expect(owners(0, "p41\np42\n", allowed: [41]) == .busy, "a second app owner blocks replacement")
        expect(owners(0, "p41\n") == .busy, "any Wine/core owner blocks replacement")
        expect(owners(1, "p41\nf3\n") == .busy,
               "recursive lsof owners block replacement even when lsof returns one")
        for result in [owners(0, ""), owners(2, ""), owners(1, "inspection warning"),
                       owners(0, "p41\ninspection warning\n", allowed: [41]), owners(0, "p0\n"),
                       owners(0, "p-1\n"), owners(0, "pnot-a-number\n")] {
            expect(result == .unavailable, "ambiguous or failed file inspection fails closed")
        }
        // A generated, inert app tree exercises the real lsof subprocess. It
        // contains no application code or user data and is never launched.
        let fixtureRoot = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
            .appendingPathComponent(".build/update-tests/fixture-\(UUID().uuidString)")
        let fixtureApp = fixtureRoot.appendingPathComponent("Caskrun.app")
        let contents = fixtureApp.appendingPathComponent("Contents")
        let resources = contents.appendingPathComponent("Resources")
        let macos = contents.appendingPathComponent("MacOS")
        try FileManager.default.createDirectory(at: resources, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: macos, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: fixtureRoot) }
        let plist: [String: Any] = ["CFBundleIdentifier": "dev.caskrun.app", "CFBundleExecutable": "Caskrun"]
        try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
            .write(to: contents.appendingPathComponent("Info.plist"))
        try Data().write(to: resources.appendingPathComponent(".caskrun-generated-app"))
        for name in ["Caskrun", "caskrun-core", "caskrun-native-icon"] {
            try Data("inert fixture".utf8).write(to: macos.appendingPathComponent(name))
        }
        let ownPID = ProcessInfo.processInfo.processIdentifier
        let executable = macos.appendingPathComponent("Caskrun")
        func inspectFixture() async -> CaskrunUpdateIdleResult {
            await CaskrunUpdateFileOwners.verify(bundleURL: fixtureApp, executableURL: executable, ownPID: ownPID)
        }
        let noOwners = await inspectFixture()
        expect(noOwners == .idle, "real lsof confirms a complete unused app fixture is idle")
        let ownHandle = try FileHandle(forReadingFrom: executable)
        let ownOwner = await inspectFixture()
        expect(ownOwner == .idle, "real lsof permits the current native app's own executable handle")
        try ownHandle.close()
        let coreHandle = try FileHandle(forReadingFrom: macos.appendingPathComponent("caskrun-core"))
        let coreOwner = await inspectFixture()
        expect(coreOwner == .busy, "real lsof detects a core handle without signalling its owner")
        try coreHandle.close()
        let runtime = resources.appendingPathComponent("wine-runtime")
        try FileManager.default.createDirectory(at: runtime, withIntermediateDirectories: false)
        let runtimeFile = runtime.appendingPathComponent("inert-runtime-fixture")
        try Data("inert fixture".utf8).write(to: runtimeFile)
        let runtimeHandle = try FileHandle(forReadingFrom: runtimeFile)
        let runtimeOwner = await inspectFixture()
        expect(runtimeOwner == .busy, "real recursive lsof detects an engine handle without signalling its owner")
        try runtimeHandle.close()
        let releasedRuntime = await inspectFixture()
        expect(releasedRuntime == .idle, "closing the fixture handle allows a later independent idle check")
        try FileManager.default.removeItem(at: runtime)
        try FileManager.default.createSymbolicLink(atPath: runtime.path, withDestinationPath: "missing-runtime-fixture")
        let brokenRuntime = await inspectFixture()
        expect(brokenRuntime == .unavailable, "a dangling engine symlink is refused rather than treated as absent")
        var stopped = false
        var inspected = false
        let refused = await CaskrunUpdateInstallationGate.prepare(prepareForQuit: { false }, verifyIdle: {
            inspected = true
            return .idle
        })
        expect(refused == .busy && !inspected, "failed shutdown never proceeds to install verification")
        let busy = await CaskrunUpdateInstallationGate.prepare(prepareForQuit: {
            stopped = true
            return true
        }, verifyIdle: {
            expect(stopped, "shutdown precedes the independent file-owner check")
            return .busy
        })
        expect(busy == .busy, "file owners block continuation after app shutdown")

        var messages: [String] = []
        var shutdownAllowed = false
        var idleResult = CaskrunUpdateIdleResult.busy
        var preparations = 0
        var recoveries = 0
        var installed = 0
        let updates = CaskrunAppUpdates(prepareForQuit: {
            preparations += 1
            return shutdownAllowed
        }, verifyIdle: { idleResult }, recoverAfterInstallationFailure: {
            recoveries += 1
        }, reportMessage: { messages.append($0) })
        updates.checkForUpdates()
        expect(!messages.isEmpty && preparations == 0,
               "unconfigured build reports unavailable without claiming up-to-date or stopping apps")
        expect(!updates.requiresInstallationFileOwnerVerification,
               "ordinary Quit and a feed-only check never arm the app replacement guard")
        updates.postponeInstallation { installed += 1 }
        expect(updates.requiresInstallationFileOwnerVerification,
               "accepted installation arms replacement verification before its async shutdown starts")
        updates.checkForUpdates()
        updates.postponeInstallation { installed += 100 }
        while updates.isPreparingInstallation { await Task.yield() }
        expect(installed == 0 && preparations == 1 && recoveries == 1 && updates.canCheckForUpdates,
               "failed shutdown retains exactly one retry and restores normal app operation")
        expect(!updates.requiresInstallationFileOwnerVerification,
               "failed installation preparation restores ordinary Quit without weakening the update owner guard")
        shutdownAllowed = true
        updates.checkForUpdates()
        while updates.isPreparingInstallation { await Task.yield() }
        expect(installed == 0 && recoveries == 2 && updates.canCheckForUpdates,
               "live runtime blocks install and remains retryable")
        idleResult = .unavailable
        updates.checkForUpdates()
        while updates.isPreparingInstallation { await Task.yield() }
        expect(installed == 0 && recoveries == 3,
               "inspection failure does not invoke the installer")
        idleResult = .idle
        updates.checkForUpdates()
        while updates.isPreparingInstallation { await Task.yield() }
        expect(installed == 1 && preparations == 4,
               "only a successful shutdown and idle verification invokes the original installer once")
        expect(updates.requiresInstallationFileOwnerVerification,
               "successful continuation keeps the final AppDelegate update guard armed")
        var normalQuitPending = true
        var backgroundAbortRecoveries = 0
        let backgroundCheck = CaskrunAppUpdates(prepareForQuit: { true }, verifyIdle: { .idle },
                                              recoverAfterInstallationFailure: {
            normalQuitPending = false
            backgroundAbortRecoveries += 1
        }, reportMessage: { _ in })
        backgroundCheck.installationAborted()
        expect(normalQuitPending && backgroundAbortRecoveries == 0,
               "a feed-check-only abort never resets an unrelated normal Quit")
        expect(!backgroundCheck.requiresInstallationFileOwnerVerification,
               "feed-check-only abort cannot enable the replacement guard on normal Quit")
        backgroundCheck.installationWillBegin()
        expect(backgroundCheck.requiresInstallationFileOwnerVerification,
               "direct or deferred installation arms verification without the postpone callback")
        backgroundCheck.installationAborted()
        expect(!backgroundCheck.requiresInstallationFileOwnerVerification && backgroundAbortRecoveries == 1,
               "aborting a genuine direct installation clears its guard and restores app state")
        var pendingInspection: CheckedContinuation<CaskrunUpdateIdleResult, Never>?
        var canceledInstallations = 0
        var canceledRecoveries = 0
        let canceled = CaskrunAppUpdates(prepareForQuit: { true }, verifyIdle: {
            await withCheckedContinuation { pendingInspection = $0 }
        }, recoverAfterInstallationFailure: { canceledRecoveries += 1 }, reportMessage: { _ in })
        canceled.postponeInstallation { canceledInstallations += 1 }
        while pendingInspection == nil { await Task.yield() }
        canceled.installationAborted()
        canceled.checkForUpdates()
        expect(canceled.isPreparingInstallation && !canceled.canCheckForUpdates,
               "aborting a pending inspection keeps preparation exclusive until it completes")
        expect(!canceled.requiresInstallationFileOwnerVerification,
               "a canceled installer cannot leave an ordinary Quit subject to the replacement guard")
        pendingInspection?.resume(returning: .idle)
        while canceled.isPreparingInstallation { await Task.yield() }
        expect(canceledInstallations == 0 && canceledRecoveries == 1 && canceled.canCheckForUpdates,
               "an aborted cycle never invokes a stale installer and restores the app after its gate completes")
        #if canImport(Sparkle)
        for selector in ["feedURLStringForUpdater:", "allowedSystemProfileKeysForUpdater:",
                         "feedParametersForUpdater:sendingSystemProfile:",
                         "updater:shouldProceedWithUpdate:updateCheck:error:",
                         "updater:willExtractUpdate:", "updater:willInstallUpdate:",
                         "updater:willInstallUpdateOnQuit:immediateInstallationBlock:",
                         "updater:userDidMakeChoice:forUpdate:state:",
                         "updater:shouldPostponeRelaunchForUpdate:untilInvokingBlock:",
                         "updater:didAbortWithError:"] {
            expect(updates.responds(to: NSSelectorFromString(selector)),
                   "the official Sparkle Objective-C delegate selector is implemented")
        }
        #endif
        print("\(checks) update configuration, privacy and installation checks passed")
    }
}
