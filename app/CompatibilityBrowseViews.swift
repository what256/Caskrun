import AppKit
import SwiftUI

struct AddBrowserGameRequest: Identifiable {
    let id = UUID()
    var name = ""
    var url = ""
}

enum DiscoverScope: String, CaseIterable, Identifiable {
    case stores = "Stores", games = "Game reports", browser = "Browser games"
    var id: Self { self }
    var symbol: String {
        switch self { case .stores: "square.grid.2x2"; case .games: "gamecontroller"; case .browser: "globe" }
    }
}

struct DiscoverScopeBar: View {
    @Binding var selection: DiscoverScope
    @Namespace private var highlight
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: 3) {
            ForEach(DiscoverScope.allCases) { scope in
                Button {
                    withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.18)) { selection = scope }
                } label: {
                    Label(scope.rawValue, systemImage: scope.symbol)
                        .font(.system(size: 11, weight: selection == scope ? .semibold : .medium))
                        .padding(.horizontal, 12).frame(height: 29)
                        .background {
                            if selection == scope {
                                Capsule().fill(.primary.opacity(0.10)).matchedGeometryEffect(id: "scope", in: highlight)
                            }
                        }
                        .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .foregroundStyle(selection == scope ? .primary : .secondary)
                .accessibilityAddTraits(selection == scope ? .isSelected : [])
            }
        }
        .padding(4)
        .background(.regularMaterial, in: Capsule())
        .overlay { Capsule().strokeBorder(.primary.opacity(0.07), lineWidth: 1) }
        .padding(.top, 5).padding(.bottom, 7)
    }
}

struct CompatibilityBrowseView: View {
    let query: String
    let device: CompatibilityDevice
    let canInstall: Bool
    let install: (CompatibilityGame) -> Void
    @State private var selectedID: UInt32?
    @State private var filter: Filter = .all
    private enum Filter: String, CaseIterable { case all = "All games", reports = "With reports", chip = "Same chip" }

    private var games: [CompatibilityGame] {
        GameCompatibilityCatalog.shared.search(query, limit: 1024).filter { game in
            switch filter {
            case .all: true
            case .reports: !game.reports.isEmpty
            case .chip: device.chip != nil && game.reports.contains { $0.chip == device.chip }
            }
        }
    }
    private var selection: CompatibilityGame? {
        games.first { $0.id == selectedID }
    }

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    HStack(alignment: .top) {
                        VStack(alignment: .leading, spacing: 5) {
                            Text("Find your next game").font(.title2.weight(.semibold))
                            Label(device.summary.isEmpty ? "Apple Silicon Mac" : device.summary, systemImage: "laptopcomputer")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Picker("Reports", selection: $filter) {
                            ForEach(Filter.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                        }.labelsHidden().frame(width: 140)
                    }
                    Text("Community evidence for Mac gaming. Select a game to compare the reported hardware, engine and graphics mode. These are not Caskrun test results.")
                        .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    if let error = GameCompatibilityCatalog.shared.error {
                        ContentUnavailableView("Reports unavailable", systemImage: "doc.text.magnifyingglass", description: Text(error))
                    } else if games.isEmpty {
                        ContentUnavailableView("No matching reports", systemImage: "magnifyingglass",
                                               description: Text("Try another title or show all games. You can still install a game by its Steam link from the + menu."))
                    } else {
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 165, maximum: 220), spacing: 10)], spacing: 10) {
                            ForEach(games) { game in
                                Button { selectedID = game.id } label: {
                                    VStack(alignment: .leading, spacing: 9) {
                                        GameArtwork(seed: "steam:\(game.steamAppID)", name: game.title, size: 38)
                                        Text(game.title).font(.system(size: 13, weight: .semibold)).lineLimit(2).frame(height: 34, alignment: .topLeading)
                                        Text(game.summary).font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                                    }
                                    .frame(maxWidth: .infinity, alignment: .leading).padding(14)
                                    .background(selectedID == game.id ? Color.accentColor.opacity(0.10) : Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 14))
                                    .overlay { RoundedRectangle(cornerRadius: 14).strokeBorder(selectedID == game.id ? Color.accentColor.opacity(0.45) : .primary.opacity(0.06)) }
                                    .contentShape(RoundedRectangle(cornerRadius: 14))
                                }.buttonStyle(.plain)
                                    .accessibilityLabel("Community reports for \(game.title)")
                                    .accessibilityAddTraits(selectedID == game.id ? .isSelected : [])
                            }
                        }
                    }
                    Text("\(games.count) games · Source: Highball community database · Offline snapshot")
                        .font(.system(size: 10)).foregroundStyle(.tertiary)
                }.padding(24)
            }.frame(maxWidth: .infinity)
            if let game = selection {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        HStack(alignment: .top) {
                            Text(game.title).font(.title3.weight(.semibold))
                            Spacer()
                            Button { selectedID = nil } label: { Image(systemName: "xmark").frame(width: 24, height: 24) }
                                .buttonStyle(.plain).accessibilityLabel("Close game reports")
                        }
                        Button { install(game) } label: { Label("Install with Steam…", systemImage: "arrow.down") }
                            .buttonStyle(.borderedProminent).disabled(!canInstall)
                        CompatibilityEvidenceView(game: game, device: device)
                    }.padding(18)
                }
                .frame(width: 310)
                .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 18))
                .padding(.trailing, 16).padding(.vertical, 18)
            }
        }
    }
}

struct SteamGameInstallRequest: Identifiable {
    let id = UUID()
    let appID: UInt32?
    let name: String?
}

struct SteamGameInstallSheet: View {
    let request: SteamGameInstallRequest
    let bottles: [Bottle]
    var isBusy = false
    let install: (UInt32, String?) throws -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var targetBottleID = ""
    @State private var address = ""
    @State private var error: String?
    private var appID: UInt32? { request.appID ?? Self.parseAppID(address) }

    static func parseAppID(_ input: String) -> UInt32? { SteamGameLink.appID(input) }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text(request.name ?? "Install a Steam game").font(.title2.weight(.semibold))
            if request.appID == nil {
                TextField("Steam store link or AppID", text: $address).textFieldStyle(.roundedBorder)
            } else if let id = request.appID {
                Text("Steam AppID \(id)").font(.caption).foregroundStyle(.secondary)
            }
            Picker("Install into", selection: $targetBottleID) {
                ForEach(bottles) { Text($0.name).tag($0.id) }
                Text("New Steam bottle").tag("")
            }
            Text(targetBottleID.isEmpty
                 ? "Caskrun will set up Steam in a new bottle, then ask Steam to install this game."
                 : "Steam will open in this bottle and show its install prompt.")
                .font(.subheadline).foregroundStyle(.secondary)
            Text("Sign in to Steam yourself. Steam checks whether you own the game and handles downloads. A community report does not guarantee Caskrun compatibility.")
                .font(.caption).foregroundStyle(.secondary)
            if let error { Text(error).font(.caption).foregroundStyle(.orange) }
            HStack {
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button(targetBottleID.isEmpty ? "Set up Steam" : "Continue in Steam") {
                    guard let appID else { return }
                    do {
                        try install(appID, targetBottleID.isEmpty ? nil : targetBottleID)
                        dismiss()
                    } catch { self.error = error.localizedDescription }
                }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction).disabled(appID == nil || isBusy)
            }
        }.padding(24).frame(width: 430)
            .onAppear { targetBottleID = bottles.first?.id ?? "" }
    }
}
