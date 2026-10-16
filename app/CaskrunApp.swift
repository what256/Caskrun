import AppKit
import AVFoundation
import SwiftUI
import ImageIO
import Darwin

private struct CatalogArtworkSource: Sendable {
    enum Format: Sendable {
        case png, icon, svg
    }

    let url: URL
    let referer: String
    let format: Format
    let cacheName: String
    let crop: CGRect?

    // These are publisher-controlled assets linked from the publishers' own pages.
    // Keep this allowlist separate from the downloadable installer catalog.
    static let byID: [String: CatalogArtworkSource] = [
        "steam": .init(url: URL(string: "https://store.steampowered.com/favicon.ico")!,
                       referer: "https://store.steampowered.com/about/", format: .icon,
                       cacheName: "steam-v1.ico", crop: nil),
        "epic-games-store": .init(url: URL(string: "https://edc-cdn.net/assets/images/favicon-light.png")!,
                                  referer: "https://dev.epicgames.com/", format: .png,
                                  cacheName: "epic-games-store-v1.png", crop: nil),
        "gog-galaxy": .init(url: URL(string: "https://www.gogalaxy.com/favicons/apple-icon-180x180.png")!,
                            referer: "https://www.gog.com/galaxy", format: .png,
                            cacheName: "gog-galaxy-v1.png", crop: nil),
        "battle-net": .init(url: URL(string: "https://blz-contentstack-images.akamaized.net/v3/assets/blt13393558c8f39060/bltc033281ebe085f52/6442f1f9fac1726ff56fc234/nexus_logomark.svg")!,
                            referer: "https://download.battle.net/en-us/desktop", format: .svg,
                            cacheName: "battle-net-v1.svg", crop: CGRect(x: 350, y: 0, width: 500, height: 500)),
        "ubisoft-connect": .init(url: URL(string: "https://static-dm.ubisoft.com/ubisoft/prod/favicon-195x195.png")!,
                                 referer: "https://www.ubisoft.com/en-us/ubisoft-connect/download", format: .png,
                                 cacheName: "ubisoft-connect-v1.png", crop: nil),
        "ea-app": .init(url: URL(string: "https://media.contentapi.ea.com/content/dam/ea/EA_logo.png")!,
                        referer: "https://www.ea.com/ea-app", format: .png,
                        cacheName: "ea-app-v1.png", crop: nil),
        "itch-io-app": .init(url: URL(string: "https://static.itch.io/images/app-icon.png")!,
                             referer: "https://itch.io/app", format: .png,
                             cacheName: "itch-io-app-v1.png", crop: nil),
        "rockstar-games-launcher": .init(url: URL(string: "https://media-rockstargames-com.akamaized.net/favicons/rockstar/favicon-180x180.png")!,
                                          referer: "https://www.rockstargames.com/", format: .png,
                                          cacheName: "rockstar-games-launcher-v1.png", crop: nil),
    ]
}

private final class CatalogArtworkNoRedirect: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask,
                    willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest,
                    completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}

@MainActor
private final class CatalogArtworkStore: ObservableObject {
    @Published private(set) var images: [String: NSImage] = [:]
    private var requested: Set<String> = []
    private let redirectBlocker = CatalogArtworkNoRedirect()
    private lazy var session: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 10
        configuration.timeoutIntervalForResource = 15
        configuration.httpMaximumConnectionsPerHost = 2
        return URLSession(configuration: configuration, delegate: redirectBlocker, delegateQueue: nil)
    }()

    private func cacheURL(for source: CatalogArtworkSource) -> URL? {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first?
            .appendingPathComponent("Caskrun/catalog-icons", isDirectory: true)
            .appendingPathComponent(source.cacheName)
    }

    private nonisolated static func prepare(_ data: Data, source: CatalogArtworkSource) -> Data? {
        guard !data.isEmpty, data.count <= 512 * 1024 else { return nil }
        switch source.format {
        case .png:
            guard data.starts(with: [137, 80, 78, 71, 13, 10, 26, 10]) else { return nil }
        case .icon:
            guard data.starts(with: [0, 0, 1, 0]) else { return nil }
        case .svg:
            guard let xml = String(data: data, encoding: .utf8), xml.contains("<svg "),
                  !["<!DOCTYPE", "<script", "<foreignObject", "<image", "<use", "href=", "url("].contains(where: xml.contains) else { return nil }
        }
        return MacAppArtwork.png(data, crop: source.crop, svg: source.format == .svg)
    }

    func load(_ id: String) {
        guard images[id] == nil,
              let source = CatalogArtworkSource.byID[id],
              source.url.scheme == "https", source.url.user == nil,
              source.url.password == nil, source.url.port == nil,
              let host = source.url.host, !host.isEmpty,
              let cache = cacheURL(for: source),
              requested.insert(id).inserted else { return }

        Task {
            defer { requested.remove(id) }
            let cached = await Task.detached(priority: .utility) {
                MacAppArtwork.read(cache.path, maximum: 512 * 1024)
                    .flatMap { Self.prepare($0, source: source) }
            }.value
            if let cached, let image = NSImage(data: cached) {
                images[id] = image
                return
            }
            do {
                var request = URLRequest(url: source.url)
                request.setValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 Caskrun/0.1", forHTTPHeaderField: "User-Agent")
                request.setValue(source.referer, forHTTPHeaderField: "Referer")
                request.setValue("image/png,image/x-icon,image/vnd.microsoft.icon,image/svg+xml", forHTTPHeaderField: "Accept")
                let (bytes, response) = try await session.bytes(for: request)
                guard let http = response as? HTTPURLResponse, http.statusCode == 200,
                      http.url?.scheme == "https", http.url?.host == host,
                      http.expectedContentLength <= 512 * 1024 || http.expectedContentLength < 0 else { return }
                let allowedTypes: [CatalogArtworkSource.Format: Set<String>] = [
                    .png: ["image/png"],
                    .icon: ["image/x-icon", "image/vnd.microsoft.icon", "image/ico"],
                    .svg: ["image/svg+xml"]
                ]
                guard let mime = http.mimeType?.lowercased(),
                      allowedTypes[source.format]?.contains(mime) == true else { return }
                var data = Data()
                for try await byte in bytes {
                    guard data.count < 512 * 1024 else { return }
                    data.append(byte)
                }
                let prepared = await Task.detached(priority: .utility) {
                    Self.prepare(data, source: source)
                }.value
                guard let prepared, let image = NSImage(data: prepared) else { return }
                images[id] = image
                // A cache write failure must not hide an image we already decoded.
                try? FileManager.default.createDirectory(at: cache.deletingLastPathComponent(),
                                                         withIntermediateDirectories: true)
                try? data.write(to: cache, options: .atomic)
            } catch {
                // Keep the text fallback; revisiting Discover can retry.
            }
        }
    }
}

struct Bottle: Codable, Identifiable, Hashable, Sendable {
    let id: String
    let name: String
    let created_at: UInt64
}

struct EngineStatus: Decodable {
    let wine_path: String?
    let bottle_count: Int
}

struct RuntimeProbeResult: Decodable, Sendable {
    let wine_path: String
    let version: String
    let file_description: String
    let wineboot_path: String
    let wineserver_present: Bool
    let log_path: String
}

struct BottleExportResult: Decodable, Sendable {
    let path: String
    let external_launchers: Int
    let host_folders: Int
}

struct BottleImportResult: Decodable, Sendable {
    let bottle: Bottle
    let external_launchers: Int
    let host_folders: Int
    let private_host_folders: Int
    let profile_account_resolved: Bool
    let profile_account_linked: Bool
    let profile_warning: String?
}

struct CrossOverImportResult: Decodable, Sendable {
    let bottle: Bottle
    let notes: [String]
    let discovery_truncated: Bool
}

struct SteamGame: Decodable, Identifiable, Sendable {
    let id: String
    let app_id: UInt32
    let name: String
    let install_path: String
    let steam_path: String
    let icon_path: String?
    let icon_hash: String?
}

struct SteamLibrary: Decodable, Sendable {
    let games: [SteamGame]
    let truncated: Bool
}

struct PublisherGame: Decodable, Identifiable, Sendable {
    let id: String
    let store: String
    let store_id: String
    let name: String
    let install_path: String
}

struct PublisherLibrary: Decodable, Sendable {
    let games: [PublisherGame]
    let truncated: Bool
}

struct PrefixPathResult: Decodable {
    let path: String
}

struct LaunchResult: Decodable, Sendable {
    let pid: UInt32
    let log_path: String
}

struct AppIconResult: Decodable, Sendable {
    let path: String?
}

struct BottleQuitResult: Decodable, Sendable {
    let message: String
}

private struct ShutdownResult: Decodable, Sendable {
    struct Failure: Decodable, Sendable {
        let bottle_id: String
        let name: String
        let error: String
    }
    let complete: Bool
    let failures: [Failure]
}

struct BottleResolutionStatus: Decodable, Sendable {
    let bottle_id: String
    let high_resolution: Bool
    let dpi: Int
}

struct BottleGraphicsStatus: Decodable, Sendable {
    let bottle_id: String
    let selected_backend: String
    let effective_backend: String
    let dxmt_available: Bool
    let unavailable_reason: String?
    let warning: String?
    let dxmt_frame_rate: UInt16
    let dxmt_spatial_scale: UInt16
    let dxmt_spatial_available: Bool
}

struct WaitLaunchResult: Decodable, Sendable {
    let pid: UInt32
    let log_path: String
    let exit_code: Int32?
    let signal: Int32?
    let elapsed_ms: UInt64
    let timed_out: Bool
}

struct CatalogInstallResult: Decodable, Sendable {
    let bottle: Bottle
    let launcher: SavedLauncher
    let installer_sha256: String
    let download_url: String
    let install_log_path: String
    let launch: LaunchResult
    let setup_note: String?
}

enum CatalogStage: String, Decodable, Sendable {
    case ready, running, failed, installerComplete = "installer_complete"
    case preparingClient = "preparing_client", installed
}

struct CatalogAttempt: Decodable, Identifiable, Sendable {
    let bottle_id: String
    let catalog_id: String
    let stage: CatalogStage
    let last_log_name: String?
    let last_error: String?
    let updated_at: UInt64
    var id: String { bottle_id }
}

struct CoreOverview: Decodable {
    let status: EngineStatus
    let bottles: [Bottle]
    let deleted_bottles: [Bottle]
    let launchers: [String: [SavedLauncher]]
    let catalog_attempts: [CatalogAttempt]
    let catalog_attempts_error: String?
    let steam_clients: [SavedSteamClient]
}

struct SavedSteamClient: Decodable { let bottle_id: String; let launcher_id: String }

struct InstallHandoff {
    let entryID: String
    let entryName: String
    let bottle: Bottle
    let launcher: SavedLauncher
    let setupNote: String?
}

struct CatalogSetupFailure {
    let entryID: String
    let entryName: String
    let bottleID: String?
    let detail: String
}

struct DiscoveredProgram: Decodable, Identifiable, Sendable {
    let name: String
    let path: String
    let source: String
    /// A shortcut's own Windows arguments. Two shortcuts can point at one
    /// program and still be different commands, so the identity includes them.
    let arguments: [String]
    let working_directory: String?
    /// A program and its arguments together: comparing paths alone would treat
    /// two different commands for one program as the same suggestion.
    var command: String { ([path.lowercased(), working_directory ?? ""] + arguments).joined(separator: "\u{0}") }
    var id: String { command }

    private enum CodingKeys: String, CodingKey { case name, path, source, arguments, working_directory }

    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        name = try values.decode(String.self, forKey: .name)
        path = try values.decode(String.self, forKey: .path)
        source = try values.decode(String.self, forKey: .source)
        // A program with no arguments may arrive without the key at all.
        arguments = try values.decodeIfPresent([String].self, forKey: .arguments) ?? []
        working_directory = try values.decodeIfPresent(String.self, forKey: .working_directory)
    }
}

struct DiscoveredProgramsResult: Decodable, Sendable {
    let candidates: [DiscoveredProgram]
    let truncated: Bool
}

struct ManualInstallReview: Identifiable {
    let id = UUID()
    let bottle: Bottle
    let installerName: String
    let candidates: [DiscoveredProgram]
    let searchWasLimited: Bool
    let discoveryUnavailable: Bool
}

struct SavedLauncher: Codable, Identifiable, Sendable {
    let id: String
    let name: String
    let path: String
    let created_at: UInt64
    let hidden_from_home: Bool
    // Older launcher records omit this field; the core treats that as no saved options.
    let saved_arguments: [String]?
    let working_directory: String?
    /// The same program-and-arguments key a discovered suggestion uses, so an
    /// already-saved command can be matched against a suggestion exactly.
    var command: String {
        ([path.lowercased(), working_directory ?? ""] + (saved_arguments ?? [])).joined(separator: "\u{0}")
    }
}

struct BottleLog: Decodable, Identifiable, Sendable {
    let name: String
    let size: UInt64
    let modified_at: UInt64
    var id: String { name }
}

struct BottleLogContent: Decodable, Sendable {
    let name: String
    let content: String
    let truncated: Bool
}

struct CatalogEntry: Decodable, Identifiable {
    let id: String
    let name: String
    let category: String
    let symbol: String
    let detail: String
    let downloadPageURL: URL
    let compatibilityStatus: String
    let installerNotes: String?
    let installMode: String?
}

private struct CatalogDocument: Decodable {
    let schemaVersion: Int
    let entries: [CatalogEntry]
}

private enum CatalogSource {
    static let loaded: (entries: [CatalogEntry], error: String?) = {
        guard let url = Bundle.main.url(forResource: "apps", withExtension: "json", subdirectory: "catalog") else {
            return ([], "The bundled app catalog is missing. Rebuild Caskrun to restore it.")
        }
        do {
            let document = try JSONDecoder().decode(CatalogDocument.self, from: Data(contentsOf: url))
            guard document.schemaVersion == 1 else {
                return ([], "This app catalog version is not supported by Caskrun.")
            }
            guard !document.entries.isEmpty,
                  document.entries.allSatisfy({ $0.downloadPageURL.scheme == "https"
                      && $0.downloadPageURL.host != nil
                      && $0.compatibilityStatus == "untested"
                      && ($0.installMode == nil || (["steam", "epic-games-store", "gog-galaxy", "battle-net", "ubisoft-connect", "ea-app", "itch-io-app", "rockstar-games-launcher"].contains($0.id) && $0.installMode == "automatic")) }) else {
                return ([], "The bundled app catalog contains invalid entries.")
            }
            return (document.entries, nil)
        } catch {
            return ([], "Could not read the app catalog: \(error.localizedDescription)")
        }
    }()
}

enum Page: Hashable { case collection, discover }

private enum CaskrunMenuCommand {
    case open, collection, discover, manageBottles, install
}

@MainActor
private final class CaskrunAppDelegate: NSObject, NSApplicationDelegate {
    var handleMenuCommand: ((CaskrunMenuCommand) -> Void)?
    var shutdownOwnedApps: (() async -> Bool)?
    var resumeAfterTerminationFailure: (() -> Void)?
    var requiresInstallationFileOwnerVerification: (() -> Bool)?
    private var terminationPending = false

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !terminationPending else { return .terminateLater }
        guard let shutdownOwnedApps else { return .terminateCancel }
        terminationPending = true
        Task {
            var complete = await shutdownOwnedApps()
            if complete && requiresInstallationFileOwnerVerification?() == true {
                complete = await CaskrunUpdateFileOwners.verify(bundleURL: Bundle.main.bundleURL,
                    executableURL: Bundle.main.executableURL,
                    ownPID: ProcessInfo.processInfo.processIdentifier) == .idle
            }
            terminationPending = false
            sender.reply(toApplicationShouldTerminate: complete)
            if !complete {
                resumeAfterTerminationFailure?()
                handleMenuCommand?(.open)
            }
        }
        return .terminateLater
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        CaskrunStatusBarController.shared.install()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        CaskrunWindowBridge.shared.reconcileAfterWindowClose()
        return false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        if !hasVisibleWindows {
            if let handleMenuCommand {
                handleMenuCommand(.open)
                return false
            }
            CaskrunWindowBridge.shared.prepareToShowMainWindow()
            return true
        }
        return true
    }
}

@MainActor
private final class CaskrunWindowBridge: NSObject {
    static let shared = CaskrunWindowBridge()
    private let mainWindows = NSHashTable<NSWindow>.weakObjects()
    private var closeDelegates: [ObjectIdentifier: CaskrunCloseToMenuBarDelegate] = [:]
    private var presentationGeneration: UInt64 = 0
    private(set) var hasPresentedMainWindow = false

    private override init() {
        super.init()
        NotificationCenter.default.addObserver(self, selector: #selector(mainWindowWillClose(_:)),
                                               name: NSWindow.willCloseNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(mainWindowDidBecomeKey(_:)),
                                               name: NSWindow.didBecomeKeyNotification, object: nil)
    }

    func register(_ window: NSWindow) {
        installCloseDelegate(for: window)
        guard !mainWindows.allObjects.contains(where: { $0 === window }) else { return }
        mainWindows.add(window)
        hasPresentedMainWindow = true
        presentationGeneration &+= 1
        // The capture view can enter its window while SwiftUI is still setting
        // up that window. Reattach once setup finishes without changing any of
        // the framework's other window-delegate callbacks.
        DispatchQueue.main.async { [weak self, weak window] in
            guard let self, let window,
                  self.mainWindows.allObjects.contains(where: { $0 === window }) else { return }
            self.installCloseDelegate(for: window)
        }
    }

    private func installCloseDelegate(for window: NSWindow) {
        let identity = ObjectIdentifier(window)
        let delegate = closeDelegates[identity] ?? CaskrunCloseToMenuBarDelegate(
            window: window, previousDelegate: window.delegate,
            hideWindow: { [weak self] _ in self?.hideMainWindow() })
        closeDelegates[identity] = delegate
        delegate.attach(to: window)
    }

    @objc private func mainWindowDidBecomeKey(_ notification: Notification) {
        guard let window = notification.object as? NSWindow,
              mainWindows.allObjects.contains(where: { $0 === window }) else { return }
        installCloseDelegate(for: window)
    }

    @objc private func mainWindowWillClose(_ notification: Notification) {
        guard let window = notification.object as? NSWindow,
              mainWindows.allObjects.contains(where: { $0 === window }) else { return }
        mainWindows.remove(window)
        let identity = ObjectIdentifier(window)
        // AppKit may still call the window delegate after this notification.
        // Keep the forwarding proxy alive through that final cleanup callback.
        DispatchQueue.main.async { [weak self] in self?.closeDelegates[identity] = nil }
        reconcileAfterWindowClose()
    }

    func reconcileAfterWindowClose() {
        let generation = presentationGeneration
        // The notification fires before AppKit has finished closing the window.
        DispatchQueue.main.async { [weak self] in
            guard let self,
                  self.presentationGeneration == generation,
                  !NSApp.windows.contains(where: { $0.isVisible && $0.title == "Caskrun" }) else { return }
            NSApp.setActivationPolicy(.accessory)
            CaskrunStatusBarController.shared.ensureVisible()
        }
    }

    func hideMainWindow() {
        presentationGeneration &+= 1
        for window in mainWindows.allObjects {
            window.orderOut(nil)
        }
        NSApp.setActivationPolicy(.accessory)
        CaskrunStatusBarController.shared.ensureVisible()
    }

    func showMainWindow() -> Bool {
        guard let mainWindow = mainWindows.allObjects.last else { return false }
        prepareToShowMainWindow()
        NSApp.activate(ignoringOtherApps: true)
        mainWindow.makeKeyAndOrderFront(nil)
        return true
    }

    func prepareToShowMainWindow() {
        presentationGeneration &+= 1
        NSApp.setActivationPolicy(.regular)
    }
}

private final class CaskrunWindowCaptureView: NSView {
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if let window {
            CaskrunWindowBridge.shared.register(window)
        }
    }
}

private struct CaskrunWindowCapture: NSViewRepresentable {
    func makeNSView(context: Context) -> CaskrunWindowCaptureView {
        CaskrunWindowCaptureView(frame: .zero)
    }

    func updateNSView(_ nsView: CaskrunWindowCaptureView, context: Context) {
        if let window = nsView.window { CaskrunWindowBridge.shared.register(window) }
    }
}

private struct CaskrunStatusBridge: View {
    @Environment(\.openWindow) private var openWindow
    @Binding var page: Page
    @Binding var didChooseInitialPage: Bool
    @Binding var showingBottleInspector: Bool
    @Binding var openBottleID: String?
    @ObservedObject var model: CaskrunModel
    @ObservedObject var ps5: PS5CollectionModel
    @ObservedObject var appUpdates: CaskrunAppUpdates
    @Binding var selectedCollectionID: String?
    @Binding var inspectingBottleOnly: Bool
    let delegate: CaskrunAppDelegate

    var body: some View {
        Color.clear
        .onAppear {
            let pageBinding = $page
            let initialPageBinding = $didChooseInitialPage
            let inspectorBinding = $showingBottleInspector
            let selectionBinding = $selectedCollectionID
            let managementBinding = $inspectingBottleOnly
            let folderBinding = $openBottleID
            let openMainWindow = openWindow
            delegate.handleMenuCommand = { command in
                switch command {
                case .open: break
                case .collection, .install:
                    managementBinding.wrappedValue = false
                    selectionBinding.wrappedValue = nil
                    pageBinding.wrappedValue = .collection
                    folderBinding.wrappedValue = nil
                    inspectorBinding.wrappedValue = false
                    initialPageBinding.wrappedValue = true
                case .discover:
                    pageBinding.wrappedValue = .discover
                    initialPageBinding.wrappedValue = true
                case .manageBottles:
                    managementBinding.wrappedValue = true
                    selectionBinding.wrappedValue = nil
                    pageBinding.wrappedValue = .collection
                    folderBinding.wrappedValue = model.bottles.first(where: { $0.id == model.selectedBottleID })?.id
                    inspectorBinding.wrappedValue = true
                    initialPageBinding.wrappedValue = true
                }
                if !CaskrunWindowBridge.shared.showMainWindow() {
                    CaskrunWindowBridge.shared.prepareToShowMainWindow()
                    openMainWindow(id: "main")
                    NSApp.activate(ignoringOtherApps: true)
                }
                if case .install = command {
                    DispatchQueue.main.async { model.chooseInstaller() }
                }
            }
            delegate.shutdownOwnedApps = {
                guard await model.shutdownForQuit() else { return false }
                guard await ps5.shutdownForQuit() else {
                    model.resumeAfterQuitFailure(ps5.error ?? "A PS5 session could not be stopped.")
                    return false
                }
                return true
            }
            delegate.resumeAfterTerminationFailure = { model.resumeAfterUpdateFailure() }
            delegate.requiresInstallationFileOwnerVerification = {
                appUpdates.requiresInstallationFileOwnerVerification
            }
            CaskrunStatusBarController.shared.configure(model: model) { [weak delegate] command in
                delegate?.handleMenuCommand?(command)
            }
        }
    }
}

@MainActor
private final class CaskrunStatusBarController: NSObject, NSMenuDelegate {
    static let shared = CaskrunStatusBarController()
    private let menuAppLimit = 8
    private var statusItem: NSStatusItem?
    private var model: CaskrunModel?
    private var handleMenuCommand: ((CaskrunMenuCommand) -> Void)?
    private var menuApps: [SavedProgram] = []

    func install() {
        guard statusItem == nil else { return }
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        if let button = item.button {
            if let logo = CaskrunMenuBarArtwork.image?.copy() as? NSImage {
                logo.size = NSSize(width: 17, height: 17)
                button.image = logo
                button.imagePosition = .imageOnly
            }
            button.title = ""
            button.toolTip = "Caskrun"
            button.setAccessibilityLabel("Caskrun menu")
        }
        let menu = NSMenu(title: "Caskrun")
        menu.delegate = self
        item.menu = menu
        item.isVisible = true
        statusItem = item
        NSLog("Caskrun status item installed; visible=%d", item.isVisible ? 1 : 0)
    }

    func ensureVisible() {
        install()
        guard let statusItem, !statusItem.isVisible else { return }
        statusItem.isVisible = true
        NSLog("Caskrun status item restored; visible=%d", statusItem.isVisible ? 1 : 0)
    }

    func configure(model: CaskrunModel, command: @escaping (CaskrunMenuCommand) -> Void) {
        self.model = model
        handleMenuCommand = command
        ensureVisible()
    }

    private var visibleApps: [SavedProgram] {
        guard let model else { return [] }
        return model.bottles.flatMap { bottle in
            (model.launchers[bottle.id] ?? [])
                .filter { !$0.hidden_from_home }
                .map { SavedProgram(bottle: bottle, launcher: $0) }
        }
        .sorted { $0.launcher.created_at > $1.launcher.created_at }
    }

    private var cannotLaunch: Bool {
        guard let model else { return true }
        return model.isQuitting || model.isLaunching || model.isManagingBottle || model.status.wine_path == nil
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        menu.addItem(item("Open Caskrun", action: #selector(open)))

        let appsMenu = NSMenu(title: "Your Apps")
        let apps = visibleApps
        menuApps = Array(apps.prefix(menuAppLimit))
        if menuApps.isEmpty {
            let hasHiddenApps = model?.launchers.values.contains(where: { !$0.isEmpty }) == true
            let placeholder = item(hasHiddenApps ? "Your apps are hidden from Collection" : "No apps installed yet",
                                   action: nil)
            placeholder.isEnabled = false
            appsMenu.addItem(placeholder)
            appsMenu.addItem(item(hasHiddenApps ? "Manage saved apps…" : "Discover apps…",
                                  action: hasHiddenApps ? #selector(bottles) : #selector(discover)))
        } else {
            for (index, app) in menuApps.enumerated() {
                let appItem = item(menuTitle(for: app, in: apps), action: #selector(launchApp(_:)))
                appItem.tag = index
                appItem.isEnabled = !cannotLaunch
                if let image = model?.launcherIcons[app.id]?.copy() as? NSImage {
                    image.size = NSSize(width: 16, height: 16)
                    appItem.image = image
                } else {
                    appItem.image = NSImage(systemSymbolName: "app", accessibilityDescription: nil)
                    model?.loadLauncherIcon(app.launcher, in: app.bottle)
                }
                appsMenu.addItem(appItem)
            }
            if apps.count > menuAppLimit {
                appsMenu.addItem(.separator())
                appsMenu.addItem(item("See all \(apps.count) apps in Collection…", action: #selector(collection)))
            }
        }
        let appsItem = item("Your Apps", action: nil)
        appsItem.image = NSImage(systemSymbolName: "square.grid.2x2", accessibilityDescription: nil)
        appsItem.submenu = appsMenu
        menu.addItem(appsItem)

        menu.addItem(.separator())
        menu.addItem(item("Collection", action: #selector(collection)))
        menu.addItem(item("Discover", action: #selector(discover)))
        menu.addItem(item("Manage Bottles…", action: #selector(bottles)))
        menu.addItem(.separator())
        let installItem = item("Install from File…", action: #selector(installFromFile))
        installItem.isEnabled = !cannotLaunch
        menu.addItem(installItem)
        menu.addItem(.separator())
        menu.addItem(item("About Caskrun", action: #selector(about)))
        let quitItem = item(model?.isQuitting == true ? "Closing apps…" : "Quit Caskrun", action: #selector(quit))
        quitItem.isEnabled = model?.isQuitting != true
        quitItem.toolTip = "Close Caskrun and its Windows apps and PS5 sessions"
        menu.addItem(quitItem)
    }

    private func item(_ title: String, action: Selector?) -> NSMenuItem {
        let menuItem = NSMenuItem(title: title, action: action, keyEquivalent: "")
        menuItem.target = self
        return menuItem
    }

    private func menuTitle(for item: SavedProgram, in apps: [SavedProgram]) -> String {
        let hasSameName = apps.contains {
            $0.id != item.id &&
                $0.launcher.name.localizedCaseInsensitiveCompare(item.launcher.name) == .orderedSame
        }
        return hasSameName ? "\(item.launcher.name) (\(item.bottle.name))" : item.launcher.name
    }

    @objc private func open() { handleMenuCommand?(.open) }
    @objc private func collection() { handleMenuCommand?(.collection) }
    @objc private func discover() { handleMenuCommand?(.discover) }
    @objc private func bottles() { handleMenuCommand?(.manageBottles) }
    @objc private func installFromFile() { handleMenuCommand?(.install) }

    @objc private func launchApp(_ sender: NSMenuItem) {
        guard menuApps.indices.contains(sender.tag), let model, !cannotLaunch else { return }
        let app = menuApps[sender.tag]
        model.runLauncher(app.launcher, in: app.bottle,
                          onFailure: { [weak self] in self?.handleMenuCommand?(.collection) })
    }

    @objc private func about() {
        NSApp.orderFrontStandardAboutPanel(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    @objc private func quit() { NSApp.terminate(nil) }
}

private enum CaskrunMenuBarArtwork {
    static let image: NSImage? = {
        guard let url = Bundle.main.url(forResource: "caskrun-menubar-template", withExtension: "png"),
              let image = NSImage(contentsOf: url) else {
            return nil
        }
        image.isTemplate = true
        return image
    }()
}

private struct SavedProgram: Identifiable {
    let bottle: Bottle
    let launcher: SavedLauncher
    var id: String { "\(bottle.id):\(launcher.id)" }
}

private final class CoreInvocationCancellation: @unchecked Sendable {
    let inputPipe = Pipe()
    private let lock = NSLock()
    private var cancelRequested = false

    var isCancelRequested: Bool {
        lock.lock()
        defer { lock.unlock() }
        return cancelRequested
    }

    func cancel() {
        lock.lock()
        defer { lock.unlock() }
        guard !cancelRequested else { return }
        cancelRequested = true
        try? inputPipe.fileHandleForWriting.close()
    }
}

private enum CoreRunner {
    static func call<T: Decodable>(_ arguments: [String], as type: T.Type, coreURL: URL,
                                   wineExecutable: String,
                                   cancellation: CoreInvocationCancellation? = nil) throws -> T {
        let process = Process()
        process.executableURL = coreURL
        process.arguments = arguments
        var environment = ProcessInfo.processInfo.environment
        if !wineExecutable.isEmpty { environment["CASKRUN_WINE"] = wineExecutable }
        if let cancellation {
            environment["CASKRUN_CANCEL_STDIN"] = "1"
            process.standardInput = cancellation.inputPipe
        } else {
            environment.removeValue(forKey: "CASKRUN_CANCEL_STDIN")
        }
        process.environment = environment
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("Caskrun-Core-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let outputURL = directory.appendingPathComponent("stdout")
        let errorURL = directory.appendingPathComponent("stderr")
        try Data().write(to: outputURL)
        try Data().write(to: errorURL)
        let output = try FileHandle(forWritingTo: outputURL)
        let errorFile = try FileHandle(forWritingTo: errorURL)
        defer {
            try? output.close()
            try? errorFile.close()
        }
        process.standardOutput = output
        process.standardError = errorFile
        try process.run()
        process.waitUntilExit()
        try output.close()
        try errorFile.close()
        let outputData = try Data(contentsOf: outputURL)
        let errorData = try Data(contentsOf: errorURL)
        if process.terminationStatus != 0 {
            let detail = String(data: errorData, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "Unknown error"
            throw NSError(domain: "Caskrun", code: Int(process.terminationStatus), userInfo: [NSLocalizedDescriptionKey: detail])
        }
        return try JSONDecoder().decode(T.self, from: outputData)
    }
}

@MainActor
final class CaskrunModel: ObservableObject {
    private static var bundledWinePath: String? {
        guard let path = Bundle.main.resourceURL?.appendingPathComponent("wine-runtime/bin/wine").path,
              FileManager.default.isExecutableFile(atPath: path) else { return nil }
        return path
    }

    private static func initialWineExecutable() -> String {
        return bundledWinePath ?? ""
    }

    @Published var bottles: [Bottle] = []
    @Published var deletedBottles: [Bottle] = []
    @Published var launchers: [String: [SavedLauncher]] = [:]
    @Published private(set) var steamGames: [String: [SteamGame]] = [:]
    private var steamArtworkRevisions: [String: UInt64] = [:]
    @Published private(set) var publisherGames: [String: [PublisherGame]] = [:]
    @Published private(set) var publisherGameErrors: [String: String] = [:]
    @Published private(set) var loadingPublisherGames: Set<String> = []
    @Published private(set) var truncatedPublisherLibraries: Set<String> = []
    @Published private(set) var savedSteamClients: [SavedSteamClient] = []
    @Published private(set) var browserGames: [BrowserGame] = []
    @Published private(set) var browserGamesError: String?
    @Published private(set) var loadingGameLibrary = false
    @Published private(set) var editingBrowserGames = false
    @Published private(set) var gameIcons: [String: NSImage] = [:]
    @Published private(set) var steamGameErrors: [String: String] = [:]
    @Published private(set) var loadingSteamGames: Set<String> = []
    @Published private(set) var truncatedSteamLibraries: Set<String> = []
    @Published var importedBottleID: String?
    @Published private(set) var launcherIcons: [String: NSImage] = [:]
    @Published private(set) var bottleIcons: [String: NSImage] = [:]
    @Published var catalogAttempts: [CatalogAttempt] = []
    @Published var catalogAttemptLoadError: String?
    @Published var status = EngineStatus(wine_path: nil, bottle_count: 0)
    @Published var message: String?
    private var quitShutdownCompleted = false
    @Published private(set) var isRequestingMicrophoneAccess = false
    @Published var selectedBottleID = ""
    @Published private(set) var bottleResolutions: [String: BottleResolutionStatus] = [:]
    @Published private(set) var loadingBottleResolutions: Set<String> = []
    @Published private(set) var bottleResolutionErrors: [String: String] = [:]
    @Published private(set) var bottleGraphics: [String: BottleGraphicsStatus] = [:]
    @Published private(set) var loadingBottleGraphics: Set<String> = []
    @Published private(set) var bottleGraphicsErrors: [String: String] = [:]
    @Published var wineExecutable = initialWineExecutable()
    @Published private(set) var isQuitting = false
    @Published var isLaunching = false
    @Published private(set) var steamStartup: SteamStartup.Progress?
    @Published private(set) var canStopWaitingForSteam = false
    private var steamStartupMonitor = SteamStartup.Monitor()
    @Published var activeInstallerName: String?
    @Published private(set) var canCancelManualInstaller = false
    @Published private(set) var isCancellingManualInstaller = false
    @Published var activeCatalogInstall: String?
    @Published private(set) var canCancelCatalogInstall = false
    @Published private(set) var isCancellingCatalogInstall = false
    @Published var activeCatalogStage: CatalogStage?
    @Published var activeCatalogBottleID: String?
    @Published var installHandoff: InstallHandoff?
    @Published var catalogSetupFailure: CatalogSetupFailure?
    @Published var manualInstallReview: ManualInstallReview?
    @Published var isManagingBottle = false
    @Published var bottleOperationLabel = "Updating bottle…"
    @Published var isProbingRuntime = false
    @Published var runtimeProbe: RuntimeProbeResult?
    @Published var pendingRun: OneTimeProgram?
    @Published private(set) var bottleWindowsVersions: [String: BottleWindowsVersion] = [:]
    @Published private(set) var loadingWindowsVersions: Set<String> = []
    @Published private(set) var windowsVersionErrors: [String: String] = [:]
    private var runtimeProbeGeneration = 0
    private var catalogMonitorGeneration = 0
    private var requestedIcons: Set<String> = []
    private var launcherIconStamps: [String: String] = [:]
    private var bottleIconStamps: [String: String] = [:]
    private var artworkGenerations: [String: Int] = [:]
    private var gameIconStamps: [String: String] = [:]
    private var gameIconOrder: [String] = []
    private var activeManualInstaller: CoreInvocationCancellation?
    private var activeCatalogCancellation: CoreInvocationCancellation?
    private var activeMaintenanceCancellation: CoreInvocationCancellation?
    @Published private(set) var bottleSessions: [String: BottleSessionState] = [:]
    private var checkingBottleActivity: Set<String> = []
    private struct BottleStopOperation {
        let id: UUID
        let cancellation: CoreInvocationCancellation
        let task: Task<Void, Never>
    }
    private var bottleStopOperations: [String: BottleStopOperation] = [:]

    func sessionState(for bottle: Bottle) -> BottleSessionState {
        bottleSessions[bottle.id] ?? BottleSessionState()
    }

    func isStoppingBottle(_ id: String) -> Bool {
        bottleSessions[id]?.isStopping == true
    }

    func refreshBottleActivity(in bottle: Bottle) async {
        guard !isQuitting, !checkingBottleActivity.contains(bottle.id) else { return }
        checkingBottleActivity.insert(bottle.id)
        let token = bottleSessions[bottle.id]?.operationID
        defer { checkingBottleActivity.remove(bottle.id) }
        do {
            let result = try await callAsync(["bottle-activity", bottle.id], as: BottleActivitySnapshot.self)
            guard !isQuitting, !Task.isCancelled, result.bottle_id == bottle.id,
                  bottleSessions[bottle.id]?.operationID == token else { return }
            bottleSessions[bottle.id, default: BottleSessionState()].checked(running: result.running)
        } catch {
            guard !isQuitting, !Task.isCancelled,
                  bottleSessions[bottle.id]?.operationID == token else { return }
            bottleSessions[bottle.id, default: BottleSessionState()].checkFailed(error.localizedDescription)
        }
    }

    func requestQuitApps(in bottle: Bottle) {
        guard !isQuitting, !isManagingBottle, !isLaunching,
              bottleStopOperations[bottle.id] == nil,
              let token = bottleSessions[bottle.id, default: BottleSessionState()].beginQuit() else { return }
        let cancellation = CoreInvocationCancellation()
        let task = Task { @MainActor in
            defer {
                if bottleStopOperations[bottle.id]?.id == token {
                    bottleStopOperations[bottle.id] = nil
                    if cancellation.isCancelRequested { bottleSessions[bottle.id]?.interrupted(token) }
                }
            }
            do {
                let result = try await callAsync(["request-bottle-quit", bottle.id], as: BottleQuitRequestResult.self,
                                                 cancellation: cancellation)
                guard !isQuitting, result.bottle_id == bottle.id,
                      bottleSessions[bottle.id]?.operationID == token else { return }
                bottleSessions[bottle.id]?.requestFinished(token, submitted: result.request_submitted)
            } catch {
                guard !isQuitting, !cancellation.isCancelRequested else { return }
                bottleSessions[bottle.id]?.failed(token, message: error.localizedDescription)
            }
            await refreshBottleActivity(in: bottle)
            await monitorBottleQuit(in: bottle, token: token, cancellation: cancellation)
        }
        bottleStopOperations[bottle.id] = BottleStopOperation(id: token, cancellation: cancellation, task: task)
    }

    private func monitorBottleQuit(in bottle: Bottle, token: UUID,
                                   cancellation: CoreInvocationCancellation) async {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: .seconds(120))
        while !isQuitting, !cancellation.isCancelRequested,
              bottleSessions[bottle.id]?.operationID == token,
              bottleSessions[bottle.id]?.phase == .waiting {
            guard clock.now < deadline else {
                bottleSessions[bottle.id]?.failed(token, message: "This bottle still has activity. Check its open apps or choose Force Quit.")
                return
            }
            do { try await Task.sleep(for: .seconds(2)) } catch { return }
            guard !isQuitting, !cancellation.isCancelRequested,
                  bottleSessions[bottle.id]?.operationID == token else { return }
            await refreshBottleActivity(in: bottle)
        }
    }

    var hasBundledRuntime: Bool { Self.bundledWinePath != nil }
    var usesBundledRuntime: Bool { Self.bundledWinePath == wineExecutable && hasBundledRuntime }

    func requestMicrophoneAccess() {
        guard !isQuitting && !isRequestingMicrophoneAccess else { return }
        let enabled = "Microphone access is enabled for Caskrun. Windows apps may request their own access when you use voice chat or recording."
        let denied = "Microphone access is disabled for Caskrun. Enable it in System Settings → Privacy & Security → Microphone when you want to use voice chat or recording."
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            message = enabled
        case .notDetermined:
            isRequestingMicrophoneAccess = true
            Task { @MainActor in
                let granted = await AVCaptureDevice.requestAccess(for: .audio)
                isRequestingMicrophoneAccess = false
                guard !isQuitting else { return }
                message = granted ? enabled : denied
            }
        case .denied:
            message = denied
        case .restricted:
            message = "Microphone access is restricted on this Mac. Check its privacy restrictions before using voice chat or recording."
        @unknown default:
            message = "Caskrun could not determine microphone access. Check System Settings → Privacy & Security → Microphone."
        }
    }

    private var coreURL: URL {
        Bundle.main.executableURL!.deletingLastPathComponent().appendingPathComponent("caskrun-core")
    }

    private func allowCoreCommandDuringShutdown(_ arguments: [String]) throws {
        let readCommands: Set<String> = ["overview", "status", "list", "deleted", "catalog-attempts",
                                         "apps", "app-icon", "bottle-icon", "bottle-resolution", "bottle-graphics",
                                         "discover-apps", "steam-games", "logs", "read-log", "prefix-path",
                                         "installed-programs", "diagnostics", "bottle-notes", "bottle-windows-version", "browser-games", "publisher-games", "bottle-activity"]
        if isQuitting, let command = arguments.first,
           command != "quit-all-bottles", !readCommands.contains(command) {
            throw NSError(domain: "Caskrun", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "Caskrun is closing its Windows apps. Try again after reopening it."])
        }
    }

    func shutdownForQuit() async -> Bool {
        guard !isQuitting else { return quitShutdownCompleted }
        quitShutdownCompleted = false
        isQuitting = true
        activeManualInstaller?.cancel()
        activeCatalogCancellation?.cancel()
        activeMaintenanceCancellation?.cancel()
        for operation in bottleStopOperations.values { operation.cancellation.cancel() }
        do {
            let clock = ContinuousClock()
            let deadline = clock.now.advanced(by: .seconds(30))
            while isLaunching || isManagingBottle || isProbingRuntime || editingBrowserGames || !bottleStopOperations.isEmpty {
                guard clock.now < deadline else {
                    throw NSError(domain: "Caskrun", code: 1,
                                  userInfo: [NSLocalizedDescriptionKey: "A setup or bottle operation is still finishing. Wait for it to finish, then choose Quit Caskrun again."])
                }
                try await Task.sleep(for: .milliseconds(100))
            }
            let cancellation = CoreInvocationCancellation()
            let deadlineTask = Task {
                do {
                    try await Task.sleep(for: .seconds(60))
                    cancellation.cancel()
                } catch {}
            }
            defer { deadlineTask.cancel() }
            let result = try await callAsync(["quit-all-bottles"], as: ShutdownResult.self,
                                             cancellation: cancellation)
            guard result.complete else {
                let detail = result.failures.map { "\($0.name): \($0.error)" }.joined(separator: "\n\n")
                throw NSError(domain: "Caskrun", code: 1,
                              userInfo: [NSLocalizedDescriptionKey: detail.isEmpty ? "Some Windows apps could not be stopped." : detail])
            }
            quitShutdownCompleted = true
            return true
        } catch {
            quitShutdownCompleted = false
            isQuitting = false
            message = "Caskrun could not finish closing its Windows apps.\n\n" + error.localizedDescription
            return false
        }
    }

    func resumeAfterQuitFailure(_ detail: String) {
        quitShutdownCompleted = false
        isQuitting = false
        message = "Caskrun could not finish closing its sessions.\n\n" + detail
    }

    func resumeAfterUpdateFailure() {
        quitShutdownCompleted = false
        isQuitting = false
    }

    private func call<T: Decodable>(_ arguments: [String], as type: T.Type) throws -> T {
        try allowCoreCommandDuringShutdown(arguments)
        return try CoreRunner.call(arguments, as: type, coreURL: coreURL, wineExecutable: wineExecutable)
    }

    private func callAsync<T: Decodable & Sendable>(_ arguments: [String], as type: T.Type,
                                                    cancellation: CoreInvocationCancellation? = nil) async throws -> T {
        try allowCoreCommandDuringShutdown(arguments)
        let url = coreURL
        let wine = wineExecutable
        return try await Task.detached(priority: .userInitiated) {
            try CoreRunner.call(arguments, as: type, coreURL: url, wineExecutable: wine,
                                cancellation: cancellation)
        }.value
    }

    func cancelManualInstaller() {
        guard canCancelManualInstaller, let activeManualInstaller, activeInstallerName != nil,
              activeCatalogInstall == nil, !isCancellingManualInstaller else { return }
        isCancellingManualInstaller = true
        activeManualInstaller.cancel()
    }

    func cancelCatalogInstall() {
        guard canCancelCatalogInstall, let activeCatalogCancellation,
              activeCatalogInstall != nil, !isCancellingCatalogInstall else { return }
        isCancellingCatalogInstall = true
        activeCatalogCancellation.cancel()
    }

    private func artworkPolicyStamp() -> String {
        let bundled = Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/caskrun-native-icon").path
        let renderer = FileManager.default.fileExists(atPath: bundled) ? bundled : status.wine_path.map {
            URL(fileURLWithPath: $0).deletingLastPathComponent().appendingPathComponent("native-icon").path
        }
        return "adaptive-v1:\(status.wine_path ?? "none"):\(renderer.map(iconStamp(for:)) ?? "none")"
    }

    private func launcherIconStamp(_ launcher: SavedLauncher, in bottleID: String) -> String {
        "\(artworkPolicyStamp()):\(artworkGenerations[bottleID, default: 0]):\(launcher.name):\(iconStamp(for: launcher.path))"
    }

    private func iconStamp(for path: String) -> String {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path),
              let size = attributes[.size] as? NSNumber,
              let modified = attributes[.modificationDate] as? Date else { return "missing" }
        return "\(size.uint64Value):\(modified.timeIntervalSince1970):\(attributes[.systemFileNumber] ?? "none")"
    }

    private func bottleIconStamp(_ bottle: Bottle) -> String {
        "\(artworkPolicyStamp()):\(artworkGenerations[bottle.id, default: 0]):" + (launchers[bottle.id] ?? []).prefix(64).map { launcher in
            "\(launcher.id):\(launcherIconStamp(launcher, in: bottle.id))"
        }.joined(separator: "|")
    }

    func loadLauncherIcon(_ launcher: SavedLauncher, in bottle: Bottle) {
        let key = "\(bottle.id):\(launcher.id)"
        let stamp = launcherIconStamp(launcher, in: bottle.id)
        let requestKey = "app:\(key)"
        guard launcherIcons[key] == nil || launcherIconStamps[key] != stamp else { return }
        guard requestedIcons.insert(requestKey).inserted else { return }
        Task {
            defer {
                requestedIcons.remove(requestKey)
                if let current = launchers[bottle.id]?.first(where: { $0.id == launcher.id }),
                   launcherIconStamp(current, in: bottle.id) != stamp {
                    loadLauncherIcon(current, in: bottle)
                }
            }
            let result = try? await callAsync(["app-icon", bottle.id, launcher.id], as: AppIconResult.self)
            guard let current = launchers[bottle.id]?.first(where: { $0.id == launcher.id }),
                  launcherIconStamp(current, in: bottle.id) == stamp else { return }
            guard let result, let path = result.path,
                  let data = await Task.detached(priority: .utility, operation: {
                      MacAppArtwork.read(path, maximum: 8 * 1024 * 1024)
                  }).value,
                  let icon = NSImage(data: data) else {
                launcherIcons.removeValue(forKey: key)
                launcherIconStamps.removeValue(forKey: key)
                return
            }
            launcherIcons[key] = icon
            launcherIconStamps[key] = stamp
        }
    }

    func loadBottleIcon(_ bottle: Bottle) {
        let stamp = bottleIconStamp(bottle)
        let requestKey = "bottle:\(bottle.id)"
        guard bottleIcons[bottle.id] == nil || bottleIconStamps[bottle.id] != stamp else { return }
        guard requestedIcons.insert(requestKey).inserted else { return }
        Task {
            defer {
                requestedIcons.remove(requestKey)
                if let current = bottles.first(where: { $0.id == bottle.id }), bottleIconStamp(current) != stamp {
                    loadBottleIcon(current)
                }
            }
            let result = try? await callAsync(["bottle-icon", bottle.id], as: AppIconResult.self)
            guard bottles.contains(where: { $0.id == bottle.id }), bottleIconStamp(bottle) == stamp else { return }
            guard let result, let path = result.path,
                  let data = await Task.detached(priority: .utility, operation: {
                      MacAppArtwork.read(path, maximum: 8 * 1024 * 1024)
                  }).value,
                  let icon = NSImage(data: data) else {
                bottleIcons.removeValue(forKey: bottle.id)
                bottleIconStamps.removeValue(forKey: bottle.id)
                return
            }
            bottleIcons[bottle.id] = icon
            bottleIconStamps[bottle.id] = stamp
        }
    }

    func refresh() {
        do {
            let snapshot = try call(["overview"], as: CoreOverview.self)
            let previousLauncherCounts = launchers.mapValues(\.count)
            status = snapshot.status
            bottles = snapshot.bottles
            deletedBottles = snapshot.deleted_bottles
            launchers = snapshot.launchers
            catalogAttempts = snapshot.catalog_attempts
            catalogAttemptLoadError = snapshot.catalog_attempts_error
            savedSteamClients = snapshot.steam_clients
            do {
                browserGames = try call(["browser-games"], as: BrowserGames.self).games
                browserGamesError = nil
            } catch { browserGamesError = error.localizedDescription }
            for bottle in bottles {
                let launcherCount = launchers[bottle.id]?.count ?? 0
                if launcherCount > 0 && launcherCount != (previousLauncherCounts[bottle.id] ?? 0) {
                    bottleIcons.removeValue(forKey: bottle.id)
                    loadBottleIcon(bottle)
                } else if bottleIcons[bottle.id] != nil {
                    loadBottleIcon(bottle)
                }
                for launcher in launchers[bottle.id] ?? []
                    where launcherIcons["\(bottle.id):\(launcher.id)"] != nil {
                    loadLauncherIcon(launcher, in: bottle)
                }
            }
            if !bottles.contains(where: { $0.id == selectedBottleID }) {
                selectedBottleID = bottles.first?.id ?? ""
            }
        } catch { message = error.localizedDescription }
    }

    @discardableResult
    func createBottle(_ name: String, announce: Bool = true) -> Bottle? {
        do {
            let bottle = try call(["create", name], as: Bottle.self)
            refresh()
            selectedBottleID = bottle.id
            if announce { message = "Created \(bottle.name). Choose a Windows installer to continue." }
            return bottle
        } catch {
            message = error.localizedDescription
            return nil
        }
    }

    func renameBottle(_ bottle: Bottle, to name: String) {
        guard !isQuitting && !isManagingBottle && !isLaunching else {
            message = "Wait for the current bottle operation to finish before renaming."
            return
        }
        do {
            _ = try call(["rename", bottle.id, name], as: Bottle.self)
            refresh()
        } catch { message = error.localizedDescription }
    }

    func duplicateBottle(_ bottle: Bottle) {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        isManagingBottle = true
        bottleOperationLabel = "Duplicating \(bottle.name)…"
        let cancellation = CoreInvocationCancellation()
        activeMaintenanceCancellation = cancellation
        Task {
            defer { activeMaintenanceCancellation = nil; isManagingBottle = false; bottleOperationLabel = "Updating bottle…" }
            do {
                let copy = try await callAsync(["duplicate", bottle.id], as: Bottle.self, cancellation: cancellation)
                refresh()
                selectedBottleID = copy.id
                message = "Created \(copy.name). Its files and saved apps are separate from \(bottle.name)."
            } catch { message = error.localizedDescription }
        }
    }

    func deleteBottle(_ bottle: Bottle) {
        guard !isQuitting && !isManagingBottle && !isLaunching else {
            message = "Wait for the current bottle operation to finish before moving it."
            return
        }
        isManagingBottle = true
        Task {
            defer { isManagingBottle = false }
            do {
                _ = try await callAsync(["delete", bottle.id], as: Bottle.self)
                refresh()
                message = "Moved \(bottle.name) to Recently Deleted. Restore it from Manage bottles on Collection."
            } catch { message = error.localizedDescription }
        }
    }

    func restoreDeletedBottle(_ bottle: Bottle) {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        isManagingBottle = true
        Task {
            defer { isManagingBottle = false }
            do {
                _ = try await callAsync(["restore-deleted", bottle.id], as: Bottle.self)
                refresh()
                selectedBottleID = bottle.id
                message = "Restored \(bottle.name)."
            } catch { message = error.localizedDescription }
        }
    }

    func permanentlyDeleteBottle(_ bottle: Bottle) {
        guard !isQuitting && !isManagingBottle && !isLaunching,
              deletedBottles.contains(where: { $0.id == bottle.id }),
              !bottles.contains(where: { $0.id == bottle.id }) else { return }
        isManagingBottle = true
        bottleOperationLabel = "Deleting \(bottle.name)…"
        Task {
            defer { isManagingBottle = false; bottleOperationLabel = "Updating bottle…" }
            do {
                _ = try await callAsync(["purge-deleted", bottle.id], as: Bottle.self)
                refresh()
                message = "Permanently deleted \(bottle.name)."
            } catch { message = error.localizedDescription }
        }
    }

    func openDriveC(in bottle: Bottle) {
        do {
            let prefix = try call(["prefix-path", bottle.id], as: PrefixPathResult.self)
            let drive = URL(fileURLWithPath: prefix.path, isDirectory: true)
                .appendingPathComponent("drive_c", isDirectory: true)
            var isDirectory: ObjCBool = false
            guard FileManager.default.fileExists(atPath: drive.path, isDirectory: &isDirectory),
                  isDirectory.boolValue else {
                message = "The C: drive is created when Wine initializes this bottle. Run an installer first."
                return
            }
            if !NSWorkspace.shared.open(drive) {
                message = "Finder could not open the C: drive for \(bottle.name)."
            }
        } catch { message = error.localizedDescription }
    }

    func exportBottle(_ bottle: Bottle) {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        let panel = NSSavePanel()
        panel.message = "Save a new compressed bottle backup. Close Windows apps in this bottle first."
        panel.nameFieldStringValue = bottle.name
            .replacingOccurrences(of: "/", with: "-")
            .replacingOccurrences(of: ":", with: "-") + ".caskrun-backup"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        bottleOperationLabel = "Exporting \(bottle.name)…"
        isManagingBottle = true
        let cancellation = CoreInvocationCancellation()
        activeMaintenanceCancellation = cancellation
        Task {
            defer { activeMaintenanceCancellation = nil; isManagingBottle = false; bottleOperationLabel = "Updating bottle…" }
            do {
                let result = try await callAsync(["export-archive", bottle.id, url.path], as: BottleExportResult.self,
                                                cancellation: cancellation)
                var note = result.external_launchers == 0 ? "" : " \(result.external_launchers) external saved app path(s) may need to be reselected after restore."
                if result.host_folders > 0 {
                    // The backup names these folders; it never copies their contents.
                    note += " \(result.host_folders) Windows user folder(s) will be reconnected to your Mac folders on restore."
                }
                message = "Saved \(bottle.name) to \(result.path).\(note)"
            } catch { message = error.localizedDescription }
        }
    }

    func importBottle() {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        let panel = NSOpenPanel()
        panel.message = "Choose a .caskrun-backup file or a .caskrun-bottle backup folder"
        panel.canChooseFiles = true
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        let compressed = url.pathExtension.lowercased() == "caskrun-backup"
        if !compressed && url.pathExtension.lowercased() != "caskrun-bottle" {
            message = "Choose a Caskrun backup file or backup folder."
            return
        }
        bottleOperationLabel = "Importing \(url.lastPathComponent)…"
        isManagingBottle = true
        let cancellation = CoreInvocationCancellation()
        activeMaintenanceCancellation = cancellation
        Task {
            defer { activeMaintenanceCancellation = nil; isManagingBottle = false; bottleOperationLabel = "Updating bottle…" }
            do {
                let result = try await callAsync([compressed ? "import-archive" : "import", url.path], as: BottleImportResult.self,
                                                cancellation: cancellation)
                refresh()
                selectedBottleID = result.bottle.id
                importedBottleID = result.bottle.id
                var note = result.external_launchers == 0 ? "" : " \(result.external_launchers) external saved app path(s) may need to be reselected."
                if result.host_folders > 0 {
                    note += " \(result.host_folders) Windows user folder(s) now use your Mac folders."
                }
                if result.private_host_folders > 0 {
                    note += " \(result.private_host_folders) had no matching Mac folder and stay inside the bottle."
                }
                // Only claim the restored profile is in use when it really is.
                if result.profile_account_resolved {
                    let connected = result.profile_account_linked ? "now uses" : "uses"
                    note += " This Mac account \(connected) the restored Windows profile, so imported apps keep their settings and data."
                }
                if let warning = result.profile_warning {
                    note += " " + warning
                }
                message = "Restored \(result.bottle.name) from \(url.lastPathComponent).\(note)"
            } catch { message = error.localizedDescription }
        }
    }

    func importCrossOverBottle() {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        let panel = NSOpenPanel()
        panel.title = "Import from CrossOver"
        panel.message = "Choose a bottle folder containing drive_c and system.reg. Close Windows apps before importing. Caskrun copies your apps and data into a new bottle."
        panel.directoryURL = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/CrossOver/Bottles", isDirectory: true)
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        bottleOperationLabel = "Importing \(url.lastPathComponent)…"
        isManagingBottle = true
        Task {
            defer { isManagingBottle = false; bottleOperationLabel = "Updating bottle…" }
            do {
                let result = try await callAsync(["import-crossover", url.path], as: CrossOverImportResult.self)
                refresh()
                selectedBottleID = result.bottle.id
                importedBottleID = result.bottle.id
                message = "Imported \(result.bottle.name).\n\n" + result.notes.joined(separator: "\n\n")
            } catch { message = error.localizedDescription }
        }
    }

    func refreshSteamGames(in bottle: Bottle) async {
        guard !loadingSteamGames.contains(bottle.id), !isManagingBottle else { return }
        loadingSteamGames.insert(bottle.id)
        defer { loadingSteamGames.remove(bottle.id) }
        do {
            let cancellation = CoreInvocationCancellation()
            let library = try await withTaskCancellationHandler {
                try await callAsync(["steam-games", bottle.id], as: SteamLibrary.self, cancellation: cancellation)
            } onCancel: { cancellation.cancel() }
            guard !Task.isCancelled, bottles.contains(where: { $0.id == bottle.id }) else { return }
            steamArtworkRevisions[bottle.id, default: 0] &+= 1
            steamGames[bottle.id] = library.games
            steamGameErrors[bottle.id] = nil
            if library.truncated { truncatedSteamLibraries.insert(bottle.id) }
            else { truncatedSteamLibraries.remove(bottle.id) }
        } catch {
            if !Task.isCancelled { steamGameErrors[bottle.id] = error.localizedDescription }
        }
    }

    func refreshPublisherGames(in bottle: Bottle) async {
        guard !loadingPublisherGames.contains(bottle.id), !isManagingBottle, !isQuitting else { return }
        loadingPublisherGames.insert(bottle.id)
        defer { loadingPublisherGames.remove(bottle.id) }
        do {
            let cancellation = CoreInvocationCancellation()
            let library = try await withTaskCancellationHandler {
                try await callAsync(["publisher-games", bottle.id], as: PublisherLibrary.self, cancellation: cancellation)
            } onCancel: { cancellation.cancel() }
            guard !Task.isCancelled, bottles.contains(where: { $0.id == bottle.id }) else { return }
            publisherGames[bottle.id] = library.games
            publisherGameErrors[bottle.id] = nil
            if library.truncated { truncatedPublisherLibraries.insert(bottle.id) }
            else { truncatedPublisherLibraries.remove(bottle.id) }
        } catch {
            if !Task.isCancelled { publisherGameErrors[bottle.id] = error.localizedDescription }
        }
    }

    func runSteamGame(_ game: SteamGame, in bottle: Bottle) {
        guard !isStoppingBottle(bottle.id), !isQuitting, status.wine_path != nil, !isLaunching, !isManagingBottle else { return }
        isLaunching = true
        Task {
            defer { isLaunching = false }
            do {
                _ = try await callAsync(["run-steam-game", bottle.id, String(game.app_id)], as: LaunchResult.self)
                CaskrunWindowBridge.shared.hideMainWindow()
            } catch { message = error.localizedDescription }
        }
    }

    func refreshGameLibrary() async {
        guard !loadingGameLibrary, !isQuitting else { return }
        loadingGameLibrary = true
        defer { loadingGameLibrary = false }
        // Two local readers at a time; cancellation belongs to the Library view.
        let snapshot = Array(bottles.prefix(128))
        for start in stride(from: 0, to: snapshot.count, by: 2) {
            guard !Task.isCancelled else { return }
            await withTaskGroup(of: Void.self) { group in
                for bottle in snapshot[start..<min(start + 2, snapshot.count)] {
                    group.addTask {
                        await self.refreshSteamGames(in: bottle)
                        if !Task.isCancelled { await self.refreshPublisherGames(in: bottle) }
                    }
                }
            }
        }
        let active = Set(bottles.map(\.id))
        steamGames = steamGames.filter { active.contains($0.key) }
        steamArtworkRevisions = steamArtworkRevisions.filter { active.contains($0.key) }
        steamGameErrors = steamGameErrors.filter { active.contains($0.key) }
        publisherGames = publisherGames.filter { active.contains($0.key) }
        publisherGameErrors = publisherGameErrors.filter { active.contains($0.key) }
        truncatedSteamLibraries.formIntersection(active)
        truncatedPublisherLibraries.formIntersection(active)
    }

    func addBrowserGame(name: String, url: String) async throws {
        guard !isQuitting, !editingBrowserGames else {
            throw NSError(domain: "Caskrun", code: 1, userInfo: [NSLocalizedDescriptionKey: "Wait for the current edit to finish."])
        }
        editingBrowserGames = true
        defer { editingBrowserGames = false }
        let game = try await callAsync(["add-browser-game", name, url], as: BrowserGame.self)
        browserGames.removeAll { $0.id == game.id }
        browserGames.insert(game, at: 0)
        browserGamesError = nil
    }

    func removeBrowserGame(_ game: BrowserGame) {
        guard !isQuitting, !editingBrowserGames else { return }
        editingBrowserGames = true
        Task {
            defer { editingBrowserGames = false }
            do {
                browserGames = try await callAsync(["remove-browser-game", game.id], as: BrowserGames.self).games
                BrowserLaunchService.shared.removePreference(for: game.url)
            }
            catch { message = error.localizedDescription }
        }
    }

    func openBrowserGame(_ game: BrowserGame) {
        guard !isQuitting, let url = BrowserGame.checkedURL(game.url) else {
            if !isQuitting { message = "This browser game needs a valid public HTTPS address." }
            return
        }
        Task { @MainActor in
            guard !isQuitting else { return }
            do { try await BrowserLaunchService.shared.open(url.absoluteString) }
            catch { if !isQuitting { message = error.localizedDescription } }
        }
    }

    func loadGameIcon(_ game: SteamGame, in bottle: Bottle) {
        let key = "\(bottle.id):\(game.id)"
        let stamp = gameArtworkRevision(game, in: bottle)
        guard gameIcons[key] == nil || gameIconStamps[key] != stamp,
              requestedIcons.insert("game:\(key)").inserted else { return }
        Task {
            defer { requestedIcons.remove("game:\(key)") }
            let data = await SteamGameArtworkStore.shared.loadIcon(appID: game.app_id, localPath: game.icon_path,
                                                                 iconHash: game.icon_hash)
            guard !Task.isCancelled, bottles.contains(where: { $0.id == bottle.id }),
                  steamGames[bottle.id]?.contains(where: { $0.id == game.id && $0.icon_path == game.icon_path && $0.icon_hash == game.icon_hash }) == true,
                  gameArtworkRevision(game, in: bottle) == stamp else { return }
            guard let data else { gameIcons[key] = nil; gameIconStamps[key] = nil; return }
            gameIcons[key] = NSImage(data: data)
            gameIconStamps[key] = stamp
            gameIconOrder.removeAll { $0 == key }
            gameIconOrder.append(key)
            while gameIconOrder.count > 256 {
                let oldest = gameIconOrder.removeFirst()
                gameIcons[oldest] = nil; gameIconStamps[oldest] = nil
            }
        }
    }

    func gameArtworkRevision(_ game: SteamGame, in bottle: Bottle) -> String {
        "icon-v1:\(game.app_id):\(artworkGenerations[bottle.id, default: 0]):\(steamArtworkRevisions[bottle.id, default: 0]):\(game.icon_path ?? "none"):\(game.icon_hash ?? "none")"
    }

    func requestSteamInstall(appID: UInt32, in bottle: Bottle) {
        guard appID > 0, !isQuitting, !isLaunching, !isManagingBottle, status.wine_path != nil else { return }
        isLaunching = true
        Task {
            defer { isLaunching = false }
            do {
                _ = try await callAsync(["install-steam-game", bottle.id, String(appID)], as: LaunchResult.self)
                CaskrunWindowBridge.shared.hideMainWindow()
            } catch { message = error.localizedDescription }
        }
    }

    func addLauncher(to bottle: Bottle) {
        guard !isQuitting && !isManagingBottle && !isLaunching else {
            message = "Wait for the current bottle operation to finish before adding a launcher."
            return
        }
        let panel = NSOpenPanel()
        panel.message = "Choose a Windows program to save in \(bottle.name)"
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsOtherFileTypes = true
        if let prefix = try? call(["prefix-path", bottle.id], as: PrefixPathResult.self) {
            let programs = URL(fileURLWithPath: prefix.path, isDirectory: true)
                .appendingPathComponent("drive_c/Program Files", isDirectory: true)
            var isDirectory: ObjCBool = false
            if FileManager.default.fileExists(atPath: programs.path, isDirectory: &isDirectory),
               isDirectory.boolValue {
                panel.directoryURL = programs
            }
        }
        if panel.runModal() == .OK, let url = panel.url {
            guard !isQuitting && !isManagingBottle && !isLaunching else {
                message = "Wait for the current bottle operation to finish before adding a launcher."
                return
            }
            do {
                let name = url.deletingPathExtension().lastPathComponent
                let launcher = try call(["add-app", bottle.id, name, url.path], as: SavedLauncher.self)
                refresh()
                installHandoff = InstallHandoff(entryID: "manual", entryName: name,
                                                bottle: bottle, launcher: launcher, setupNote: nil)
            } catch { message = error.localizedDescription }
        }
    }

    func saveDiscoveredProgram(_ program: DiscoveredProgram, in bottle: Bottle) {
        guard !isQuitting && !isLaunching && !isManagingBottle else { return }
        do {
            // Save the whole command: dropping a shortcut's arguments would start
            // a different program than the shortcut does.
            var command = ["add-app", bottle.id, program.name, program.path]
            if let directory = program.working_directory {
                command.append(contentsOf: ["--working-directory", directory])
            }
            if !program.arguments.isEmpty {
                command.append("--")
                command.append(contentsOf: program.arguments)
            }
            let launcher = try call(command, as: SavedLauncher.self)
            refresh()
            manualInstallReview = nil
            installHandoff = InstallHandoff(entryID: "manual", entryName: program.name,
                                            bottle: bottle, launcher: launcher, setupNote: nil)
        } catch { message = error.localizedDescription }
    }

    func findInstalledPrograms(in bottle: Bottle) {
        guard !isQuitting && !isLaunching && !isManagingBottle else { return }
        isManagingBottle = true
        Task {
            defer { isManagingBottle = false }
            do {
                let result = try await callAsync(["discover-apps", bottle.id], as: DiscoveredProgramsResult.self)
                let saved = Set((launchers[bottle.id] ?? []).map(\.command))
                manualInstallReview = ManualInstallReview(
                    bottle: bottle,
                    installerName: "",
                    candidates: result.candidates.filter { !saved.contains($0.command) },
                    searchWasLimited: result.truncated,
                    discoveryUnavailable: false
                )
            } catch { message = error.localizedDescription }
        }
    }

    func runLauncher(_ launcher: SavedLauncher, in bottle: Bottle,
                     arguments: [String] = [], onFailure: (() -> Void)? = nil) {
        guard status.wine_path != nil else {
            message = "The included Windows engine is unavailable. Reinstall a complete Caskrun build."
            onFailure?()
            return
        }
        guard !isQuitting && !isLaunching && !isManagingBottle && !isStoppingBottle(bottle.id) else { return }
        isLaunching = true
        let isSteam = SteamStartup.isSteamLauncher(name: launcher.name, path: launcher.path,
            registered: savedSteamClients.contains { $0.bottle_id == bottle.id && $0.launcher_id == launcher.id })
        if isSteam { steamStartup = .init(elapsed: .zero) }
        Task {
            defer { isLaunching = false; steamStartup = nil }
            do {
                var command = ["run-app", bottle.id, launcher.id]
                if !arguments.isEmpty {
                    command.append("--")
                    command.append(contentsOf: arguments)
                }
                _ = try await callAsync(command, as: LaunchResult.self)
                if isSteam { try await waitForSteamWindow(in: bottle, launcher: launcher) }
                CaskrunWindowBridge.shared.hideMainWindow()
            } catch let outcome as SteamStartup.MonitorError {
                message = outcome.message
            } catch {
                message = error.localizedDescription
                onFailure?()
            }
        }
    }

    private func waitForSteamWindow(in bottle: Bottle, launcher: SavedLauncher,
                                    cancellation: CoreInvocationCancellation? = nil) async throws {
        let clock = ContinuousClock()
        let started = clock.now
        let token = steamStartupMonitor.begin()
        canStopWaitingForSteam = true
        defer {
            steamStartupMonitor.finish(token)
            canStopWaitingForSteam = false
            steamStartup = nil
        }
        repeat {
            guard !isQuitting, !Task.isCancelled, cancellation?.isCancelRequested != true else {
                throw CancellationError()
            }
            guard steamStartupMonitor.isCurrent(token) else { throw SteamStartup.MonitorError.stopped }
            steamStartup = .init(elapsed: started.duration(to: clock.now))
            if SteamStartup.hasVisibleClientWindow(bottleID: bottle.id, displayName: launcher.name) {
                guard steamStartupMonitor.isCurrent(token) else { throw SteamStartup.MonitorError.stopped }
                return
            }
            try await Task.sleep(for: .milliseconds(500))
        } while started.duration(to: clock.now) < SteamStartup.timeout
        guard steamStartupMonitor.isCurrent(token) else { throw SteamStartup.MonitorError.stopped }
        throw SteamStartup.MonitorError.timedOut
    }

    func stopWaitingForSteam() {
        guard canStopWaitingForSteam else { return }
        steamStartupMonitor.stop()
        canStopWaitingForSteam = false
        // The polling task will report the distinct stopped outcome and release
        // launch controls. Nothing is sent to Steam, Wine or the Core invocation.
    }

    func runCommand(in bottle: Bottle) {
        guard !isQuitting, status.wine_path != nil, !isLaunching, !isManagingBottle, !isStoppingBottle(bottle.id) else { return }
        let panel = NSOpenPanel()
        panel.message = "Choose a Windows .exe to run once in \(bottle.name)"
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowsOtherFileTypes = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        guard url.pathExtension.lowercased() == "exe" else {
            message = "Choose a Windows .exe program. Use Install app for .msi installers."
            return
        }
        let chosenPath = url.resolvingSymlinksInPath().standardizedFileURL.path
        guard !isQuitting && !isLaunching && !isManagingBottle else { return }
        let savedLaunchers = (launchers[bottle.id] ?? []).filter {
            URL(fileURLWithPath: $0.path).resolvingSymlinksInPath().standardizedFileURL.path == chosenPath
        }
        pendingRun = OneTimeProgram(bottle: bottle, url: url, savedLaunchers: savedLaunchers)
    }

    func runOnce(_ program: OneTimeProgram, arguments: [String], launcher: SavedLauncher?) {
        guard !isQuitting && !isLaunching && !isManagingBottle && !isStoppingBottle(program.bottle.id) && status.wine_path != nil else { return }
        if let saved = launcher {
            // Preserve saved recipe options, Steam identity and bootstrap routing.
            runLauncher(saved, in: program.bottle, arguments: arguments)
            return
        }
        isLaunching = true
        Task {
            defer { isLaunching = false }
            do {
                _ = try await callAsync(["launch", program.bottle.id, program.url.path, "--"] + arguments,
                                        as: LaunchResult.self)
                CaskrunWindowBridge.shared.hideMainWindow()
            } catch { message = error.localizedDescription }
        }
    }

    func forceQuitApps(in bottle: Bottle) {
        guard !isQuitting, !isManagingBottle, !isLaunching,
              sessionState(for: bottle).phase != .forcing else { return }
        let pending = bottleStopOperations[bottle.id]
        let token = bottleSessions[bottle.id, default: BottleSessionState()].beginForceQuit()
        pending?.cancellation.cancel()
        let cancellation = CoreInvocationCancellation()
        let task = Task { @MainActor in
            defer {
                if bottleStopOperations[bottle.id]?.id == token {
                    bottleStopOperations[bottle.id] = nil
                    if cancellation.isCancelRequested { bottleSessions[bottle.id]?.interrupted(token) }
                }
            }
            // Reap the polite request before forcing the bottle. Otherwise its
            // late cleanup could overwrite the Force Quit tracking record.
            await pending?.task.value
            guard !isQuitting, !cancellation.isCancelRequested else { return }
            do {
                _ = try await callAsync(["quit-bottle", bottle.id], as: BottleQuitResult.self,
                                       cancellation: cancellation)
                bottleSessions[bottle.id]?.forceFinished(token)
            } catch {
                guard !isQuitting else { return }
                bottleSessions[bottle.id]?.failed(token, message: error.localizedDescription)
            }
            await refreshBottleActivity(in: bottle)
            await monitorBottleQuit(in: bottle, token: token, cancellation: cancellation)
        }
        bottleStopOperations[bottle.id] = BottleStopOperation(id: token, cancellation: cancellation, task: task)
    }

    func openUninstaller(in bottle: Bottle) {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        isLaunching = true
        Task {
            defer { isLaunching = false }
            do {
                _ = try await callAsync(["open-uninstaller", bottle.id], as: LaunchResult.self)
            } catch { message = error.localizedDescription }
        }
    }

    func openWineConfiguration(in bottle: Bottle) {
        openWineTool("open-winecfg", in: bottle)
    }

    func openRegistryEditor(in bottle: Bottle) {
        openWineTool("open-regedit", in: bottle)
    }

    func openControllerPanel(in bottle: Bottle) {
        openWineTool("open-controller-panel", in: bottle)
    }

    func openTaskManager(in bottle: Bottle) {
        openWineTool("open-taskmgr", in: bottle)
    }

    func openInternetSettings(in bottle: Bottle) {
        openWineTool("open-internet-settings", in: bottle)
    }

    func openCommandPrompt(in bottle: Bottle) {
        openWineTool("open-command-prompt", in: bottle)
    }

    func simulateReboot(in bottle: Bottle) {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        isManagingBottle = true
        Task {
            defer { isManagingBottle = false }
            do {
                let result = try await callAsync(["simulate-reboot", bottle.id], as: BottleQuitResult.self)
                message = result.message
            } catch { message = error.localizedDescription }
        }
    }

    func refreshBottleResolution(in bottle: Bottle) {
        guard !loadingBottleResolutions.contains(bottle.id) else { return }
        loadingBottleResolutions.insert(bottle.id)
        bottleResolutionErrors.removeValue(forKey: bottle.id)
        Task {
            defer { loadingBottleResolutions.remove(bottle.id) }
            do {
                let result = try await callAsync(["bottle-resolution", bottle.id], as: BottleResolutionStatus.self)
                guard result.bottle_id == bottle.id else {
                    throw NSError(domain: "Caskrun", code: 1,
                                  userInfo: [NSLocalizedDescriptionKey: "Display status belongs to another bottle."])
                }
                bottleResolutions[bottle.id] = result
            } catch {
                bottleResolutions.removeValue(forKey: bottle.id)
                bottleResolutionErrors[bottle.id] = error.localizedDescription
            }
        }
    }

    func setBottleResolution(in bottle: Bottle, enabled: Bool) {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        isManagingBottle = true
        Task {
            defer { isManagingBottle = false }
            do {
                let result = try await callAsync(
                    ["set-bottle-resolution", bottle.id, enabled ? "on" : "off"],
                    as: BottleResolutionStatus.self)
                guard result.bottle_id == bottle.id else {
                    throw NSError(domain: "Caskrun", code: 1,
                                  userInfo: [NSLocalizedDescriptionKey: "Display result belongs to another bottle."])
                }
                bottleResolutions[bottle.id] = result
                bottleResolutionErrors.removeValue(forKey: bottle.id)
            } catch { message = error.localizedDescription }
        }
    }

    func refreshBottleGraphics(in bottle: Bottle) {
        guard !loadingBottleGraphics.contains(bottle.id) else { return }
        let requestedWine = wineExecutable
        loadingBottleGraphics.insert(bottle.id)
        bottleGraphicsErrors.removeValue(forKey: bottle.id)
        Task {
            defer { loadingBottleGraphics.remove(bottle.id) }
            do {
                let result = try await callAsync(["bottle-graphics", bottle.id], as: BottleGraphicsStatus.self)
                guard wineExecutable == requestedWine, bottles.contains(where: { $0.id == bottle.id }) else { return }
                try validateGraphicsResult(result, for: bottle)
                bottleGraphics[bottle.id] = result
            } catch {
                guard wineExecutable == requestedWine, bottles.contains(where: { $0.id == bottle.id }) else { return }
                bottleGraphics.removeValue(forKey: bottle.id)
                bottleGraphicsErrors[bottle.id] = error.localizedDescription
            }
        }
    }

    func setBottleGraphics(in bottle: Bottle, backend: String) {
        guard !isQuitting && !isManagingBottle && !isLaunching &&
                !loadingBottleGraphics.contains(bottle.id) else { return }
        guard ["auto", "wined3d", "dxmt"].contains(backend),
              let current = bottleGraphics[bottle.id], current.selected_backend != backend,
              backend != "dxmt" || current.dxmt_available else { return }
        let requestedWine = wineExecutable
        isManagingBottle = true
        bottleOperationLabel = "Updating graphics…"
        bottleGraphicsErrors.removeValue(forKey: bottle.id)
        Task {
            defer {
                isManagingBottle = false
                bottleOperationLabel = "Updating bottle…"
            }
            do {
                let result = try await callAsync(["set-bottle-graphics", bottle.id, backend], as: BottleGraphicsStatus.self)
                guard wineExecutable == requestedWine, bottles.contains(where: { $0.id == bottle.id }) else { return }
                try validateGraphicsResult(result, for: bottle)
                bottleGraphics[bottle.id] = result
            } catch {
                guard wineExecutable == requestedWine else { return }
                let detail = error.localizedDescription
                // Keep the old selection visible unless the core confirms a change.
                // Reading again also reports a successful rollback after a refusal.
                let restored = try? await callAsync(["bottle-graphics", bottle.id], as: BottleGraphicsStatus.self)
                guard wineExecutable == requestedWine, bottles.contains(where: { $0.id == bottle.id }) else { return }
                if let restored,
                   (try? validateGraphicsResult(restored, for: bottle)) != nil {
                    bottleGraphics[bottle.id] = restored
                }
                bottleGraphicsErrors[bottle.id] = detail
                message = detail
            }
        }
    }

    private func validateGraphicsResult(_ result: BottleGraphicsStatus, for bottle: Bottle) throws {
        guard result.bottle_id == bottle.id,
              ["auto", "wined3d", "dxmt"].contains(result.selected_backend),
              result.effective_backend == (result.selected_backend == "dxmt" ? "dxmt" : "wined3d"),
              result.dxmt_frame_rate <= 240, [100, 125, 150, 200].contains(result.dxmt_spatial_scale) else {
            throw NSError(domain: "Caskrun", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "Graphics status is unavailable for this bottle."])
        }
    }

    func setDXMTOptions(in bottle: Bottle, frameRate: UInt16, spatialScale: UInt16) {
        guard !isQuitting, !isLaunching, !isManagingBottle,
              let current = bottleGraphics[bottle.id], current.selected_backend == "dxmt",
              current.dxmt_available, frameRate <= 240,
              [100, 125, 150, 200].contains(spatialScale),
              spatialScale == 100 || current.dxmt_spatial_available else { return }
        isManagingBottle = true
        bottleOperationLabel = "Updating DXMT options…"
        let requestedWine = wineExecutable
        Task {
            defer { isManagingBottle = false; bottleOperationLabel = "Updating bottle…" }
            do {
                let result = try await callAsync(["set-bottle-dxmt-options", bottle.id, String(frameRate), String(spatialScale)], as: BottleGraphicsStatus.self)
                guard requestedWine == wineExecutable, bottles.contains(where: { $0.id == bottle.id }) else { return }
                try validateGraphicsResult(result, for: bottle)
                bottleGraphics[bottle.id] = result
                bottleGraphicsErrors[bottle.id] = nil
            } catch {
                guard requestedWine == wineExecutable else { return }
                bottleGraphicsErrors[bottle.id] = error.localizedDescription
                message = error.localizedDescription
                refreshBottleGraphics(in: bottle)
            }
        }
    }

    private func openWineTool(_ command: String, in bottle: Bottle) {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        isLaunching = true
        Task {
            defer { isLaunching = false }
            do {
                _ = try await callAsync([command, bottle.id], as: LaunchResult.self)
            } catch { message = error.localizedDescription }
        }
    }

    func setLauncherVisibility(_ launcher: SavedLauncher, in bottle: Bottle, hidden: Bool) {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        do {
            _ = try call(["set-app-visibility", bottle.id, launcher.id, hidden ? "hide" : "show"],
                         as: SavedLauncher.self)
            refresh()
        } catch { message = error.localizedDescription }
    }

    func setLauncherArguments(_ launcher: SavedLauncher, in bottle: Bottle, arguments: [String]) {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        do {
            _ = try call(["set-app-arguments", bottle.id, launcher.id, "--"] + arguments,
                         as: SavedLauncher.self)
            refresh()
        } catch { message = error.localizedDescription }
    }

    func renameLauncher(_ launcher: SavedLauncher, in bottle: Bottle, to name: String) {
        guard !isQuitting && !isLaunching && !isManagingBottle else { return }
        do {
            _ = try call(["rename-app", bottle.id, launcher.id, name], as: SavedLauncher.self)
            refresh()
        } catch { message = error.localizedDescription }
    }

    func repairIcons(in bottle: Bottle) {
        guard !isQuitting && !isLaunching && !isManagingBottle else { return }
        isManagingBottle = true
        bottleOperationLabel = "Rebuilding icons…"
        Task {
            defer { isManagingBottle = false; bottleOperationLabel = "Updating bottle…" }
            do {
                let result = try await callAsync(["rebuild-icons", bottle.id], as: IconRepairResult.self)
                guard result.bottle_id == bottle.id else { return }
                artworkGenerations[bottle.id, default: 0] += 1
                bottleIcons.removeValue(forKey: bottle.id)
                bottleIconStamps.removeValue(forKey: bottle.id)
                for launcher in launchers[bottle.id] ?? [] {
                    let key = "\(bottle.id):\(launcher.id)"
                    launcherIcons.removeValue(forKey: key)
                    launcherIconStamps.removeValue(forKey: key)
                    loadLauncherIcon(launcher, in: bottle)
                }
                loadBottleIcon(bottle)
                message = result.message
            } catch { message = error.localizedDescription }
        }
    }

    func installedPrograms(in bottle: Bottle) async throws -> ProgramInventory {
        try await callAsync(["installed-programs", bottle.id], as: ProgramInventory.self)
    }

    func diagnostics(in bottle: Bottle) async throws -> BottleDiagnosticReport {
        try await callAsync(["diagnostics", bottle.id], as: BottleDiagnosticReport.self)
    }

    func notes(in bottle: Bottle) async throws -> BottleNotes {
        try await callAsync(["bottle-notes", bottle.id], as: BottleNotes.self)
    }

    func saveNotes(_ notes: String, in bottle: Bottle) async throws {
        guard !isQuitting && !isLaunching && !isManagingBottle else {
            throw CocoaError(.userCancelled)
        }
        isManagingBottle = true
        defer { isManagingBottle = false }
        let result = try await callAsync(["set-bottle-notes", bottle.id, notes], as: BottleNotes.self)
        guard result.bottle_id == bottle.id else { throw CocoaError(.coderInvalidValue) }
    }

    func refreshWindowsVersion(in bottle: Bottle) {
        guard !isQuitting && !loadingWindowsVersions.contains(bottle.id) else { return }
        loadingWindowsVersions.insert(bottle.id)
        let wine = wineExecutable
        Task {
            defer { loadingWindowsVersions.remove(bottle.id) }
            do {
                let result = try await callAsync(["bottle-windows-version", bottle.id], as: BottleWindowsVersion.self)
                guard result.bottle_id == bottle.id, wineExecutable == wine,
                      bottles.contains(where: { $0.id == bottle.id }) else { return }
                bottleWindowsVersions[bottle.id] = result
                windowsVersionErrors.removeValue(forKey: bottle.id)
            } catch {
                guard wineExecutable == wine else { return }
                windowsVersionErrors[bottle.id] = error.localizedDescription
            }
        }
    }

    func setWindowsVersion(_ version: String, in bottle: Bottle) {
        guard !isQuitting && !isLaunching && !isManagingBottle && !loadingWindowsVersions.contains(bottle.id) else { return }
        isManagingBottle = true
        loadingWindowsVersions.insert(bottle.id)
        bottleOperationLabel = "Updating Windows version…"
        let wine = wineExecutable
        Task {
            defer { loadingWindowsVersions.remove(bottle.id); isManagingBottle = false; bottleOperationLabel = "Updating bottle…" }
            do {
                let result = try await callAsync(["set-bottle-windows-version", bottle.id, version], as: BottleWindowsVersion.self)
                guard result.bottle_id == bottle.id, wineExecutable == wine else { return }
                bottleWindowsVersions[bottle.id] = result
                windowsVersionErrors.removeValue(forKey: bottle.id)
            } catch {
                guard wineExecutable == wine else { return }
                windowsVersionErrors[bottle.id] = error.localizedDescription
                if let current = try? await callAsync(["bottle-windows-version", bottle.id], as: BottleWindowsVersion.self),
                   current.bottle_id == bottle.id, wineExecutable == wine { bottleWindowsVersions[bottle.id] = current }
            }
        }
    }

    func logs(in bottle: Bottle) async throws -> [BottleLog] {
        try await callAsync(["logs", bottle.id], as: [BottleLog].self)
    }

    func readLog(_ name: String, in bottle: Bottle) async throws -> BottleLogContent {
        try await callAsync(["read-log", bottle.id, name], as: BottleLogContent.self)
    }

    func chooseWine() {
        let panel = NSOpenPanel()
        panel.message = "Choose an independently installed Wine executable"
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsOtherFileTypes = true
        if panel.runModal() == .OK, let url = panel.url {
            let lower = url.path.lowercased()
            if lower.contains("/crossover.app/") || lower.contains("/crossover-lab/") {
                message = "Choose an independent Wine runtime outside CrossOver and CrossOver-Lab."
                return
            }
            let previous = wineExecutable
            wineExecutable = url.path
            do {
                let candidate = try call(["status"], as: EngineStatus.self)
                guard candidate.wine_path != nil else {
                    wineExecutable = previous
                    message = "Choose an executable Wine file with wineboot beside it, outside CrossOver."
                    return
                }
            } catch {
                wineExecutable = previous
                message = error.localizedDescription
                return
            }
            UserDefaults.standard.set(url.path, forKey: "wineExecutable")
            UserDefaults.standard.set(true, forKey: "useCustomWine")
            runtimeProbeGeneration += 1
            isProbingRuntime = false
            runtimeProbe = nil
            refresh()
        }
    }

    func useBundledRuntime() {
        guard let path = Self.bundledWinePath else { return }
        UserDefaults.standard.set(false, forKey: "useCustomWine")
        wineExecutable = path
        runtimeProbeGeneration += 1
        isProbingRuntime = false
        runtimeProbe = nil
        refresh()
    }

    func checkWineCommand() {
        guard status.wine_path != nil else {
            message = "The Windows engine is unavailable in this build."
            return
        }
        guard !isQuitting && !isProbingRuntime else { return }
        runtimeProbeGeneration += 1
        let generation = runtimeProbeGeneration
        let requestedWine = wineExecutable
        isProbingRuntime = true
        Task {
            defer {
                if runtimeProbeGeneration == generation { isProbingRuntime = false }
            }
            do {
                let result = try await callAsync(["probe-runtime"], as: RuntimeProbeResult.self)
                guard runtimeProbeGeneration == generation, wineExecutable == requestedWine else { return }
                runtimeProbe = result
            } catch {
                guard runtimeProbeGeneration == generation, wineExecutable == requestedWine else { return }
                runtimeProbe = nil
                message = error.localizedDescription
            }
        }
    }

    func chooseInstaller(in requestedBottleID: String? = nil, for appName: String? = nil) {
        guard !isQuitting && !isLaunching && !isManagingBottle else { return }
        if let requestedBottleID, !bottles.contains(where: { $0.id == requestedBottleID }) {
            message = "The selected bottle is unavailable. Choose another bottle."
            return
        }
        guard status.wine_path != nil else {
            message = "The included Windows engine is unavailable. Reinstall a complete Caskrun build."
            return
        }
        let panel = NSOpenPanel()
        panel.message = appName.map { "Choose the downloaded Windows installer for \($0)" }
            ?? "Choose a Windows .exe or .msi installer"
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsOtherFileTypes = true
        if panel.runModal() == .OK, let url = panel.url {
            guard ["exe", "msi"].contains(url.pathExtension.lowercased()) else {
                message = "Choose a Windows .exe or .msi installer."
                return
            }
            let bottle: Bottle
            if let requestedBottleID, let existing = bottles.first(where: { $0.id == requestedBottleID }) {
                bottle = existing
            } else {
                let proposedName = String((appName ?? url.deletingPathExtension().lastPathComponent).prefix(80))
                guard let created = createBottle(proposedName.isEmpty ? "Windows app" : proposedName, announce: false) else { return }
                bottle = created
            }
            let bottleID = bottle.id
            selectedBottleID = bottleID
            isLaunching = true
            activeInstallerName = url.lastPathComponent
            manualInstallReview = nil
            let cancellation = CoreInvocationCancellation()
            activeManualInstaller = cancellation
            canCancelManualInstaller = true
            isCancellingManualInstaller = false
            Task {
                defer {
                    if activeManualInstaller === cancellation {
                        activeManualInstaller = nil
                    }
                    canCancelManualInstaller = false
                    isCancellingManualInstaller = false
                    isLaunching = false
                    activeInstallerName = nil
                }
                do {
                    let before = try? await callAsync(["discover-apps", bottleID], as: DiscoveredProgramsResult.self)
                    let knownCommands = Set((before?.candidates ?? []).map(\.command))
                    let result = try await callAsync(["launch-wait", bottleID, url.path],
                                                     as: WaitLaunchResult.self, cancellation: cancellation)
                    canCancelManualInstaller = false
                    if cancellation.isCancelRequested {
                        message = "The installer finished before Caskrun could cancel it. Open Manage bottles to inspect its bottle."
                    } else if result.timed_out {
                        message = "The installer did not finish within 30 minutes. Open Logs for \(bottle.name) in its Windows details panel to investigate."
                    } else if result.exit_code == 0 {
                        let after = try? await callAsync(["discover-apps", bottleID], as: DiscoveredProgramsResult.self)
                        let newPrograms = before == nil ? [] : (after?.candidates ?? []).filter {
                            !knownCommands.contains($0.command)
                        }
                        manualInstallReview = ManualInstallReview(bottle: bottle,
                                                                  installerName: url.lastPathComponent,
                                                                  candidates: newPrograms,
                                                                  searchWasLimited: (before?.truncated == true || after?.truncated == true),
                                                                  discoveryUnavailable: before == nil || after == nil)
                    } else if let code = result.exit_code {
                        message = "The installer closed with error code \(code). Open Logs for \(bottle.name) in its Windows details panel to investigate."
                    } else if let signal = result.signal {
                        message = "The installer stopped unexpectedly (signal \(signal)). Open Logs for \(bottle.name) in its Windows details panel to investigate."
                    } else {
                        message = "The installer stopped unexpectedly. Open Logs for \(bottle.name) in its Windows details panel to investigate."
                    }
                } catch {
                    let coreError = error as NSError
                    if coreError.domain == "Caskrun" && coreError.code == 130 {
                        message = "Setup canceled. The bottle remains available on Collection."
                    } else if cancellation.isCancelRequested {
                        message = "Setup needs attention after cancellation: \(error.localizedDescription)"
                    } else {
                        message = error.localizedDescription
                    }
                }
            }
        }
    }

    func installCatalog(_ entry: CatalogEntry, thenInstallSteamAppID: UInt32? = nil) {
        guard entry.installMode == "automatic" else { return }
        guard status.wine_path != nil else {
            message = "The included Windows engine is unavailable. Reinstall a complete Caskrun build."
            return
        }
        guard !isQuitting && !isLaunching && !isManagingBottle else { return }
        let cancellation = CoreInvocationCancellation()
        activeCatalogCancellation = cancellation
        canCancelCatalogInstall = true
        isCancellingCatalogInstall = false
        isLaunching = true
        activeCatalogInstall = entry.name
        activeCatalogStage = nil
        activeCatalogBottleID = nil
        installHandoff = nil
        catalogSetupFailure = nil
        let previousBottleIDs = Set(catalogAttempts.map(\.bottle_id))
        monitorCatalogAttempt(catalogID: entry.id,
                              expectedBottleID: nil,
                              excludedBottleIDs: previousBottleIDs)
        Task {
            defer {
                if activeCatalogCancellation === cancellation {
                    activeCatalogCancellation = nil
                }
                canCancelCatalogInstall = false
                isCancellingCatalogInstall = false
                isLaunching = false
                activeCatalogInstall = nil
                activeCatalogStage = nil
                activeCatalogBottleID = nil
                steamStartup = nil
                catalogMonitorGeneration += 1
            }
            do {
                let result = try await callAsync(["install-catalog", entry.id],
                                                 as: CatalogInstallResult.self, cancellation: cancellation)
                canCancelCatalogInstall = false
                refresh()
                if cancellation.isCancelRequested {
                    message = "Setup finished before Caskrun could cancel it. Select the bottle on Collection."
                    return
                }
                selectedBottleID = result.bottle.id
                installHandoff = InstallHandoff(entryID: entry.id,
                                                entryName: entry.name,
                                                bottle: result.bottle,
                                                launcher: result.launcher,
                                                setupNote: result.setup_note)
                if entry.id == "steam" {
                    try await waitForSteamWindow(in: result.bottle, launcher: result.launcher, cancellation: cancellation)
                }
                if entry.id == "steam", let appID = thenInstallSteamAppID, appID > 0,
                   !isQuitting, !cancellation.isCancelRequested {
                    do {
                        _ = try await callAsync(["install-steam-game", result.bottle.id, String(appID)], as: LaunchResult.self)
                    } catch {
                        message = "Steam is set up, but the game install request needs another try: \(error.localizedDescription)"
                        return
                    }
                }
                CaskrunWindowBridge.shared.hideMainWindow()
            } catch let outcome as SteamStartup.MonitorError {
                // Core setup succeeded. A slow or dismissed window monitor is
                // startup feedback, never an installer failure requiring Retry.
                message = outcome.message
            } catch {
                canCancelCatalogInstall = false
                refresh()
                let coreError = error as NSError
                if coreError.domain == "Caskrun" && coreError.code == 130 {
                    message = "Setup canceled. If timeout cleanup had already begun, Windows apps in this bottle may also have stopped. Inspect the bottle on Collection before retrying."
                    return
                }
                if cancellation.isCancelRequested {
                    message = "Setup needs attention after cancellation: \(error.localizedDescription)"
                    return
                }
                let bottleID = activeCatalogBottleID ?? catalogAttempts
                    .filter { $0.catalog_id == entry.id && !previousBottleIDs.contains($0.bottle_id) }
                    .max(by: { $0.updated_at < $1.updated_at })?.bottle_id
                catalogSetupFailure = CatalogSetupFailure(entryID: entry.id,
                                                          entryName: entry.name,
                                                          bottleID: bottleID,
                                                          detail: error.localizedDescription)
            }
        }
    }

    func retryCatalog(_ attempt: CatalogAttempt) {
        guard attempt.stage != .installed else { return }
        guard status.wine_path != nil else {
            message = "The included Windows engine is unavailable. Reinstall a complete Caskrun build."
            return
        }
        guard !isQuitting && !isLaunching && !isManagingBottle else { return }
        let entry = CatalogSource.loaded.entries.first { $0.id == attempt.catalog_id }
        let entryName = entry?.name ?? "store"
        let cancellation = CoreInvocationCancellation()
        activeCatalogCancellation = cancellation
        canCancelCatalogInstall = true
        isCancellingCatalogInstall = false
        isLaunching = true
        activeCatalogInstall = entryName
        activeCatalogStage = attempt.stage
        activeCatalogBottleID = attempt.bottle_id
        installHandoff = nil
        catalogSetupFailure = nil
        monitorCatalogAttempt(catalogID: attempt.catalog_id,
                              expectedBottleID: attempt.bottle_id,
                              excludedBottleIDs: [])
        Task {
            defer {
                if activeCatalogCancellation === cancellation {
                    activeCatalogCancellation = nil
                }
                canCancelCatalogInstall = false
                isCancellingCatalogInstall = false
                isLaunching = false
                activeCatalogInstall = nil
                activeCatalogStage = nil
                activeCatalogBottleID = nil
                steamStartup = nil
                catalogMonitorGeneration += 1
            }
            do {
                let result = try await callAsync(["retry-catalog", attempt.catalog_id, attempt.bottle_id],
                                                 as: CatalogInstallResult.self, cancellation: cancellation)
                canCancelCatalogInstall = false
                refresh()
                if cancellation.isCancelRequested {
                    message = "Setup finished before Caskrun could cancel it. Select the bottle on Collection."
                    return
                }
                selectedBottleID = result.bottle.id
                installHandoff = InstallHandoff(entryID: attempt.catalog_id,
                                                entryName: entryName,
                                                bottle: result.bottle,
                                                launcher: result.launcher,
                                                setupNote: result.setup_note)
                if attempt.catalog_id == "steam" {
                    try await waitForSteamWindow(in: result.bottle, launcher: result.launcher, cancellation: cancellation)
                }
                CaskrunWindowBridge.shared.hideMainWindow()
            } catch let outcome as SteamStartup.MonitorError {
                message = outcome.message
            } catch {
                canCancelCatalogInstall = false
                refresh()
                let coreError = error as NSError
                if coreError.domain == "Caskrun" && coreError.code == 130 {
                    message = "Setup canceled. If timeout cleanup had already begun, Windows apps in this bottle may also have stopped. Inspect the bottle on Collection before retrying."
                    return
                }
                if cancellation.isCancelRequested {
                    message = "Setup needs attention after cancellation: \(error.localizedDescription)"
                    return
                }
                catalogSetupFailure = CatalogSetupFailure(entryID: attempt.catalog_id,
                                                          entryName: entryName,
                                                          bottleID: attempt.bottle_id,
                                                          detail: error.localizedDescription)
            }
        }
    }

    private func monitorCatalogAttempt(catalogID: String,
                                       expectedBottleID: String?,
                                       excludedBottleIDs: Set<String>) {
        catalogMonitorGeneration += 1
        let generation = catalogMonitorGeneration
        Task {
            while isLaunching && generation == catalogMonitorGeneration {
                if let attempts = try? await callAsync(["catalog-attempts"], as: [CatalogAttempt].self),
                   let attempt = attempts.filter({ candidate in
                       candidate.catalog_id == catalogID &&
                           (expectedBottleID == nil
                            ? !excludedBottleIDs.contains(candidate.bottle_id)
                            : candidate.bottle_id == expectedBottleID)
                   }).max(by: { $0.updated_at < $1.updated_at }),
                   generation == catalogMonitorGeneration {
                    activeCatalogStage = attempt.stage
                    activeCatalogBottleID = attempt.bottle_id
                }
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }
}

private struct LauncherArtwork: View {
    @ObservedObject var model: CaskrunModel
    let bottle: Bottle
    let launcher: SavedLauncher
    let fallbackSymbol: String
    let size: CGFloat

    var body: some View {
        ZStack {
            if let icon = model.launcherIcons["\(bottle.id):\(launcher.id)"] {
                Image(nsImage: icon)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
            } else {
                RoundedRectangle(cornerRadius: 10)
                    .fill(.primary.opacity(0.06))
                Image(systemName: fallbackSymbol)
                    .font(.system(size: size * 0.46))
                    .foregroundStyle(.secondary)
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
        .task(id: launcher.id) { model.loadLauncherIcon(launcher, in: bottle) }
    }
}

private struct LauncherBadge: View {
    @ObservedObject var model: CaskrunModel
    let bottle: Bottle
    let launcher: SavedLauncher
    let fallbackSymbol: String
    var size: CGFloat = 56

    var body: some View {
        LauncherArtwork(model: model, bottle: bottle, launcher: launcher,
                        fallbackSymbol: fallbackSymbol, size: size)
    }
}

private struct RunWithOptionsSheet: View {
    let item: SavedProgram
    let onRun: ([String]) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var argumentLines = ""

    private var arguments: [String] {
        LaunchArgumentDraft.parse(argumentLines)
    }

    private var validationMessage: String? {
        LaunchArgumentDraft.validation(arguments, saved: item.launcher.saved_arguments ?? [])
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Run \(item.launcher.name) with options")
                .font(.title2.weight(.semibold))
            Text("Add temporary arguments after any saved options for this launch. Your saved launcher stays unchanged.")
                .foregroundStyle(.secondary)
            Text("Arguments — one per line")
                .font(.subheadline.weight(.semibold))
            TextEditor(text: $argumentLines)
                .font(.system(.body, design: .monospaced))
                .frame(height: 150)
                .overlay {
                    RoundedRectangle(cornerRadius: 8)
                        .strokeBorder(.primary.opacity(0.15))
                }
                .accessibilityLabel("Arguments, one per line")
            Text("Each line is one argument, including spaces within it. Blank lines are ignored; do not add shell quotes. Caskrun keeps required app options automatically.")
                .font(.caption)
                .foregroundStyle(.secondary)
            if let validationMessage {
                Text(validationMessage)
                    .font(.caption)
                    .foregroundStyle(.red)
            }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Run") {
                    onRun(arguments)
                    dismiss()
                }
                .buttonStyle(.borderedProminent)
                .disabled(validationMessage != nil)
            }
        }
        .padding(24)
        .frame(width: 520)
    }
}

private struct SavedLaunchArgumentsSheet: View {
    let item: SavedProgram
    let onSave: ([String]) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var argumentLines: String

    init(item: SavedProgram, onSave: @escaping ([String]) -> Void) {
        self.item = item
        self.onSave = onSave
        _argumentLines = State(initialValue: (item.launcher.saved_arguments ?? []).joined(separator: "\n"))
    }

    private var arguments: [String] {
        LaunchArgumentDraft.parse(argumentLines)
    }

    private var validationMessage: String? {
        LaunchArgumentDraft.validation(arguments)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Saved arguments for \(item.launcher.name)")
                .font(.title2.weight(.semibold))
            Text("These options are used whenever you open this app from Collection or its bottle.")
                .foregroundStyle(.secondary)
            Text("Arguments — one per line")
                .font(.subheadline.weight(.semibold))
            TextEditor(text: $argumentLines)
                .font(.system(.body, design: .monospaced))
                .frame(height: 150)
                .overlay {
                    RoundedRectangle(cornerRadius: 8)
                        .strokeBorder(.primary.opacity(0.15))
                }
                .accessibilityLabel("Saved arguments, one per line")
            Text("Each line is one argument; spaces within a line are kept. Do not add shell quotes. Caskrun keeps required app options automatically.")
                .font(.caption)
                .foregroundStyle(.secondary)
            if let validationMessage {
                Text(validationMessage)
                    .font(.caption)
                    .foregroundStyle(.red)
            }
            HStack {
                Button("Reset to Default") {
                    onSave([])
                    dismiss()
                }
                .disabled((item.launcher.saved_arguments ?? []).isEmpty && arguments.isEmpty)
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Save") {
                    onSave(arguments)
                    dismiss()
                }
                .buttonStyle(.borderedProminent)
                .disabled(validationMessage != nil)
            }
        }
        .padding(24)
        .frame(width: 520)
    }
}

private struct BottleArtwork: View {
    @ObservedObject var model: CaskrunModel
    let bottle: Bottle
    var size: CGFloat = 42

    var body: some View {
        ZStack {
            if let icon = model.bottleIcons[bottle.id] {
                Image(nsImage: icon)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
            } else {
                RoundedRectangle(cornerRadius: 12)
                    .fill(.primary.opacity(0.06))
                Image(systemName: "shippingbox.fill")
                    .font(.title2)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
        .task(id: bottle.id) { model.loadBottleIcon(bottle) }
    }
}

private struct CatalogArtworkTile: View {
    @ObservedObject var store: CatalogArtworkStore
    let id: String
    let size: CGFloat

    private var fallback: (label: String, background: Color, foreground: Color) {
        switch id {
        case "steam": return ("S", Color(red: 0.07, green: 0.19, blue: 0.31), .white)
        case "epic-games-store": return ("EPIC", .white, .black)
        case "gog-galaxy": return ("GOG", Color(red: 0.16, green: 0.03, blue: 0.24), .white)
        case "battle-net": return ("B", Color(red: 0.07, green: 0.12, blue: 0.2), .white)
        case "ubisoft-connect": return ("UBI", .white, .black)
        case "ea-app": return ("EA", .black, .white)
        case "itch-io-app": return ("itch", Color(red: 0.96, green: 0.36, blue: 0.36), .white)
        case "rockstar-games-launcher": return ("R*", Color(red: 1, green: 0.69, blue: 0), .black)
        default: return ("APP", .gray, .white)
        }
    }

    var body: some View {
        ZStack {
            if let image = store.images[id] {
                Image(nsImage: image)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
            } else {
                RoundedRectangle(cornerRadius: size * 0.23).fill(fallback.background)
                Text(fallback.label)
                    .font(.system(size: size * (fallback.label.count > 2 ? 0.25 : 0.39),
                                  weight: .black, design: .rounded))
                    .foregroundStyle(fallback.foreground)
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
        .task(id: id) { store.load(id) }
    }
}

private struct QuietToolButtonStyle: ButtonStyle {
    @State private var hovered = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(.primary.opacity(configuration.isPressed ? 0.11 : hovered ? 0.075 : 0.035),
                        in: RoundedRectangle(cornerRadius: 8))
            .onHover { hovered = $0 }
            .animation(.easeOut(duration: 0.12), value: hovered)
    }
}

private struct InspectorCloseButtonStyle: ButtonStyle {
    @State private var hovered = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(hovered ? Color.primary : Color.secondary)
            .background(.primary.opacity(configuration.isPressed ? 0.12 : hovered ? 0.075 : 0),
                        in: RoundedRectangle(cornerRadius: 8))
            .onHover { hovered = $0 }
            .animation(.easeOut(duration: 0.10), value: hovered)
    }
}

@main
struct CaskrunApp: App {
    @NSApplicationDelegateAdaptor(CaskrunAppDelegate.self) private var appDelegate
    @StateObject private var model: CaskrunModel
    @StateObject private var catalogArtwork = CatalogArtworkStore()
    @StateObject private var ps5: PS5CollectionModel
    @StateObject private var appUpdates: CaskrunAppUpdates

    init() {
        let model = CaskrunModel()
        let ps5 = PS5CollectionModel()
        _model = StateObject(wrappedValue: model)
        _ps5 = StateObject(wrappedValue: ps5)
        _appUpdates = StateObject(wrappedValue: CaskrunAppUpdates(prepareForQuit: {
            guard await model.shutdownForQuit() else { return false }
            guard await ps5.shutdownForQuit() else {
                model.resumeAfterQuitFailure(ps5.error ?? "A PS5 session could not be stopped.")
                return false
            }
            return true
        }, recoverAfterInstallationFailure: {
            model.resumeAfterUpdateFailure()
        }, reportMessage: { model.message = $0 }))
    }
    @State private var collectionSelectedID: String?
    @State private var collectionPlatform: CollectionPlatform = .all
    @State private var collectionKind: CollectionItemKind?
    @State private var showingPS5Import = false
    @State private var ps5Artwork: [String: NSImage] = [:]
    @State private var requestedPS5Artwork: Set<String> = []
    @State private var ps5ArtworkOrder: [String] = []
    @State private var ps5ArtworkTasks: [String: Task<Void, Never>] = [:]
    @State private var ps5ArtworkGenerations: [String: UUID] = [:]
    @State private var page: Page = .discover
    @State private var didChooseInitialPage = false
    @State private var showingNewBottle = false
    @State private var newBottleName = ""
    @State private var renameTarget: Bottle?
    @State private var deleteTarget: Bottle?
    @State private var purgeTarget: Bottle?
    @State private var quitTarget: Bottle?
    @State private var rebootTarget: Bottle?
    @State private var renameText = ""
    @State private var librarySearch = ""
    @State private var catalogSearch = ""
    @State private var discoverScope: DiscoverScope = .stores
    @State private var browserGameRequest: AddBrowserGameRequest?
    @State private var steamInstallRequest: SteamGameInstallRequest?
    @State private var selectedCatalogEntry: CatalogEntry?
    @State private var catalogBottleID = ""
    @State private var catalogUseExistingBottle = false
    @State private var pendingCatalogInstall: (bottleID: String?, appName: String)?
    @State private var pendingManualLauncherBottle: Bottle?
    @State private var logBottle: Bottle?
    @State private var runOptionsItem: SavedProgram?
    @State private var savedArgumentsItem: SavedProgram?
    @State private var renameLauncherItem: SavedProgram?
    @State private var inventoryBottle: Bottle?
    @State private var diagnosticBottle: Bottle?
    @State private var notesBottle: Bottle?
    @State private var hoveredPage: Page?
    @State private var hoveredInstall = false
    @State private var hoveredCatalogID: String?
    @State private var showingBottleInspector = false
    @State private var recentlyDeletedScrollRequest = UUID()
    @State private var scrollToRecentlyDeleted = false
    @State private var inspectingBottleOnly = false
    @State private var openBottleID: String?
    @State private var selectedSteamGameID: String?
    @State private var showingSearch = false
    @State private var selectedLauncherID: String?
    @State private var showBottleManagement = false
    @State private var showLaunchOptions = false
    @State private var showBottleDisplay = false
    @State private var showBottleTools = false
    @State private var showBottleMaintenance = false
    private enum SearchField: Hashable { case library, catalog }
    @FocusState private var focusedSearch: SearchField?

    private var filteredCatalog: [CatalogEntry] {
        let query = catalogSearch.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return CatalogSource.loaded.entries }
        return CatalogSource.loaded.entries.filter {
            $0.name.localizedCaseInsensitiveContains(query)
                || $0.detail.localizedCaseInsensitiveContains(query)
                || $0.category.localizedCaseInsensitiveContains(query)
        }
    }

    private struct CatalogGroup: Identifiable {
        let title: String
        let entries: [CatalogEntry]
        var id: String { title }
    }

    private var catalogGroups: [CatalogGroup] {
        let grouped = Dictionary(grouping: filteredCatalog, by: \.category)
        let preferred = ["Game stores", "Publisher launchers"]
        let categories = preferred + grouped.keys.filter { !preferred.contains($0) }.sorted()
        return categories.compactMap { category in
            guard let entries = grouped[category], !entries.isEmpty else { return nil }
            return CatalogGroup(title: category, entries: entries)
        }
    }

    private var allSavedPrograms: [SavedProgram] {
        model.bottles.flatMap { bottle in
            (model.launchers[bottle.id] ?? []).map { SavedProgram(bottle: bottle, launcher: $0) }
        }
        .sorted { $0.launcher.created_at > $1.launcher.created_at }
    }

    private var selectedBottle: Bottle? {
        model.bottles.first { $0.id == model.selectedBottleID }
    }

    private var inspectedProgram: SavedProgram? {
        guard let bottle = selectedBottle else { return nil }
        let apps = model.launchers[bottle.id] ?? []
        guard let launcher = apps.first(where: { $0.id == selectedLauncherID })
            ?? apps.first(where: { !$0.hidden_from_home }) ?? apps.first else { return nil }
        return SavedProgram(bottle: bottle, launcher: launcher)
    }

    private var openBottle: Bottle? { model.bottles.first { $0.id == openBottleID } }

    private var selectedSteamGame: SteamGame? {
        guard let bottle = openBottle else { return nil }
        return model.steamGames[bottle.id]?.first { $0.id == selectedSteamGameID }
    }

    private func inspectBottle(_ bottle: Bottle, launcher: SavedLauncher? = nil) {
        inspectingBottleOnly = launcher == nil
        collectionSelectedID = launcher.map { "app:\(bottle.id):\($0.id)" }
        model.selectedBottleID = bottle.id
        openBottleID = bottle.id
        selectedLauncherID = launcher?.id ?? primaryLauncher(in: bottle)?.id
        selectedSteamGameID = nil
        page = .collection
        showingBottleInspector = true
    }

    private func openBottleManager() {
        collectionSelectedID = nil
        inspectingBottleOnly = true
        if let bottle = selectedBottle ?? model.bottles.first { inspectBottle(bottle) }
        page = .collection
        showingBottleInspector = true
        showBottleManagement = true
    }

    private func openRecentlyDeleted() {
        openBottleManager()
        scrollToRecentlyDeleted = true
        recentlyDeletedScrollRequest = UUID()
    }

    private func installedProgram(for app: CatalogEntry) -> SavedProgram? {
        if let attempt = model.catalogAttempts.first(where: {
            $0.catalog_id == app.id && $0.stage == .installed
        }), let bottle = model.bottles.first(where: { $0.id == attempt.bottle_id }),
           let launcher = model.launchers[bottle.id]?.first(where: { candidate in
               switch app.id {
               case "steam": return candidate.name.localizedCaseInsensitiveCompare("Steam") == .orderedSame
               case "epic-games-store": return candidate.name.localizedCaseInsensitiveCompare("Epic Games") == .orderedSame
               case "gog-galaxy": return candidate.name.localizedCaseInsensitiveCompare("GOG Galaxy") == .orderedSame
               case "battle-net": return candidate.name.localizedCaseInsensitiveCompare("Battle.net") == .orderedSame
               case "ubisoft-connect": return candidate.name.localizedCaseInsensitiveCompare("Ubisoft Connect") == .orderedSame
               case "ea-app": return candidate.name.localizedCaseInsensitiveCompare("EA app") == .orderedSame
               case "itch-io-app": return candidate.name.localizedCaseInsensitiveCompare("itch.io app") == .orderedSame
               case "rockstar-games-launcher": return candidate.name.localizedCaseInsensitiveCompare("Rockstar Games Launcher") == .orderedSame
               default: return false
               }
           }) {
            return SavedProgram(bottle: bottle, launcher: launcher)
        }
        return allSavedPrograms.first {
            let path = $0.launcher.path.lowercased()
            guard let drive = path.range(of: "/prefix/drive_c/") else { return false }
            let program = path[drive.upperBound...]
            switch app.id {
            case "steam":
                return program == "program files (x86)/steam/steam.exe"
                    || program == "program files/steam/steam.exe"
            case "epic-games-store":
                return (program.hasPrefix("program files (x86)/epic games/launcher/")
                    || program.hasPrefix("program files/epic games/launcher/"))
                    && program.hasSuffix("/epicgameslauncher.exe")
            case "gog-galaxy":
                return program == "program files (x86)/gog galaxy/galaxyclient.exe"
                    || program == "program files/gog galaxy/galaxyclient.exe"
            case "battle-net":
                return ["program files (x86)/battle.net/", "program files/battle.net/"].contains {
                    program == "\($0)battle.net launcher.exe" || program == "\($0)battle.net.exe"
                }
            case "ubisoft-connect":
                return ["program files (x86)/ubisoft/ubisoft game launcher/", "program files/ubisoft/ubisoft game launcher/"].contains {
                    program == "\($0)ubisoftconnect.exe" || program == "\($0)uplay.exe"
                }
            case "ea-app":
                return ["program files/electronic arts/ea desktop/ea desktop/eadesktop.exe",
                        "program files (x86)/electronic arts/ea desktop/ea desktop/eadesktop.exe"].contains(String(program))
            case "itch-io-app":
                return program.hasPrefix("users/") && program.hasSuffix("/appdata/local/itch/itch-setup.exe")
            case "rockstar-games-launcher":
                return ["program files/rockstar games/launcher/launcher.exe",
                        "program files (x86)/rockstar games/launcher/launcher.exe"].contains(String(program))
            default:
                return false
            }
        }
    }

    private var pendingCatalogAttempts: [CatalogAttempt] {
        model.catalogAttempts.filter { $0.stage != .installed }
            .sorted { $0.updated_at > $1.updated_at }
    }

    private var homeRecoveryAttempts: [CatalogAttempt] {
        pendingCatalogAttempts.filter { $0.bottle_id != model.catalogSetupFailure?.bottleID }
    }

    private func pendingAttempt(for app: CatalogEntry) -> CatalogAttempt? {
        pendingCatalogAttempts.first { $0.catalog_id == app.id }
    }

    private func catalogName(for attempt: CatalogAttempt) -> String {
        CatalogSource.loaded.entries.first { $0.id == attempt.catalog_id }?.name ?? "Windows app"
    }

    private func attempt(for failure: CatalogSetupFailure) -> CatalogAttempt? {
        guard let bottleID = failure.bottleID else { return nil }
        return model.catalogAttempts.first {
            $0.catalog_id == failure.entryID && $0.bottle_id == bottleID
        }
    }

    private func retry(_ failure: CatalogSetupFailure) {
        if let attempt = attempt(for: failure), attempt.stage != .installed {
            model.catalogSetupFailure = nil
            model.retryCatalog(attempt)
        } else if failure.bottleID != nil && attempt(for: failure) == nil {
            model.message = "The saved setup record could not be loaded. Select its bottle on Collection to inspect this install."
        } else if let entry = CatalogSource.loaded.entries.first(where: { $0.id == failure.entryID }) {
            model.catalogSetupFailure = nil
            if let installed = installedProgram(for: entry) {
                model.runLauncher(installed.launcher, in: installed.bottle)
            } else {
                model.installCatalog(entry)
            }
        }
    }

    private func failureSummary(_ failure: CatalogSetupFailure) -> String {
        guard failure.bottleID != nil else {
            return "Caskrun could not start setup. Try again to prepare the store."
        }
        if failure.detail.localizedCaseInsensitiveContains("still be running")
            || failure.detail.localizedCaseInsensitiveContains("Close Windows apps") {
            return "Windows apps may still be running in this bottle. Quit them from its Windows details panel, then retry setup."
        }
        let timedOut = failure.detail.localizedCaseInsensitiveContains("timed out")
            || failure.detail.localizedCaseInsensitiveContains("timeout")
        return timedOut
            ? "The installer did not finish in time. Retry in the saved bottle or inspect its logs."
            : "Setup stopped before the launcher was ready. Retry in the saved bottle or inspect its logs."
    }

    private func openLogs(for attempt: CatalogAttempt, name: String) {
        logBottle = model.bottles.first { $0.id == attempt.bottle_id }
            ?? Bottle(id: attempt.bottle_id, name: name, created_at: 0)
    }

    private func launcherSymbol(_ name: String) -> String {
        switch name.lowercased() {
        case "steam": "gamecontroller.fill"
        case "epic games": "sparkles.rectangle.stack"
        case "gog galaxy": "square.stack.3d.up.fill"
        case "battle.net": "circle.hexagongrid.fill"
        default: "app.fill"
        }
    }

    private func openOrSetup(_ app: CatalogEntry) {
        if let attempt = pendingAttempt(for: app) {
            model.retryCatalog(attempt)
            return
        }
        if app.installMode == "automatic", let installed = installedProgram(for: app) {
            model.runLauncher(installed.launcher, in: installed.bottle)
            return
        }
        catalogBottleID = model.selectedBottleID
        catalogUseExistingBottle = false
        selectedCatalogEntry = app
    }

    private var mainContent: some View {
        VStack(spacing: 0) {
            topBar
                .padding(.top, 12)
                .padding(.bottom, 2)
            operationPanel
            Group {
                switch page {
                case .collection: collectionView
                case .discover: discoverView
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(minWidth: 850, minHeight: 560)
        .disabled(model.isQuitting)
        .background(CaskrunWindowCapture().frame(width: 0, height: 0))
        .background(CaskrunStatusBridge(page: $page,
                                        didChooseInitialPage: $didChooseInitialPage,
                                        showingBottleInspector: $showingBottleInspector,
                                        openBottleID: $openBottleID,
                                        model: model, ps5: ps5, appUpdates: appUpdates,
                                        selectedCollectionID: $collectionSelectedID, inspectingBottleOnly: $inspectingBottleOnly, delegate: appDelegate))
        .onAppear {
            model.refresh()
            if !didChooseInitialPage {
                page = !model.bottles.isEmpty ||
                    !model.catalogAttempts.isEmpty || !model.browserGames.isEmpty || !ps5.games.isEmpty ? .collection : .discover
                didChooseInitialPage = true
            }
            if let url = Bundle.main.url(forResource: "caskrun-icon", withExtension: "png"),
               let icon = NSImage(contentsOf: url) {
                NSApplication.shared.applicationIconImage = icon
            }
        }
        .onChange(of: model.selectedBottleID) { previous, current in
            if !previous.isEmpty && !model.bottles.contains(where: { $0.id == previous }) {
                showingBottleInspector = false
                openBottleID = nil
                selectedSteamGameID = nil
                collectionSelectedID = nil
            }
            if !(model.launchers[current] ?? []).contains(where: { $0.id == selectedLauncherID }) {
                selectedLauncherID = nil
            }
            showBottleTools = false
            showBottleMaintenance = false
            showBottleDisplay = false
            showBottleManagement = false
            showLaunchOptions = false
        }
        .onChange(of: model.installHandoff?.launcher.id) { _, launcherID in
            if launcherID != nil, let handoff = model.installHandoff {
                inspectBottle(handoff.bottle, launcher: handoff.launcher)
            }
        }
        .onChange(of: model.importedBottleID) { _, bottleID in
            if let bottle = model.bottles.first(where: { $0.id == bottleID }) { inspectBottle(bottle) }
        }
        .onChange(of: collectionSelectedID) { _, selection in
            if selection == nil && !inspectingBottleOnly { closeCollectionInspector() }
        }
        .onChange(of: collectionPlatform) { _, _ in closeCollectionInspector() }
        .onChange(of: collectionKind) { _, _ in closeCollectionInspector() }
        .onChange(of: ps5.games.map(\.id)) { _, ids in
            let retained = Set(ids)
            ps5Artwork = ps5Artwork.filter { retained.contains($0.key) }
            requestedPS5Artwork.formIntersection(retained)
            ps5ArtworkOrder.removeAll { !retained.contains($0) }
            for (id, task) in ps5ArtworkTasks where !retained.contains(id) { task.cancel() }
            ps5ArtworkTasks = ps5ArtworkTasks.filter { retained.contains($0.key) }
            ps5ArtworkGenerations = ps5ArtworkGenerations.filter { retained.contains($0.key) }
        }
        .onChange(of: page) { _, current in
            if current != .collection {
                for task in ps5ArtworkTasks.values { task.cancel() }
                ps5ArtworkTasks.removeAll()
                ps5ArtworkGenerations.removeAll()
                requestedPS5Artwork = Set(ps5Artwork.keys)
            }
        }
        .task(id: page) {
            guard page == .collection else { return }
            repeat {
                await model.refreshGameLibrary()
                do { try await Task.sleep(for: .seconds(15)) }
                catch { return }
            } while !Task.isCancelled
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            if page == .collection {
                ps5.refresh()
                Task { await model.refreshGameLibrary() }
            }
        }
    }

    var body: some Scene {
        Window("Caskrun", id: "main") {
            mainContent
            .alert("New bottle", isPresented: $showingNewBottle) {
                TextField("Bottle name", text: $newBottleName)
                Button("Create") {
                    if let bottle = model.createBottle(newBottleName) { inspectBottle(bottle) }
                    newBottleName = ""
                }
                Button("Cancel", role: .cancel) { newBottleName = "" }
            } message: {
                Text("Each bottle keeps its Windows files and settings separate.")
            }
            .alert("Rename bottle", isPresented: Binding(
                get: { renameTarget != nil },
                set: { if !$0 { renameTarget = nil } }
            )) {
                TextField("Bottle name", text: $renameText)
                Button("Rename") {
                    if let bottle = renameTarget { model.renameBottle(bottle, to: renameText) }
                    renameTarget = nil
                }
                Button("Cancel", role: .cancel) { renameTarget = nil }
            }
            .alert("Move bottle to Recently Deleted?", isPresented: Binding(
                get: { deleteTarget != nil },
                set: { if !$0 { deleteTarget = nil } }
            )) {
                Button("Move Bottle", role: .destructive) {
                    if let bottle = deleteTarget { model.deleteBottle(bottle) }
                    deleteTarget = nil
                }
                Button("Cancel", role: .cancel) { deleteTarget = nil }
            } message: {
                Text("The bottle and its Windows files will leave the active list. You can restore it from Recently Deleted.")
            }
            .alert("Permanently delete \(purgeTarget?.name ?? "this bottle")?", isPresented: Binding(
                get: { purgeTarget != nil },
                set: { if !$0 { purgeTarget = nil } }
            )) {
                Button("Delete Permanently", role: .destructive) {
                    if let bottle = purgeTarget { model.permanentlyDeleteBottle(bottle) }
                    purgeTarget = nil
                }
                Button("Cancel", role: .cancel) { purgeTarget = nil }
            } message: {
                Text("All apps, games and files in this deleted bottle will be erased. This cannot be undone.")
            }
            .alert("Force quit apps in this bottle?", isPresented: Binding(
                get: { quitTarget != nil },
                set: { if !$0 { quitTarget = nil } }
            )) {
                Button("Force Quit", role: .destructive) {
                    if let bottle = quitTarget { model.forceQuitApps(in: bottle) }
                    quitTarget = nil
                }
                Button("Cancel", role: .cancel) { quitTarget = nil }
            } message: {
                Text("All Windows apps in \(quitTarget?.name ?? "this bottle") will stop immediately. Unsaved work may be lost.")
            }
            .alert("Simulate a Windows reboot?", isPresented: Binding(
                get: { rebootTarget != nil },
                set: { if !$0 { rebootTarget = nil } }
            )) {
                Button("Reboot Bottle") {
                    if let bottle = rebootTarget { model.simulateReboot(in: bottle) }
                    rebootTarget = nil
                }
                Button("Cancel", role: .cancel) { rebootTarget = nil }
            } message: {
                Text("Windows apps in \(rebootTarget?.name ?? "this bottle") will close. Save your work in them first.")
            }
            .alert("Caskrun", isPresented: Binding(
                get: { model.message != nil },
                set: { if !$0 { model.message = nil } }
            )) {
                Button("OK") { model.message = nil }
            } message: {
                Text(model.message ?? "")
            }
            .sheet(item: $selectedCatalogEntry, onDismiss: {
                if let request = pendingCatalogInstall {
                    pendingCatalogInstall = nil
                    model.chooseInstaller(in: request.bottleID, for: request.appName)
                }
            }) { entry in
                catalogSetupView(for: entry)
            }
            .sheet(item: $model.manualInstallReview, onDismiss: {
                if let bottle = pendingManualLauncherBottle {
                    pendingManualLauncherBottle = nil
                    model.addLauncher(to: bottle)
                }
            }) { review in
                manualInstallReviewView(review)
            }
            .sheet(item: $logBottle) { bottle in
                BottleLogsView(model: model, bottle: bottle)
            }
            .sheet(item: $runOptionsItem) { item in
                RunWithOptionsSheet(item: item) { arguments in
                    model.runLauncher(item.launcher, in: item.bottle, arguments: arguments)
                }
            }
            .sheet(item: $savedArgumentsItem) { item in
                SavedLaunchArgumentsSheet(item: item) { arguments in
                    model.setLauncherArguments(item.launcher, in: item.bottle, arguments: arguments)
                }
            }
            .sheet(item: $renameLauncherItem) { item in
                RenameLauncherSheet(launcher: item.launcher, model: model) { name in
                    model.renameLauncher(item.launcher, in: item.bottle, to: name)
                }
            }
            .sheet(item: $model.pendingRun) { program in
                OneTimeRunSheet(program: program, model: model)
            }
            .sheet(item: $inventoryBottle) { bottle in
                InstalledProgramsView(model: model, bottle: bottle)
            }
            .sheet(item: $diagnosticBottle) { bottle in
                BottleDiagnosticsView(model: model, bottle: bottle)
            }
            .sheet(item: $notesBottle) { bottle in
                BottleNotesView(model: model, bottle: bottle)
            }
            .sheet(isPresented: $showingPS5Import) {
                ImportPS5GameSheet(model: ps5) { game in
                    selectPS5Game(game)
                }
            }
            .sheet(item: $browserGameRequest) { request in
                AddBrowserGameSheet(initialName: request.name, initialURL: request.url) { name, url in
                    try await model.addBrowserGame(name: name, url: url)
                }
            }
            .sheet(item: $steamInstallRequest) { request in
                SteamGameInstallSheet(request: request, bottles: steamBottles, isBusy: model.isQuitting || model.isLaunching || model.isManagingBottle || model.status.wine_path == nil) { appID, bottleID in
                    guard !model.isQuitting, !model.isLaunching, !model.isManagingBottle, model.status.wine_path != nil else {
                        throw NSError(domain: "Caskrun", code: 1, userInfo: [NSLocalizedDescriptionKey: "Wait for the current operation to finish and check that the Windows engine is available."])
                    }
                    if let bottleID {
                        guard let bottle = steamBottles.first(where: { $0.id == bottleID }) else {
                            throw NSError(domain: "Caskrun", code: 1, userInfo: [NSLocalizedDescriptionKey: "That Steam bottle is no longer available. Choose another bottle."])
                        }
                        model.requestSteamInstall(appID: appID, in: bottle)
                    } else if let steam = CatalogSource.loaded.entries.first(where: { $0.id == "steam" }) {
                        model.installCatalog(steam, thenInstallSteamAppID: appID)
                    } else {
                        throw NSError(domain: "Caskrun", code: 1, userInfo: [NSLocalizedDescriptionKey: "The Steam setup entry is missing. Rebuild Caskrun to restore it."])
                    }
                }
            }
        }
        .defaultSize(width: 1000, height: 680)
        .commands {
            CommandGroup(after: .appInfo) {
                Button("Check for Updates…") { appUpdates.checkForUpdates() }
                    .disabled(!appUpdates.canCheckForUpdates || model.isQuitting || model.isRequestingMicrophoneAccess)
                Button("Microphone Access…") { model.requestMicrophoneAccess() }
                    .disabled(model.isQuitting || model.isRequestingMicrophoneAccess)
            }
            CommandGroup(replacing: .appSettings) {}
            CommandGroup(replacing: .newItem) {
                Button("Install from file…") { model.chooseInstaller(in: page == .collection ? openBottleID : nil) }
                    .keyboardShortcut("n", modifiers: .command)
                    .disabled(model.isQuitting || model.status.wine_path == nil || model.isLaunching || model.isManagingBottle)
            }
            CommandGroup(after: .textEditing) {
                Button("Find Apps") {
                    showingSearch = true
                    focusedSearch = page == .discover ? .catalog : .library
                }
                .keyboardShortcut("f", modifiers: .command)
                .disabled(model.isQuitting)
                Button("Manage Bottles") { openBottleManager() }
                    .keyboardShortcut("3", modifiers: .command)
                    .disabled(model.isQuitting)
                Button("Open Selected Item") {
                    if let item = selectedCollectionItem { openCollectionItem(item) }
                }
                .keyboardShortcut("o", modifiers: .command)
                .disabled(model.isQuitting || page != .collection || selectedCollectionItem?.canOpen != true)
            }
        }
    }

    private func navigate(to target: Page) {
        librarySearch = ""
        catalogSearch = ""
        showingSearch = false
        focusedSearch = nil
        if target == .collection {
            inspectingBottleOnly = false
            collectionSelectedID = nil
            openBottleID = nil
            selectedSteamGameID = nil
            showingBottleInspector = false
        }
        page = target
    }

    private var topBar: some View {
        HStack(spacing: 4) {
            navigationButton(.collection, title: "Collection", symbol: "square.grid.2x2", shortcut: "1")
            navigationButton(.discover, title: "Discover", symbol: "sparkle.magnifyingglass", shortcut: "2")
            Rectangle().fill(.primary.opacity(0.12))
                .frame(width: 1, height: 18).padding(.horizontal, 5)
            Menu {
                Button("Discover apps") { navigate(to: .discover) }
                Button("Import PS5 game…", action: beginPS5Import)
                    .disabled(ps5.busy)
                Button("Add browser game…") { browserGameRequest = AddBrowserGameRequest() }
                Button("Install Steam game…") { steamInstallRequest = SteamGameInstallRequest(appID: nil, name: nil) }
                    .disabled(model.status.wine_path == nil || model.isLaunching || model.isManagingBottle)
                Button("Install from file…") { model.chooseInstaller(in: page == .collection ? openBottleID : nil) }
                    .disabled(model.status.wine_path == nil || model.isLaunching || model.isManagingBottle)
                Divider()
                Button("New bottle…") { showingNewBottle = true }
                    .disabled(model.isLaunching || model.isManagingBottle)
                Button("Import Caskrun backup…") { model.importBottle() }
                    .disabled(model.isLaunching || model.isManagingBottle)
                Button("Import from CrossOver…") { model.importCrossOverBottle() }
                    .disabled(model.isLaunching || model.isManagingBottle)
                Divider()
                Button("Manage bottles…") { openBottleManager() }
                if !model.deletedBottles.isEmpty {
                    Button("Recently Deleted…") { openRecentlyDeleted() }
                }
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(.primary)
            }
            .menuStyle(.borderlessButton).menuIndicator(.hidden)
            .frame(width: 34, height: 34)
            .background(.primary.opacity(hoveredInstall ? 0.13 : 0.065), in: Circle())
            .contentShape(Circle())
            .help("Add games, apps or import a bottle")
            .accessibilityLabel("Add games, apps or import a bottle")
            .onHover { hoveredInstall = $0 }
            .animation(.easeOut(duration: 0.12), value: hoveredInstall)
            if showingSearch {
                searchField(page == .discover ? "Search Discover" : "Search Collection",
                            text: page == .discover ? $catalogSearch : $librarySearch,
                            field: page == .discover ? .catalog : .library)
                    .frame(width: 210)
                Button {
                    librarySearch = ""
                    catalogSearch = ""
                    showingSearch = false
                    focusedSearch = nil
                } label: {
                    Image(systemName: "xmark").font(.system(size: 10, weight: .semibold))
                        .frame(width: 28, height: 28).contentShape(Rectangle())
                }
                .buttonStyle(.plain).accessibilityLabel("Close search")
            }
        }
        .padding(6).fixedSize()
        .background(.regularMaterial, in: Capsule())
        .overlay { Capsule().strokeBorder(.primary.opacity(0.12), lineWidth: 1) }
        .shadow(color: .black.opacity(0.14), radius: 12, y: 5)
        .frame(maxWidth: .infinity)
    }

    private func navigationButton(_ target: Page, title: String, symbol: String, shortcut: KeyEquivalent) -> some View {
        let selected = page == target
        return Button { navigate(to: target) } label: {
            Label(title, systemImage: symbol)
                .font(.system(size: 12, weight: selected ? .semibold : .medium))
                .padding(.horizontal, 12)
                .frame(height: 34)
                .foregroundStyle(selected ? Color.primary : Color.secondary)
                .background(selected ? Color.primary.opacity(0.10) :
                    (hoveredPage == target ? Color.primary.opacity(0.055) : .clear), in: Capsule())
        }
        .buttonStyle(.plain)
        .keyboardShortcut(shortcut, modifiers: .command)
        .onHover { hoveredPage = $0 ? target : nil }
        .animation(.easeOut(duration: 0.12), value: hoveredPage)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private func closeCollectionInspector() {
        inspectingBottleOnly = false
        collectionSelectedID = nil
        openBottleID = nil
        selectedSteamGameID = nil
        selectedLauncherID = nil
        showingBottleInspector = false
    }

    private var collectionView: some View {
        GeometryReader { viewport in
            HStack(alignment: .top, spacing: 0) {
                VStack(spacing: 0) {
                    VStack(alignment: .leading, spacing: 12) {
                        if collectionPlatform == .all || collectionPlatform == .windows {
                            runtimeBanner
                            recoveryPanel
                        }
                        if !collectionWarnings.isEmpty {
                            DisclosureGroup("Some collection items could not be read") {
                                ForEach(Array(collectionWarnings.enumerated()), id: \.offset) { _, warning in
                                    Text(warning).font(.caption).foregroundStyle(.secondary)
                                }
                            }.font(.caption)
                        }
                        if let error = ps5.error {
                            Label(error, systemImage: "exclamationmark.circle")
                                .font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                        }
                    }.padding(.horizontal, 24)
                    CollectionGrid(entries: collectionItems, folders: collectionFolders, openFolderID: $openBottleID,
                                   selectedID: $collectionSelectedID,
                                   platform: $collectionPlatform, kind: $collectionKind,
                                   query: librarySearch, isLoading: model.loadingGameLibrary,
                                   onSelect: selectCollectionItem, onOpen: openCollectionItem,
                                   onLoadArtwork: loadCollectionArtwork,
                                   onSelectFolder: selectCollectionFolder, onOpenFolder: openCollectionFolder,
                                   onLoadFolderArtwork: { folder in
                                       if let bottle = model.bottles.first(where: { $0.id == folder.id }) {
                                           model.loadBottleIcon(bottle)
                                           if let primary = primaryLauncher(in: bottle) { model.loadLauncherIcon(primary, in: bottle) }
                                       }
                                   }, onBack: closeCollectionInspector,
                                   onDiscover: {
                                       discoverScope = collectionPlatform == .browser ? .browser : .stores
                                       navigate(to: .discover)
                                   }, onImportPS5: beginPS5Import)
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
                if let game = selectedPS5Game {
                    PS5GameInspector(model: ps5, game: game, onClose: closeCollectionInspector)
                        .frame(height: max(0, viewport.size.height - 36))
                        .padding(.top, 20).padding(.trailing, 16).padding(.bottom, 16)
                        .transition(.identity)
                } else if let game = selectedBrowserGame, let item = selectedCollectionItem {
                    browserGameInspector(game, item: item)
                        .frame(height: max(0, viewport.size.height - 36))
                        .padding(.top, 20).padding(.trailing, 16).padding(.bottom, 16)
                        .transition(.identity)
                } else if showingBottleInspector {
                    bottleInspector.frame(width: 320, height: max(0, viewport.size.height - 36))
                        .padding(.top, 20).padding(.trailing, 16).padding(.bottom, 16)
                        .transition(.identity)
                }
            }
            .frame(width: viewport.size.width, height: viewport.size.height)
        }
    }

    private var collectionWarnings: [String] {
        switch collectionPlatform {
        case .all, .windows: gameLibraryWarnings
        case .browser: model.browserGamesError.map { [$0] } ?? []
        case .ps5: []
        }
    }

    private var selectedCollectionItem: CollectionItem? {
        collectionItems.first { $0.id == collectionSelectedID }
    }

    private var selectedPS5Game: PS5CollectionGame? {
        ps5.games.first { "ps5:\($0.id)" == collectionSelectedID }
    }

    private var selectedBrowserGame: BrowserGame? {
        model.browserGames.first { "browser:\($0.id)" == collectionSelectedID }
    }

    private var collectionItems: [CollectionItem] {
        var items = gameLibraryEntries.map { entry in
            CollectionItem(id: entry.id, name: entry.name,
                           platform: entry.source == .browser ? .browser : .windows,
                           kind: entry.source == .apps ? .app : .game,
                           subtitle: entry.subtitle, icon: entry.icon, browserURL: entry.browserURL,
                           artworkRevision: entry.artworkRevision, canOpen: entry.canPlay,
                           openActionTitle: entry.source == .epic || entry.source == .gog ? "Open store" : nil,
                           bottleID: collectionBottleID(for: entry))
        }
        items += ps5.games.map { game in
            CollectionItem(id: "ps5:\(game.id)", name: game.name, platform: .ps5, kind: .game,
                           subtitle: "PS5 · Experimental", icon: ps5Artwork[game.id],
                           artworkRevision: game.path + String(game.createdAt),
                           canOpen: !model.isQuitting && !ps5.busy && ps5.engine != nil,
                           status: ps5.runningGameIDs.contains(game.id) ? "Running" : "",
                           openActionTitle: ps5.runningGameIDs.contains(game.id) ? "Show window" : "Open game")
        }
        return items.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    private func collectionBottleID(for entry: GameLibraryEntry) -> String? {
        guard entry.source != .browser else { return nil }
        return model.bottles.first { bottle in
            ["app:", "steam:", "publisher:"].contains { entry.id.hasPrefix($0 + bottle.id + ":") }
        }?.id
    }

    private func primaryLauncher(in bottle: Bottle) -> SavedLauncher? {
        let launchers = (model.launchers[bottle.id] ?? []).filter { !$0.hidden_from_home }
        // Steam owns games within a Steam bottle; importing it must not change
        // the folder's double-click into a random game's launch action.
        return launchers.first { URL(fileURLWithPath: $0.path).lastPathComponent.lowercased() == "steam.exe" }
            ?? launchers.first
    }

    private var collectionFolders: [CollectionFolder] {
        let canLaunch = !model.isQuitting && !model.isLaunching && !model.isManagingBottle && model.status.wine_path != nil
        return model.bottles.map { bottle in
            let primary = primaryLauncher(in: bottle)
            let icon = primary.flatMap { model.launcherIcons["\(bottle.id):\($0.id)"] }
                ?? model.bottleIcons[bottle.id]
            return CollectionFolder(id: bottle.id, name: bottle.name, icon: icon,
                                    canOpen: canLaunch && primary != nil, primaryAppName: primary?.name)
        }.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    private func selectCollectionFolder(_ folder: CollectionFolder) {
        guard !model.isQuitting, let bottle = model.bottles.first(where: { $0.id == folder.id }) else { return }
        inspectBottle(bottle)
    }

    private func openCollectionFolder(_ folder: CollectionFolder) {
        guard !model.isQuitting, folder.canOpen,
              let bottle = model.bottles.first(where: { $0.id == folder.id }),
              let launcher = primaryLauncher(in: bottle) else { return }
        model.runLauncher(launcher, in: bottle)
    }

    private func beginPS5Import() {
        closeCollectionInspector()
        page = .collection
        collectionPlatform = .ps5
        collectionKind = nil
        // Let filter changes settle while the user chooses their input. The
        // success callback only selects; it never races a filter reset.
        showingPS5Import = true
    }

    private func selectPS5Game(_ game: PS5CollectionGame) {
        inspectingBottleOnly = false
        guard ps5.games.contains(where: { $0.id == game.id }) else { return }
        openBottleID = nil
        showingBottleInspector = false
        selectedSteamGameID = nil
        collectionSelectedID = "ps5:\(game.id)"
    }

    private func selectCollectionItem(_ item: CollectionItem) {
        guard !model.isQuitting else { return }
        if let game = ps5.games.first(where: { "ps5:\($0.id)" == item.id }) {
            selectPS5Game(game); return
        }
        if item.platform == .browser {
            inspectingBottleOnly = false
            openBottleID = nil
            selectedSteamGameID = nil
            showingBottleInspector = false
            collectionSelectedID = item.id
            return
        }
        for bottle in model.bottles {
            if let game = model.steamGames[bottle.id]?.first(where: { "steam:\(bottle.id):\($0.app_id)" == item.id }) {
                inspectBottle(bottle)
                selectedSteamGameID = game.id
                inspectingBottleOnly = false
                collectionSelectedID = item.id
                return
            }
            if let game = model.publisherGames[bottle.id]?.first(where: { "publisher:\(bottle.id):\($0.id)" == item.id }) {
                inspectBottle(bottle, launcher: publisherClient(for: game, in: bottle))
                inspectingBottleOnly = false
                collectionSelectedID = item.id
                return
            }
            if let launcher = model.launchers[bottle.id]?.first(where: { "app:\(bottle.id):\($0.id)" == item.id }) {
                inspectBottle(bottle, launcher: launcher)
                inspectingBottleOnly = false
                collectionSelectedID = item.id
                return
            }
        }
    }

    private func openCollectionItem(_ item: CollectionItem) {
        guard !model.isQuitting,
              let current = collectionItems.first(where: { $0.id == item.id }), current.canOpen else { return }
        if let game = ps5.games.first(where: { "ps5:\($0.id)" == item.id }) {
            do {
                if ps5.runningGameIDs.contains(game.id) {
                    if !ps5.focus(game) { throw PS5CollectionError.invalid("The engine has no visible window yet.") }
                } else { try ps5.launch(game) }
            } catch { model.message = error.localizedDescription }
            return
        }
        if let entry = gameLibraryEntries.first(where: { $0.id == item.id }) { playLibraryEntry(entry) }
    }

    private func loadCollectionArtwork(_ item: CollectionItem) {
        if let game = ps5.games.first(where: { "ps5:\($0.id)" == item.id }) {
            guard requestedPS5Artwork.insert(game.id).inserted else { return }
            let generation = UUID()
            ps5ArtworkGenerations[game.id] = generation
            ps5ArtworkTasks[game.id] = Task {
                defer {
                    if ps5ArtworkGenerations[game.id] == generation {
                        ps5ArtworkTasks[game.id] = nil
                        ps5ArtworkGenerations[game.id] = nil
                    }
                }
                let image = await ps5.gameArtwork(game)
                guard !Task.isCancelled, ps5.games.contains(game) else { return }
                if let image {
                    // At most 40 decoded 512px pictures stay in the grid cache.
                    while ps5ArtworkOrder.count >= 40 {
                        let oldest = ps5ArtworkOrder.removeFirst()
                        ps5Artwork[oldest] = nil
                        requestedPS5Artwork.remove(oldest)
                    }
                    ps5Artwork[game.id] = image
                    ps5ArtworkOrder.append(game.id)
                }
            }
        } else if let entry = gameLibraryEntries.first(where: { $0.id == item.id }) { loadLibraryArtwork(entry) }
    }

    private func browserGameInspector(_ game: BrowserGame, item: CollectionItem) -> some View {
        VStack(spacing: 0) {
            HStack {
                Text("BROWSER GAME").font(.system(size: 10, weight: .semibold)).foregroundStyle(.secondary)
                Spacer()
                Button(action: closeCollectionInspector) { Image(systemName: "xmark").frame(width: 28, height: 28) }
                    .buttonStyle(InspectorCloseButtonStyle()).keyboardShortcut(.escape, modifiers: [])
                    .accessibilityLabel("Close game details")
            }
            .padding(.horizontal, 18).padding(.top, 12).padding(.bottom, 8)
            Divider().padding(.horizontal, 18)
            ScrollView {
                BrowserGameInspectorContent(game: game, isQuitting: model.isQuitting,
                                            isEditing: model.editingBrowserGames,
                                            onPlay: { model.openBrowserGame(game) },
                                            onRemove: { model.removeBrowserGame(game) })
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(18)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(width: 320).frame(maxHeight: .infinity)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20))
        .clipShape(RoundedRectangle(cornerRadius: 20))
        .overlay {
            RoundedRectangle(cornerRadius: 20).strokeBorder(.primary.opacity(0.10))
                .allowsHitTesting(false)
        }
    }

    @ViewBuilder
    private var recoveryPanel: some View {
        if let error = model.catalogAttemptLoadError {
            Label("Setup history could not be read: \(error)", systemImage: "exclamationmark.triangle")
                .font(.caption)
                .foregroundStyle(.orange)
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.orange.opacity(0.10), in: RoundedRectangle(cornerRadius: 12))
        }
        if !homeRecoveryAttempts.isEmpty {
            VStack(alignment: .leading, spacing: 12) {
                Text("App setup")
                    .font(.title3.weight(.semibold))
                ForEach(homeRecoveryAttempts) { attempt in
                    VStack(alignment: .leading, spacing: 12) {
                        HStack(alignment: .top, spacing: 12) {
                            Image(systemName: attempt.stage == .failed
                                  ? "exclamationmark.circle.fill" : "arrow.clockwise.circle.fill")
                                .font(.title2)
                                .foregroundStyle(.orange)
                            VStack(alignment: .leading, spacing: 4) {
                                Text(catalogName(for: attempt)).font(.headline)
                                Text(model.isLaunching && model.activeCatalogBottleID == attempt.bottle_id
                                     ? (model.isCancellingCatalogInstall
                                        ? "Stopping setup. Caskrun will wait for the installer to exit."
                                        : "Setup is running. Caskrun will update this view when the installer responds.")
                                     : attempt.stage == .failed
                                     ? "Setup stopped before the launcher was ready. Retry in this bottle or inspect its logs."
                                     : "Setup was interrupted. Continue in this bottle.")
                                    .font(.subheadline)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer(minLength: 0)
                        }
                        HStack(spacing: 12) {
                            if attempt.last_log_name != nil {
                                Button("View logs") {
                                    openLogs(for: attempt, name: catalogName(for: attempt))
                                }
                            }
                            Button(attempt.stage == .failed ? "Retry setup" : "Continue setup") {
                                model.retryCatalog(attempt)
                            }
                            .buttonStyle(.borderedProminent)
                            .disabled(model.isLaunching || model.isManagingBottle || model.status.wine_path == nil)
                        }
                        if let error = attempt.last_error, !error.isEmpty {
                            DisclosureGroup("Technical details") {
                                Text(error)
                                    .font(.caption.monospaced())
                                    .foregroundStyle(.secondary)
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            .font(.caption)
                        }
                    }
                    .padding(18)
                    .background(.quaternary.opacity(0.45), in: RoundedRectangle(cornerRadius: 12))
                }
            }
        }
    }

    private var steamBottles: [Bottle] {
        model.bottles.filter { bottle in model.savedSteamClients.contains { $0.bottle_id == bottle.id } }
    }

    private var gameLibraryEntries: [GameLibraryEntry] {
        let canLaunch = !model.isQuitting && !model.isLaunching && !model.isManagingBottle && model.status.wine_path != nil
        var entries: [GameLibraryEntry] = []
        for bottle in model.bottles {
            for game in model.steamGames[bottle.id] ?? [] where game.app_id != 228980 {
                let compatibility = GameCompatibilityCatalog.shared.game(appID: game.app_id)
                entries.append(GameLibraryEntry(id: "steam:\(bottle.id):\(game.app_id)", name: game.name, source: .steam,
                    subtitle: "Steam · \(bottle.name)", compatibilityText: compatibility?.summary ?? "Not tested in Caskrun",
                    compatibility: compatibility, icon: model.gameIcons["\(bottle.id):\(game.id)"],
                    artworkRevision: model.gameArtworkRevision(game, in: bottle),
                    canPlay: canLaunch && model.savedSteamClients.contains(where: { $0.bottle_id == bottle.id }), canManageBottle: true,
                    availableRenderers: model.bottleGraphics[bottle.id].map { $0.dxmt_available ? Set(["wined3d", "dxmt"]) : Set(["wined3d"]) },
                    graphicsDescription: model.bottleGraphics[bottle.id].map { "Bottle graphics: " + ($0.effective_backend == "dxmt" ? "DXMT" : "WineD3D") }))
            }
            for game in model.publisherGames[bottle.id] ?? [] {
                let scope: LibraryScope = game.store == "epic" ? .epic : .gog
                entries.append(GameLibraryEntry(id: "publisher:\(bottle.id):\(game.id)", name: game.name, source: scope,
                    subtitle: "\(scope.title) · \(bottle.name)", compatibilityText: "Not tested in Caskrun",
                    compatibilityDetail: "This title was found in the store's local installation manifest. Compatibility reports need an exact game identifier; a matching title alone is not evidence.",
                    canPlay: canLaunch && publisherClient(for: game, in: bottle) != nil, canManageBottle: true))
            }
            for launcher in model.launchers[bottle.id] ?? [] where !launcher.hidden_from_home {
                entries.append(GameLibraryEntry(id: "app:\(bottle.id):\(launcher.id)", name: launcher.name, source: .apps,
                    subtitle: "Windows app · \(bottle.name)", compatibilityText: "Saved app",
                    compatibilityDetail: "This is a saved Windows app shortcut. A launcher opening does not establish game compatibility.",
                    icon: model.launcherIcons["\(bottle.id):\(launcher.id)"], canPlay: canLaunch, canManageBottle: true))
            }
        }
        for game in model.browserGames {
            entries.append(GameLibraryEntry(id: "browser:\(game.id)", name: game.name, source: .browser,
                subtitle: BrowserGame.checkedURL(game.url)?.host ?? "Browser game", compatibilityText: "Opens in browser",
                browserURL: game.url, canPlay: !model.isQuitting))
        }
        return entries.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    private var gameLibraryWarnings: [String] {
        var warnings = model.bottles.compactMap { bottle in model.steamGameErrors[bottle.id].map { "\(bottle.name): \($0)" } }
        warnings += model.bottles.compactMap { bottle in model.publisherGameErrors[bottle.id].map { "\(bottle.name): \($0)" } }
        if !model.truncatedPublisherLibraries.isEmpty { warnings.append("Some Epic or GOG manifests were skipped or exceeded the local scan limit.") }
        if model.bottles.count > 128 { warnings.append("Only the first 128 bottles were scanned. Use Manage bottles to select another bottle and read its library.") }
        if !model.truncatedSteamLibraries.isEmpty { warnings.append("Some Steam manifests could not be read completely.") }
        if let error = model.browserGamesError { warnings.append(error) }
        return warnings
    }

    private func loadLibraryArtwork(_ entry: GameLibraryEntry) {
        for bottle in model.bottles {
            if let game = model.steamGames[bottle.id]?.first(where: { "steam:\(bottle.id):\($0.app_id)" == entry.id }) {
                model.loadGameIcon(game, in: bottle); return
            }
            if let launcher = model.launchers[bottle.id]?.first(where: { "app:\(bottle.id):\($0.id)" == entry.id }) {
                model.loadLauncherIcon(launcher, in: bottle); return
            }
        }
    }

    private func publisherClient(for game: PublisherGame, in bottle: Bottle) -> SavedLauncher? {
        model.launchers[bottle.id]?.first { launcher in
            guard launcher.saved_arguments?.isEmpty ?? true else { return false }
            let path = launcher.path.lowercased()
            guard let drive = path.range(of: "/prefix/drive_c/") else { return false }
            let program = path[drive.upperBound...]
            if game.store == "epic" {
                return (program.hasPrefix("program files (x86)/epic games/launcher/") || program.hasPrefix("program files/epic games/launcher/"))
                    && program.hasSuffix("/epicgameslauncher.exe")
            }
            return game.store == "gog" && (program == "program files (x86)/gog galaxy/galaxyclient.exe" || program == "program files/gog galaxy/galaxyclient.exe")
        }
    }

    private func playLibraryEntry(_ entry: GameLibraryEntry) {
        guard entry.canPlay else { return }
        if let game = model.browserGames.first(where: { "browser:\($0.id)" == entry.id }) { model.openBrowserGame(game); return }
        for bottle in model.bottles {
            if let game = model.publisherGames[bottle.id]?.first(where: { "publisher:\(bottle.id):\($0.id)" == entry.id }),
               let client = publisherClient(for: game, in: bottle) {
                model.runLauncher(client, in: bottle); return
            }
            if let game = model.steamGames[bottle.id]?.first(where: { "steam:\(bottle.id):\($0.app_id)" == entry.id }) {
                model.runSteamGame(game, in: bottle); return
            }
            if let launcher = model.launchers[bottle.id]?.first(where: { "app:\(bottle.id):\($0.id)" == entry.id }) {
                model.runLauncher(launcher, in: bottle); return
            }
        }
    }

    @ViewBuilder private var discoverView: some View {
        VStack(spacing: 0) {
            DiscoverScopeBar(selection: $discoverScope)
            Group {
                switch discoverScope {
                case .stores: storeDiscoverView
                case .browser: browserDiscoverView
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private var browserDiscoverView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                HStack {
                    VStack(alignment: .leading, spacing: 5) {
                        Text("Browser games").font(.title2.weight(.semibold))
                        Text("Find a game. Save a favourite. Play in your browser.").font(.subheadline).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Add a game link…") { browserGameRequest = AddBrowserGameRequest() }.buttonStyle(.bordered)
                }
                if let error = BrowserGameCatalog.loaded.error { Text(error).font(.caption).foregroundStyle(.secondary) }
                let entries = BrowserGameCatalog.loaded.entries.filter {
                    catalogSearch.isEmpty || $0.name.localizedStandardContains(catalogSearch) || $0.detail.localizedStandardContains(catalogSearch)
                }
                if entries.isEmpty { ContentUnavailableView.search(text: catalogSearch) }
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 240, maximum: 360), spacing: 16)], spacing: 16) {
                    ForEach(entries) { game in
                        BrowserDiscoverCard(entry: game,
                                            isSaved: model.browserGames.contains { $0.url == game.url.absoluteString },
                                            onPlay: {
                            model.openBrowserGame(BrowserGame(id: game.id, name: game.name,
                                                             url: game.url.absoluteString, created_at: 0))
                        }, onAdd: {
                            browserGameRequest = AddBrowserGameRequest(name: game.name, url: game.url.absoluteString)
                        })
                    }
                }
                Text("Community ports and recreations are separate from official releases. Each site handles your game files and saves. Gameplay is untested.")
                    .font(.caption).foregroundStyle(.secondary)
            }.padding(24)
        }
    }

    private var storeDiscoverView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                runtimeBanner
                if let error = CatalogSource.loaded.error {
                    ContentUnavailableView("Catalog unavailable", systemImage: "exclamationmark.triangle",
                                           description: Text(error))
                        .frame(maxWidth: .infinity, minHeight: 150)
                } else if filteredCatalog.isEmpty {
                    ContentUnavailableView.search(text: catalogSearch)
                        .frame(maxWidth: .infinity, minHeight: 150)
                } else {
                    ForEach(catalogGroups) { group in
                        VStack(alignment: .leading, spacing: 12) {
                            Text(group.title)
                                .font(.system(size: 12, weight: .semibold))
                                .foregroundStyle(.secondary)
                                .padding(.leading, 3)
                            LazyVGrid(columns: [GridItem(.flexible(), spacing: 12), GridItem(.flexible())],
                                      spacing: 12) {
                                ForEach(group.entries) { app in catalogCard(app) }
                            }
                        }
                    }
                }
                HStack(alignment: .center, spacing: 16) {
                    Text("Setup is experimental. Game compatibility is unverified.")
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                    Spacer(minLength: 12)
                    Button { model.chooseInstaller() } label: {
                        Label("Install from file…", systemImage: "plus.app")
                            .font(.system(size: 12, weight: .medium))
                    }
                    .buttonStyle(.borderless)
                    .disabled(model.isQuitting || model.isLaunching || model.isManagingBottle || model.status.wine_path == nil)
                }
                .padding(.top, 2).padding(.horizontal, 3)
            }
            .padding(24)
        }
    }

    private func catalogCard(_ app: CatalogEntry) -> some View {
        let hovered = hoveredCatalogID == app.id
        let pending = pendingAttempt(for: app)
        let installed = app.installMode == "automatic" ? installedProgram(for: app) : nil
        let action = pending?.stage == .failed ? "Retry" :
            pending != nil ? "Resume" : installed != nil ? "Open" :
            app.installMode == "automatic" ? "Install" : "Set up"
        let actionSymbol = pending?.stage == .failed ? "arrow.clockwise" :
            pending != nil || installed != nil ? "play.fill" :
            app.installMode == "automatic" ? "arrow.down" : "arrow.right"
        return Button { openOrSetup(app) } label: {
            HStack(spacing: 14) {
                CatalogArtworkTile(store: catalogArtwork, id: app.id, size: 40)
                VStack(alignment: .leading, spacing: 4) {
                    Text(app.name)
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(2).fixedSize(horizontal: false, vertical: true)
                    Text(app.detail)
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                        .lineLimit(1)
                    if let pending {
                        Label(pending.stage == .failed ? "Setup stopped" : "Setup paused",
                              systemImage: pending.stage == .failed ? "exclamationmark.circle" : "pause.circle")
                            .font(.system(size: 10, weight: .medium)).foregroundStyle(.orange)
                    } else if installed != nil {
                        Label("Installed", systemImage: "checkmark.circle.fill")
                            .font(.system(size: 10, weight: .medium)).foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                HStack(spacing: 6) {
                    Text(action).font(.system(size: 11, weight: .semibold))
                    Image(systemName: actionSymbol)
                        .font(.system(size: 9, weight: .semibold))
                }
                .foregroundStyle(installed != nil && pending == nil ? Color.accentColor : Color.primary)
                .padding(.horizontal, 12).frame(height: 30)
                .background(.primary.opacity(hovered ? 0.12 : 0.065), in: Capsule())
            }
            .frame(maxWidth: .infinity, minHeight: 66, alignment: .leading)
            .padding(.horizontal, 16).padding(.vertical, 12)
            .background(.primary.opacity(hovered ? 0.065 : 0.025), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .strokeBorder(.primary.opacity(hovered ? 0.16 : 0.07), lineWidth: 1)
            }
            .contentShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        }
        .buttonStyle(.plain)
        .disabled(model.isQuitting || model.isLaunching || model.isManagingBottle ||
                  (app.installMode == "automatic" && model.status.wine_path == nil))
        .onHover { hoveredCatalogID = $0 ? app.id : nil }
        .animation(.easeOut(duration: 0.12), value: hovered)
        .accessibilityLabel("\(action) \(app.name)")
    }

    private var bottleInspector: some View {
        VStack(spacing: 0) {
            HStack {
                bottlePanelLabel(collectionSelectedID == nil ? "BOTTLE CONTROLS" : "WINDOWS DETAILS")
                Spacer()
                Button(action: closeCollectionInspector) {
                    Image(systemName: "xmark").font(.system(size: 11, weight: .semibold))
                        .frame(width: 28, height: 28).contentShape(Rectangle())
                }
                .buttonStyle(InspectorCloseButtonStyle())
                .keyboardShortcut(.escape, modifiers: [])
                .help("Close details and return to Collection (Esc)")
                .accessibilityLabel("Close details and return to Collection")
            }
            .padding(.horizontal, 18).padding(.top, 12).padding(.bottom, 8)
            Divider().padding(.horizontal, 18)
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 18) {
                        if model.isManagingBottle {
                            HStack(spacing: 8) {
                                ProgressView().controlSize(.small)
                                Text("Updating this bottle…").font(.caption).foregroundStyle(.secondary)
                            }
                        }
                        if let bottle = selectedBottle {
                            inspectorHeader(for: bottle).id("inspector-top")
                            if model.bottles.count > 1 {
                                Picker("Bottle", selection: Binding(
                                    get: { model.selectedBottleID },
                                    set: { id in
                                        if let bottle = model.bottles.first(where: { $0.id == id }) { inspectBottle(bottle) }
                                    }
                                )) {
                                    ForEach(model.bottles) { Text($0.name).tag($0.id) }
                                }
                                .labelsHidden().accessibilityLabel("Choose bottle")
                            }
                            inspectorApps(for: bottle)
                            Divider()
                            bottleActionsPanel(for: bottle)
                            Divider()
                            DisclosureGroup(isExpanded: $showBottleManagement) {
                                inspectorManagement(for: bottle).padding(.top, 10)
                            } label: {
                                Label("Manage Bottle", systemImage: "shippingbox")
                                    .font(.system(size: 13, weight: .medium))
                            }
                        } else {
                            Text("Your Windows spaces").font(.title3.weight(.semibold))
                            Text("Choose an app in Discover, or create a bottle for your own Windows software.")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        Divider()
                        inspectorLibraryActions
                        recentlyDeletedBottles.id("recently-deleted")
                    }
                    .padding(18)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .onChange(of: selectedSteamGame?.id ?? inspectedProgram?.id ?? selectedBottle?.id) { _, _ in
                    proxy.scrollTo("inspector-top", anchor: .top)
                }
                .task(id: recentlyDeletedScrollRequest) {
                    guard scrollToRecentlyDeleted else { return }
                    await Task.yield()
                    proxy.scrollTo("recently-deleted", anchor: .bottom)
                    scrollToRecentlyDeleted = false
                }
            }
            .frame(maxHeight: .infinity)
        }
        .frame(maxHeight: .infinity)
        .tint(.primary)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .strokeBorder(.primary.opacity(0.10))
                .allowsHitTesting(false)
        }
        .shadow(color: .black.opacity(0.10), radius: 12, y: 5)
        .task(id: selectedBottle?.id) {
            if let bottle = selectedBottle {
                model.refreshBottleResolution(in: bottle)
                model.refreshBottleGraphics(in: bottle)
                while !Task.isCancelled {
                    await model.refreshBottleActivity(in: bottle)
                    do { try await Task.sleep(for: .seconds(3)) } catch { break }
                }
            }
        }
    }

    private func inspectorHeader(for bottle: Bottle) -> some View {
        HStack(spacing: 12) {
            if let item = selectedCollectionItem, item.platform == .windows {
                if let icon = item.icon {
                    Image(nsImage: icon).resizable().interpolation(.high).scaledToFit()
                        .frame(width: 48, height: 48)
                } else {
                    Image(systemName: item.kind == .app ? "app" : "gamecontroller")
                        .font(.system(size: 28)).foregroundStyle(.secondary).frame(width: 48, height: 48)
                }
            } else if let icon = collectionFolders.first(where: { $0.id == bottle.id })?.icon {
                Image(nsImage: icon).resizable().interpolation(.high).scaledToFit()
                    .frame(width: 42, height: 42)
            } else { BottleArtwork(model: model, bottle: bottle) }
            VStack(alignment: .leading, spacing: 4) {
                Text(selectedCollectionItem?.platform == .windows ? (selectedCollectionItem?.name ?? bottle.name) : bottle.name).font(.system(size: 19, weight: .semibold, design: .rounded))
                    .lineLimit(2)
                let count = (model.launchers[bottle.id] ?? []).count
                let games = (model.steamGames[bottle.id] ?? []).filter { $0.app_id != 228980 }.count
                Text(selectedCollectionItem?.platform == .windows ? "Windows · " + bottle.name : "\(count) \(count == 1 ? "app" : "apps")" + (games == 0 ? "" : " · \(games) \(games == 1 ? "game" : "games")"))
                    .font(.system(size: 11)).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
    }

    private func inspectorApps(for bottle: Bottle) -> some View {
        let apps = model.launchers[bottle.id] ?? []
        let games = (model.steamGames[bottle.id] ?? []).filter { $0.app_id != 228980 }
        let busy = model.isManagingBottle || model.isLaunching || model.isStoppingBottle(bottle.id)
        return VStack(alignment: .leading, spacing: 12) {
            if let entry = selectedCollectionItem, entry.id.hasPrefix("publisher:") {
                inspectorOpenControls(for: bottle, title: "Open store", canOpen: entry.canOpen && !busy) {
                    openCollectionItem(entry)
                }
                Text("This installed title was found in its local store manifest. Start it from the store; gameplay is not verified.")
                    .font(.caption).foregroundStyle(.secondary)
            } else if let item = inspectedProgram {
                if apps.count + games.count > 1 {
                    Picker("App", selection: Binding(
                        get: { selectedSteamGame.map { "game:" + $0.id } ?? "app:" + (inspectedProgram?.launcher.id ?? "") },
                        set: { value in
                            if value.hasPrefix("game:"), let game = games.first(where: { $0.id == String(value.dropFirst(5)) }) {
                                selectedSteamGameID = game.id
                                collectionSelectedID = "steam:\(bottle.id):\(game.app_id)"
                            } else {
                                selectedLauncherID = String(value.dropFirst(4)); selectedSteamGameID = nil
                                collectionSelectedID = "app:\(bottle.id):\(selectedLauncherID ?? "")"
                            }
                        }
                    )) {
                        ForEach(apps) { app in
                            Text(app.name + (app.hidden_from_home ? " (hidden)" : "")).tag("app:" + app.id)
                        }
                        ForEach(games) { Text($0.name).tag("game:" + $0.id) }
                    }
                }
                inspectorOpenControls(for: bottle, title: "Open \(selectedSteamGame?.name ?? item.launcher.name)",
                                      canOpen: !busy && model.status.wine_path != nil) {
                    if let game = selectedSteamGame { model.runSteamGame(game, in: bottle) }
                    else { model.runLauncher(item.launcher, in: bottle) }
                }
                if selectedSteamGame != nil {
                    Text("Opens through Steam in this bottle.").font(.caption).foregroundStyle(.secondary)
                } else {
                DisclosureGroup(isExpanded: $showLaunchOptions) {
                    VStack(spacing: 6) {
                        bottleAction("Run with Options…", symbol: "play.circle") { runOptionsItem = item }
                        bottleAction("Edit Saved Arguments…", symbol: "text.alignleft") { savedArgumentsItem = item }
                        bottleAction("Rename App…", symbol: "pencil") { renameLauncherItem = item }
                        bottleAction(item.launcher.hidden_from_home ? "Show on Collection" : "Hide from Collection",
                                     symbol: item.launcher.hidden_from_home ? "eye" : "eye.slash") {
                            model.setLauncherVisibility(item.launcher, in: bottle,
                                                        hidden: !item.launcher.hidden_from_home)
                        }
                    }
                    .disabled(busy).padding(.top, 10)
                } label: {
                    Label("App Options", systemImage: "slider.horizontal.3")
                        .font(.system(size: 13, weight: .medium))
                }
                }
            } else if let game = selectedSteamGame {
                inspectorOpenControls(for: bottle, title: "Open " + game.name,
                                      canOpen: !busy && model.status.wine_path != nil) {
                    model.runSteamGame(game, in: bottle)
                }
                Text("Add Steam from this bottle with Find installed apps to open its games.")
                    .font(.caption).foregroundStyle(.secondary)
            } else {
                Text("No saved apps yet").font(.system(size: 13, weight: .semibold))
                Text("Install an app or add a program already in this bottle.")
                    .font(.caption).foregroundStyle(.secondary)
                bottleQuitControls(for: bottle)
            }
            Menu {
                Button("Install from file…") { model.chooseInstaller(in: bottle.id) }
                    .disabled(model.status.wine_path == nil)
                Button("Find installed apps…") { model.findInstalledPrograms(in: bottle) }
                Button("Add an EXE…") { model.addLauncher(to: bottle) }
            } label: {
                Label("Add app to this bottle", systemImage: "plus")
                    .font(.system(size: 12, weight: .medium))
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .disabled(busy)
        }
    }

    private func inspectorOpenControls(for bottle: Bottle, title: String, canOpen: Bool,
                                       action: @escaping () -> Void) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Button(action: action) {
                    Label(title, systemImage: "play.fill")
                        .font(.system(size: 12, weight: .semibold))
                        .lineLimit(1)
                        .frame(maxWidth: .infinity).frame(height: 28)
                }
                .buttonStyle(.borderedProminent).tint(.accentColor)
                .disabled(!canOpen || model.isQuitting)
                .help(title)
                bottleQuitButton(for: bottle)
            }
            bottleQuitStatus(for: bottle)
        }
    }

    private func bottleQuitControls(for bottle: Bottle) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            bottleQuitButton(for: bottle)
            bottleQuitStatus(for: bottle)
        }
    }

    private func bottleQuitButton(for bottle: Bottle) -> some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let session = model.sessionState(for: bottle)
            if session.showsQuitControl {
                let force = session.canForceQuit(at: context.date)
                Button {
                    if force { quitTarget = bottle }
                    else { model.requestQuitApps(in: bottle) }
                } label: {
                    HStack(spacing: 5) {
                        if session.isStopping && !force {
                            ProgressView().controlSize(.mini)
                        } else {
                            Image(systemName: force ? "stop.fill" : "stop")
                                .font(.system(size: 10, weight: .semibold))
                        }
                        Text(force ? "Force Quit…" : session.isStopping ? "Quitting…" : "Quit Apps")
                            .font(.system(size: 12, weight: .medium)).lineLimit(1)
                    }
                    .frame(height: 28)
                }
                .buttonStyle(.bordered)
                .fixedSize(horizontal: true, vertical: false)
                .disabled(model.isQuitting || model.isManagingBottle || model.isLaunching ||
                          (session.isStopping && !force))
                .help(force ? "Force quit all Windows apps in this bottle" : "Quit all Windows apps in this bottle")
                .accessibilityLabel(force ? "Force quit apps in \(bottle.name)" : "Quit apps in \(bottle.name)")
            }
        }
    }

    @ViewBuilder
    private func bottleQuitStatus(for bottle: Bottle) -> some View {
        let session = model.sessionState(for: bottle)
        if let error = session.error {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(session.activity == .unknown ? "Couldn’t check running apps." : "Apps could not finish quitting.")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
                    .help(error)
                Spacer(minLength: 0)
                Button(session.activity == .unknown ? "Retry" : "Check") {
                    Task { await model.refreshBottleActivity(in: bottle) }
                }
                .buttonStyle(.plain).font(.system(size: 11, weight: .medium))
            }
        } else if session.isStopping {
            Text(session.phase == .forcing ? "Stopping the remaining apps…" : "Save any open work. Stuck apps can be force quit.")
                .font(.system(size: 11)).foregroundStyle(.secondary)
        }
    }

    private func inspectorManagement(for bottle: Bottle) -> some View {
        VStack(spacing: 6) {
            bottleAction("Rename…", symbol: "pencil") { renameText = bottle.name; renameTarget = bottle }
            bottleAction("Duplicate Bottle", symbol: "square.on.square") { model.duplicateBottle(bottle) }
            bottleAction("Export Backup…", symbol: "square.and.arrow.up") { model.exportBottle(bottle) }
            bottleAction("Move to Recently Deleted…", symbol: "trash") { deleteTarget = bottle }
        }
        .disabled(model.isManagingBottle || model.isLaunching || model.isStoppingBottle(bottle.id))
    }

    private var inspectorLibraryActions: some View {
        HStack(spacing: 10) {
            Button { showingNewBottle = true } label: { Label("New bottle", systemImage: "plus") }
            Spacer(minLength: 0)
            Menu {
                Button("Caskrun backup…") { model.importBottle() }
                Button("From CrossOver…") { model.importCrossOverBottle() }
            } label: { Label("Import…", systemImage: "square.and.arrow.down") }
        }
        .font(.system(size: 12))
        .disabled(model.isManagingBottle || model.isLaunching)
    }

    private func bottleAction(_ title: String, symbol: String,
                              action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: symbol)
                    .frame(width: 17)
                    .foregroundStyle(.secondary)
                Text(title)
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.system(size: 8, weight: .bold))
                    .foregroundStyle(.tertiary)
            }
            .font(.system(size: 12, weight: .medium))
            .padding(.horizontal, 10)
            .frame(height: 34)
        }
        .buttonStyle(QuietToolButtonStyle())
    }

    private func bottlePanelLabel(_ title: String) -> some View {
        Text(title)
            .font(.system(size: 10, weight: .semibold, design: .monospaced))
            .tracking(1.2)
            .foregroundStyle(.secondary)
    }

    private func bottleWindowsVersionPanel(for bottle: Bottle) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            if let status = model.bottleWindowsVersions[bottle.id] {
                Picker("Windows version", selection: Binding(
                    get: { model.bottleWindowsVersions[bottle.id]?.selected_version ?? "default" },
                    set: { model.setWindowsVersion($0, in: bottle) }
                )) {
                    Text("Engine default").tag("default")
                    Text("Windows 11").tag("win11")
                    Text("Windows 10").tag("win10")
                    Text("Windows 8.1").tag("win81")
                    Text("Windows 8").tag("win8")
                    Text("Windows 7").tag("win7")
                    Text("Windows Vista").tag("vista")
                    Text("Windows XP").tag("winxp")
                }
                .disabled(model.isQuitting || model.isManagingBottle || model.isLaunching || model.status.wine_path == nil || model.loadingWindowsVersions.contains(bottle.id))
                Text("Changes the compatibility version reported to Windows apps. Close them before changing it; app-specific Wine settings take priority.")
                    .font(.caption).foregroundStyle(.secondary)
                if let warning = status.warning { Text(warning).font(.caption).foregroundStyle(.secondary) }
            } else if model.loadingWindowsVersions.contains(bottle.id) {
                ProgressView("Reading Windows version…").controlSize(.small)
            }
            if let error = model.windowsVersionErrors[bottle.id] {
                Text(error).font(.caption).foregroundStyle(.orange)
                Button("Retry") { model.refreshWindowsVersion(in: bottle) }
                    .disabled(model.isQuitting || model.isManagingBottle || model.isLaunching)
            }
        }
        .task(id: "\(bottle.id):\(model.wineExecutable)") { model.refreshWindowsVersion(in: bottle) }
    }

    private func bottleActionsPanel(for bottle: Bottle) -> some View {
        let busy = model.isLaunching || model.isManagingBottle || model.isStoppingBottle(bottle.id)
        let runtimeMissing = model.status.wine_path == nil
        return VStack(alignment: .leading, spacing: 14) {
            DisclosureGroup(isExpanded: $showBottleDisplay) {
                VStack(alignment: .leading, spacing: 16) {
                    bottleDisplayPanel(for: bottle)
                    bottleGraphicsPanel(for: bottle)
                }
                .disabled(model.isStoppingBottle(bottle.id))
                .padding(.top, 12)
                .padding(.bottom, 4)
            } label: {
                Label("Display & Graphics", systemImage: "display")
                    .font(.system(size: 13, weight: .medium))
            }
            .id("bottle-display")
            Divider()
            DisclosureGroup(isExpanded: $showBottleTools) {
                VStack(spacing: 6) {
                    bottleWindowsVersionPanel(for: bottle)
                    Divider().padding(.vertical, 6)
                    bottleAction("Wine Configuration", symbol: "slider.horizontal.3") { model.openWineConfiguration(in: bottle) }
                    bottleAction("Game Controllers", symbol: "gamecontroller") { model.openControllerPanel(in: bottle) }
                    bottleAction("Task Manager", symbol: "list.bullet.rectangle") { model.openTaskManager(in: bottle) }
                    bottleAction("Internet Settings", symbol: "network") { model.openInternetSettings(in: bottle) }
                    bottleAction("Registry Editor", symbol: "curlybraces") { model.openRegistryEditor(in: bottle) }
                    bottleAction("Command Prompt", symbol: "terminal") { model.openCommandPrompt(in: bottle) }
                    bottleAction("Simulate Reboot…", symbol: "arrow.clockwise") { rebootTarget = bottle }
                }
                .disabled(busy || runtimeMissing)
                .padding(.top, 12)
            } label: {
                Label("Windows Tools", systemImage: "wrench.and.screwdriver")
                    .font(.system(size: 13, weight: .medium))
            }
            Divider()
            DisclosureGroup(isExpanded: $showBottleMaintenance) {
                VStack(spacing: 6) {
                    bottleAction("Run an EXE…", symbol: "play") { model.runCommand(in: bottle) }
                        .disabled(busy || runtimeMissing)
                    bottleAction("Open C: Drive", symbol: "folder") { model.openDriveC(in: bottle) }
                    bottleAction("Uninstall Windows app…", symbol: "minus.circle") { model.openUninstaller(in: bottle) }
                        .disabled(busy || runtimeMissing)
                    bottleAction("Force Quit Apps…", symbol: "stop.circle") { quitTarget = bottle }
                        .disabled(busy || runtimeMissing)
                    bottleAction("View Logs", symbol: "doc.text") { logBottle = bottle }
                    bottleAction("Installed Programs…", symbol: "list.bullet.rectangle") { inventoryBottle = bottle }
                    bottleAction("Rebuild App Icons", symbol: "arrow.triangle.2.circlepath") { model.repairIcons(in: bottle) }
                        .disabled(busy)
                    bottleAction("Diagnostics…", symbol: "stethoscope") { diagnosticBottle = bottle }
                    bottleAction("Bottle Notes…", symbol: "note.text") { notesBottle = bottle }
                }
                .padding(.top, 12)
            } label: {
                Label("Files & Maintenance", systemImage: "folder")
                    .font(.system(size: 13, weight: .medium))
            }
        }
        .tint(.primary)
    }

    @ViewBuilder private func bottleDisplayPanel(for bottle: Bottle) -> some View {
        bottlePanelLabel("DISPLAY")
        if let status = model.bottleResolutions[bottle.id] {
            Toggle("High Resolution Mode", isOn: Binding(
                get: { model.bottleResolutions[bottle.id]?.high_resolution ?? false },
                set: { model.setBottleResolution(in: bottle, enabled: $0) }
            ))
            .font(.subheadline.weight(.medium))
            .disabled(model.isLaunching || model.isManagingBottle ||
                      model.loadingBottleResolutions.contains(bottle.id) || model.status.wine_path == nil)
            Text("\(status.high_resolution ? "Retina · 192 DPI" : "Standard · 96 DPI"). Close apps before changing this.")
                .font(.caption)
                .foregroundStyle(.secondary)
        } else if model.loadingBottleResolutions.contains(bottle.id) {
            HStack(spacing: 7) {
                ProgressView().controlSize(.small)
                Text("Reading display mode…")
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        } else {
            Text("Display mode unavailable")
                .font(.subheadline.weight(.medium))
            if let error = model.bottleResolutionErrors[bottle.id] {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
            }
        }
        HStack(spacing: 6) {
            Button("Refresh display status") { model.refreshBottleResolution(in: bottle) }
                .font(.caption)
                .buttonStyle(.borderless)
                .disabled(model.loadingBottleResolutions.contains(bottle.id) || model.isManagingBottle)
            ZStack {
                if model.loadingBottleResolutions.contains(bottle.id),
                   model.bottleResolutions[bottle.id] != nil {
                    ProgressView().controlSize(.mini)
                        .accessibilityLabel("Refreshing display status")
                }
            }
            .frame(width: 12, height: 12)
        }
    }

    private func bottleGraphicsPanel(for bottle: Bottle) -> some View {
        VStack(alignment: .leading, spacing: 9) {
            bottlePanelLabel("GRAPHICS")
            if let status = model.bottleGraphics[bottle.id] {
                Picker("Renderer", selection: Binding(
                    get: { model.bottleGraphics[bottle.id]?.selected_backend ?? "auto" },
                    set: { model.setBottleGraphics(in: bottle, backend: $0) }
                )) {
                    Text("Automatic (WineD3D)").tag("auto")
                    Text("WineD3D").tag("wined3d")
                    Text("DXMT · Experimental").tag("dxmt").disabled(!status.dxmt_available)
                }
                .pickerStyle(.menu)
                .font(.subheadline)
                .disabled(model.isQuitting || model.isLaunching || model.isManagingBottle ||
                          model.loadingBottleGraphics.contains(bottle.id) || model.status.wine_path == nil)
                Text(status.effective_backend == "dxmt"
                     ? "DXMT translates DirectX 10/11 to Metal. Game compatibility still needs testing."
                     : "WineD3D is the default. Close this bottle’s apps before changing its renderer.")
                    .font(.caption).foregroundStyle(.secondary)
                if status.selected_backend == "dxmt" {
                    DXMTOptionsView(status: status,
                        isBusy: model.isQuitting || model.isLaunching || model.isManagingBottle || model.loadingBottleGraphics.contains(bottle.id)) { rate, scale in
                            model.setDXMTOptions(in: bottle, frameRate: rate, spatialScale: scale)
                        }
                }
                if !status.dxmt_available, let reason = status.unavailable_reason {
                    Text(reason).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                if let warning = status.warning {
                    Text(warning).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            } else if model.loadingBottleGraphics.contains(bottle.id) {
                HStack(spacing: 7) {
                    ProgressView().controlSize(.small)
                    Text("Reading graphics options…")
                }
                .font(.caption).foregroundStyle(.secondary)
            } else {
                Text("Graphics options unavailable").font(.subheadline.weight(.medium))
            }
            if let error = model.bottleGraphicsErrors[bottle.id] {
                Text(error).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            HStack(spacing: 6) {
                Button("Refresh graphics status") { model.refreshBottleGraphics(in: bottle) }
                    .font(.caption).buttonStyle(.borderless)
                    .disabled(model.loadingBottleGraphics.contains(bottle.id) || model.isManagingBottle)
                if model.loadingBottleGraphics.contains(bottle.id), model.bottleGraphics[bottle.id] != nil {
                    ProgressView().controlSize(.mini).accessibilityLabel("Refreshing graphics status")
                }
            }
            DisclosureGroup("Other graphics & sync options") {
                ForEach(["DXVK", "D3DMetal", "MSync"], id: \.self) { name in
                    HStack {
                        Text(name)
                        Spacer()
                        Text("Not available yet").foregroundStyle(.secondary)
                    }
                    .font(.caption)
                    .padding(.vertical, 3)
                }
            }
            .font(.caption)
        }
    }

    @ViewBuilder private var recentlyDeletedBottles: some View {
        if !model.deletedBottles.isEmpty {
            VStack(alignment: .leading, spacing: 10) {
                Text("Recently Deleted").font(.title3.weight(.semibold))
                Text("Restore a bottle to use it again, or permanently delete its apps, games and files.")
                    .font(.caption).foregroundStyle(.secondary)
                ForEach(model.deletedBottles) { bottle in
                    HStack {
                        Image(systemName: "shippingbox")
                        Text(bottle.name)
                        Spacer()
                        Button("Restore") { model.restoreDeletedBottle(bottle) }
                            .disabled(model.isQuitting || model.isManagingBottle || model.isLaunching)
                        Button { purgeTarget = bottle } label: {
                            Image(systemName: "trash").frame(width: 24, height: 24)
                        }
                        .buttonStyle(.borderless)
                        .foregroundStyle(.secondary)
                        .disabled(model.isQuitting || model.isManagingBottle || model.isLaunching)
                        .help("Delete \(bottle.name) permanently…")
                        .accessibilityLabel("Delete \(bottle.name) permanently…")
                    }
                    .padding(12)
                    .background(.quaternary.opacity(0.45), in: RoundedRectangle(cornerRadius: 10))
                }
            }
        }
    }

    private func heading(_ title: String, subtitle: String) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(title).font(.system(size: 25, weight: .semibold, design: .rounded))
            Text(subtitle).font(.system(size: 12)).foregroundStyle(.secondary)
        }
    }

    private func searchField(_ title: String, text: Binding<String>, field: SearchField) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField(title, text: text)
                .textFieldStyle(.plain)
                .focused($focusedSearch, equals: field)
            if !text.wrappedValue.isEmpty {
                Button { text.wrappedValue = "" } label: {
                    Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .help("Clear search")
                .accessibilityLabel("Clear search")
            }
        }
        .font(.system(size: 12))
        .padding(.horizontal, 10)
        .frame(height: 32)
        .background(.primary.opacity(0.045), in: RoundedRectangle(cornerRadius: 8))
        .overlay {
            RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.08))
        }
    }

    @ViewBuilder
    private var operationPanel: some View {
        if model.isQuitting {
            HStack(spacing: 10) {
                ProgressView().controlSize(.small)
                Text("Closing apps…").font(.system(size: 12, weight: .medium))
                Spacer()
            }
            .padding(.horizontal, 24).padding(.vertical, 10)
            .background(Color.accentColor.opacity(0.06))
        } else if model.isLaunching {
            HStack(spacing: 12) {
                ProgressView().controlSize(.small)
                VStack(alignment: .leading, spacing: 2) {
                    Text(model.steamStartup?.title ?? model.activeCatalogInstall.map { "Setting up \($0)" }
                         ?? model.activeInstallerName.map { "Running \($0)" }
                         ?? "Opening your app")
                        .font(.subheadline.weight(.semibold))
                    Text(model.steamStartup?.detail ?? (model.activeCatalogInstall == nil
                         ? (model.isCancellingManualInstaller
                            ? "Stopping this setup. Caskrun will update the result shortly."
                            : "Caskrun will update this view when the program responds.")
                         : (model.isCancellingCatalogInstall
                            ? "Stopping this setup. Caskrun will update the result shortly."
                            : catalogProgressText)))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if model.canStopWaitingForSteam {
                    Button("Stop waiting") { model.stopWaitingForSteam() }
                        .help("Keep Steam running and return to Caskrun.")
                } else if model.canCancelManualInstaller && model.activeCatalogInstall == nil {
                    Button(model.isCancellingManualInstaller ? "Canceling…" : "Cancel setup") {
                        model.cancelManualInstaller()
                    }
                    .disabled(model.isCancellingManualInstaller)
                } else if model.canCancelCatalogInstall && model.activeCatalogInstall != nil {
                    Button(model.isCancellingCatalogInstall ? "Canceling…" : "Cancel setup") {
                        model.cancelCatalogInstall()
                    }
                    .disabled(model.isCancellingCatalogInstall)
                }
                if let id = model.activeCatalogBottleID,
                   let name = model.activeCatalogInstall {
                    Button("Logs") {
                        logBottle = Bottle(id: id, name: name, created_at: 0)
                    }
                }
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.accentColor.opacity(0.09))
        } else if model.isManagingBottle {
            HStack(spacing: 10) {
                ProgressView().controlSize(.small)
                Text(model.bottleOperationLabel).font(.system(size: 12, weight: .medium))
                Spacer()
            }
            .padding(.horizontal, 24).padding(.vertical, 10)
            .background(Color.accentColor.opacity(0.06))
        } else if let failure = model.catalogSetupFailure {
            VStack(alignment: .leading, spacing: 9) {
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "exclamationmark.circle.fill")
                        .font(.title3)
                        .foregroundStyle(.orange)
                    VStack(alignment: .leading, spacing: 3) {
                        Text("\(failure.entryName) setup couldn't finish")
                            .font(.subheadline.weight(.semibold))
                        Text(failureSummary(failure))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 8)
                    if let attempt = attempt(for: failure), attempt.last_log_name != nil {
                        Button("View logs") { openLogs(for: attempt, name: failure.entryName) }
                    }
                    Button("Retry setup") { retry(failure) }
                        .buttonStyle(.borderedProminent)
                        .disabled(model.isManagingBottle || model.status.wine_path == nil)
                    Button("Dismiss") { model.catalogSetupFailure = nil }
                        .buttonStyle(.borderless)
                }
                DisclosureGroup("Technical details") {
                    Text(failure.detail)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .font(.caption)
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 13)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.orange.opacity(0.10))
        } else if let completion = model.installHandoff {
            HStack(spacing: 14) {
                Image(systemName: completion.setupNote == nil
                      ? "checkmark.circle.fill" : "arrow.clockwise.circle.fill")
                    .font(.title3)
                    .foregroundStyle(completion.setupNote == nil ? .green : .orange)
                VStack(alignment: .leading, spacing: 2) {
                    Text(completion.entryID == "manual"
                         ? "\(completion.entryName) is ready in Collection"
                         : "Continue in \(completion.entryName)")
                        .font(.subheadline.weight(.semibold))
                    Text(completion.setupNote ?? (completion.entryID == "manual"
                         ? "The launcher was saved. Open it to test; compatibility has not been verified."
                         : completion.entryID == "steam"
                         ? "Steam's installer finished and its updater started. Open Steam to continue, then sign in there. Games remain unverified."
                         : completion.entryID == "epic-games-store"
                         ? "The launcher was saved and started. Complete Epic setup and sign in there. Games remain unverified."
                         : "The launcher was saved and started. Sign in inside \(completion.entryName). Games remain unverified."))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                if completion.entryID == "manual" {
                    Button("Open \(completion.entryName)") {
                        model.runLauncher(completion.launcher, in: completion.bottle)
                    }
                    .buttonStyle(.borderedProminent)
                } else {
                    Button("View bottle") {
                        inspectBottle(completion.bottle, launcher: completion.launcher)
                    }
                    .buttonStyle(.borderedProminent)
                }
                Button("Dismiss") { model.installHandoff = nil }
                    .buttonStyle(.borderless)
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background((completion.setupNote == nil ? Color.green : Color.orange).opacity(0.09))
        }
    }

    private var catalogProgressText: String {
        switch model.activeCatalogStage {
        case nil: "Downloading the Windows installer from the publisher…"
        case .ready: "Preparing the Windows environment. This may take several minutes."
        case .running: "Running the publisher's installer. Complete any setup prompts that appear."
        case .installerComplete: "Saving the launcher in your library…"
        case .preparingClient: "Preparing the current Steam client before opening it…"
        case .installed: "Opening the launcher. Sign in there when it appears."
        case .failed: "Setup stopped. The bottle and installer are available for retry."
        }
    }

    @ViewBuilder
    private var runtimeBanner: some View {
        if model.status.wine_path == nil {
            HStack(spacing: 10) {
                Image(systemName: "exclamationmark.circle")
                Text("The included Windows engine is unavailable. Reinstall a complete Caskrun build.")
            }
            .padding(13)
            .background(Color.orange.opacity(0.13), in: RoundedRectangle(cornerRadius: 12))
        }
    }

    private func catalogSetupView(for entry: CatalogEntry) -> some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(spacing: 14) {
                CatalogArtworkTile(store: catalogArtwork, id: entry.id, size: 46)
                VStack(alignment: .leading, spacing: 3) {
                    Text("\(entry.id == "epic-games-store" ? "Try" : "Set up") \(entry.name)").font(.title2.weight(.semibold))
                    Text(entry.installMode == "automatic"
                         ? "Experimental setup · sign in inside the launcher · game support unverified"
                         : "Guided manual install · compatibility unverified")
                        .font(.subheadline).foregroundStyle(.secondary)
                }
                Spacer()
            }
            Divider()
            if entry.installMode == "automatic" {
                VStack(alignment: .leading, spacing: 16) {
                    Label("Caskrun downloads the Windows installer from the publisher, creates a bottle, and saves a launcher.", systemImage: "arrow.down.app")
                    Label("After setup, sign in inside \(entry.name) and install a game from your library.", systemImage: "person.crop.circle")
                    if entry.id == "epic-games-store" {
                        Label("Complete any Epic setup wizard prompts that appear during installation.", systemImage: "hand.tap")
                    }
                    Label("This setup is experimental. A successful install does not guarantee sign-in or game play.", systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.secondary)
                    Spacer()
                    HStack {
                        Link("Publisher page", destination: entry.downloadPageURL)
                        Spacer()
                        Button("Cancel") { selectedCatalogEntry = nil }
                        Button(entry.id == "epic-games-store" ? "Try Epic setup" : "Install \(entry.name)") {
                            model.installCatalog(entry)
                            selectedCatalogEntry = nil
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(model.status.wine_path == nil || model.isLaunching)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            } else {
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    catalogStep(1, title: "Get the Windows installer") {
                        Text("Open the publisher's page and download the Windows .exe or .msi version. The site may show a Mac download first.")
                            .foregroundStyle(.secondary)
                        Link(destination: entry.downloadPageURL) {
                            Label("Open official download page", systemImage: "arrow.up.right.square")
                        }
                        Text(entry.downloadPageURL.host ?? entry.downloadPageURL.absoluteString)
                            .font(.caption).foregroundStyle(.tertiary)
                        if let note = entry.installerNotes {
                            Text(note).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    catalogStep(2, title: "Install \(entry.name)") {
                        Text("Choose the downloaded file. Caskrun creates a separate bottle automatically and reports when the installer exits. Compatibility is not yet verified.")
                            .foregroundStyle(.secondary)
                        if !model.bottles.isEmpty {
                            DisclosureGroup("Advanced: use an existing bottle", isExpanded: $catalogUseExistingBottle) {
                                Picker("Bottle", selection: $catalogBottleID) {
                                    ForEach(model.bottles) { bottle in
                                        Text(bottle.name).tag(bottle.id)
                                    }
                                }
                                .padding(.top, 8)
                            }
                        }
                        if model.status.wine_path == nil {
                            Text("The included Windows engine is unavailable. Reinstall a complete Caskrun build.")
                                .font(.caption).foregroundStyle(.orange)
                        }
                        Button("Choose Windows installer…") {
                            pendingCatalogInstall = (catalogUseExistingBottle ? catalogBottleID : nil, entry.name)
                            selectedCatalogEntry = nil
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled((catalogUseExistingBottle && catalogBottleID.isEmpty) || model.status.wine_path == nil || model.isLaunching)
                    }
                }
            }
            HStack {
                Spacer()
                Button("Done") { selectedCatalogEntry = nil }
                    .keyboardShortcut(.cancelAction)
            }
            }
        }
        .padding(24)
        .frame(width: 560)
        .frame(minHeight: entry.installMode == "automatic" ? 320 : 500)
    }

    private func manualInstallReviewView(_ review: ManualInstallReview) -> some View {
        VStack(alignment: .leading, spacing: 15) {
            Text("Find the installed program")
                .font(.title2.weight(.semibold))
            Text(review.installerName.isEmpty
                 ? "Choose a program found in this bottle to add to Collection. You can open it from the bottle panel on Collection."
                 : "\(review.installerName) closed. Choose a new program to add to Collection, once the publisher's setup has finished.")
                .foregroundStyle(.secondary)
            if review.candidates.isEmpty {
                ContentUnavailableView(review.discoveryUnavailable ? "Could not search for programs" : "No new program found",
                                       systemImage: "magnifyingglass",
                                       description: Text(review.discoveryUnavailable
                                                         ? "You can find the installed program's EXE yourself."
                                                         : "The installer may still be finishing, or its program may be in another location. You can find its EXE yourself."))
                    .frame(maxWidth: .infinity, minHeight: 150)
            } else {
                Text(review.installerName.isEmpty ? "Programs found" : "New programs found")
                    .font(.headline)
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(review.candidates) { program in
                            HStack(spacing: 12) {
                                Image(systemName: "app.fill")
                                    .frame(width: 30, height: 30)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(program.name).font(.subheadline.weight(.semibold))
                                    Text(URL(fileURLWithPath: program.path).deletingLastPathComponent().lastPathComponent)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                                Spacer()
                                Button("Add to Collection") {
                                    model.saveDiscoveredProgram(program, in: review.bottle)
                                }
                            }
                            .padding(10)
                            .background(.quaternary.opacity(0.45), in: RoundedRectangle(cornerRadius: 10))
                        }
                    }
                }
                .frame(maxHeight: 250)
            }
            if review.searchWasLimited {
                Text("The search was limited. Use Find program to check another location.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Text("Caskrun has not verified this app's compatibility. Installer logs are available in the bottle panel on Collection.")
                .font(.caption)
                .foregroundStyle(.secondary)
            HStack {
                Button("Find program…") {
                    pendingManualLauncherBottle = review.bottle
                    model.manualInstallReview = nil
                }
                Spacer()
                Button("Later") { model.manualInstallReview = nil }
            }
        }
        .padding(24)
        .frame(width: 560)
        .frame(minHeight: 360)
    }

    private func catalogStep<Content: View>(_ number: Int, title: String, @ViewBuilder content: () -> Content) -> some View {
        HStack(alignment: .top, spacing: 14) {
            Text("\(number)")
                .font(.subheadline.weight(.bold))
                .frame(width: 28, height: 28)
                .background(.quaternary, in: Circle())
            VStack(alignment: .leading, spacing: 10) {
                Text(title).font(.headline)
                content()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

}


private struct BottleLogsView: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var model: CaskrunModel
    let bottle: Bottle

    @State private var logs: [BottleLog] = []
    @State private var selectedName: String?
    @State private var selectedContent: BottleLogContent?
    @State private var listError: String?
    @State private var contentError: String?
    @State private var isLoadingList = false
    @State private var isLoadingContent = false
    @State private var exportError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Logs for \(bottle.name)").font(.title2.weight(.semibold))
                    Text("Wine startup and installer output for this bottle")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Refresh") { Task { await loadLogs() } }
                    .disabled(isLoadingList)
                Button("Done") { dismiss() }
                    .keyboardShortcut(.cancelAction)
            }
            Divider()
            HStack(spacing: 0) {
                logList
                    .frame(width: 255)
                    .frame(maxHeight: .infinity)
                Divider()
                logDetail
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            .frame(minHeight: 430)
        }
        .padding(20)
        .frame(width: 840)
        .task { await loadLogs() }
        .alert("Could not save log", isPresented: Binding(
            get: { exportError != nil }, set: { if !$0 { exportError = nil } }
        )) {
            Button("OK") { exportError = nil }
        } message: { Text(exportError ?? "") }
    }

    private var logList: some View {
        Group {
            if isLoadingList && logs.isEmpty {
                ProgressView("Loading logs…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let listError {
                errorView("Could not list logs", detail: listError) { Task { await loadLogs() } }
            } else if logs.isEmpty {
                ContentUnavailableView("No logs yet", systemImage: "doc.text",
                                       description: Text("Logs appear here after running an installer or app in this bottle."))
            } else {
                ScrollView {
                    LazyVStack(spacing: 4) {
                        ForEach(logs) { log in
                            Button {
                                Task { await loadLog(log.name) }
                            } label: {
                                VStack(alignment: .leading, spacing: 5) {
                                    Text(log.name)
                                        .font(.subheadline.weight(.medium))
                                        .lineLimit(2)
                                    Text("\(ByteCountFormatter.string(fromByteCount: Int64(clamping: log.size), countStyle: .file)) · \(Date(timeIntervalSince1970: TimeInterval(log.modified_at)).formatted(date: .abbreviated, time: .shortened))")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(10)
                                .background(selectedName == log.name ? Color.accentColor.opacity(0.15) : Color.clear,
                                            in: RoundedRectangle(cornerRadius: 8))
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.trailing, 10)
                }
            }
        }
    }

    private var logDetail: some View {
        Group {
            if isLoadingContent {
                ProgressView("Reading log…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let contentError {
                errorView("Could not read log", detail: contentError) {
                    if let selectedName { Task { await loadLog(selectedName) } }
                }
            } else if let selectedContent {
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        Text(selectedContent.name)
                            .font(.headline)
                            .lineLimit(1)
                            .help(selectedContent.name)
                        Spacer()
                        Button("Copy") {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(selectedContent.content, forType: .string)
                        }
                        Button("Save Text…") {
                            do {
                                try LocalReportSave.text(selectedContent.content, name: "Caskrun-log.txt",
                                    message: "Saves the displayed log text, including any local paths or account data it contains.")
                            } catch { exportError = error.localizedDescription }
                        }
                    }
                    if selectedContent.truncated {
                        Label("Showing the latest 64 KiB of this log.", systemImage: "text.alignleft")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    ScrollView {
                        Text(selectedContent.content.isEmpty ? "This log is empty." : selectedContent.content)
                            .font(.system(.caption, design: .monospaced))
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(.leading, 16)
            } else {
                ContentUnavailableView("Select a log", systemImage: "doc.text.magnifyingglass")
            }
        }
    }

    private func errorView(_ title: String, detail: String, retry: @escaping () -> Void) -> some View {
        VStack(spacing: 10) {
            Image(systemName: "exclamationmark.triangle")
                .font(.title2)
                .foregroundStyle(.orange)
            Text(title)
                .font(.headline)
            Text(detail)
                .font(.caption)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .textSelection(.enabled)
            Button("Retry", action: retry)
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @MainActor
    private func loadLogs() async {
        guard !isLoadingList else { return }
        isLoadingList = true
        listError = nil
        do {
            let fetched = try await model.logs(in: bottle)
            logs = fetched
            if let preferred = selectedName, fetched.contains(where: { $0.name == preferred }) {
                await loadLog(preferred)
            } else if let first = fetched.first {
                await loadLog(first.name)
            } else {
                selectedName = nil
                selectedContent = nil
                contentError = nil
                isLoadingContent = false
            }
        } catch {
            logs = []
            selectedName = nil
            selectedContent = nil
            contentError = nil
            isLoadingContent = false
            listError = error.localizedDescription
        }
        isLoadingList = false
    }

    @MainActor
    private func loadLog(_ name: String) async {
        selectedName = name
        selectedContent = nil
        contentError = nil
        isLoadingContent = true
        do {
            let result = try await model.readLog(name, in: bottle)
            guard selectedName == name else { return }
            selectedContent = result
        } catch {
            guard selectedName == name else { return }
            contentError = error.localizedDescription
        }
        isLoadingContent = false
    }
}
