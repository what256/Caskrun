import AppKit
import SwiftUI

struct BrowserGame: Codable, Identifiable, Sendable {
    let id: String
    let name: String
    let url: String
    let created_at: UInt64

    static func checkedURL(_ value: String) -> URL? { BrowserGameLink.validatedURL(value) }
}

struct BrowserGames: Decodable, Sendable {
    let games: [BrowserGame]
}

struct BrowserGameCatalogEntry: Decodable, Identifiable, Sendable {
    let id: String
    let name: String
    let detail: String
    let url: URL
    let sourceURL: URL
    let symbol: String
    let requiresOwnFiles: Bool
}

enum BrowserGameCatalog {
    private struct Document: Decodable {
        let schemaVersion: Int
        let entries: [BrowserGameCatalogEntry]
    }

    static let loaded: (entries: [BrowserGameCatalogEntry], error: String?) = {
        guard let url = Bundle.main.url(forResource: "browser-games", withExtension: "json", subdirectory: "catalog") else {
            return ([], "The browser game catalog is missing. You can still add your own game link.")
        }
        do {
            let bytes = try Data(contentsOf: url)
            guard bytes.count <= 128 * 1024 else { throw CocoaError(.fileReadTooLarge) }
            let document = try JSONDecoder().decode(Document.self, from: bytes)
            guard document.schemaVersion == 1, document.entries.count <= 64,
                  Set(document.entries.map(\.id)).count == document.entries.count,
                  document.entries.allSatisfy({ entry in
                      !entry.id.isEmpty && entry.id.utf8.count <= 64 && !entry.name.isEmpty && entry.name.utf8.count <= 160
                          && entry.detail.utf8.count <= 512 && !entry.symbol.isEmpty
                          && BrowserGameLink.validatedURL(entry.url.absoluteString) != nil
                          && BrowserGameLink.validatedURL(entry.sourceURL.absoluteString) != nil
                  }) else { throw CocoaError(.coderInvalidValue) }
            return (document.entries, nil)
        } catch {
            return ([], "The browser game catalog could not be read. You can still add your own game link.")
        }
    }()
}

/// Mirrors the core's conservative bookmark policy. The core remains the
/// authority when saving. This check also prevents opening altered on-disk
/// data through a file URL, custom application scheme or credentialed URL.
enum BrowserGameLink {
    static func validatedURL(_ input: String) -> URL? {
        let value = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard value.hasPrefix("https://"), !value.isEmpty, value.utf8.count <= 2048,
              value.utf8.allSatisfy({ $0 > 0x20 && $0 < 0x7f && ![UInt8(0x5c), 0x22, 0x3c, 0x3e, 0x60, 0x5e, 0x7b, 0x7d, 0x7c].contains($0) }) else { return nil }
        let rest = value.dropFirst(8)
        let authority = String(rest.prefix { !"/?#".contains($0) }).lowercased()
        guard !authority.isEmpty, authority.utf8.count <= 253,
              !authority.contains(where: { "@:%[]".contains($0) }) else { return nil }
        let labels = authority.split(separator: ".", omittingEmptySubsequences: false)
        guard labels.count >= 2, labels.allSatisfy({ label in
            !label.isEmpty && label.utf8.count <= 63 && !label.hasPrefix("-") && !label.hasSuffix("-")
                && label.utf8.allSatisfy { ($0 >= 97 && $0 <= 122) || ($0 >= 48 && $0 <= 57) || $0 == 45 }
        }), let last = labels.last, last.count >= 2,
              last.utf8.allSatisfy({ $0 >= 97 && $0 <= 122 }),
              !["localhost", "local", "internal", "lan", "home", "invalid", "test", "example", "onion"].contains(String(last)) else { return nil }
        let bytes = Array(value.utf8)
        var index = 0
        while index < bytes.count {
            if bytes[index] == 37 {
                guard index + 2 < bytes.count,
                      let decoded = UInt8(String(bytes: bytes[(index + 1)...(index + 2)], encoding: .utf8) ?? "", radix: 16),
                      decoded >= 0x20, decoded != 0x7f, decoded != 0x5c else { return nil }
                index += 3
            } else { index += 1 }
        }
        let suffix = rest.dropFirst(authority.count)
        for parameters in suffix.split(omittingEmptySubsequences: false, whereSeparator: { $0 == "?" || $0 == "#" }).dropFirst() {
            for parameter in parameters.split(whereSeparator: { $0 == "&" || $0 == ";" }) {
                let key = String(parameter.prefix { $0 != "=" }).removingPercentEncoding ?? ""
                let plain = key.prefix { $0 != "[" }
                let normalized = plain.lowercased().filter { $0.isASCII && ($0.isLetter || $0.isNumber) }
                if ["token", "accesstoken", "idtoken", "refreshtoken", "auth", "authtoken", "bearer", "bearertoken", "authorization", "authorizationcode", "code", "clientsecret", "jwt", "oauthtoken", "oauthtokensecret", "oauthverifier", "session", "sessionid", "sid", "apikey", "apitoken", "key", "password", "passwd", "pwd", "email"].contains(normalized) { return nil }
            }
        }
        guard let url = URL(string: value), url.scheme == "https", url.user == nil, url.password == nil,
              url.port == nil, url.host?.lowercased() == authority else { return nil }
        return url
    }
}

struct AddBrowserGameSheet: View {
    let onSave: (String, String) async throws -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var name: String
    @State private var link: String
    @State private var saving = false
    @State private var error: String?

    init(initialName: String = "", initialURL: String = "", onSave: @escaping (String, String) async throws -> Void) {
        self.onSave = onSave
        _name = State(initialValue: initialName)
        _link = State(initialValue: initialURL)
    }

    private var validName: Bool {
        let value = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return !value.isEmpty && value.unicodeScalars.count <= 80
            && !value.unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 12) {
                BrowserGameArtwork(seed: link, size: 42)
                VStack(alignment: .leading, spacing: 4) {
                    Text("Add a browser game").font(.title2.weight(.semibold))
                    Text("A shortcut on Home. Play in your usual browser.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            TextField("Game name", text: $name).textFieldStyle(.roundedBorder)
                .accessibilityLabel("Game name")
            TextField("https://game-website.com", text: $link).textFieldStyle(.roundedBorder)
                .accessibilityLabel("HTTPS game link")
            Text("Use a public game page from a site you trust, without account or sign-in details in its link. Some community ports need game files you already own. Your browser handles the game and its sign-in.")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            if !link.isEmpty && BrowserGameLink.validatedURL(link) == nil {
                Text("Enter an HTTPS website link without spaces, credentials or a custom port.")
                    .font(.caption).foregroundStyle(.orange)
            }
            if let error { Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).disabled(saving)
                Button {
                    saving = true
                    error = nil
                    Task { @MainActor in
                        defer { saving = false }
                        do {
                            try await onSave(name.trimmingCharacters(in: .whitespacesAndNewlines), link.trimmingCharacters(in: .whitespacesAndNewlines))
                            dismiss()
                        } catch { self.error = error.localizedDescription }
                    }
                } label: {
                    if saving { ProgressView().controlSize(.small).frame(width: 92) }
                    else { Text("Add to Home") }
                }
                .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                .disabled(saving || !validName || BrowserGameLink.validatedURL(link) == nil)
            }
        }
        .padding(24).frame(width: 470)
        .interactiveDismissDisabled(saving)
    }
}

struct BrowserGameArtwork: View {
    let seed: String
    var size: CGFloat = 56

    private var tint: Color {
        let value = seed.utf8.reduce(UInt64(5381)) { ($0 &* 33) &+ UInt64($1) }
        return Color(hue: Double(value % 360) / 360, saturation: 0.48, brightness: 0.73)
    }

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size * 0.23, style: .continuous)
                .fill(LinearGradient(colors: [tint.opacity(0.72), tint], startPoint: .topLeading, endPoint: .bottomTrailing))
            Image(systemName: "safari.fill").font(.system(size: size * 0.57, weight: .light))
                .foregroundStyle(.white.opacity(0.95))
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

struct BrowserGameTile: View {
    let game: BrowserGame
    var isSelected = false
    var opensOnSingleClick = false
    let onSelect: () -> Void
    let onOpen: () -> Void
    var onRemove: (() -> Void)? = nil
    @State private var hovered = false
    @State private var confirmRemove = false

    var body: some View {
        Button {
            onSelect()
            let clicks = NSApp.currentEvent?.clickCount ?? 0
            if opensOnSingleClick ? clicks < 2 : clicks >= 2 { onOpen() }
        } label: {
            VStack(spacing: 8) {
                BrowserGameArtwork(seed: game.url)
                Text(game.name).font(.system(size: 13, weight: .semibold)).lineLimit(2)
                    .multilineTextAlignment(.center)
                    .foregroundStyle(isSelected || hovered ? Color.accentColor : Color.primary)
                Text("Browser").font(.system(size: 10)).foregroundStyle(.secondary)
            }
            .padding(12).frame(width: 120).contentShape(Rectangle())
        }
        .buttonStyle(.plain).frame(maxWidth: .infinity)
        .onHover { hovered = $0 }
        .help(opensOnSingleClick ? "Play \(game.name) in your browser." : "Click to select \(game.name). Double-click to play in your browser.")
        .accessibilityLabel(game.name)
        .accessibilityHint(opensOnSingleClick ? "Opens in your browser." : "Browser game. Double-click to open.")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .contextMenu {
            Button("Play in browser", action: onOpen)
            if onRemove != nil { Button("Remove from Home", role: .destructive) { confirmRemove = true } }
        }
        .confirmationDialog("Remove \(game.name) from Home?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button("Remove shortcut", role: .destructive) { onRemove?() }
            Button("Cancel", role: .cancel) {}
        } message: { Text("This removes the saved link. Your browser's game data stays in your browser.") }
    }
}

enum LibraryScope: String, CaseIterable, Identifiable {
    case all, steam, epic, gog, apps, browser
    var id: String { rawValue }
    var title: String {
        switch self { case .all: "All"; case .steam: "Steam"; case .epic: "Epic"; case .gog: "GOG"; case .apps: "Apps"; case .browser: "Browser" }
    }
    var symbol: String {
        switch self { case .all: "square.grid.2x2"; case .steam: "gamecontroller"; case .epic: "gamecontroller"; case .gog: "gamecontroller"; case .apps: "app"; case .browser: "safari" }
    }
}

struct LibraryScopeBar: View {
    @Binding var selection: LibraryScope
    var counts: [LibraryScope: Int] = [:]
    @Namespace private var highlight
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: 3) {
            ForEach(LibraryScope.allCases) { scope in
                Button {
                    withAnimation(reduceMotion ? nil : .easeOut(duration: 0.16)) { selection = scope }
                } label: {
                    HStack(spacing: 5) {
                Text(scope.title)
                        if let count = counts[scope] { Text("\(count)").font(.system(size: 10)).monospacedDigit().opacity(0.65) }
                    }
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(selection == scope ? Color.primary : Color.secondary)
                    .padding(.horizontal, 10).frame(height: 28)
                    .background {
                        if selection == scope {
                            Capsule().fill(.primary.opacity(0.09)).matchedGeometryEffect(id: "scope-selection", in: highlight)
                        }
                    }
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(scope.title)\(counts[scope].map { ", \($0) items" } ?? "")")
                .accessibilityAddTraits(selection == scope ? .isSelected : [])
            }
        }
        .padding(4).background(.quaternary.opacity(0.40), in: Capsule())
        .fixedSize()
    }
}

struct GameLibraryEntry: Identifiable {
    let id: String
    let name: String
    let source: LibraryScope
    let subtitle: String
    var compatibilityText = "Compatibility unknown"
    var compatibilityDetail = "This game has not been tested with Caskrun."
    var compatibility: CompatibilityGame? = nil
    var icon: NSImage? = nil
    var artworkRevision = ""
    var browserURL: String? = nil
    var canPlay = true
    var canManageBottle = false
    var availableRenderers: Set<String>? = nil
    var graphicsDescription: String? = nil
}

struct GameLibraryView: View {
    let entries: [GameLibraryEntry]
    let device: CompatibilityDevice
    var isLoading = false
    var warnings: [String] = []
    var query = ""
    let onRefresh: () -> Void
    let onPlay: (GameLibraryEntry) -> Void
    let onManage: (GameLibraryEntry) -> Void
    let onRemoveBrowser: (GameLibraryEntry) -> Void
    let onAddBrowser: () -> Void
    var onLoadArtwork: (GameLibraryEntry) -> Void = { _ in }
    var onInspect: (GameLibraryEntry) -> Void = { _ in }
    @State private var scope: LibraryScope = .all
    @State private var selectedID: String?
    @State private var confirmRemove = false

    private var filtered: [GameLibraryEntry] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        return entries.filter { item in
            (scope == .all || item.source == scope) && (trimmed.isEmpty || item.name.localizedCaseInsensitiveContains(trimmed)
                || item.subtitle.localizedCaseInsensitiveContains(trimmed))
        }
    }
    private var selected: GameLibraryEntry? { filtered.first { $0.id == selectedID } }
    private var counts: [LibraryScope: Int] {
        var result: [LibraryScope: Int] = [.all: entries.count]
        for item in entries { result[item.source, default: 0] += 1 }
        for item in LibraryScope.allCases where result[item] == nil { result[item] = 0 }
        return result
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                LibraryScopeBar(selection: $scope, counts: counts)
                Spacer(minLength: 12)
                if isLoading { ProgressView().controlSize(.small) }
                Button(action: onRefresh) { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.borderless).help("Refresh installed games").accessibilityLabel("Refresh library")
                    .disabled(isLoading)
                Button(action: onAddBrowser) { Label("Add link", systemImage: "plus") }
                    .font(.system(size: 11, weight: .medium)).buttonStyle(.borderless)
            }
            .padding(.horizontal, 24).padding(.top, 12).padding(.bottom, 14)
            HStack(alignment: .top, spacing: 0) {
                ScrollView {
                    VStack(alignment: .leading, spacing: 18) {
                        if entries.isEmpty && isLoading {
                            ProgressView("Reading your installed games…").frame(maxWidth: .infinity, minHeight: 160)
                        } else if filtered.isEmpty {
                            emptyState
                        } else {
                            LazyVGrid(columns: [GridItem(.adaptive(minimum: 132, maximum: 158), spacing: 14)], alignment: .leading, spacing: 14) {
                                ForEach(filtered) { item in
                                    GameLibraryCard(item: item, isSelected: selectedID == item.id) {
                                        selectedID = item.id
                                    } open: { onPlay(item) }
                                        .task(id: item.id + ":" + item.artworkRevision) { onLoadArtwork(item) }
                                }
                            }
                        }
                        if !warnings.isEmpty {
                            DisclosureGroup("Some library information could not be read") {
                                ForEach(Array(warnings.enumerated()), id: \.offset) { _, warning in
                                    Text(warning).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                                        .frame(maxWidth: .infinity, alignment: .leading).padding(.top, 4)
                                }
                            }.font(.caption).foregroundStyle(.orange)
                        }
                        Text("Installed games stay with their store and bottle. Community reports are guidance; Caskrun gameplay remains unverified.")
                            .font(.system(size: 11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.horizontal, 24).padding(.bottom, 24)
                }
                .frame(maxWidth: .infinity)
                if let selected {
                    detail(selected).frame(width: 296).padding(.trailing, 16).padding(.bottom, 16)
                }
            }
        }
        .onChange(of: scope) { _, _ in selectedID = nil }
        .onChange(of: selectedID) { _, _ in if let selected { onInspect(selected) } }
        .onChange(of: entries.map(\.id)) { _, ids in if let selectedID, !ids.contains(selectedID) { self.selectedID = nil } }
        .confirmationDialog("Remove this browser game?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button("Remove shortcut", role: .destructive) { if let selected { onRemoveBrowser(selected) } }
            Button("Cancel", role: .cancel) {}
        } message: { Text("Only the saved Caskrun link is removed. Your browser's game data stays in your browser.") }
    }

    @ViewBuilder private var emptyState: some View {
        if !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            ContentUnavailableView.search(text: query).frame(maxWidth: .infinity, minHeight: 190)
        } else {
            VStack(alignment: .leading, spacing: 12) {
                Image(systemName: scope == .browser ? "safari" : "gamecontroller").font(.system(size: 25)).foregroundStyle(.secondary)
                Text(scope == .browser ? "Your browser games" : "Your library, together").font(.title3.weight(.semibold))
                Text(scope == .browser ? "Save a game link here and play in your browser." : "Installed Steam, Epic and GOG games appear here alongside saved apps. Sign in and launch publisher games through their store.")
                    .font(.subheadline).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                Button("Add a browser game", action: onAddBrowser).buttonStyle(.bordered)
            }
            .padding(20).frame(maxWidth: .infinity, minHeight: 190, alignment: .leading)
            .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 18))
        }
    }

    private func detail(_ item: GameLibraryEntry) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text(item.source == .browser ? "BROWSER GAME" : item.source == .apps ? "SAVED APP" : "IN YOUR LIBRARY")
                        .font(.system(size: 10, weight: .semibold, design: .monospaced)).foregroundStyle(.secondary)
                    Spacer()
                    Button { selectedID = nil } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .semibold)).frame(width: 24, height: 24) }
                        .buttonStyle(.plain).help("Close game details").accessibilityLabel("Close game details")
                }
                GameLibraryArtwork(item: item, size: 64)
                VStack(alignment: .leading, spacing: 5) {
                    Text(item.name).font(.system(size: 21, weight: .semibold)).fixedSize(horizontal: false, vertical: true)
                    Text(item.subtitle).font(.caption).foregroundStyle(.secondary)
                }
                Button { onPlay(item) } label: {
                    Label(item.source == .browser ? "Play in browser" : item.source == .apps ? "Open app" : (item.source == .epic || item.source == .gog) ? "Open store" : "Play", systemImage: item.source == .browser ? "arrow.up.right" : "play.fill")
                        .font(.system(size: 13, weight: .semibold)).frame(maxWidth: .infinity).padding(.vertical, 4)
                }
                .buttonStyle(.borderedProminent).disabled(!item.canPlay)
                if item.canManageBottle {
                    Button { onManage(item) } label: { Label("Bottle controls", systemImage: "slider.horizontal.3") }
                        .font(.caption).buttonStyle(.borderless)
                }
                if let graphics = item.graphicsDescription {
                    Label(graphics, systemImage: "display").font(.caption).foregroundStyle(.secondary)
                }
                Divider()
                if item.source == .browser {
                    Label("Opens in your default browser", systemImage: "safari").font(.caption.weight(.medium))
                    if let link = item.browserURL, let url = BrowserGameLink.validatedURL(link) {
                        Text(url.host ?? "").font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                    Text("Browser requirements and game data are handled by the website. Windows graphics settings do not apply.")
                        .font(.caption).foregroundStyle(.secondary)
                    Button("Remove from Home", role: .destructive) { confirmRemove = true }
                        .font(.caption).buttonStyle(.borderless)
                } else if item.source == .apps {
                    Label("Saved Windows shortcut", systemImage: "app").font(.caption.weight(.medium))
                    Text(item.compatibilityDetail).font(.caption).foregroundStyle(.secondary)
                    Text("Opens using this bottle's saved program, arguments and working folder.")
                        .font(.caption).foregroundStyle(.secondary)
                } else if item.source == .epic || item.source == .gog {
                    Label("Installed game", systemImage: "internaldrive").font(.caption.weight(.medium))
                    Text(item.compatibilityDetail).font(.caption).foregroundStyle(.secondary)
                    Text(item.canPlay ? "Open its store to play. Your sign-in and game ownership stay with the publisher." : "Save this store's Windows client in the bottle to open it here.")
                        .font(.caption).foregroundStyle(.secondary)
                } else {
                    CompatibilityEvidenceView(game: item.compatibility, device: device, availableRenderers: item.availableRenderers)
                    if item.compatibility == nil {
                        Text(item.compatibilityDetail).font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            .padding(18)
        }
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay { RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(.primary.opacity(0.10)) }
    }
}

/// Original local artwork while a publisher's installed icon is unavailable.
/// A stable seed gives the same title the same colour without any web request.
struct GameArtwork: View {
    let seed: String
    let name: String
    var size: CGFloat = 56
    var symbol: String? = nil

    private var tint: Color {
        let value = seed.utf8.reduce(UInt64(5381)) { ($0 &* 33) &+ UInt64($1) }
        return Color(hue: Double(value % 360) / 360, saturation: 0.42, brightness: 0.64)
    }
    private var initials: String {
        let words = name.split { !$0.isLetter && !$0.isNumber }
        return String(words.prefix(2).compactMap(\.first)).uppercased()
    }
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size * 0.23, style: .continuous)
                .fill(LinearGradient(colors: [tint.opacity(0.65), tint], startPoint: .topLeading, endPoint: .bottomTrailing))
            if let symbol {
                Image(systemName: symbol).font(.system(size: size * 0.5, weight: .light)).foregroundStyle(.white.opacity(0.94))
            } else if !initials.isEmpty {
                Text(initials).font(.system(size: size * 0.34, weight: .semibold, design: .rounded))
                    .foregroundStyle(.white.opacity(0.94)).lineLimit(1)
            } else {
                Image(systemName: "gamecontroller.fill").font(.system(size: size * 0.5, weight: .light)).foregroundStyle(.white.opacity(0.94))
            }
        }
        .frame(width: size, height: size).accessibilityHidden(true)
    }
}

private struct GameLibraryArtwork: View {
    let item: GameLibraryEntry
    let size: CGFloat
    var body: some View {
        Group {
            if let icon = item.icon { Image(nsImage: icon).resizable().interpolation(.high).scaledToFit() }
            else if item.source == .browser { BrowserGameArtwork(seed: item.browserURL ?? item.id, size: size) }
            else {
                GameArtwork(seed: item.name.lowercased(), name: item.name, size: size,
                            symbol: item.source == .apps ? "app.fill" : nil)
            }
        }
        .frame(width: size, height: size).accessibilityHidden(true)
    }
}

private struct GameLibraryCard: View {
    let item: GameLibraryEntry
    let isSelected: Bool
    let select: () -> Void
    let open: () -> Void
    @State private var hovered = false
    var body: some View {
        Button {
            select()
            if (NSApp.currentEvent?.clickCount ?? 0) >= 2 && item.canPlay { open() }
        } label: {
            VStack(spacing: 8) {
                GameLibraryArtwork(item: item, size: 54).frame(height: 64)
                Text(item.name).font(.system(size: 12, weight: .semibold)).lineLimit(2)
                    .multilineTextAlignment(.center).frame(height: 32, alignment: .top)
                Text(item.source.title).font(.system(size: 10)).foregroundStyle(.secondary)
                if item.source != .browser {
                    Text(item.compatibilityText).font(.system(size: 9)).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            .padding(12).frame(maxWidth: .infinity)
            .foregroundStyle(isSelected ? Color.accentColor : Color.primary)
            .background(isSelected ? Color.accentColor.opacity(0.09) : Color.primary.opacity(hovered ? 0.035 : 0), in: RoundedRectangle(cornerRadius: 15))
            .overlay { RoundedRectangle(cornerRadius: 15).strokeBorder(isSelected ? Color.accentColor.opacity(0.22) : .clear) }
            .contentShape(RoundedRectangle(cornerRadius: 15))
        }
        .buttonStyle(.plain).onHover { hovered = $0 }
        .help("Click for \(item.name) details. Double-click to \(item.source == .apps ? "open" : "play").")
        .accessibilityLabel("\(item.name), \(item.source.title), \(item.compatibilityText)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}
