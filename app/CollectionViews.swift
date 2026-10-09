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

    private var emptyState: some View {
        VStack(alignment: .leading, spacing: 7) {
            Text(emptyTitle).font(.system(size: 14, weight: .semibold))
            Text(emptyDescription).font(.system(size: 12)).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            if isEmptyTypeFilter && query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                Button("Show all items") { kind = nil }
                    .font(.system(size: 12, weight: .medium)).buttonStyle(.borderless)
                    .padding(.top, 3)
            }
        }
        .frame(maxWidth: 440, alignment: .leading)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 24).padding(.top, 20).padding(.bottom, 24)
    }

    private var isEmptyTypeFilter: Bool {
        guard kind != nil else { return false }
        return entries.contains { platform == .all || $0.platform == platform }
            || (kind == .app && (platform == .ps5 || platform == .browser))
    }

    private var emptyTitle: String {
        if !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "No results" }
        if isEmptyTypeFilter { return kind == .app ? "No apps in this view" : "No games in this view" }
        if platform == .ps5 { return "No PS5 games yet" }
        if platform == .browser { return "No browser games yet" }
        return kind == .app ? "No apps yet" : "Your collection starts here"
    }

    private var emptyDescription: String {
        let search = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if !search.isEmpty { return "Nothing in this view matches “\(search)”." }
        if isEmptyTypeFilter { return "Change the item filter to see the rest of your collection." }
        if platform == .ps5 { return "Import a game you own with the + menu." }
        if platform == .browser { return "Save a game from Discover, or add your own link with +." }
        if kind == .app { return "Add an app from Discover or the + menu." }
        return "Add games and apps from Discover or the + menu."
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

    private var caption: String {
        if item.kind == .app { return "Windows app" }
        return item.subtitle.isEmpty ? item.platform.title : item.subtitle
    }

    var body: some View {
        Button(action: onSelect) {
            VStack(spacing: 7) {
                CollectionArtwork(item: item)
                VStack(spacing: 3) {
                    Text(item.name)
                        .font(.system(size: 12, weight: .semibold)).lineLimit(2)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 8).padding(.vertical, 3)
                        .foregroundStyle(isSelected || hovered ? Color.accentColor : Color.primary)
                        .background(isSelected ? Color.accentColor.opacity(0.10) : .clear,
                                    in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                        .frame(maxWidth: .infinity)
                        .frame(height: 36, alignment: .top)
                    Text(caption)
                        .font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                        .frame(height: 12)
                    Text(item.status.isEmpty ? " " : item.status)
                        .font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                        .frame(height: 12)
                        .accessibilityHidden(item.status.isEmpty)
                }
            }
            .padding(.horizontal, 4).padding(.vertical, 4)
            .frame(maxWidth: .infinity, alignment: .top)
            .contentShape(Rectangle())
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
            Color.clear
                .aspectRatio(16.0 / 9.0, contentMode: .fit)
                .overlay(alignment: .bottom) {
                    Group {
                        if let icon = item.icon {
                            Image(nsImage: icon).resizable().interpolation(.high).scaledToFit()
                        } else {
                            Image(systemName: "app").font(.system(size: 28, weight: .light)).foregroundStyle(.tertiary)
                        }
                    }
                    .frame(width: 48, height: 48)
                }
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
