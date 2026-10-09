import AppKit
import CryptoKit
import Foundation
import SwiftUI

struct BrowserApplication: Identifiable, Equatable {
    let bundleIdentifier: String
    let name: String
    let applicationURL: URL
    var id: String { bundleIdentifier }
}

enum BrowserLaunchChoice: Equatable {
    case systemDefault
    case application(bundleIdentifier: String)
}

enum BrowserLaunchFailure: LocalizedError, Equatable {
    case invalidGameURL
    case invalidBrowserIdentifier
    case browserUnavailable
    case couldNotOpen(browserName: String)

    var errorDescription: String? {
        switch self {
        case .invalidGameURL: "This game needs a valid public HTTPS address."
        case .invalidBrowserIdentifier: "The saved browser choice is invalid. Choose a browser again."
        case .browserUnavailable: "Your chosen browser is no longer available. Choose another browser before playing."
        case .couldNotOpen(let browserName): "macOS could not open this game in \(browserName)."
        }
    }
}

enum BrowserLaunchRequest: Equatable {
    case systemDefault(URL)
    case application(URL, BrowserApplication)
}

/// Preferences hold only a public game URL's hash and an app identifier, never
/// the URL itself or the selected app's location on this Mac.
enum BrowserLaunchPolicy {
    /// HTTPS registration alone also includes apps which intercept web links
    /// without browsing pages. Require a declared HTML viewing role as well.
    static func isBrowserApplication(_ information: [String: Any]) -> Bool {
        let urlTypes = information["CFBundleURLTypes"] as? [[String: Any]] ?? []
        let schemes = urlTypes.flatMap { $0["CFBundleURLSchemes"] as? [String] ?? [] }
        guard schemes.contains(where: { $0.lowercased() == "https" }) else { return false }
        let documentTypes = information["CFBundleDocumentTypes"] as? [[String: Any]] ?? []
        return documentTypes.contains { document in
            guard (document["CFBundleTypeRole"] as? String)?.lowercased() == "viewer" else { return false }
            // Apple gives UTIs precedence when this key is present. Do not
            // accept an ignored HTML extension beside an unrelated UTI.
            if document["LSItemContentTypes"] != nil {
                return (document["LSItemContentTypes"] as? [String] ?? []).contains("public.html")
            }
            let extensions = document["CFBundleTypeExtensions"] as? [String] ?? []
            if extensions.contains(where: {
                let value = $0.lowercased()
                return ["html", "htm", ".html", ".htm"].contains(value)
            }) { return true }
            let mimeTypes = document["CFBundleTypeMIMETypes"] as? [String] ?? []
            return mimeTypes.contains(where: { $0.lowercased() == "text/html" })
        }
    }

    static func validBundleIdentifier(_ identifier: String) -> Bool {
        guard !identifier.isEmpty, identifier.utf8.count <= 255 else { return false }
        let components = identifier.split(separator: ".", omittingEmptySubsequences: false)
        return components.count > 1 && components.allSatisfy { component in
            !component.isEmpty && component.utf8.allSatisfy {
                (65...90).contains($0) || (97...122).contains($0)
                    || (48...57).contains($0) || $0 == 45
            }
        }
    }

    static func preferenceKey(for gameURL: String) -> String? {
        guard let url = BrowserGameLink.validatedURL(gameURL),
              var components = URLComponents(url: url, resolvingAgainstBaseURL: false) else { return nil }
        components.scheme = "https"
        components.host = components.host?.lowercased()
        if components.percentEncodedPath.isEmpty { components.percentEncodedPath = "/" }
        guard let canonical = components.string else { return nil }
        let digest = SHA256.hash(data: Data(canonical.utf8)).map { String(format: "%02x", $0) }.joined()
        return "browserGameBrowser.v1.\(digest)"
    }

    static func applications(_ candidates: [BrowserApplication]) -> [BrowserApplication] {
        // Discovery supplies macOS's preferred copy of each app first. Retain
        // that copy when another installation has the same bundle identifier.
        var seen = Set<String>()
        return candidates.filter {
            validBundleIdentifier($0.bundleIdentifier) && $0.applicationURL.isFileURL
                && $0.applicationURL.pathExtension.lowercased() == "app"
                && !$0.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                && seen.insert($0.bundleIdentifier).inserted
        }.sorted {
            let order = $0.name.localizedStandardCompare($1.name)
            return order == .orderedSame ? $0.bundleIdentifier < $1.bundleIdentifier : order == .orderedAscending
        }
    }

    static func request(gameURL: String, choice: BrowserLaunchChoice,
                        applications: [BrowserApplication]) throws -> BrowserLaunchRequest {
        guard let url = BrowserGameLink.validatedURL(gameURL) else { throw BrowserLaunchFailure.invalidGameURL }
        switch choice {
        case .systemDefault: return .systemDefault(url)
        case .application(let identifier):
            guard validBundleIdentifier(identifier) else { throw BrowserLaunchFailure.invalidBrowserIdentifier }
            guard let app = self.applications(applications).first(where: { $0.bundleIdentifier == identifier }) else {
                throw BrowserLaunchFailure.browserUnavailable
            }
            return .application(url, app)
        }
    }
}

struct BrowserSelectionStore {
    let defaults: UserDefaults

    func choice(for gameURL: String) -> BrowserLaunchChoice {
        guard let key = BrowserLaunchPolicy.preferenceKey(for: gameURL),
              let identifier = defaults.string(forKey: key) else { return .systemDefault }
        // Keep a stale or malformed selection visible so launch reports it.
        // Never silently open a different browser than the user selected.
        return .application(bundleIdentifier: identifier)
    }

    func set(_ choice: BrowserLaunchChoice, for gameURL: String) throws {
        guard let key = BrowserLaunchPolicy.preferenceKey(for: gameURL) else { throw BrowserLaunchFailure.invalidGameURL }
        switch choice {
        case .systemDefault: defaults.removeObject(forKey: key)
        case .application(let identifier):
            guard BrowserLaunchPolicy.validBundleIdentifier(identifier) else {
                throw BrowserLaunchFailure.invalidBrowserIdentifier
            }
            defaults.set(identifier, forKey: key)
        }
    }

    func remove(for gameURL: String) {
        guard let key = BrowserLaunchPolicy.preferenceKey(for: gameURL) else { return }
        defaults.removeObject(forKey: key)
    }
}

/// The injected environment lets selection and launch routing be verified
/// without inspecting personal app installations or opening a browser.
@MainActor
struct BrowserLaunchEnvironment {
    let applications: @MainActor (URL) -> [BrowserApplication]
    let defaultBrowserName: @MainActor (URL) -> String?
    let openDefault: @MainActor (URL) -> Bool
    let openApplication: @MainActor (URL, BrowserApplication) async -> Bool

    static var system: BrowserLaunchEnvironment {
        let workspace = NSWorkspace.shared
        return BrowserLaunchEnvironment(
            applications: { url in
                let candidates = workspace.urlsForApplications(toOpen: url)
                let preferred = candidates.compactMap { Bundle(url: $0)?.bundleIdentifier }
                    .compactMap { workspace.urlForApplication(withBundleIdentifier: $0) }
                return (preferred + candidates).compactMap(application(at:))
            },
            defaultBrowserName: { url in workspace.urlForApplication(toOpen: url).flatMap(application(at:))?.name },
            openDefault: { workspace.open($0) },
            openApplication: { url, application in
                let configuration = NSWorkspace.OpenConfiguration()
                configuration.activates = true
                configuration.createsNewApplicationInstance = false
                return await withCheckedContinuation { continuation in
                    workspace.open([url], withApplicationAt: application.applicationURL, configuration: configuration) { app, error in
                        continuation.resume(returning: app != nil && error == nil)
                    }
                }
            }
        )
    }

    private static func application(at url: URL) -> BrowserApplication? {
        guard url.isFileURL, url.pathExtension.lowercased() == "app",
              (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true,
              let bundle = Bundle(url: url), let identifier = bundle.bundleIdentifier,
              identifier != Bundle.main.bundleIdentifier,
              BrowserLaunchPolicy.validBundleIdentifier(identifier),
              let executable = bundle.executableURL, FileManager.default.isExecutableFile(atPath: executable.path) else { return nil }
        guard BrowserLaunchPolicy.isBrowserApplication(bundle.infoDictionary ?? [:]) else { return nil }
        let rawName = bundle.object(forInfoDictionaryKey: "CFBundleDisplayName") as? String
            ?? bundle.object(forInfoDictionaryKey: "CFBundleName") as? String
            ?? url.deletingPathExtension().lastPathComponent
        let name = String(rawName.unicodeScalars.filter { !CharacterSet.controlCharacters.contains($0) }.prefix(100))
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return nil }
        return BrowserApplication(bundleIdentifier: identifier, name: name, applicationURL: url)
    }
}

@MainActor
final class BrowserLaunchService: ObservableObject {
    static let shared = BrowserLaunchService()
    @Published private(set) var applications: [BrowserApplication] = []
    @Published private(set) var defaultBrowserName: String?
    @Published private(set) var selectionRevision = 0
    private let store: BrowserSelectionStore
    private let environment: BrowserLaunchEnvironment

    init(defaults: UserDefaults = .standard, environment: BrowserLaunchEnvironment? = nil) {
        store = BrowserSelectionStore(defaults: defaults)
        self.environment = environment ?? .system
    }

    func refresh(for gameURL: String) {
        guard let url = BrowserGameLink.validatedURL(gameURL) else {
            applications = []; defaultBrowserName = nil
            return
        }
        applications = BrowserLaunchPolicy.applications(environment.applications(url))
        defaultBrowserName = environment.defaultBrowserName(url)
    }

    func choice(for gameURL: String) -> BrowserLaunchChoice { store.choice(for: gameURL) }

    func select(_ choice: BrowserLaunchChoice, for gameURL: String) throws {
        try store.set(choice, for: gameURL)
        selectionRevision &+= 1
    }

    func selectedName(for gameURL: String) -> String {
        switch choice(for: gameURL) {
        case .systemDefault: return "Default browser"
        case .application(let identifier):
            return applications.first(where: { $0.bundleIdentifier == identifier })?.name ?? "Browser unavailable"
        }
    }

    func removePreference(for gameURL: String) {
        store.remove(for: gameURL)
        selectionRevision &+= 1
    }

    func open(_ gameURL: String) async throws {
        // Re-discover at the point of launch, including whether the selected
        // app still exists and still declares HTTPS and HTML viewing support.
        refresh(for: gameURL)
        switch try BrowserLaunchPolicy.request(gameURL: gameURL, choice: choice(for: gameURL), applications: applications) {
        case .systemDefault(let url):
            guard environment.openDefault(url) else { throw BrowserLaunchFailure.couldNotOpen(browserName: "your default browser") }
        case .application(let url, let application):
            let opened = await environment.openApplication(url, application)
            guard opened else { throw BrowserLaunchFailure.couldNotOpen(browserName: application.name) }
        }
    }
}

struct BrowserChoiceMenu: View {
    let gameURL: String
    @ObservedObject private var service: BrowserLaunchService
    @State private var selectionError: String?

    @MainActor
    init(gameURL: String, service: BrowserLaunchService? = nil) {
        self.gameURL = gameURL
        self.service = service ?? .shared
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text("Browser").font(.system(size: 11, weight: .medium)).foregroundStyle(.secondary)
            Menu {
                Button { select(.systemDefault) } label: {
                    choiceLabel("Default browser", choice: .systemDefault)
                }
                if let name = service.defaultBrowserName {
                    Text("Currently \(name)")
                }
                if !service.applications.isEmpty {
                    Divider()
                    ForEach(service.applications) { application in
                        Button { select(.application(bundleIdentifier: application.bundleIdentifier)) } label: {
                            choiceLabel(application.name, choice: .application(bundleIdentifier: application.bundleIdentifier))
                        }
                    }
                }
            } label: {
                Label(service.selectedName(for: gameURL), systemImage: "globe")
            }
            .font(.system(size: 12, weight: .medium))
            .menuStyle(.borderlessButton).menuIndicator(.visible)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 12).padding(.vertical, 10)
            .background(.primary.opacity(0.045), in: RoundedRectangle(cornerRadius: 10))
            .contentShape(RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(.primary.opacity(0.06), lineWidth: 1).allowsHitTesting(false))
            .accessibilityLabel("Browser for this game")
            .help("Choose which browser opens this game. This choice is remembered for this game only.")
            .disabled(BrowserGameLink.validatedURL(gameURL) == nil)
            if let selectionError {
                Text(selectionError).font(.caption).foregroundStyle(.secondary)
            }
        }
        .onAppear { service.refresh(for: gameURL) }
        .onChange(of: gameURL) { _, newValue in selectionError = nil; service.refresh(for: newValue) }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            service.refresh(for: gameURL)
        }
    }

    @ViewBuilder
    private func choiceLabel(_ name: String, choice: BrowserLaunchChoice) -> some View {
        if service.choice(for: gameURL) == choice { Label(name, systemImage: "checkmark") }
        else { Text(name) }
    }

    private func select(_ choice: BrowserLaunchChoice) {
        do { try service.select(choice, for: gameURL); selectionError = nil }
        catch { selectionError = error.localizedDescription }
    }
}
