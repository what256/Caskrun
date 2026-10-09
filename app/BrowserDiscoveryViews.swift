import SwiftUI

/// A picture-led catalogue card. Longer descriptions and attribution live in
/// the information popover so the catalogue stays easy to scan.
struct BrowserDiscoverCard: View {
    let entry: BrowserGameCatalogEntry
    let isSaved: Bool
    let onPlay: () -> Void
    let onAdd: () -> Void
    @State private var showingInformation = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            BrowserGameCover(entry: entry)
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .top, spacing: 8) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(entry.name)
                            .font(.system(size: 14, weight: .semibold))
                            .lineLimit(2)
                        Text(entry.url.host ?? "Browser game")
                            .font(.system(size: 10))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    Spacer(minLength: 0)
                    Button { showingInformation = true } label: {
                        Image(systemName: "info.circle")
                            .font(.system(size: 13))
                            .foregroundStyle(.secondary)
                            .frame(width: 24, height: 24)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help("About \(entry.name)")
                    .accessibilityLabel("About \(entry.name)")
                    .popover(isPresented: $showingInformation, arrowEdge: .bottom) {
                        BrowserCatalogInformation(entry: entry)
                            .padding(18).frame(width: 300)
                    }
                }
                BrowserCatalogNotices(entry: entry)
                HStack(spacing: 8) {
                    Button(action: onPlay) {
                        Label(entry.browserActionTitle, systemImage: entry.kind == .collection ? "square.grid.2x2" : "play.fill")
                    }
                    .buttonStyle(.borderedProminent).controlSize(.small)
                    .help(entry.browserActionTitle == "Play" ? "Play \(entry.name) in your chosen browser" : entry.browserActionTitle)
                    Spacer(minLength: 4)
                    if isSaved {
                        Image(systemName: "checkmark")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(.secondary)
                            .frame(width: 26, height: 26)
                            .help("Saved to Collection")
                            .accessibilityLabel("Saved to Collection")
                    } else {
                        Button(action: onAdd) { Label("Add", systemImage: "plus") }
                            .buttonStyle(.bordered).controlSize(.small)
                            .help("Add \(entry.name) to Collection")
                            .accessibilityLabel("Add \(entry.name) to Collection")
                    }
                }
            }
            .padding(.horizontal, 2)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .padding(.bottom, 8)
    }
}

/// Content only: the Collection inspector supplies its existing close control,
/// material, finite viewport and scroll view.
struct BrowserGameInspectorContent: View {
    let game: BrowserGame
    let isQuitting: Bool
    let isEditing: Bool
    let onPlay: () -> Void
    let onRemove: () -> Void
    @State private var confirmingRemoval = false

    private var entry: BrowserGameCatalogEntry? { BrowserGameCatalog.entry(forSavedURL: game.url) }
    private var url: URL? { BrowserGame.checkedURL(game.url) }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            VStack(alignment: .leading, spacing: 12) {
                if let entry {
                    BrowserGameCover(entry: entry)
                } else {
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .fill(Color.primary.opacity(0.035))
                        .aspectRatio(16.0 / 9.0, contentMode: .fit)
                        .overlay {
                            Image(systemName: "globe")
                                .font(.system(size: 28, weight: .light))
                                .foregroundStyle(.tertiary)
                        }
                        .accessibilityHidden(true)
                }
                VStack(alignment: .leading, spacing: 5) {
                    Text(game.name)
                        .font(.system(size: 20, weight: .semibold))
                        .fixedSize(horizontal: false, vertical: true)
                    if let url {
                        Link(url.host ?? "Game website", destination: url)
                            .font(.system(size: 11))
                            .foregroundStyle(.secondary)
                    }
                }
            }
            VStack(alignment: .leading, spacing: 10) {
                BrowserChoiceMenu(gameURL: game.url)
                    .disabled(isQuitting)
                Button(action: onPlay) {
                    Label(entry?.browserInspectorActionTitle ?? "Play in browser", systemImage: "play.fill")
                        .font(.system(size: 13, weight: .semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 5)
                }
                .buttonStyle(.borderedProminent)
                .disabled(isQuitting || url == nil)
                Text("Your browser runs the game. Its tabs stay open when Caskrun closes.")
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let entry {
                Divider()
                VStack(alignment: .leading, spacing: 10) {
                    BrowserCatalogNotices(entry: entry)
                    Text(entry.detail)
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Link(destination: entry.sourceURL) {
                        Label("Project website", systemImage: "arrow.up.right")
                            .font(.system(size: 11))
                    }
                    .foregroundStyle(.secondary)
                }
            }
            Divider()
            Button(role: .destructive) { confirmingRemoval = true } label: {
                Label("Remove from Collection", systemImage: "trash")
                    .font(.system(size: 11))
            }
            .buttonStyle(.borderless)
            .disabled(isEditing || isQuitting)
            .alert("Remove \(game.name) from Collection?", isPresented: $confirmingRemoval) {
                Button("Cancel", role: .cancel) {}
                Button("Remove", role: .destructive, action: onRemove)
            } message: {
                Text("Only this saved shortcut is removed. Browser tabs and saved game data stay untouched.")
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct BrowserCatalogInformation: View {
    let entry: BrowserGameCatalogEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(entry.name).font(.system(size: 15, weight: .semibold))
            BrowserCatalogNotices(entry: entry)
            Text(entry.detail)
                .font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
            Text("Community ports and recreations are separate from official releases. Gameplay is untested.")
                .font(.system(size: 11)).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Link(destination: entry.sourceURL) {
                Label("Project website", systemImage: "arrow.up.right")
                    .font(.system(size: 12))
            }
        }
    }
}

private struct BrowserCatalogNotices: View {
    let entry: BrowserGameCatalogEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if entry.requiresOwnFiles {
                Label("Requires your game files", systemImage: "folder")
            }
            if entry.kind == .unverified {
                Label("Link unverified", systemImage: "questionmark.circle")
            }
        }
        .font(.system(size: 10))
        .foregroundStyle(.secondary)
    }
}

private extension BrowserGameCatalogEntry {
    var browserActionTitle: String {
        switch kind {
        case .game: "Play"
        case .collection: "Browse games"
        case .unverified: "Visit website"
        }
    }

    var browserInspectorActionTitle: String {
        kind == .game ? "Play in browser" : browserActionTitle
    }
}
