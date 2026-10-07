import AppKit
import SwiftUI
import ImageIO
import Darwin

// Shared with the offscreen Dock renderer; no window or Windows process is created.
private enum MacAppArtwork {
    static func png(_ data: Data, crop: CGRect? = nil, svg: Bool = false) -> Data? {
        guard !data.isEmpty, data.count <= 16 * 1024 * 1024 else { return nil }
        let bitmap: CGImage
        if svg {
            guard let image = NSImage(data: data), image.isValid,
                  image.size.width > 0, image.size.height > 0,
                  image.size.width <= 2048, image.size.height <= 2048,
                  let decoded = image.cgImage(forProposedRect: nil, context: nil, hints: nil),
                  decoded.width <= 4096, decoded.height <= 4096 else { return nil }
            if let crop {
                let sx = CGFloat(decoded.width) / image.size.width
                let sy = CGFloat(decoded.height) / image.size.height
                let rect = CGRect(x: crop.minX * sx, y: crop.minY * sy,
                                  width: crop.width * sx, height: crop.height * sy).integral
                guard rect.minX >= 0, rect.minY >= 0,
                      rect.maxX <= CGFloat(decoded.width), rect.maxY <= CGFloat(decoded.height),
                      let cropped = decoded.cropping(to: rect) else { return nil }
                bitmap = cropped
            } else { bitmap = decoded }
        } else {
            guard let source = CGImageSourceCreateWithData(data as CFData, nil),
                  CGImageSourceGetCount(source) > 0, CGImageSourceGetCount(source) <= 64 else { return nil }
            var best = 0, area = 0
            for index in 0..<CGImageSourceGetCount(source) {
                guard let props = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any],
                      let width = props[kCGImagePropertyPixelWidth] as? NSNumber,
                      let height = props[kCGImagePropertyPixelHeight] as? NSNumber,
                      width.intValue > 0, height.intValue > 0,
                      width.intValue <= 4096, height.intValue <= 4096 else { return nil }
                if width.intValue * height.intValue > area {
                    best = index; area = width.intValue * height.intValue
                }
            }
            let options: [CFString: Any] = [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceThumbnailMaxPixelSize: 1024,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceShouldCacheImmediately: true
            ]
            guard let decoded = CGImageSourceCreateThumbnailAtIndex(source, best, options as CFDictionary) else { return nil }
            bitmap = decoded
        }
        return try? caskrunAdaptiveIconPNG(bitmap, size: 256)
    }

    static func read(_ path: String, maximum: Int) -> Data? {
        guard path.hasPrefix("/"), !path.contains("\0"),
              URL(fileURLWithPath: path).standardizedFileURL.path == path else { return nil }
        let fd = Darwin.open(path, O_RDONLY | O_CLOEXEC | O_NONBLOCK | O_NOFOLLOW | 0x2000_0000)
        guard fd >= 0 else { return nil }
        defer { Darwin.close(fd) }
        var before = stat()
        guard fstat(fd, &before) == 0, (before.st_mode & S_IFMT) == S_IFREG,
              before.st_size > 0, before.st_size <= maximum else { return nil }
        var data = Data(count: Int(before.st_size))
        let readOK = data.withUnsafeMutableBytes { bytes in
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.read(fd, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if count < 0, errno == EINTR { continue }
                guard count > 0 else { return false }
                offset += count
            }
            return true
        }
        var after = stat()
        guard readOK, fstat(fd, &after) == 0, before.st_size == after.st_size,
              before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
              before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
              before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
              before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec else { return nil }
        return data
    }
}

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
    let profile_account_linked: Bool
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
}

struct SteamLibrary: Decodable, Sendable {
    let games: [SteamGame]
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
    case ready, running, failed, installerComplete = "installer_complete", installed
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
}

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
    var id: String { path }
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

enum Page: Hashable { case home, discover }

private enum CaskrunMenuCommand {
    case open, home, discover, manageBottles, install
}

@MainActor
private final class CaskrunAppDelegate: NSObject, NSApplicationDelegate {
    var handleMenuCommand: ((CaskrunMenuCommand) -> Void)?
    var shutdownWindowsApps: (() async -> Bool)?
    private var terminationPending = false

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !terminationPending else { return .terminateLater }
        guard let shutdownWindowsApps else { return .terminateCancel }
        terminationPending = true
        Task {
            let complete = await shutdownWindowsApps()
            terminationPending = false
            sender.reply(toApplicationShouldTerminate: complete)
            if !complete { handleMenuCommand?(.open) }
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
    private var presentationGeneration: UInt64 = 0
    private(set) var hasPresentedMainWindow = false

    private override init() {
        super.init()
        NotificationCenter.default.addObserver(self, selector: #selector(mainWindowWillClose(_:)),
                                               name: NSWindow.willCloseNotification, object: nil)
    }

    func register(_ window: NSWindow) {
        guard !mainWindows.allObjects.contains(where: { $0 === window }) else { return }
        mainWindows.add(window)
        hasPresentedMainWindow = true
        presentationGeneration &+= 1
    }

    @objc private func mainWindowWillClose(_ notification: Notification) {
        guard let window = notification.object as? NSWindow,
              mainWindows.allObjects.contains(where: { $0 === window }) else { return }
        mainWindows.remove(window)
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

    func updateNSView(_ nsView: CaskrunWindowCaptureView, context: Context) {}
}

// Preserve the original double-click target when opening its inspector
// changes the grid layout. Single selection never waits for this monitor.
@MainActor
private final class CaskrunHomeClickRouter {
    static let shared = CaskrunHomeClickRouter()
    private var monitor: Any?
    private weak var firstWindow: NSWindow?
    private var firstPoint = NSPoint.zero
    private var firstTimestamp: TimeInterval = 0
    private var launch: (@MainActor @Sendable () -> Void)?
    private var consumedWindowNumber: Int?

    private init() {
        monitor = NSEvent.addLocalMonitorForEvents(matching: [
            .leftMouseDown, .leftMouseUp, .rightMouseDown, .scrollWheel, .keyDown
        ]) { [weak self] event in
            guard let self else { return event }
            return self.route(event)
        }
    }

    deinit {
        if let monitor { NSEvent.removeMonitor(monitor) }
    }

    func remember(_ event: NSEvent?, launch: @escaping @MainActor @Sendable () -> Void) {
        cancel()
        guard let event, event.type == .leftMouseUp, event.clickCount == 1,
              let window = event.window else { return }
        firstWindow = window
        firstPoint = event.locationInWindow
        firstTimestamp = event.timestamp
        self.launch = launch
    }

    func cancel() {
        firstWindow = nil
        launch = nil
    }

    private func route(_ event: NSEvent) -> NSEvent? {
        if event.type == .leftMouseUp, consumedWindowNumber == event.windowNumber {
            consumedWindowNumber = nil
            return nil
        }
        guard event.type == .leftMouseDown else {
            if event.type != .leftMouseUp { cancel() }
            return event
        }
        consumedWindowNumber = nil
        let elapsed = event.timestamp - firstTimestamp
        let distance = hypot(event.locationInWindow.x - firstPoint.x,
                             event.locationInWindow.y - firstPoint.y)
        guard event.clickCount == 2, let firstWindow, event.window === firstWindow,
              elapsed >= 0, elapsed <= NSEvent.doubleClickInterval,
              distance <= 8, let action = launch else {
            cancel()
            return event
        }
        cancel()
        consumedWindowNumber = event.windowNumber
        DispatchQueue.main.async(execute: action)
        return nil
    }
}

private struct CaskrunStatusBridge: View {
    @Environment(\.openWindow) private var openWindow
    @Binding var page: Page
    @Binding var didChooseInitialPage: Bool
    @Binding var showingBottleInspector: Bool
    @Binding var openBottleID: String?
    @ObservedObject var model: CaskrunModel
    let delegate: CaskrunAppDelegate

    var body: some View {
        Color.clear
        .onAppear {
            let pageBinding = $page
            let initialPageBinding = $didChooseInitialPage
            let inspectorBinding = $showingBottleInspector
            let folderBinding = $openBottleID
            let openMainWindow = openWindow
            delegate.handleMenuCommand = { command in
                switch command {
                case .open: break
                case .home, .install:
                    pageBinding.wrappedValue = .home
                    folderBinding.wrappedValue = nil
                    inspectorBinding.wrappedValue = false
                    initialPageBinding.wrappedValue = true
                case .discover:
                    pageBinding.wrappedValue = .discover
                    initialPageBinding.wrappedValue = true
                case .manageBottles:
                    pageBinding.wrappedValue = .home
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
            delegate.shutdownWindowsApps = { await model.shutdownForQuit() }
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
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = item.button {
            if let logo = CaskrunMenuBarArtwork.image?.copy() as? NSImage {
                logo.size = NSSize(width: 17, height: 17)
                button.image = logo
                button.imagePosition = .imageLeading
            }
            // Keep a readable label even if the image is unavailable or clipped.
            button.title = "Caskrun"
            button.font = .systemFont(ofSize: 12, weight: .semibold)
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
            let placeholder = item(hasHiddenApps ? "Your apps are hidden from Home" : "No apps installed yet",
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
                appsMenu.addItem(item("See all \(apps.count) apps in Home…", action: #selector(home)))
            }
        }
        let appsItem = item("Your Apps", action: nil)
        appsItem.image = NSImage(systemSymbolName: "square.grid.2x2", accessibilityDescription: nil)
        appsItem.submenu = appsMenu
        menu.addItem(appsItem)

        menu.addItem(.separator())
        menu.addItem(item("Home", action: #selector(home)))
        menu.addItem(item("Discover", action: #selector(discover)))
        menu.addItem(item("Manage Bottles…", action: #selector(bottles)))
        menu.addItem(.separator())
        let installItem = item("Install from File…", action: #selector(installFromFile))
        installItem.isEnabled = !cannotLaunch
        menu.addItem(installItem)
        menu.addItem(.separator())
        menu.addItem(item("About Caskrun", action: #selector(about)))
        let quitItem = item(model?.isQuitting == true ? "Closing Windows apps…" : "Quit Caskrun", action: #selector(quit))
        quitItem.isEnabled = model?.isQuitting != true
        quitItem.toolTip = "Close Caskrun and all Windows apps in its bottles"
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
    @objc private func home() { handleMenuCommand?(.home) }
    @objc private func discover() { handleMenuCommand?(.discover) }
    @objc private func bottles() { handleMenuCommand?(.manageBottles) }
    @objc private func installFromFile() { handleMenuCommand?(.install) }

    @objc private func launchApp(_ sender: NSMenuItem) {
        guard menuApps.indices.contains(sender.tag), let model, !cannotLaunch else { return }
        let app = menuApps[sender.tag]
        model.runLauncher(app.launcher, in: app.bottle,
                          onFailure: { [weak self] in self?.handleMenuCommand?(.home) })
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
    @Published var selectedBottleID = ""
    @Published private(set) var bottleResolutions: [String: BottleResolutionStatus] = [:]
    @Published private(set) var loadingBottleResolutions: Set<String> = []
    @Published private(set) var bottleResolutionErrors: [String: String] = [:]
    @Published var wineExecutable = initialWineExecutable()
    @Published private(set) var isQuitting = false
    @Published var isLaunching = false
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
    private var runtimeProbeGeneration = 0
    private var catalogMonitorGeneration = 0
    private var requestedIcons: Set<String> = []
    private var launcherIconStamps: [String: String] = [:]
    private var bottleIconStamps: [String: String] = [:]
    private var activeManualInstaller: CoreInvocationCancellation?
    private var activeCatalogCancellation: CoreInvocationCancellation?

    var hasBundledRuntime: Bool { Self.bundledWinePath != nil }
    var usesBundledRuntime: Bool { Self.bundledWinePath == wineExecutable && hasBundledRuntime }

    private var coreURL: URL {
        Bundle.main.executableURL!.deletingLastPathComponent().appendingPathComponent("caskrun-core")
    }

    private func allowCoreCommandDuringShutdown(_ arguments: [String]) throws {
        let readCommands: Set<String> = ["overview", "status", "list", "deleted", "catalog-attempts",
                                         "apps", "app-icon", "bottle-icon", "bottle-resolution",
                                         "discover-apps", "steam-games", "logs", "read-log", "prefix-path"]
        if isQuitting, let command = arguments.first,
           command != "quit-all-bottles", !readCommands.contains(command) {
            throw NSError(domain: "Caskrun", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "Caskrun is closing its Windows apps. Try again after reopening it."])
        }
    }

    func shutdownForQuit() async -> Bool {
        guard !isQuitting else { return false }
        isQuitting = true
        CaskrunHomeClickRouter.shared.cancel()
        activeManualInstaller?.cancel()
        activeCatalogCancellation?.cancel()
        do {
            let clock = ContinuousClock()
            let deadline = clock.now.advanced(by: .seconds(30))
            while isLaunching || isManagingBottle || isProbingRuntime {
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
            return true
        } catch {
            isQuitting = false
            message = "Caskrun could not finish closing its Windows apps.\n\n" + error.localizedDescription
            return false
        }
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

    private func launcherIconStamp(_ launcher: SavedLauncher) -> String {
        "\(artworkPolicyStamp()):\(launcher.name):\(iconStamp(for: launcher.path))"
    }

    private func iconStamp(for path: String) -> String {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path),
              let size = attributes[.size] as? NSNumber,
              let modified = attributes[.modificationDate] as? Date else { return "missing" }
        return "\(size.uint64Value):\(modified.timeIntervalSince1970):\(attributes[.systemFileNumber] ?? "none")"
    }

    private func bottleIconStamp(_ bottle: Bottle) -> String {
        artworkPolicyStamp() + ":" + (launchers[bottle.id] ?? []).prefix(64).map { launcher in
            "\(launcher.id):\(launcherIconStamp(launcher))"
        }.joined(separator: "|")
    }

    func loadLauncherIcon(_ launcher: SavedLauncher, in bottle: Bottle) {
        let key = "\(bottle.id):\(launcher.id)"
        let stamp = launcherIconStamp(launcher)
        let requestKey = "app:\(key)"
        guard launcherIcons[key] == nil || launcherIconStamps[key] != stamp else { return }
        guard requestedIcons.insert(requestKey).inserted else { return }
        Task {
            defer {
                requestedIcons.remove(requestKey)
                if let current = launchers[bottle.id]?.first(where: { $0.id == launcher.id }),
                   launcherIconStamp(current) != stamp {
                    loadLauncherIcon(current, in: bottle)
                }
            }
            let result = try? await callAsync(["app-icon", bottle.id, launcher.id], as: AppIconResult.self)
            guard let current = launchers[bottle.id]?.first(where: { $0.id == launcher.id }),
                  launcherIconStamp(current) == stamp else { return }
            guard let result, let path = result.path,
                  let icon = NSImage(contentsOfFile: path) else {
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
                  let icon = NSImage(contentsOfFile: path) else {
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
        guard !isQuitting && !isManagingBottle else { return }
        isManagingBottle = true
        Task {
            defer { isManagingBottle = false }
            do {
                let copy = try await callAsync(["duplicate", bottle.id], as: Bottle.self)
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
                message = "Moved \(bottle.name) to Recently Deleted. Restore it from Manage bottles on Home."
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
        guard !isQuitting && !isManagingBottle else { return }
        let panel = NSSavePanel()
        panel.message = "Save a portable bottle folder. Close Windows apps in this bottle first."
        panel.nameFieldStringValue = bottle.name
            .replacingOccurrences(of: "/", with: "-")
            .replacingOccurrences(of: ":", with: "-") + ".caskrun-bottle"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        bottleOperationLabel = "Exporting \(bottle.name)…"
        isManagingBottle = true
        Task {
            defer { isManagingBottle = false; bottleOperationLabel = "Updating bottle…" }
            do {
                let result = try await callAsync(["export", bottle.id, url.path], as: BottleExportResult.self)
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
        guard !isQuitting && !isManagingBottle else { return }
        let panel = NSOpenPanel()
        panel.message = "Choose a .caskrun-bottle backup folder"
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        bottleOperationLabel = "Importing \(url.lastPathComponent)…"
        isManagingBottle = true
        Task {
            defer { isManagingBottle = false; bottleOperationLabel = "Updating bottle…" }
            do {
                let result = try await callAsync(["import", url.path], as: BottleImportResult.self)
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
                if result.profile_account_linked {
                    note += " This Mac account now uses the restored Windows profile, so imported apps keep their settings and data."
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
            let library = try await callAsync(["steam-games", bottle.id], as: SteamLibrary.self)
            guard bottles.contains(where: { $0.id == bottle.id }) else { return }
            steamGames[bottle.id] = library.games
            steamGameErrors[bottle.id] = nil
            if library.truncated { truncatedSteamLibraries.insert(bottle.id) }
            else { truncatedSteamLibraries.remove(bottle.id) }
        } catch {
            if !Task.isCancelled { steamGameErrors[bottle.id] = error.localizedDescription }
        }
    }

    func runSteamGame(_ game: SteamGame, in bottle: Bottle) {
        guard !isQuitting, status.wine_path != nil, !isLaunching, !isManagingBottle else { return }
        isLaunching = true
        Task {
            defer { isLaunching = false }
            do {
                _ = try await callAsync(["run-steam-game", bottle.id, String(game.app_id)], as: LaunchResult.self)
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
            let launcher = try call(["add-app", bottle.id, program.name, program.path], as: SavedLauncher.self)
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
                let saved = Set((launchers[bottle.id] ?? []).map { $0.path.lowercased() })
                manualInstallReview = ManualInstallReview(
                    bottle: bottle,
                    installerName: "",
                    candidates: result.candidates.filter { !saved.contains($0.path.lowercased()) },
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
        guard !isQuitting && !isLaunching && !isManagingBottle else { return }
        isLaunching = true
        Task {
            defer { isLaunching = false }
            do {
                var command = ["run-app", bottle.id, launcher.id]
                if !arguments.isEmpty {
                    command.append("--")
                    command.append(contentsOf: arguments)
                }
                _ = try await callAsync(command, as: LaunchResult.self)
                CaskrunWindowBridge.shared.hideMainWindow()
            } catch {
                message = error.localizedDescription
                onFailure?()
            }
        }
    }

    func runCommand(in bottle: Bottle) {
        guard !isQuitting, status.wine_path != nil, !isLaunching, !isManagingBottle else { return }
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
        if let savedLauncher = (launchers[bottle.id] ?? []).first(where: {
            URL(fileURLWithPath: $0.path).resolvingSymlinksInPath().standardizedFileURL.path == chosenPath
        }) {
            // Saved launchers may carry recipe arguments and runtime behavior,
            // including Steam's CEF flag and Dock identity.
            runLauncher(savedLauncher, in: bottle)
            return
        }
        isLaunching = true
        Task {
            defer { isLaunching = false }
            do {
                _ = try await callAsync(["launch", bottle.id, url.path], as: LaunchResult.self)
                CaskrunWindowBridge.shared.hideMainWindow()
            } catch { message = error.localizedDescription }
        }
    }

    func quitWindowsApps(in bottle: Bottle) {
        guard !isQuitting && !isManagingBottle && !isLaunching else { return }
        isManagingBottle = true
        Task {
            defer { isManagingBottle = false }
            do {
                let result = try await callAsync(["quit-bottle", bottle.id], as: BottleQuitResult.self)
                message = result.message
            } catch { message = error.localizedDescription }
        }
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
                    let knownPaths = Set((before?.candidates ?? []).map { $0.path.lowercased() })
                    let result = try await callAsync(["launch-wait", bottleID, url.path],
                                                     as: WaitLaunchResult.self, cancellation: cancellation)
                    canCancelManualInstaller = false
                    if cancellation.isCancelRequested {
                        message = "The installer finished before Caskrun could cancel it. Select its bottle on Home to inspect it."
                    } else if result.timed_out {
                        message = "The installer did not finish within 30 minutes. Open Logs for \(bottle.name) in its Home bottle panel to investigate."
                    } else if result.exit_code == 0 {
                        let after = try? await callAsync(["discover-apps", bottleID], as: DiscoveredProgramsResult.self)
                        let newPrograms = before == nil ? [] : (after?.candidates ?? []).filter {
                            !knownPaths.contains($0.path.lowercased())
                        }
                        manualInstallReview = ManualInstallReview(bottle: bottle,
                                                                  installerName: url.lastPathComponent,
                                                                  candidates: newPrograms,
                                                                  searchWasLimited: (before?.truncated == true || after?.truncated == true),
                                                                  discoveryUnavailable: before == nil || after == nil)
                    } else if let code = result.exit_code {
                        message = "The installer closed with error code \(code). Open Logs for \(bottle.name) in its Home bottle panel to investigate."
                    } else if let signal = result.signal {
                        message = "The installer stopped unexpectedly (signal \(signal)). Open Logs for \(bottle.name) in its Home bottle panel to investigate."
                    } else {
                        message = "The installer stopped unexpectedly. Open Logs for \(bottle.name) in its Home bottle panel to investigate."
                    }
                } catch {
                    let coreError = error as NSError
                    if coreError.domain == "Caskrun" && coreError.code == 130 {
                        message = "Setup canceled. The bottle remains available on Home."
                    } else if cancellation.isCancelRequested {
                        message = "Setup needs attention after cancellation: \(error.localizedDescription)"
                    } else {
                        message = error.localizedDescription
                    }
                }
            }
        }
    }

    func installCatalog(_ entry: CatalogEntry) {
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
                catalogMonitorGeneration += 1
            }
            do {
                let result = try await callAsync(["install-catalog", entry.id],
                                                 as: CatalogInstallResult.self, cancellation: cancellation)
                canCancelCatalogInstall = false
                refresh()
                if cancellation.isCancelRequested {
                    message = "Setup finished before Caskrun could cancel it. Select the bottle on Home."
                    return
                }
                selectedBottleID = result.bottle.id
                installHandoff = InstallHandoff(entryID: entry.id,
                                                entryName: entry.name,
                                                bottle: result.bottle,
                                                launcher: result.launcher,
                                                setupNote: result.setup_note)
                CaskrunWindowBridge.shared.hideMainWindow()
            } catch {
                canCancelCatalogInstall = false
                refresh()
                let coreError = error as NSError
                if coreError.domain == "Caskrun" && coreError.code == 130 {
                    message = "Setup canceled. If timeout cleanup had already begun, Windows apps in this bottle may also have stopped. Inspect the bottle on Home before retrying."
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
                catalogMonitorGeneration += 1
            }
            do {
                let result = try await callAsync(["retry-catalog", attempt.catalog_id, attempt.bottle_id],
                                                 as: CatalogInstallResult.self, cancellation: cancellation)
                canCancelCatalogInstall = false
                refresh()
                if cancellation.isCancelRequested {
                    message = "Setup finished before Caskrun could cancel it. Select the bottle on Home."
                    return
                }
                selectedBottleID = result.bottle.id
                installHandoff = InstallHandoff(entryID: attempt.catalog_id,
                                                entryName: entryName,
                                                bottle: result.bottle,
                                                launcher: result.launcher,
                                                setupNote: result.setup_note)
                CaskrunWindowBridge.shared.hideMainWindow()
            } catch {
                canCancelCatalogInstall = false
                refresh()
                let coreError = error as NSError
                if coreError.domain == "Caskrun" && coreError.code == 130 {
                    message = "Setup canceled. If timeout cleanup had already begun, Windows apps in this bottle may also have stopped. Inspect the bottle on Home before retrying."
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

private struct HomeProgramTile: View {
    @ObservedObject var model: CaskrunModel
    let item: SavedProgram
    let fallbackSymbol: String
    let isSelected: Bool
    let showBottle: (Int) -> Void
    @State private var isHovered = false

    var body: some View {
        Button {
            showBottle(NSApp.currentEvent?.clickCount ?? 0)
        } label: {
            VStack(spacing: 0) {
                LauncherBadge(model: model, bottle: item.bottle,
                              launcher: item.launcher, fallbackSymbol: fallbackSymbol)
                    .frame(width: 80, height: 56)
                    .padding(.top, 6)
                Text(item.launcher.name)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(isSelected || isHovered ? Color.accentColor : Color.primary)
                    .animation(.easeOut(duration: 0.10), value: isHovered)
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
                    .frame(maxWidth: .infinity)
                    .padding(.top, 8)
                Capsule().fill(isSelected ? Color.accentColor : Color.primary.opacity(isHovered ? 0.16 : 0))
                    .frame(width: 18, height: 3)
                    .animation(.easeOut(duration: 0.10), value: isHovered)
                    .padding(.top, 8)
            }
            .padding(12)
            .frame(maxWidth: .infinity)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .frame(width: 120)
        .help("Click to manage \(item.launcher.name). Double-click to open.")
        .accessibilityLabel("Manage \(item.launcher.name)")
        .accessibilityHint("Opens bottle controls. Double-click to launch the app.")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .onHover { isHovered = $0 }
        .frame(maxWidth: .infinity)
    }
}

private struct RunWithOptionsSheet: View {
    let item: SavedProgram
    let onRun: ([String]) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var argumentLines = ""

    private var arguments: [String] {
        argumentLines.components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
    }

    private var validationMessage: String? {
        let all = (item.launcher.saved_arguments ?? []) + arguments
        if all.count > 64 { return "Saved and temporary arguments must total at most 64." }
        if all.contains(where: { $0.utf8.count > 4096 }) {
            return "Each argument must be 4096 bytes or shorter."
        }
        if all.reduce(0, { $0 + $1.utf8.count }) > 16 * 1024 {
            return "Saved and temporary arguments must total 16 KiB or less."
        }
        return nil
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
        argumentLines.components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
    }

    private var validationMessage: String? {
        if arguments.count > 64 { return "Use at most 64 arguments." }
        if arguments.contains(where: { $0.utf8.count > 4096 }) {
            return "Each argument must be 4096 bytes or shorter."
        }
        if arguments.reduce(0, { $0 + $1.utf8.count }) > 16 * 1024 {
            return "Arguments must total 16 KiB or less."
        }
        return nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Saved arguments for \(item.launcher.name)")
                .font(.title2.weight(.semibold))
            Text("These options are used whenever you open this app from Home or its bottle.")
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

private struct SteamGameTile: View {
    let game: SteamGame
    let isSelected: Bool
    let select: () -> Void
    @State private var hovered = false
    @State private var icon: NSImage?

    var body: some View {
        Button(action: select) {
            VStack(spacing: 8) {
                ZStack {
                    if let icon {
                        Image(nsImage: icon).resizable().interpolation(.high).scaledToFit()
                    } else {
                        Image(systemName: "gamecontroller.fill")
                            .font(.system(size: 30)).foregroundStyle(.secondary)
                    }
                }
                .frame(width: 56, height: 56)
                Text(game.name).font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(isSelected || hovered ? Color.accentColor : Color.primary)
                    .lineLimit(2).multilineTextAlignment(.center)
                Capsule().fill(isSelected ? Color.accentColor : Color.primary.opacity(hovered ? 0.16 : 0))
                    .frame(width: 18, height: 3)
            }
            .padding(12).frame(width: 120).contentShape(Rectangle())
        }
        .buttonStyle(.plain).frame(maxWidth: .infinity)
        .onHover { hovered = $0 }
        .animation(.easeOut(duration: 0.10), value: hovered)
        .help("Click to select \(game.name). Double-click to open through Steam.")
        .accessibilityLabel("Manage \(game.name)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .task(id: game.icon_path) {
            icon = nil
            if let path = game.icon_path {
                let data = await Task.detached(priority: .utility) { () -> Data? in
                    guard let bytes = MacAppArtwork.read(path, maximum: 8 * 1024 * 1024) else { return nil }
                    return MacAppArtwork.png(bytes)
                }.value
                if !Task.isCancelled, let data { icon = NSImage(data: data) }
            }
        }
    }
}

@main
struct CaskrunApp: App {
    @NSApplicationDelegateAdaptor(CaskrunAppDelegate.self) private var appDelegate
    @StateObject private var model = CaskrunModel()
    @StateObject private var catalogArtwork = CatalogArtworkStore()
    @State private var page: Page = .discover
    @State private var didChooseInitialPage = false
    @State private var showingNewBottle = false
    @State private var newBottleName = ""
    @State private var renameTarget: Bottle?
    @State private var deleteTarget: Bottle?
    @State private var quitTarget: Bottle?
    @State private var rebootTarget: Bottle?
    @State private var renameText = ""
    @State private var librarySearch = ""
    @State private var catalogSearch = ""
    @State private var selectedCatalogEntry: CatalogEntry?
    @State private var catalogBottleID = ""
    @State private var catalogUseExistingBottle = false
    @State private var pendingCatalogInstall: (bottleID: String?, appName: String)?
    @State private var pendingManualLauncherBottle: Bottle?
    @State private var logBottle: Bottle?
    @State private var runOptionsItem: SavedProgram?
    @State private var savedArgumentsItem: SavedProgram?
    @State private var hoveredPage: Page?
    @State private var hoveredInstall = false
    @State private var hoveredCatalogID: String?
    @State private var hoveredHomeBottleID: String?
    @State private var showingBottleInspector = false
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

    private var filteredBottles: [Bottle] {
        let query = librarySearch.trimmingCharacters(in: .whitespacesAndNewlines)
        return model.bottles.filter { bottle in
            query.isEmpty || bottle.name.localizedCaseInsensitiveContains(query)
                || (model.launchers[bottle.id] ?? []).contains { $0.name.localizedCaseInsensitiveContains(query) }
        }
    }

    private var openBottle: Bottle? { model.bottles.first { $0.id == openBottleID } }

    private var selectedSteamGame: SteamGame? {
        guard let bottle = openBottle else { return nil }
        return model.steamGames[bottle.id]?.first { $0.id == selectedSteamGameID }
    }

    private var filteredSteamGames: [SteamGame] {
        guard let bottle = openBottle else { return [] }
        let games = model.steamGames[bottle.id] ?? []
        let query = librarySearch.trimmingCharacters(in: .whitespacesAndNewlines)
        return query.isEmpty ? games : games.filter { $0.name.localizedCaseInsensitiveContains(query) }
    }

    private func launchHomeProgram(id: String) {
        guard page == .home, showingBottleInspector,
              let item = allSavedPrograms.first(where: { $0.id == id }),
              openBottleID == item.bottle.id,
              !model.isLaunching, !model.isManagingBottle, model.status.wine_path != nil else { return }
        inspectBottle(item.bottle, launcher: item.launcher)
        model.runLauncher(item.launcher, in: item.bottle)
    }

    private func selectHomeProgram(_ item: SavedProgram, clicks: Int) {
        let event = NSApp.currentEvent
        inspectBottle(item.bottle, launcher: item.launcher)
        if clicks == 1 {
            CaskrunHomeClickRouter.shared.remember(event) { launchHomeProgram(id: item.id) }
        }
    }

    private func selectSteamGame(_ game: SteamGame, in bottle: Bottle) {
        let event = NSApp.currentEvent
        inspectBottle(bottle)
        selectedSteamGameID = game.id
        CaskrunHomeClickRouter.shared.remember(event) {
            guard page == .home, openBottleID == bottle.id,
                  let current = model.steamGames[bottle.id]?.first(where: { $0.id == game.id }) else { return }
            model.runSteamGame(current, in: bottle)
        }
    }

    private func inspectBottle(_ bottle: Bottle, launcher: SavedLauncher? = nil) {
        CaskrunHomeClickRouter.shared.cancel()
        model.selectedBottleID = bottle.id
        openBottleID = bottle.id
        selectedLauncherID = launcher?.id
        selectedSteamGameID = nil
        page = .home
        showingBottleInspector = true
    }

    private func openBottleManager() {
        CaskrunHomeClickRouter.shared.cancel()
        if let bottle = selectedBottle ?? model.bottles.first { inspectBottle(bottle) }
        page = .home
        showingBottleInspector = true
        showBottleManagement = true
    }

    private var savedPrograms: [SavedProgram] {
        allSavedPrograms.filter { !$0.launcher.hidden_from_home && $0.bottle.id == openBottleID }
    }

    private var filteredSavedPrograms: [SavedProgram] {
        let query = librarySearch.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return savedPrograms }
        return savedPrograms.filter {
            $0.launcher.name.localizedCaseInsensitiveContains(query)
                || $0.bottle.name.localizedCaseInsensitiveContains(query)
        }
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
            model.message = "The saved setup record could not be loaded. Select its bottle on Home to inspect this install."
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
            return "Windows apps may still be running in this bottle. Quit them from its Home bottle panel, then retry setup."
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
                case .home: homeView
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
                                        model: model, delegate: appDelegate))
        .onAppear {
            model.refresh()
            if !didChooseInitialPage {
                page = !model.bottles.isEmpty ||
                    !model.catalogAttempts.isEmpty ? .home : .discover
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
            } else if showingBottleInspector, model.bottles.contains(where: { $0.id == current }) {
                openBottleID = current
                selectedSteamGameID = nil
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
        .onChange(of: openBottleID) { _, _ in
            CaskrunHomeClickRouter.shared.cancel()
            selectedSteamGameID = nil
            librarySearch = ""
        }
        .task(id: page == .home ? openBottleID : nil) {
            guard page == .home, let bottle = openBottle else { return }
            repeat {
                await model.refreshSteamGames(in: bottle)
                do { try await Task.sleep(for: .seconds(15)) }
                catch { return }
            } while !Task.isCancelled
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            if page == .home, let bottle = openBottle {
                Task { await model.refreshSteamGames(in: bottle) }
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
            .alert("Quit Windows apps in this bottle?", isPresented: Binding(
                get: { quitTarget != nil },
                set: { if !$0 { quitTarget = nil } }
            )) {
                Button("Quit Windows Apps", role: .destructive) {
                    if let bottle = quitTarget { model.quitWindowsApps(in: bottle) }
                    quitTarget = nil
                }
                Button("Cancel", role: .cancel) { quitTarget = nil }
            } message: {
                Text("All Windows apps in \(quitTarget?.name ?? "this bottle") will close. Unsaved work in those apps may be lost.")
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
        }
        .defaultSize(width: 1000, height: 680)
        .commands {
            CommandGroup(replacing: .appSettings) {}
            CommandGroup(replacing: .newItem) {
                Button("Install from file…") { model.chooseInstaller(in: page == .home ? openBottleID : nil) }
                    .keyboardShortcut("n", modifiers: .command)
                    .disabled(model.isQuitting || model.status.wine_path == nil || model.isLaunching || model.isManagingBottle)
            }
            CommandGroup(after: .textEditing) {
                Button("Find Apps") {
                    if page == .home && model.bottles.isEmpty {
                        page = .discover
                    }
                    showingSearch = true
                    focusedSearch = page == .discover ? .catalog : .library
                }
                .keyboardShortcut("f", modifiers: .command)
                .disabled(model.isQuitting)
                Button("Manage Bottles") { openBottleManager() }
                    .keyboardShortcut("3", modifiers: .command)
                    .disabled(model.isQuitting)
                Button("Open Selected App") {
                    if let game = selectedSteamGame, let bottle = openBottle { model.runSteamGame(game, in: bottle) }
                    else if let item = inspectedProgram { model.runLauncher(item.launcher, in: item.bottle) }
                }
                .keyboardShortcut("o", modifiers: .command)
                .disabled(model.isQuitting || page != .home || !showingBottleInspector || (inspectedProgram == nil && selectedSteamGame == nil) ||
                          model.isLaunching || model.isManagingBottle || model.status.wine_path == nil)
            }
        }
    }

    private func navigate(to target: Page) {
        CaskrunHomeClickRouter.shared.cancel()
        librarySearch = ""
        catalogSearch = ""
        showingSearch = false
        focusedSearch = nil
        if target == .home {
            openBottleID = nil
            selectedSteamGameID = nil
            showingBottleInspector = false
        }
        page = target
    }

    private var topBar: some View {
        HStack(spacing: 4) {
            navigationButton(.home, title: "Home", symbol: "house", shortcut: "1")
            navigationButton(.discover, title: "Discover", symbol: "square.grid.2x2", shortcut: "2")
            Rectangle().fill(.primary.opacity(0.12))
                .frame(width: 1, height: 18).padding(.horizontal, 5)
            Menu {
                Button("Discover apps") { navigate(to: .discover) }
                Button("Install from file…") { model.chooseInstaller(in: page == .home ? openBottleID : nil) }
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
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 13, weight: .semibold))
                    .frame(width: 34, height: 34)
                    .background(.primary.opacity(hoveredInstall ? 0.13 : 0.065), in: Circle())
            }
            .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
            .help("Add apps or import a bottle")
            .accessibilityLabel("Add apps or import a bottle")
            .onHover { hoveredInstall = $0 }
            .animation(.easeOut(duration: 0.12), value: hoveredInstall)
            if showingSearch {
                searchField(page == .home ? "Search bottles and apps" : "Search stores and apps",
                            text: page == .home ? $librarySearch : $catalogSearch,
                            field: page == .home ? .library : .catalog)
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

    private func showAllBottles() {
        CaskrunHomeClickRouter.shared.cancel()
        openBottleID = nil
        selectedSteamGameID = nil
        showingBottleInspector = false
        librarySearch = ""
    }

    private var homeView: some View {
        HStack(alignment: .top, spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    runtimeBanner
                    recoveryPanel
                    if let bottle = openBottle {
                        HStack(spacing: 8) {
                            Button(action: showAllBottles) {
                                Label("All bottles", systemImage: "chevron.left")
                                    .font(.system(size: 12, weight: .medium))
                            }
                            .buttonStyle(.borderless)
                            Image(systemName: "chevron.right").font(.system(size: 9)).foregroundStyle(.tertiary)
                            Text(bottle.name).font(.system(size: 12, weight: .semibold)).lineLimit(1)
                            Spacer()
                        }
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 120, maximum: 144), spacing: 12)],
                                  alignment: .leading, spacing: 12) {
                            ForEach(filteredSavedPrograms) { item in
                                HomeProgramTile(model: model, item: item,
                                                fallbackSymbol: launcherSymbol(item.launcher.name),
                                                isSelected: showingBottleInspector && selectedSteamGameID == nil && inspectedProgram?.id == item.id,
                                                showBottle: { selectHomeProgram(item, clicks: $0) })
                            }
                            ForEach(filteredSteamGames) { game in
                                SteamGameTile(game: game, isSelected: selectedSteamGameID == game.id) {
                                    selectSteamGame(game, in: bottle)
                                }
                            }
                        }
                        if filteredSavedPrograms.isEmpty && filteredSteamGames.isEmpty {
                            if !librarySearch.isEmpty {
                                ContentUnavailableView.search(text: librarySearch)
                            } else if model.loadingSteamGames.contains(bottle.id) && model.steamGames[bottle.id] == nil {
                                ProgressView().controlSize(.small).frame(maxWidth: .infinity, minHeight: 80)
                            } else {
                                VStack(alignment: .leading, spacing: 6) {
                                    Text("No apps to show").font(.headline)
                                    Text((model.launchers[bottle.id] ?? []).isEmpty
                                         ? "Add an app using the controls. Installed Steam games appear here automatically."
                                         : "Use App Options in the controls to show hidden apps again.")
                                        .font(.caption).foregroundStyle(.secondary)
                                }
                                .padding(.vertical, 18)
                            }
                        }
                        if let error = model.steamGameErrors[bottle.id] {
                            Label("Steam library could not be read: \(error)", systemImage: "exclamationmark.triangle")
                                .font(.caption).foregroundStyle(.orange)
                        }
                        if model.truncatedSteamLibraries.contains(bottle.id) {
                            Text("Some Steam library entries could not be listed.").font(.caption).foregroundStyle(.secondary)
                        }
                    } else if model.bottles.isEmpty {
                        homeEmptyState
                    } else {
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 120, maximum: 144), spacing: 12)],
                                  alignment: .leading, spacing: 12) {
                            ForEach(filteredBottles) { bottle in homeBottleTile(bottle) }
                        }
                        if !librarySearch.isEmpty && filteredBottles.isEmpty {
                            ContentUnavailableView.search(text: librarySearch)
                        }
                    }
                }
                .padding(24)
            }
            .frame(maxWidth: .infinity)
            if showingBottleInspector {
                bottleInspector.frame(width: 320)
                    .padding(.top, 20).padding(.trailing, 16).padding(.bottom, 16)
                    .transition(.identity)
            }
        }
        .onDisappear { CaskrunHomeClickRouter.shared.cancel() }
    }

    private func homeBottleTile(_ bottle: Bottle) -> some View {
        let hovered = hoveredHomeBottleID == bottle.id
        return Button { inspectBottle(bottle) } label: {
            VStack(spacing: 8) {
                ZStack(alignment: .center) {
                    Image(systemName: "folder.fill")
                        .resizable().scaledToFit().foregroundStyle(Color.accentColor.opacity(0.72))
                        .frame(width: 76, height: 60)
                    BottleArtwork(model: model, bottle: bottle, size: 28).offset(y: 7)
                }
                .frame(width: 80, height: 64)
                Text(bottle.name).font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(hovered ? Color.accentColor : Color.primary)
                    .lineLimit(2).multilineTextAlignment(.center)
                    .animation(.easeOut(duration: 0.10), value: hovered)
            }
            .padding(12).frame(width: 120).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help("Open \(bottle.name) to see its apps and controls")
        .accessibilityLabel("Open bottle \(bottle.name)")
        .onHover { hoveredHomeBottleID = $0 ? bottle.id : nil }
        .frame(maxWidth: .infinity)
    }

    private var homeEmptyState: some View {
        VStack(alignment: .leading, spacing: 14) {
            Image(systemName: "square.stack.3d.up")
                .font(.system(size: 24, weight: .medium)).foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 5) {
                Text("Your apps will appear here").font(.title3.weight(.semibold))
                Text("Choose a store in Discover or install your own Windows app.")
                    .font(.subheadline).foregroundStyle(.secondary)
            }
            HStack {
                Button("Browse stores") { page = .discover }.buttonStyle(.borderedProminent)
                Button("Install from file…") { model.chooseInstaller() }
                    .disabled(model.status.wine_path == nil || model.isLaunching || model.isManagingBottle)
            }
        }
        .padding(22)
        .frame(maxWidth: .infinity, minHeight: 160, alignment: .leading)
        .background(.quaternary.opacity(0.45), in: RoundedRectangle(cornerRadius: 18))
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

    private var discoverView: some View {
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
                bottlePanelLabel("BOTTLE CONTROLS")
                Spacer()
                Button(action: showAllBottles) {
                    Image(systemName: "xmark").font(.system(size: 11, weight: .semibold))
                        .frame(width: 28, height: 28).contentShape(Rectangle())
                }
                .buttonStyle(InspectorCloseButtonStyle())
                .keyboardShortcut(.escape, modifiers: [])
                .help("Close bottle and return to Home (Esc)")
                .accessibilityLabel("Close bottle and return to Home")
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
                        recentlyDeletedBottles
                    }
                    .padding(18)
                }
                .onChange(of: selectedSteamGame?.id ?? inspectedProgram?.id ?? selectedBottle?.id) { _, _ in
                    proxy.scrollTo("inspector-top", anchor: .top)
                }
            }
        }
        .tint(.primary)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .strokeBorder(.primary.opacity(0.10))
        }
        .shadow(color: .black.opacity(0.10), radius: 12, y: 5)
        .task(id: selectedBottle?.id) {
            if let bottle = selectedBottle { model.refreshBottleResolution(in: bottle) }
        }
    }

    private func inspectorHeader(for bottle: Bottle) -> some View {
        HStack(spacing: 12) {
            BottleArtwork(model: model, bottle: bottle)
            VStack(alignment: .leading, spacing: 4) {
                Text(bottle.name).font(.system(size: 19, weight: .semibold, design: .rounded))
                    .lineLimit(2)
                let count = (model.launchers[bottle.id] ?? []).count
                let games = (model.steamGames[bottle.id] ?? []).count
                Text("\(count) \(count == 1 ? "app" : "apps")" + (games == 0 ? "" : " · \(games) \(games == 1 ? "game" : "games")"))
                    .font(.system(size: 11)).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
    }

    private func inspectorApps(for bottle: Bottle) -> some View {
        let apps = model.launchers[bottle.id] ?? []
        let games = model.steamGames[bottle.id] ?? []
        let busy = model.isManagingBottle || model.isLaunching
        return VStack(alignment: .leading, spacing: 12) {
            if let item = inspectedProgram {
                if apps.count + games.count > 1 {
                    Picker("App", selection: Binding(
                        get: { selectedSteamGame.map { "game:" + $0.id } ?? "app:" + (inspectedProgram?.launcher.id ?? "") },
                        set: { value in
                            CaskrunHomeClickRouter.shared.cancel()
                            if value.hasPrefix("game:") { selectedSteamGameID = String(value.dropFirst(5)) }
                            else { selectedLauncherID = String(value.dropFirst(4)); selectedSteamGameID = nil }
                        }
                    )) {
                        ForEach(apps) { app in
                            Text(app.name + (app.hidden_from_home ? " (hidden)" : "")).tag("app:" + app.id)
                        }
                        ForEach(games) { Text($0.name).tag("game:" + $0.id) }
                    }
                }
                Button {
                    if let game = selectedSteamGame { model.runSteamGame(game, in: bottle) }
                    else { model.runLauncher(item.launcher, in: bottle) }
                } label: {
                    Label("Open \(selectedSteamGame?.name ?? item.launcher.name)", systemImage: "play.fill")
                        .font(.system(size: 13, weight: .semibold))
                        .frame(maxWidth: .infinity).frame(height: 30)
                }
                .buttonStyle(.borderedProminent).tint(.accentColor)
                .disabled(busy || model.status.wine_path == nil)
                if selectedSteamGame != nil {
                    Text("Opens through Steam in this bottle.").font(.caption).foregroundStyle(.secondary)
                } else {
                DisclosureGroup(isExpanded: $showLaunchOptions) {
                    VStack(spacing: 6) {
                        bottleAction("Run with Options…", symbol: "play.circle") { runOptionsItem = item }
                        bottleAction("Edit Saved Arguments…", symbol: "text.alignleft") { savedArgumentsItem = item }
                        bottleAction(item.launcher.hidden_from_home ? "Show on Home" : "Hide from Home",
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
                Button { model.runSteamGame(game, in: bottle) } label: {
                    Label("Open " + game.name, systemImage: "play.fill")
                        .font(.system(size: 13, weight: .semibold))
                        .frame(maxWidth: .infinity).frame(height: 30)
                }
                .buttonStyle(.borderedProminent).tint(.accentColor)
                .disabled(busy || model.status.wine_path == nil)
                Text("Add Steam from this bottle with Find installed apps to open its games.")
                    .font(.caption).foregroundStyle(.secondary)
            } else {
                Text("No saved apps yet").font(.system(size: 13, weight: .semibold))
                Text("Install an app or add a program already in this bottle.")
                    .font(.caption).foregroundStyle(.secondary)
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

    private func inspectorManagement(for bottle: Bottle) -> some View {
        VStack(spacing: 6) {
            bottleAction("Rename…", symbol: "pencil") { renameText = bottle.name; renameTarget = bottle }
            bottleAction("Duplicate Bottle", symbol: "square.on.square") { model.duplicateBottle(bottle) }
            bottleAction("Export Backup…", symbol: "square.and.arrow.up") { model.exportBottle(bottle) }
            bottleAction("Move to Recently Deleted…", symbol: "trash") { deleteTarget = bottle }
        }
        .disabled(model.isManagingBottle || model.isLaunching)
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

    private func bottleActionsPanel(for bottle: Bottle) -> some View {
        let busy = model.isLaunching || model.isManagingBottle
        let runtimeMissing = model.status.wine_path == nil
        return VStack(alignment: .leading, spacing: 14) {
            DisclosureGroup(isExpanded: $showBottleDisplay) {
                VStack(alignment: .leading, spacing: 16) {
                    bottleDisplayPanel(for: bottle)
                    bottleGraphicsPanel
                }
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
                    bottleAction("Quit Windows apps…", symbol: "xmark.circle") { quitTarget = bottle }
                        .disabled(busy || runtimeMissing)
                    bottleAction("View Logs", symbol: "doc.text") { logBottle = bottle }
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

    private var bottleGraphicsPanel: some View {
        VStack(alignment: .leading, spacing: 9) {
            bottlePanelLabel("GRAPHICS")
            HStack {
                Text("Wine (WineD3D)").font(.subheadline.weight(.medium))
                Spacer()
                Text("Default").font(.caption).foregroundStyle(.secondary)
            }
            Text("Game compatibility varies by app.")
                .font(.caption).foregroundStyle(.secondary)
            DisclosureGroup("Other graphics & sync options") {
                ForEach(["DXVK", "DXMT", "D3DMetal", "MSync"], id: \.self) { name in
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
                Text("These bottles remain on disk until permanent removal is added. Restore one to use it again.")
                    .font(.caption).foregroundStyle(.secondary)
                ForEach(model.deletedBottles) { bottle in
                    HStack {
                        Image(systemName: "shippingbox")
                        Text(bottle.name)
                        Spacer()
                        Button("Restore") { model.restoreDeletedBottle(bottle) }
                            .disabled(model.isManagingBottle || model.isLaunching)
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
                Text("Closing Windows apps…").font(.system(size: 12, weight: .medium))
                Spacer()
            }
            .padding(.horizontal, 24).padding(.vertical, 10)
            .background(Color.accentColor.opacity(0.06))
        } else if model.isLaunching {
            HStack(spacing: 12) {
                ProgressView().controlSize(.small)
                VStack(alignment: .leading, spacing: 2) {
                    Text(model.activeCatalogInstall.map { "Setting up \($0)" }
                         ?? model.activeInstallerName.map { "Running \($0)" }
                         ?? "Opening your app")
                        .font(.subheadline.weight(.semibold))
                    Text(model.activeCatalogInstall == nil
                         ? (model.isCancellingManualInstaller
                            ? "Stopping this setup. Caskrun will update the result shortly."
                            : "Caskrun will update this view when the program responds.")
                         : (model.isCancellingCatalogInstall
                            ? "Stopping this setup. Caskrun will update the result shortly."
                            : catalogProgressText))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if model.canCancelManualInstaller && model.activeCatalogInstall == nil {
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
                         ? "\(completion.entryName) is ready in Home"
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
                 ? "Choose a program found in this bottle to add to Home. You can open it from the bottle panel on Home."
                 : "\(review.installerName) closed. Choose a new program to add to Home, once the publisher's setup has finished.")
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
                                Button("Add to Home") {
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
            Text("Caskrun has not verified this app's compatibility. Installer logs are available in the bottle panel on Home.")
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
