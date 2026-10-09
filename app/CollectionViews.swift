import AppKit
import SwiftUI

enum CollectionPlatform: String, CaseIterable, Identifiable {
    case all, windows, ps5, browser
    var id: String { rawValue }
    var title: String {
        switch self { case .all: "All"; case .windows: "Windows"; case .ps5: "PS5"; case .browser: "Browser" }
    }
    var symbol: String {
        switch self { case .all: "square.grid.2x2"; case .windows: "desktopcomputer"; case .ps5: "gamecontroller"; case .browser: "safari" }
    }
}

enum CollectionItemKind: String, CaseIterable, Identifiable {
    case game, app
    var id: String { rawValue }
    var title: String { self == .game ? "Games" : "Apps" }
}

struct CollectionItem: Identifiable {
    let id: String
    let name: String
    let platform: CollectionPlatform
    let kind: CollectionItemKind
    let subtitle: String
    var icon: NSImage? = nil
    var browserURL: String? = nil
    var artworkRevision = ""
    var canOpen = true
    var status = ""
    var openActionTitle: String? = nil

    // Length-prefixed components keep a game's source and its local identifier
    // distinct even when a publisher identifier contains separators.
    static func namespacedID(platform: CollectionPlatform, source: String, rawID: String) -> String {
        [platform.rawValue, source, rawID].map { "\($0.utf8.count):\($0)" }.joined()
    }

    var actionTitle: String {
        openActionTitle ?? (platform == .browser ? "Play in browser" : kind == .game ? "Play" : "Open app")
    }

    var cardHelp: String {
        let select = "Click for \(name) details."
        guard canOpen else { return select }
        if openActionTitle == "Open store" { return select + " Double-click to open its store." }
        if openActionTitle == "Show window" { return select + " Double-click to show its window." }
        if openActionTitle == "Open game" { return select + " Double-click to open the game." }
        return select + (platform == .browser ? " Double-click to play in your browser."
                         : kind == .game ? " Double-click to play." : " Double-click to open the app.")
    }
}

enum CollectionFilter {
    static func entries(_ entries: [CollectionItem], platform: CollectionPlatform,
                        kind: CollectionItemKind?, query: String) -> [CollectionItem] {
        let query = query.trimmingCharacters(in: .whitespacesAndNewlines)
        return entries.filter { item in
            (platform == .all || item.platform == platform) && (kind == nil || item.kind == kind)
                && (query.isEmpty || item.name.localizedCaseInsensitiveContains(query)
                    || item.subtitle.localizedCaseInsensitiveContains(query))
        }
    }

    static func counts(_ entries: [CollectionItem], kind: CollectionItemKind?) -> [CollectionPlatform: Int] {
        var counts = Dictionary(uniqueKeysWithValues: CollectionPlatform.allCases.map { ($0, 0) })
        for entry in entries where kind == nil || entry.kind == kind {
            counts[.all, default: 0] += 1
            if entry.platform != .all { counts[entry.platform, default: 0] += 1 }
        }
        return counts
    }
}

/// The inspector is owned by the containing page. This grid keeps the first
/// clicked item's identity while opening that inspector changes the grid width.
struct CollectionGrid: View {
    let entries: [CollectionItem]
    @Binding var selectedID: String?
    @Binding var platform: CollectionPlatform
    @Binding var kind: CollectionItemKind?
    var query = ""
    var isLoading = false
    let onSelect: (CollectionItem) -> Void
    let onOpen: (CollectionItem) -> Void
    var onLoadArtwork: (CollectionItem) -> Void = { _ in }
    let onDiscover: () -> Void
    let onImportPS5: () -> Void
    @StateObject private var clickRouter = LibraryClickRouter()

    private var filtered: [CollectionItem] {
        CollectionFilter.entries(entries, platform: platform, kind: kind, query: query)
    }
    private var counts: [CollectionPlatform: Int] { CollectionFilter.counts(entries, kind: kind) }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            filterRow.padding(.horizontal, 24).padding(.top, 12).padding(.bottom, 16)
            ScrollView {
                if entries.isEmpty && isLoading {
                    ProgressView("Reading your collection…")
                        .frame(maxWidth: .infinity, minHeight: 190)
                } else if filtered.isEmpty {
                    emptyState
                } else {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 144, maximum: 176), spacing: 16)],
                              alignment: .leading, spacing: 20) {
                        ForEach(filtered) { item in
                            CollectionCard(item: item, isSelected: selectedID == item.id,
                                           onSelect: { select(item) }, onOpen: { open(item) })
                                .task(id: item.id + ":" + item.artworkRevision) { onLoadArtwork(item) }
                        }
                    }
                    .padding(.horizontal, 24).padding(.bottom, 24)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .onChange(of: platform) { _, _ in resetSelection() }
        .onChange(of: kind) { _, _ in resetSelection() }
        .onChange(of: query) { _, _ in
            clickRouter.cancel()
            clearHiddenSelection()
        }
        .onChange(of: selectedID) { _, id in if id == nil { clickRouter.cancel() } }
        .onChange(of: entries.map(\.id)) { _, _ in
            clickRouter.cancel()
            clearHiddenSelection()
        }
        .onChange(of: entries.filter(\.canOpen).map(\.id)) { _, _ in clickRouter.cancel() }
        .onDisappear { clickRouter.cancel() }
    }

    private var filterRow: some View {
        HStack(spacing: 8) {
            Menu {
                Section("Platform") {
                    ForEach(CollectionPlatform.allCases) { choice in
                        Button { platform = choice } label: {
                            Label("\(choice.title)  \(counts[choice, default: 0])",
                                  systemImage: platform == choice ? "checkmark" : choice.symbol)
                        }
                    }
                }
                Section("Show") {
                    Button { kind = nil } label: { Label("All items", systemImage: kind == nil ? "checkmark" : "square.grid.2x2") }
                    ForEach(CollectionItemKind.allCases) { choice in
                        Button { kind = choice } label: {
                            Label(choice.title, systemImage: kind == choice ? "checkmark" : choice == .game ? "gamecontroller" : "app")
                        }
                    }
                }
            } label: {
                Label(kind.map { "\(platform.title) · \($0.title)" } ?? platform.title, systemImage: platform.symbol)
                    .font(.system(size: 12, weight: .medium))
            }
            .menuStyle(.borderlessButton).fixedSize().frame(height: 28)
            .help("Filter collection by platform and item type")
            .accessibilityLabel("Filter: \(platform.title), \(kind?.title ?? "All items")")
            Spacer(minLength: 8)
            if isLoading { ProgressView().controlSize(.small).accessibilityLabel("Refreshing collection") }
            Text("\(filtered.count) \(filtered.count == 1 ? "item" : "items")")
                .font(.system(size: 11)).foregroundStyle(.secondary).monospacedDigit()
        }
    }

    @ViewBuilder private var emptyState: some View {
        if !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            ContentUnavailableView.search(text: query).frame(maxWidth: .infinity, minHeight: 190)
        } else {
            VStack(alignment: .leading, spacing: 12) {
                Image(systemName: platform.symbol).font(.system(size: 25)).foregroundStyle(.secondary)
                Text(platform == .ps5 ? "Your PS5 games" : platform == .browser ? "Your browser games"
                     : kind == .app ? "Your apps" : "Your collection")
                    .font(.title3.weight(.semibold))
                Text(emptyDescription).font(.subheadline).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                Button(platform == .ps5 ? "Import PS5 game" : platform == .browser ? "Find browser games" : "Explore Discover",
                       action: platform == .ps5 ? onImportPS5 : onDiscover)
                    .buttonStyle(.bordered)
            }
            .padding(22).frame(maxWidth: .infinity, minHeight: 190, alignment: .leading)
            .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 18))
            .padding(.horizontal, 24).padding(.bottom, 24)
        }
    }

    private var emptyDescription: String {
        if platform == .ps5 { return "Import a game you own. PS5 launching needs a compatible engine and is still experimental." }
        if platform == .browser { return "Save a game from Discover or add your own link. It opens in your usual browser." }
        if kind == .app { return "Installed launchers and saved Windows apps appear here. Add an app using the + menu." }
        return "Your games, apps and browser links appear together here. Add something using the + menu or Discover."
    }

    private func select(_ item: CollectionItem) {
        clickRouter.remember(NSApp.currentEvent, id: item.id) { id in
            guard selectedID == id, let current = filtered.first(where: { $0.id == id }), current.canOpen else { return }
            onOpen(current)
        }
        selectedID = item.id
        onSelect(item)
    }

    private func open(_ item: CollectionItem) { if item.canOpen { onOpen(item) } }
    private func resetSelection() { clickRouter.cancel(); selectedID = nil }
    private func clearHiddenSelection() {
        if let selectedID, !filtered.contains(where: { $0.id == selectedID }) { self.selectedID = nil }
    }
}

private struct CollectionCard: View {
    let item: CollectionItem
    let isSelected: Bool
    let onSelect: () -> Void
    let onOpen: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: onSelect) {
            if item.kind == .app {
                VStack(spacing: 5) {
                    CollectionArtwork(item: item)
                    Text(item.name)
                        .font(.system(size: 12, weight: .semibold)).lineLimit(2)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 8).padding(.vertical, 3)
                        .foregroundStyle(isSelected || hovered ? Color.accentColor : Color.primary)
                        .background(isSelected ? Color.accentColor.opacity(0.10) : .clear, in: Capsule())
                        .frame(maxWidth: 120)
                }
                .padding(.top, 8).frame(width: 120, height: 116, alignment: .top)
                .frame(maxWidth: .infinity, alignment: .center).contentShape(Rectangle())
            } else {
                VStack(alignment: .leading, spacing: 9) {
                    CollectionArtwork(item: item)
                    VStack(alignment: .leading, spacing: 4) {
                        Text(item.name).font(.system(size: 12, weight: .semibold)).lineLimit(2)
                            .frame(height: 32, alignment: .topLeading)
                        Text(item.subtitle.isEmpty ? item.platform.title : item.subtitle)
                            .font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                        if !item.status.isEmpty {
                            Text(item.status).font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    .padding(.horizontal, 3)
                }
                .padding(8)
                .foregroundStyle(Color.primary)
                .background(isSelected ? Color.accentColor.opacity(0.08) : Color.primary.opacity(hovered ? 0.035 : 0),
                            in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .strokeBorder(isSelected ? Color.accentColor.opacity(0.3) : .clear)
                }
                .contentShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
            }
        }
        .buttonStyle(.plain).onHover { hovered = $0 }
        .help(item.cardHelp)
        .accessibilityLabel("\(item.name), \(item.platform.title)\(item.status.isEmpty ? "" : ", " + item.status)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .accessibilityAction(named: Text(item.actionTitle)) { if item.canOpen { onOpen() } }
        .contextMenu { if item.canOpen { Button(item.actionTitle, action: onOpen) } }
    }
}

struct CollectionArtwork: View {
    let item: CollectionItem

    var body: some View {
        if item.kind == .app {
            Group {
                if let icon = item.icon {
                    Image(nsImage: icon).resizable().interpolation(.high).scaledToFit()
                } else {
                    Image(systemName: "app").font(.system(size: 28, weight: .light)).foregroundStyle(.tertiary)
                }
            }
            .frame(width: 48, height: 48).frame(width: 56, height: 56)
            .accessibilityHidden(true)
        } else if item.platform == .browser, let url = item.browserURL,
           let entry = BrowserGameCatalog.entry(forSavedURL: url) {
            BrowserGameCover(entry: entry)
        } else {
            Rectangle().fill(Color(nsColor: .controlBackgroundColor))
                .aspectRatio(16.0 / 9.0, contentMode: .fit)
                .overlay {
                    GeometryReader { geometry in
                        if let icon = item.icon {
                            let wide = icon.size.height > 0 && icon.size.width / icon.size.height >= 1.4
                            Image(nsImage: icon).resizable().interpolation(.high)
                                .aspectRatio(contentMode: wide ? .fill : .fit)
                                .frame(width: wide ? geometry.size.width : 68,
                                       height: wide ? geometry.size.height : 68)
                                .frame(width: geometry.size.width, height: geometry.size.height)
                                .clipped()
                        } else {
                            Image(systemName: item.platform == .browser ? "safari" : "gamecontroller")
                                .font(.system(size: 34, weight: .light))
                                .foregroundStyle(.tertiary)
                                .frame(width: geometry.size.width, height: geometry.size.height)
                        }
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                .accessibilityHidden(true)
        }
    }
}
