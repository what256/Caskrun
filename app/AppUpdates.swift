import AppKit
import Combine
import Foundation
#if canImport(Sparkle)
import Sparkle
#endif

/// The feed and key come from reviewed release packaging, never user input or
/// an authenticated GitHub session. A source-only build has neither configured.
struct CaskrunUpdateConfiguration: Equatable {
    let feedURL: URL
    let publicKey: String
    let repositoryOwner: String
    let repositoryName: String

    enum Failure: Error, Equatable {
        case unconfigured
        case invalid
    }

    static func read(_ info: [String: Any]) -> Result<Self, Failure> {
        let feed = info["SUFeedURL"] as? String
        let key = info["SUPublicEDKey"] as? String
        guard feed != nil || key != nil else { return .failure(.unconfigured) }
        guard let feed, let key, let url = URL(string: feed),
              let decodedKey = Data(base64Encoded: key), decodedKey.count == 32,
              info["SURequireSignedFeed"] as? Bool == true,
              info["SUVerifyUpdateBeforeExtraction"] as? Bool == true,
              info["SUAllowsAutomaticUpdates"] as? Bool == false,
              let path = safeComponents(url), path.count >= 4,
              path.last == "appcast.xml", validRepositoryComponent(path[0]),
              validRepositoryComponent(path[1]) else { return .failure(.invalid) }
        switch url.host?.lowercased() {
        case "raw.githubusercontent.com": break
        case "github.com":
            guard path.count == 6, path[2] == "releases",
                  (path[3] == "latest" && path[4] == "download") ||
                  (path[3] == "download" && !path[4].isEmpty) else {
                return .failure(.invalid)
            }
        default: return .failure(.invalid)
        }
        return .success(Self(feedURL: url, publicKey: key,
                             repositoryOwner: path[0], repositoryName: path[1]))
    }

    func allowsArchive(_ url: URL?, delta: Bool = false) -> Bool {
        guard let url, let path = Self.safeComponents(url), matchesRepository(path),
              url.host?.lowercased() == "github.com", path.count == 6,
              path[2] == "releases", path[3] == "download" else { return false }
        let name = path[5].lowercased()
        if delta { return name.hasSuffix(".delta") }
        return [".dmg", ".zip", ".tar.xz", ".aar"].contains { name.hasSuffix($0) }
    }

    func allowsNotes(_ url: URL?) -> Bool {
        guard let url else { return true }
        guard let path = Self.safeComponents(url), matchesRepository(path) else { return false }
        switch url.host?.lowercased() {
        case "raw.githubusercontent.com": return path.count >= 4
        case "github.com": return path.count >= 2
        default: return false
        }
    }

    private func matchesRepository(_ path: [String]) -> Bool {
        path.count >= 2 && path[0].lowercased() == repositoryOwner.lowercased() &&
        path[1].lowercased() == repositoryName.lowercased()
    }

    private static func validRepositoryComponent(_ value: String) -> Bool {
        !value.isEmpty && value != "." && value != ".." && value.unicodeScalars.allSatisfy {
            CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.").contains($0)
        }
    }

    private static func safeComponents(_ url: URL) -> [String]? {
        guard let parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
              parts.scheme?.lowercased() == "https", parts.user == nil,
              parts.password == nil, parts.port == nil, parts.query == nil,
              parts.fragment == nil, !parts.percentEncodedPath.contains("%"),
              !parts.path.contains("\\"), !parts.path.contains("//") else { return nil }
        let path = parts.path.split(separator: "/").map(String.init)
        guard !path.contains("."), !path.contains("..") else { return nil }
        return path
    }
}

enum CaskrunUpdateIdleResult: Equatable {
    case idle
    case busy
    case unavailable
}

/// Independent of Sparkle so cancellation and the process guard can be tested
/// without downloading, installing or terminating an application.
enum CaskrunUpdateInstallationGate {
    @MainActor
    static func prepare(prepareForQuit: () async -> Bool,
                        verifyIdle: () async -> CaskrunUpdateIdleResult) async -> CaskrunUpdateIdleResult {
        guard await prepareForQuit() else { return .busy }
        return await verifyIdle()
    }
}

enum CaskrunUpdateFileOwners {
    static func classify(status: Int32, output: Data, allowedOwners: Set<Int32>) -> CaskrunUpdateIdleResult {
        if status == 1 && output.isEmpty { return .idle }
        guard (status == 0 || status == 1), !output.isEmpty,
              let text = String(data: output, encoding: .utf8) else { return .unavailable }
        var sawOwner = false
        for line in text.split(separator: "\n") {
            // lsof emits mandatory file-descriptor records even with -Fp.
            // They belong to the preceding PID and do not change its owner.
            if line.first == "f", sawOwner, line.count > 1,
               line.dropFirst().unicodeScalars.allSatisfy({ (0x21...0x7e).contains($0.value) }) {
                continue
            }
            guard line.first == "p", let pid = Int32(line.dropFirst()), pid > 0 else { return .unavailable }
            sawOwner = true
            if !allowedOwners.contains(pid) { return .busy }
        }
        return sawOwner && status == 0 ? .idle : .unavailable
    }

    static func verify(bundleURL: URL, executableURL: URL?, ownPID: Int32) async -> CaskrunUpdateIdleResult {
        await Task.detached(priority: .userInitiated) {
            guard bundleURL.pathExtension == "app", let executableURL,
                  Bundle(url: bundleURL)?.bundleIdentifier == "dev.caskrun.app",
                  isRegularFile(bundleURL.appendingPathComponent("Contents/Resources/.caskrun-generated-app")),
                  isRegularFile(executableURL) else { return .unavailable }
            let ownExecutable = inspect(executableURL, recursive: false, allowedOwners: [ownPID])
            guard ownExecutable == .idle else { return ownExecutable }
            for name in ["caskrun-core", "caskrun-native-icon"] {
                let file = bundleURL.appendingPathComponent("Contents/MacOS/\(name)")
                guard isRegularFile(file) else { return .unavailable }
                let result = inspect(file, recursive: false, allowedOwners: [])
                guard result == .idle else { return result }
            }
            let runtime = bundleURL.appendingPathComponent("Contents/Resources/wine-runtime")
            var runtimeStat = stat()
            if lstat(runtime.path, &runtimeStat) == 0 {
                guard runtimeStat.st_mode & S_IFMT == S_IFDIR else { return .unavailable }
                return inspect(runtime, recursive: true, allowedOwners: [])
            }
            guard errno == ENOENT else { return .unavailable }
            return .idle
        }.value
    }

    private static func isRegularFile(_ url: URL) -> Bool {
        guard let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey]) else { return false }
        return values.isRegularFile == true && values.isSymbolicLink != true
    }

    private static func inspect(_ url: URL, recursive: Bool, allowedOwners: Set<Int32>) -> CaskrunUpdateIdleResult {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/sbin/lsof")
        process.arguments = ["-nP", "-Fp"] + (recursive ? ["+D", url.path] : ["--", url.path])
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        process.standardInput = FileHandle.nullDevice
        let capture = CaskrunUpdateProcessCapture()
        do { try process.run() } catch { return .unavailable }
        let readers = DispatchGroup()
        readers.enter()
        DispatchQueue.global(qos: .userInitiated).async {
            while true {
                let bytes = pipe.fileHandleForReading.readData(ofLength: 4096)
                if bytes.isEmpty { break }
                capture.append(bytes)
            }
            readers.leave()
        }
        let timeout = DispatchWorkItem {
            if process.isRunning {
                capture.markTimedOut()
                process.terminate()
                DispatchQueue.global().asyncAfter(deadline: .now() + 1) {
                    if process.isRunning { kill(process.processIdentifier, SIGKILL) }
                }
            }
        }
        DispatchQueue.global().asyncAfter(deadline: .now() + 15, execute: timeout)
        process.waitUntilExit()
        timeout.cancel()
        readers.wait()
        guard let output = capture.finishedData() else { return .unavailable }
        return classify(status: process.terminationStatus, output: output, allowedOwners: allowedOwners)
    }
}

private final class CaskrunUpdateProcessCapture: @unchecked Sendable {
    private let lock = NSLock()
    private var data = Data()
    private var invalid = false

    func append(_ bytes: Data) {
        lock.lock()
        defer { lock.unlock() }
        guard !invalid else { return }
        if data.count + bytes.count > 64 * 1024 { invalid = true; return }
        data.append(bytes)
    }

    func markTimedOut() {
        lock.lock()
        invalid = true
        lock.unlock()
    }

    func finishedData() -> Data? {
        lock.lock()
        defer { lock.unlock() }
        return invalid ? nil : data
    }
}

@MainActor
final class CaskrunAppUpdates: NSObject, ObservableObject {
    @Published private(set) var canCheckForUpdates = true
    @Published private(set) var isPreparingInstallation = false
    /// AppDelegate applies the stronger replacement guard only after an
    /// accepted update has reached the installer. Ordinary Quit never needs
    /// to reject unrelated macOS UI services mapping the app executable.
    @Published private(set) var requiresInstallationFileOwnerVerification = false
    private let configuration: CaskrunUpdateConfiguration?
    private let unavailableMessage: String
    private let bundle: Bundle
    private let prepareForQuit: () async -> Bool
    private let verifyIdle: () async -> CaskrunUpdateIdleResult
    private let recoverAfterInstallationFailure: () -> Void
    private let reportMessage: (String) -> Void
    private var installContinuation: (() -> Void)?
    private var installationGeneration: UInt64 = 0
    #if canImport(Sparkle)
    private var updaterController: SPUStandardUpdaterController?
    private var observation: AnyCancellable?
    #endif

    init(bundle: Bundle = .main, prepareForQuit: @escaping () async -> Bool,
         verifyIdle: (() async -> CaskrunUpdateIdleResult)? = nil,
         recoverAfterInstallationFailure: @escaping () -> Void = {},
         reportMessage: @escaping (String) -> Void) {
        self.bundle = bundle
        self.prepareForQuit = prepareForQuit
        self.verifyIdle = verifyIdle ?? {
            await CaskrunUpdateFileOwners.verify(bundleURL: bundle.bundleURL,
                                                executableURL: bundle.executableURL,
                                                ownPID: ProcessInfo.processInfo.processIdentifier)
        }
        self.reportMessage = reportMessage
        self.recoverAfterInstallationFailure = recoverAfterInstallationFailure
        switch CaskrunUpdateConfiguration.read(bundle.infoDictionary ?? [:]) {
        case .success(let configuration):
            self.configuration = configuration
            unavailableMessage = "This build does not include the updater framework. Signed GitHub updates will be available in a release build."
        case .failure(.unconfigured):
            configuration = nil
            unavailableMessage = "Signed GitHub updates are not published for this build yet. Your installed apps and bottles are unchanged."
        case .failure(.invalid):
            configuration = nil
            unavailableMessage = "This build's update configuration could not be verified. No update was downloaded or installed."
        }
        super.init()
        #if canImport(Sparkle)
        if configuration != nil {
            let controller = SPUStandardUpdaterController(startingUpdater: false,
                                                          updaterDelegate: self, userDriverDelegate: nil)
            updaterController = controller
            observation = controller.updater.publisher(for: \.canCheckForUpdates)
                .receive(on: RunLoop.main)
                .sink { [weak self] available in
                    guard let self else { return }
                    self.canCheckForUpdates = !self.isPreparingInstallation &&
                        (self.installContinuation != nil || available)
                }
            controller.startUpdater()
        }
        #endif
    }

    func checkForUpdates() {
        guard !isPreparingInstallation else { return }
        if installContinuation != nil { continueInstallation(); return }
        #if canImport(Sparkle)
        if let updaterController {
            updaterController.checkForUpdates(nil)
            return
        }
        #endif
        reportMessage(unavailableMessage)
    }

    /// Also used by fixture checks; the callback is retained when a safe stop
    /// fails so a later explicit Check for Updates can retry the same update.
    func postponeInstallation(until continuation: @escaping () -> Void) {
        guard installContinuation == nil else { return }
        installContinuation = continuation
        continueInstallation()
    }

    private func continueInstallation() {
        guard !isPreparingInstallation, installContinuation != nil else { return }
        installationWillBegin()
        isPreparingInstallation = true
        canCheckForUpdates = false
        installationGeneration &+= 1
        let generation = installationGeneration
        Task {
            let result = await CaskrunUpdateInstallationGate.prepare(prepareForQuit: prepareForQuit,
                                                                    verifyIdle: verifyIdle)
            guard installationGeneration == generation else {
                isPreparingInstallation = false
                recoverAfterInstallationFailure()
                canCheckForUpdates = true
                return
            }
            isPreparingInstallation = false
            switch result {
            case .idle:
                let continuation = installContinuation
                installContinuation = nil
                continuation?()
            case .busy:
                requiresInstallationFileOwnerVerification = false
                recoverAfterInstallationFailure()
                canCheckForUpdates = true
                reportMessage("The update is ready, but Caskrun could not safely stop all its apps. Close them and choose Check for Updates to try again.")
            case .unavailable:
                requiresInstallationFileOwnerVerification = false
                recoverAfterInstallationFailure()
                canCheckForUpdates = true
                reportMessage("The update is ready, but Caskrun could not verify that its app and Windows engine are idle. No files were replaced. Choose Check for Updates to retry.")
            }
        }
    }

    /// Sparkle also calls willInstallUpdate when a deferred install resumes,
    /// before it asks the installer to terminate the app. Extraction arms the
    /// same guard because dismissing an already prepared update can leave its
    /// installer waiting for a later Quit without calling the postpone hook.
    func installationWillBegin() {
        requiresInstallationFileOwnerVerification = true
    }

    func installationAborted() {
        let hadInstallation = installContinuation != nil || isPreparingInstallation
            || requiresInstallationFileOwnerVerification
        installationGeneration &+= 1
        installContinuation = nil
        requiresInstallationFileOwnerVerification = false
        // A shutdown may still be awaiting a child process. Keep this attempt
        // exclusive until it finishes, then restore normal app operation.
        if !isPreparingInstallation {
            if hadInstallation { recoverAfterInstallationFailure() }
            canCheckForUpdates = true
        }
    }
}

#if canImport(Sparkle)
extension CaskrunAppUpdates: SPUUpdaterDelegate {
    // Ignore a legacy per-user feed override; release packaging is the sole
    // source of the trusted feed and public key.
    func feedURLString(for updater: SPUUpdater) -> String? { configuration?.feedURL.absoluteString }

    func allowedSystemProfileKeys(for updater: SPUUpdater) -> [String]? { [] }

    func feedParameters(for updater: SPUUpdater, sendingSystemProfile sendingProfile: Bool) -> [[String: String]] { [] }

    func updater(_ updater: SPUUpdater, shouldProceedWithUpdate item: SUAppcastItem,
                 updateCheck: SPUUpdateCheck) throws {
        guard let configuration, item.installationType == "application",
              configuration.allowsArchive(item.fileURL), configuration.allowsNotes(item.infoURL),
              configuration.allowsNotes(item.releaseNotesURL),
              configuration.allowsNotes(item.fullReleaseNotesURL) else {
            throw NSError(domain: "CaskrunUpdates", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "This update does not use the configured GitHub release location. No update was installed."])
        }
        for delta in item.deltaUpdates?.values ?? Dictionary<String, SUAppcastItem>().values {
            guard delta.installationType == "application", configuration.allowsArchive(delta.fileURL, delta: true) else {
                throw NSError(domain: "CaskrunUpdates", code: 1,
                              userInfo: [NSLocalizedDescriptionKey: "This update contains an unverified download location. No update was installed."])
            }
        }
    }

    func updater(_ updater: SPUUpdater, shouldDownloadReleaseNotesForUpdate item: SUAppcastItem) -> Bool {
        configuration?.allowsNotes(item.releaseNotesURL) == true
    }

    func updater(_ updater: SPUUpdater, willExtractUpdate item: SUAppcastItem) {
        installationWillBegin()
    }

    func updater(_ updater: SPUUpdater, willInstallUpdate item: SUAppcastItem) {
        installationWillBegin()
    }

    func updater(_ updater: SPUUpdater, willInstallUpdateOnQuit item: SUAppcastItem,
                 immediateInstallationBlock immediateInstallHandler: @escaping () -> Void) -> Bool {
        installationWillBegin()
        return false
    }

    func updater(_ updater: SPUUpdater, userDidMake choice: SPUUserUpdateChoice,
                 forUpdate item: SUAppcastItem, state: SPUUserUpdateState) {
        if choice == .skip { installationAborted() }
        // Dismiss at Installing keeps the update scheduled for termination;
        // preserve its verification requirement rather than treating it as a
        // feed-only session ending.
    }

    func updater(_ updater: SPUUpdater, shouldPostponeRelaunchForUpdate item: SUAppcastItem,
                 untilInvokingBlock installHandler: @escaping () -> Void) -> Bool {
        postponeInstallation(until: installHandler)
        return true
    }

    func updater(_ updater: SPUUpdater, didAbortWithError error: Error) {
        installationAborted()
        if !isPreparingInstallation { canCheckForUpdates = updater.canCheckForUpdates }
    }
}
#endif
