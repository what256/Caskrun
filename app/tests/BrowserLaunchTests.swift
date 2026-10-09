import AppKit
import Foundation

@main
struct BrowserLaunchTests {
    @MainActor
    static func main() async throws {
        var checks = 0
        func expect(_ value: Bool, _ message: String) {
            precondition(value, message)
            checks += 1
        }
        func metadata(schemes: [String] = ["http", "https"], documents: [[String: Any]]) -> [String: Any] {
            ["CFBundleURLTypes": [["CFBundleURLSchemes": schemes]], "CFBundleDocumentTypes": documents]
        }
        let htmlViewer: [String: Any] = ["CFBundleTypeRole": "Viewer", "LSItemContentTypes": ["public.html"]]
        expect(BrowserLaunchPolicy.isBrowserApplication(metadata(documents: [htmlViewer])),
               "HTTPS with an HTML UTI Viewer is a browser")
        expect(!BrowserLaunchPolicy.isBrowserApplication(metadata(documents: [])),
               "a general HTTP and HTTPS handler without HTML document support is not a browser")
        for fileExtension in ["html", "htm", "HTML", ".html", ".htm"] {
            expect(BrowserLaunchPolicy.isBrowserApplication(metadata(documents: [
                ["CFBundleTypeRole": "Viewer", "CFBundleTypeExtensions": [fileExtension]]
            ])), "HTML extension Viewer with HTTPS is a browser")
        }
        expect(BrowserLaunchPolicy.isBrowserApplication(metadata(documents: [
            ["CFBundleTypeRole": "Viewer", "CFBundleTypeMIMETypes": ["text/html"]]
        ])), "HTML MIME Viewer with HTTPS is a browser")
        expect(!BrowserLaunchPolicy.isBrowserApplication(metadata(schemes: ["http"], documents: [htmlViewer])),
               "HTML viewer without HTTPS support is not offered")
        expect(!BrowserLaunchPolicy.isBrowserApplication(metadata(schemes: ["other"], documents: [htmlViewer])),
               "custom-scheme HTML viewer is not offered")
        for role in ["Editor", "Shell", "None", ""] {
            expect(!BrowserLaunchPolicy.isBrowserApplication(metadata(documents: [
                ["CFBundleTypeRole": role, "LSItemContentTypes": ["public.html"]]
            ])), "HTML editing or shell role alone is not browser viewing support")
        }
        expect(!BrowserLaunchPolicy.isBrowserApplication(metadata(documents: [
            ["CFBundleTypeRole": "Viewer", "CFBundleTypeExtensions": ["txt", "*"]]
        ])), "non-HTML document types and a wildcard do not qualify as HTML viewing support")
        expect(!BrowserLaunchPolicy.isBrowserApplication(metadata(documents: [
            ["CFBundleTypeRole": "Viewer", "LSItemContentTypes": ["public.plain-text"], "CFBundleTypeExtensions": ["html"]]
        ])), "an unrelated UTI overrides the ignored legacy HTML extension")
        expect(!BrowserLaunchPolicy.isBrowserApplication(metadata(documents: [
            ["CFBundleTypeRole": "Viewer", "LSItemContentTypes": "public.html", "CFBundleTypeMIMETypes": ["text/html"]]
        ])), "malformed content type metadata never qualifies through ignored MIME fallback")
        expect(!BrowserLaunchPolicy.isBrowserApplication(["CFBundleURLTypes": "https", "CFBundleDocumentTypes": [htmlViewer]]),
               "malformed scheme metadata is refused")
        expect(BrowserLaunchPolicy.isBrowserApplication(metadata(documents: [
            ["CFBundleTypeRole": "Viewer", "CFBundleTypeExtensions": ["txt"]], htmlViewer
        ])), "one valid HTML Viewer declaration qualifies alongside other document types")
        let firstGame = "https://games.example.org/first"
        let secondGame = "https://games.example.org/second"
        let alpha = BrowserApplication(bundleIdentifier: "org.example.Alpha", name: "Alpha Browser",
                                       applicationURL: URL(fileURLWithPath: "/Applications/Alpha.app"))
        let zulu = BrowserApplication(bundleIdentifier: "org.example.Zulu", name: "Zulu Browser",
                                      applicationURL: URL(fileURLWithPath: "/Applications/Zulu.app"))
        let duplicate = BrowserApplication(bundleIdentifier: alpha.bundleIdentifier, name: "Another Alpha",
                                           applicationURL: URL(fileURLWithPath: "/Applications/OtherAlpha.app"))
        let invalid = BrowserApplication(bundleIdentifier: "org/example/NotAnApp", name: "Unsafe",
                                         applicationURL: URL(fileURLWithPath: "/Applications/Unsafe.app"))
        let remote = BrowserApplication(bundleIdentifier: "org.example.Remote", name: "Remote",
                                        applicationURL: URL(string: "https://games.example.org/Remote.app")!)
        expect(BrowserLaunchPolicy.applications([zulu, alpha, duplicate, invalid, remote]) == [alpha, zulu],
               "sorts browsers by name, keeps preferred bundle copy and excludes invalid candidates")
        for identifier in ["org.example.Browser", "com.example.Browser-canary", "org.example.browser2"] {
            expect(BrowserLaunchPolicy.validBundleIdentifier(identifier), "valid application identifier accepted")
        }
        for identifier in ["", "browser", ".org.example", "org..example", "org.example.", "org.example Browser",
                           "org.example/Browser", "org.example\\Browser", "org.example\nBrowser",
                           "org.example." + String(repeating: "a", count: 256)] {
            expect(!BrowserLaunchPolicy.validBundleIdentifier(identifier), "malformed application identifier refused")
        }
        expect(BrowserLaunchPolicy.preferenceKey(for: " https://Games.EXAMPLE.org ")
               == BrowserLaunchPolicy.preferenceKey(for: "https://games.example.org/"),
               "public URL identity normalizes host case, whitespace and root slash")
        let key = BrowserLaunchPolicy.preferenceKey(for: firstGame)!
        expect(key.hasPrefix("browserGameBrowser.v1.") && key.count == "browserGameBrowser.v1.".count + 64,
               "preference key is bounded SHA256 with a versioned prefix")
        expect(!key.contains("example") && !key.contains("https") && !key.contains("first"),
               "saved preference key contains no URL or host text")
        expect(key != BrowserLaunchPolicy.preferenceKey(for: secondGame), "games on one host keep independent choices")
        expect(BrowserLaunchPolicy.preferenceKey(for: firstGame + "?level=2") != key,
               "distinct public game parameters keep independent choices")
        expect(BrowserLaunchPolicy.preferenceKey(for: firstGame + "#game") != key,
               "distinct game fragments keep independent choices")
        // Construct synthetic credentials locally so no credential-bearing URL
        // literal is retained in the published test source.
        var credentialFixture = URLComponents(string: firstGame)!
        credentialFixture.user = "fixture-user"
        credentialFixture.password = "placeholder"
        for invalidURL in ["http://games.example.org/", "file:///Applications/Alpha.app", "javascript:alert(1)",
                           "https://localhost/", "https://127.0.0.1/", "https://games.example.org/?token=private",
                           credentialFixture.string!, "https://games.example.org:443/",
                           "https://games.example.org/%0a"] {
            expect(BrowserLaunchPolicy.preferenceKey(for: invalidURL) == nil, "unsafe URLs never produce preference keys")
            do {
                _ = try BrowserLaunchPolicy.request(gameURL: invalidURL, choice: .systemDefault, applications: [alpha])
                preconditionFailure("unsafe launch URL accepted")
            } catch {
                expect(error as? BrowserLaunchFailure == .invalidGameURL, "all launch routes preserve HTTPS validation")
            }
        }

        // The test suite owns one disposable domain; it never accesses or
        // modifies Caskrun's preferences or the user's installed applications.
        let suiteName = "org.example.CaskrunBrowserLaunchTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        defaults.removePersistentDomain(forName: suiteName)
        let store = BrowserSelectionStore(defaults: defaults)
        expect(store.choice(for: firstGame) == .systemDefault, "new games follow the system default browser")
        try store.set(.application(bundleIdentifier: alpha.bundleIdentifier), for: firstGame)
        expect(store.choice(for: firstGame) == .application(bundleIdentifier: alpha.bundleIdentifier),
               "explicit browser choice is persisted")
        expect(store.choice(for: secondGame) == .systemDefault, "one game's preference leaves another game unchanged")
        let reloadedStore = BrowserSelectionStore(defaults: UserDefaults(suiteName: suiteName)!)
        expect(reloadedStore.choice(for: firstGame) == store.choice(for: firstGame), "preference survives a fresh store instance")
        expect(defaults.persistentDomain(forName: suiteName) as? [String: String] == [key: alpha.bundleIdentifier],
               "persistence contains only the hashed key and bundle identifier")
        do {
            try store.set(.application(bundleIdentifier: "not/a/browser"), for: firstGame)
            preconditionFailure("invalid browser identifier persisted")
        } catch {
            expect(error as? BrowserLaunchFailure == .invalidBrowserIdentifier, "invalid preference rejected before writing")
        }
        expect(store.choice(for: firstGame) == .application(bundleIdentifier: alpha.bundleIdentifier),
               "failed preference update preserves the valid selection")
        try store.set(.systemDefault, for: firstGame)
        expect(defaults.persistentDomain(forName: suiteName)?.isEmpty != false,
               "system default removes the explicit preference rather than pinning the current default browser")

        var installed = [alpha, zulu]
        var defaultRequests: [URL] = []
        var explicitRequests: [(URL, BrowserApplication)] = []
        var discoveryCount = 0
        var defaultResult = true
        var explicitResult = true
        let environment = BrowserLaunchEnvironment(
            applications: { _ in discoveryCount += 1; return installed },
            defaultBrowserName: { _ in alpha.name },
            openDefault: { url in defaultRequests.append(url); return defaultResult },
            openApplication: { url, browser in explicitRequests.append((url, browser)); return explicitResult }
        )
        let service = BrowserLaunchService(defaults: defaults, environment: environment)
        try await service.open(firstGame)
        expect(defaultRequests == [URL(string: firstGame)!] && explicitRequests.isEmpty,
               "unset preference routes only through the system default opener")
        try service.select(.application(bundleIdentifier: zulu.bundleIdentifier), for: firstGame)
        try await service.open(firstGame)
        expect(explicitRequests.count == 1 && explicitRequests[0].0.absoluteString == firstGame
               && explicitRequests[0].1 == zulu && defaultRequests.count == 1,
               "explicit selection opens the exact URL in the exact selected browser")
        expect(service.selectedName(for: firstGame) == zulu.name, "sidebar displays the selected installed browser")

        installed = [alpha]
        do { try await service.open(firstGame); preconditionFailure("stale browser selection launched") }
        catch { expect(error as? BrowserLaunchFailure == .browserUnavailable, "uninstalled browser reports explicit failure") }
        expect(explicitRequests.count == 1 && defaultRequests.count == 1,
               "stale browser selection neither opens an unavailable app nor falls back to another browser")
        expect(service.choice(for: firstGame) == .application(bundleIdentifier: zulu.bundleIdentifier)
               && service.selectedName(for: firstGame) == "Browser unavailable",
               "stale selection remains visible and is not silently erased")

        defaults.set("not/a/browser", forKey: key)
        do { try await service.open(firstGame); preconditionFailure("malformed persisted choice launched") }
        catch { expect(error as? BrowserLaunchFailure == .invalidBrowserIdentifier, "malformed stored choice is actionable") }
        expect(explicitRequests.count == 1 && defaultRequests.count == 1,
               "malformed preference never silently uses the default browser")
        let beforeInvalidURL = discoveryCount
        do { try await service.open("https://games.example.org/?token=private"); preconditionFailure("unsafe URL launched") }
        catch { expect(error as? BrowserLaunchFailure == .invalidGameURL, "invalid game URL reports a safe validation error") }
        expect(discoveryCount == beforeInvalidURL && explicitRequests.count == 1 && defaultRequests.count == 1,
               "invalid URL never reaches app discovery or either opener")

        installed = [alpha, zulu]
        try service.select(.application(bundleIdentifier: alpha.bundleIdentifier), for: firstGame)
        explicitResult = false
        do { try await service.open(firstGame); preconditionFailure("failed explicit open claimed success") }
        catch { expect(error as? BrowserLaunchFailure == .couldNotOpen(browserName: alpha.name), "explicit launch errors name the chosen browser") }
        expect(defaultRequests.count == 1 && explicitRequests.count == 2, "failed explicit launch does not fall back")
        try service.select(.systemDefault, for: firstGame)
        defaultResult = false
        do { try await service.open(firstGame); preconditionFailure("failed default open claimed success") }
        catch { expect(error as? BrowserLaunchFailure == .couldNotOpen(browserName: "your default browser"), "default launch failure is reported") }
        expect(explicitRequests.count == 2 && defaultRequests.count == 2, "failed default launch does not try an arbitrary browser")
        service.removePreference(for: firstGame)
        expect(service.choice(for: firstGame) == .systemDefault, "collection cleanup removes only the game's browser preference")
        defaults.removePersistentDomain(forName: suiteName)
        expect(defaults.persistentDomain(forName: suiteName) == nil, "disposable preference domain is removed after verification")
        print("\(checks) browser choice and launch routing checks passed; no browser was launched")
    }
}
