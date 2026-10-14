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
    var bottleID: String? = nil

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

struct CollectionFolder: Identifiable {
    let id: String
    let name: String
    var icon: NSImage? = nil
    var canOpen = false
    var primaryAppName: String? = nil
}

struct CollectionBrowseResult {
    let folders: [CollectionFolder]
    let entries: [CollectionItem]
    var count: Int { folders.count + entries.count }
}

/// Windows items belong to their bottle, not to a second, flat library. An
/// explicit bottle identity keeps similarly named imported bottles separate.
enum CollectionGrouping {
    static func browse(_ entries: [CollectionItem], folders: [CollectionFolder],
                       openFolderID: String?, platform: CollectionPlatform,
                       kind: CollectionItemKind?, query: String) -> CollectionBrowseResult {
        let filtered = CollectionFilter.entries(entries, platform: platform, kind: kind, query: query)
        if let openFolderID {
            return CollectionBrowseResult(folders: [], entries: filtered.filter { $0.bottleID == openFolderID }
                .sorted { lhs, rhs in
                    if lhs.kind != rhs.kind { return lhs.kind == .app }
                    return lhs.name.localizedStandardCompare(rhs.name) == .orderedAscending
                })
        }
        let query = query.trimmingCharacters(in: .whitespacesAndNewlines)
        let children = Dictionary(grouping: entries.filter { $0.platform == .windows }, by: { $0.bottleID ?? "" })
        let matches = Set(filtered.compactMap(\.bottleID))
        let visibleFolders = platform == .all || platform == .windows ? folders.filter { folder in
            let folderMatches = query.isEmpty || folder.name.localizedCaseInsensitiveContains(query)
            let hasKind = kind == nil || (children[folder.id] ?? []).contains { $0.kind == kind }
            return matches.contains(folder.id) || (folderMatches && hasKind)
        } : []
        return CollectionBrowseResult(folders: visibleFolders,
                                      entries: filtered.filter { $0.platform != .windows || $0.bottleID == nil })
    }

    static func contents(_ entries: [CollectionItem], folderID: String) -> [CollectionItem] {
        entries.filter { $0.bottleID == folderID }
    }
}

/// The inspector is owned by the containing page. This grid keeps the first
/// clicked item's identity while opening that inspector changes the grid width.
struct CollectionGrid: View {
    let entries: [CollectionItem]
    var folders: [CollectionFolder] = []
    @Binding var openFolderID: String?
    @Binding var selectedID: String?
    @Binding var platform: CollectionPlatform
    @Binding var kind: CollectionItemKind?
    var query = ""
    var isLoading = false
    let onSelect: (CollectionItem) -> Void
    let onOpen: (CollectionItem) -> Void
    var onLoadArtwork: (CollectionItem) -> Void = { _ in }
    var onSelectFolder: (CollectionFolder) -> Void = { _ in }
    var onOpenFolder: (CollectionFolder) -> Void = { _ in }
    var onLoadFolderArtwork: (CollectionFolder) -> Void = { _ in }
    var onBack: () -> Void = {}
    let onDiscover: () -> Void
    let onImportPS5: () -> Void
    @StateObject private var clickRouter = LibraryClickRouter()

    private var browsed: CollectionBrowseResult {
        CollectionGrouping.browse(entries, folders: folders, openFolderID: openFolderID,
                                  platform: platform, kind: kind, query: query)
    }
    private var filtered: [CollectionItem] { browsed.entries }
    private var openFolder: CollectionFolder? { folders.first { $0.id == openFolderID } }
    private var counts: [CollectionPlatform: Int] {
        Dictionary(uniqueKeysWithValues: CollectionPlatform.allCases.map { choice in
            (choice, CollectionGrouping.browse(entries, folders: folders, openFolderID: nil,
                                               platform: choice, kind: kind, query: "").count)
        })
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            filterRow.padding(.horizontal, 24).padding(.top, 12).padding(.bottom, 16)
            ScrollView {
                if entries.isEmpty && folders.isEmpty && isLoading {
                    ProgressView("Reading your collection…")
                        .frame(maxWidth: .infinity, minHeight: 190)
                } else if browsed.count == 0 {
                    emptyState
                } else {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 144, maximum: 176), spacing: 16)],
                              alignment: .leading, spacing: 20) {
                        ForEach(browsed.folders) { folder in
                            CollectionFolderCard(folder: folder,
                                entries: CollectionGrouping.contents(entries, folderID: folder.id),
                                onSelect: { selectFolder(folder) }, onOpen: { onOpenFolder(folder) })
                                .task(id: folder.id) { onLoadFolderArtwork(folder) }
                        }
                        ForEach(filtered) { item in
                            CollectionCard(item: item, isSelected: selectedID == item.id,
                                           onSelect: { select(item) }, onOpen: { open(item) })
                                .task(id: item.id + ":" + item.artworkRevision) { onLoadArtwork(item) }
                        }
                    }
                    .padding(.horizontal, 24).padding(.bottom, 24)
                }
            }
            .id(openFolderID ?? "collection-root")
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .onChange(of: platform) { _, _ in resetSelection() }
        .onChange(of: kind) { _, _ in resetSelection() }
        .onChange(of: query) { _, _ in
            clickRouter.cancel()
            clearHiddenSelection()
        }
        .onChange(of: selectedID) { _, id in if id == nil && openFolderID == nil { clickRouter.cancel() } }
        .onChange(of: entries.map(\.id)) { _, _ in
            clickRouter.cancel()
            clearHiddenSelection()
        }
        .onChange(of: entries.filter(\.canOpen).map(\.id)) { _, _ in clickRouter.cancel() }
        .onDisappear { clickRouter.cancel() }
    }

    private var filterRow: some View {
        HStack(spacing: 8) {
            if let openFolder {
                Button(action: onBack) {
                    Label("Collection", systemImage: "chevron.left")
                        .font(.system(size: 12, weight: .medium))
                }
                .buttonStyle(.plain).foregroundStyle(.secondary)
                .help("Back to Collection")
                Text("/").font(.system(size: 12)).foregroundStyle(.tertiary)
                Text(openFolder.name).font(.system(size: 12, weight: .semibold)).lineLimit(1)
            } else {
                platformFilter
            }
            Spacer(minLength: 8)
            if isLoading { ProgressView().controlSize(.small).accessibilityLabel("Refreshing collection") }
            Text("\(browsed.count) \(browsed.count == 1 ? "item" : "items")")
                .font(.system(size: 11)).foregroundStyle(.secondary).monospacedDigit()
        }
    }

    private var platformFilter: some View {
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
                Button { kind = nil } label: {
                    Label("All items", systemImage: kind == nil ? "checkmark" : "square.grid.2x2")
                }
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
        if openFolder != nil { return "Nothing here yet" }
        if platform == .ps5 { return "No PS5 games yet" }
        if platform == .browser { return "No browser games yet" }
        return kind == .app ? "No apps yet" : "Your collection starts here"
    }

    private var emptyDescription: String {
        let search = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if !search.isEmpty { return "Nothing in this view matches “\(search)”." }
        if isEmptyTypeFilter { return "Change the item filter to see the rest of your collection." }
        if openFolder != nil { return "Install apps with +, or open this bottle’s store to add games." }
        if platform == .ps5 { return "Import a game you own with the + menu." }
        if platform == .browser { return "Save a game from Discover, or add your own link with +." }
        if kind == .app { return "Add an app from Discover or the + menu." }
        return "Add games and apps from Discover or the + menu."
    }

    private func selectFolder(_ folder: CollectionFolder) {
        clickRouter.remember(NSApp.currentEvent, id: "folder:" + folder.id) { id in
            guard openFolderID == folder.id,
                  let current = folders.first(where: { "folder:" + $0.id == id }), current.canOpen else { return }
            onOpenFolder(current)
        }
        onSelectFolder(folder)
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

private struct CollectionFolderCard: View {
    let folder: CollectionFolder
    let entries: [CollectionItem]
    let onSelect: () -> Void
    let onOpen: () -> Void
    @State private var hovered = false

    private var summary: String {
        let games = entries.filter { $0.kind == .game }.count
        let apps = entries.filter { $0.kind == .app }.count
        var parts: [String] = []
        if games > 0 { parts.append("\(games) \(games == 1 ? "game" : "games")") }
        if apps > 0 { parts.append("\(apps) \(apps == 1 ? "app" : "apps")") }
        return parts.isEmpty ? "Empty bottle" : parts.joined(separator: " · ")
    }

    var body: some View {
        Button(action: onSelect) {
            VStack(spacing: 7) {
                Color.clear.aspectRatio(16.0 / 9.0, contentMode: .fit)
                    .overlay {
                        ZStack {
                            Image(systemName: "folder.fill")
                                .font(.system(size: 76, weight: .regular))
                                .foregroundStyle(Color(red: 0.34, green: 0.57, blue: 0.73))
                                .shadow(color: .black.opacity(0.12), radius: 3, y: 2)
                            if let icon = folder.icon {
                                Image(nsImage: icon).resizable().interpolation(.high).scaledToFit()
                                    .frame(width: 34, height: 34).offset(y: 8)
                            } else {
                                Image(systemName: "app.fill").font(.system(size: 25))
                                    .foregroundStyle(.white.opacity(0.8)).offset(y: 8)
                            }
                        }
                    }
                    .accessibilityHidden(true)
                Text(folder.name).font(.system(size: 12, weight: .semibold)).lineLimit(2)
                    .multilineTextAlignment(.center)
                    .foregroundStyle(hovered ? Color.accentColor : Color.primary)
                    .frame(height: 36, alignment: .top).frame(maxWidth: .infinity)
                Text(summary).font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                Text(" ").font(.system(size: 10)).frame(height: 12).accessibilityHidden(true)
            }
            .padding(4).frame(maxWidth: .infinity, alignment: .top)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain).onHover { hovered = $0 }
        .help("Click to view \(folder.name)." + (folder.canOpen ? " Double-click to open \(folder.primaryAppName ?? "its app")." : ""))
        .accessibilityLabel("\(folder.name), bottle folder, \(summary)")
        .accessibilityAction(named: Text("View bottle")) { onSelect() }
        .contextMenu { if folder.canOpen { Button("Open \(folder.primaryAppName ?? "app")", action: onOpen) } }
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
        return item.subtitle.isEmpty ? item.platform.title : (item.platform == .windows ? item.subtitle.components(separatedBy: " · ").first ?? item.subtitle : item.subtitle)
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
                                .aspectRatio(contentMode: .fit)
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
