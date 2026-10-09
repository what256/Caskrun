import AppKit
import SwiftUI
import UniformTypeIdentifiers

@MainActor
enum PS5CollectionPicker {
    static func game() async -> URL? {
        let panel = NSOpenPanel()
        panel.title = "Import PS5 game"
        panel.message = "Choose a game folder, ELF executable or ZAR archive."
        panel.prompt = "Choose Game"
        panel.canChooseFiles = true
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [.folder] + ["elf", "zar"].compactMap { UTType(filenameExtension: $0) }
        return await selectedURL(from: panel)
    }

    static func engine() async -> URL? {
        let panel = NSOpenPanel()
        panel.title = "Choose PS5 engine"
        panel.message = "Choose your complete KytyPS5 Mac folder or app, including its MoltenVK library."
        panel.prompt = "Choose Package"
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.treatsFilePackagesAsDirectories = true
        panel.allowedContentTypes = [.folder, .applicationBundle]
        return await selectedURL(from: panel)
    }

    private static func selectedURL(from panel: NSOpenPanel) async -> URL? {
        await withCheckedContinuation { continuation in
            let completion: (NSApplication.ModalResponse) -> Void = { result in
                continuation.resume(returning: result == .OK ? panel.url : nil)
            }
            if let window = NSApp.keyWindow {
                panel.beginSheetModal(for: window, completionHandler: completion)
            } else {
                panel.begin(completionHandler: completion)
            }
        }
    }
}

struct ImportPS5GameSheet: View {
    @ObservedObject var model: PS5CollectionModel
    let onImported: (PS5CollectionGame) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var gameURL: URL?
    @State private var name = ""
    @State private var error: String?
    @State private var selecting = false
    @State private var importing = false

    private var busy: Bool { selecting || importing || model.busy }
    private var cleanName: String { name.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var validName: Bool {
        !cleanName.isEmpty && cleanName.utf8.count <= 240 &&
            !cleanName.unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack {
                Text("Import PS5 game").font(.title2.weight(.semibold))
                Spacer()
                PS5ExperimentalBadge()
            }
            Text("Add a game folder, ELF executable or ZAR archive to your Collection.")
                .font(.subheadline).foregroundStyle(.secondary)

            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 12) {
                    Image(systemName: gameURL?.hasDirectoryPath == true ? "folder" : "doc")
                        .font(.system(size: 23)).foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(gameURL?.lastPathComponent ?? "Choose your game files")
                            .font(.system(size: 13, weight: .medium)).lineLimit(2)
                        Text(gameURL == nil ? "Folder · ELF · ZAR" : "Original files stay in place")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 8)
                    Button(gameURL == nil ? "Choose…" : "Change…") { Task { await chooseGame() } }
                        .disabled(busy)
                }
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.primary.opacity(0.045), in: RoundedRectangle(cornerRadius: 12))
                if let gameURL {
                    Text(gameURL.path).font(.caption).foregroundStyle(.secondary)
                        .lineLimit(2).truncationMode(.middle).textSelection(.enabled)
                }
            }

            TextField("Game name", text: $name).textFieldStyle(.roundedBorder)
                .accessibilityLabel("PS5 game name").disabled(busy)

            Divider()
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 5) {
                    Text("PS5 engine").font(.system(size: 13, weight: .medium))
                    Text(model.engine == nil ? "Choose KytyPS5 now or after importing." : "KytyPS5 is selected.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button(model.engine == nil ? "Choose Engine…" : "Change Engine…") {
                    Task { await chooseEngine() }
                }
                .disabled(busy || !model.runningGameIDs.isEmpty)
            }
            Text("Experimental: importing does not verify game compatibility. Controller, graphics and save support depend on KytyPS5 and the game.")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)

            if let error {
                Label(error, systemImage: "exclamationmark.circle")
                    .font(.caption).foregroundStyle(.red).textSelection(.enabled)
            }
            HStack {
                if importing { ProgressView().controlSize(.small) }
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).disabled(busy)
                Button("Import") { Task { await importGame() } }
                    .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                    .disabled(busy || gameURL == nil || !validName)
            }
        }
        .padding(24).frame(width: 500)
        .interactiveDismissDisabled(busy)
    }

    @MainActor private func chooseGame() async {
        guard !busy else { return }
        selecting = true
        defer { selecting = false }
        guard let url = await PS5CollectionPicker.game() else { return }
        let previousSuggestion = gameURL.map {
            $0.hasDirectoryPath ? $0.lastPathComponent : $0.deletingPathExtension().lastPathComponent
        }
        gameURL = url
        if cleanName.isEmpty || cleanName == previousSuggestion {
            name = url.hasDirectoryPath ? url.lastPathComponent : url.deletingPathExtension().lastPathComponent
        }
        error = nil
    }

    @MainActor private func chooseEngine() async {
        guard !busy else { return }
        selecting = true
        defer { selecting = false }
        guard let url = await PS5CollectionPicker.engine() else { return }
        do { try await model.configureEngine(path: url); error = nil }
        catch { self.error = error.localizedDescription }
    }

    @MainActor private func importGame() async {
        guard !busy, validName, let gameURL else { return }
        importing = true
        defer { importing = false }
        error = nil
        do {
            let game = try await model.importGame(name: cleanName, path: gameURL)
            onImported(game)
            dismiss()
        } catch { self.error = error.localizedDescription }
    }
}

struct PS5GameArtwork: View {
    @ObservedObject var model: PS5CollectionModel
    let game: PS5CollectionGame
    @State private var artwork: NSImage?

    var body: some View {
        Group {
            if let artwork {
                Image(nsImage: artwork).resizable().interpolation(.high).scaledToFit()
            } else {
                ZStack {
                    RoundedRectangle(cornerRadius: 12).fill(.primary.opacity(0.045))
                    Image(systemName: "gamecontroller").font(.system(size: 30)).foregroundStyle(.secondary)
                }
            }
        }
        .accessibilityHidden(true)
        .task(id: game) {
            artwork = nil
            let loaded = await model.gameArtwork(game)
            guard !Task.isCancelled else { return }
            artwork = loaded
        }
    }
}

struct PS5GameInspector: View {
    @ObservedObject var model: PS5CollectionModel
    let game: PS5CollectionGame
    let onClose: () -> Void
    @State private var selectingEngine = false
    @State private var stopping = false
    @State private var confirmRemove = false
    @State private var error: String?
    @State private var showEngineDetails = false

    private var running: Bool { model.runningGameIDs.contains(game.id) }
    private var busy: Bool { model.busy || selectingEngine || stopping }
    private var gameURL: URL { URL(fileURLWithPath: game.path) }
    private var gameExists: Bool { FileManager.default.fileExists(atPath: game.path) }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("PS5 GAME").font(.system(size: 10, weight: .semibold, design: .monospaced))
                    .tracking(1).foregroundStyle(.secondary)
                Spacer()
                Button(action: onClose) {
                    Image(systemName: "xmark").font(.system(size: 11, weight: .semibold))
                        .frame(width: 28, height: 28).contentShape(Rectangle())
                }
                .buttonStyle(.plain).keyboardShortcut(.escape, modifiers: [])
                .help("Close game details (Esc)").accessibilityLabel("Close game details")
            }
            .padding(.horizontal, 18).padding(.top, 12).padding(.bottom, 8)
            Divider().padding(.horizontal, 18)
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    HStack(alignment: .center, spacing: 12) {
                        PS5GameArtwork(model: model, game: game)
                            .frame(width: 64, height: 64).clipShape(RoundedRectangle(cornerRadius: 12))
                        VStack(alignment: .leading, spacing: 6) {
                            Text(game.name).font(.system(size: 19, weight: .semibold, design: .rounded))
                                .lineLimit(3)
                            PS5ExperimentalBadge()
                        }
                        Spacer(minLength: 0)
                    }

                    VStack(alignment: .leading, spacing: 10) {
                        Button { openGame() } label: {
                            Label(running ? "Show Window" : "Open " + game.name,
                                  systemImage: running ? "macwindow" : "play.fill")
                                .font(.system(size: 13, weight: .semibold))
                                .frame(maxWidth: .infinity).frame(height: 30)
                        }
                        .buttonStyle(.borderedProminent).tint(.accentColor)
                        .disabled(busy || (!running && (model.engine == nil || !gameExists)))
                        if running {
                            Button { Task { await stopGame() } } label: {
                                Label(stopping ? "Stopping…" : "Stop Game", systemImage: "stop.fill")
                                    .frame(maxWidth: .infinity).frame(height: 26)
                            }
                            .buttonStyle(.bordered).disabled(busy)
                        }
                        if !gameExists {
                            Label("Game files are unavailable. Restore them to their imported location.",
                                  systemImage: "folder.badge.questionmark")
                                .font(.caption).foregroundStyle(.secondary)
                        } else {
                            Text(running ? "Running with KytyPS5." : "Compatibility has not been verified in Caskrun.")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }

                    Divider()
                    VStack(alignment: .leading, spacing: 10) {
                        HStack {
                            Text("Engine").font(.system(size: 13, weight: .semibold))
                            Spacer()
                            Text(model.engine == nil ? "Not selected" : "KytyPS5")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        action(model.engine == nil ? "Choose Engine…" : "Change Engine…", symbol: "cpu") {
                            Task { await chooseEngine() }
                        }
                        .disabled(busy || !model.runningGameIDs.isEmpty)
                        if let engine = model.engine {
                            DisclosureGroup("Engine Details", isExpanded: $showEngineDetails) {
                                VStack(alignment: .leading, spacing: 6) {
                                    Text("Version: " + engine.version).font(.caption)
                                    Text(engine.path).font(.caption).foregroundStyle(.secondary)
                                        .lineLimit(3).truncationMode(.middle).textSelection(.enabled)
                                }
                                .padding(.top, 8)
                            }
                            .font(.system(size: 12))
                        }
                        Text("Uses your local engine. Choosing it checks the package; Open starts the game.")
                            .font(.caption).foregroundStyle(.secondary)
                    }

                    Divider()
                    VStack(alignment: .leading, spacing: 10) {
                        Text("Game Files").font(.system(size: 13, weight: .semibold))
                        action("Show in Finder", symbol: "folder") {
                            guard gameExists else { return }
                            NSWorkspace.shared.activateFileViewerSelecting([gameURL])
                        }
                        .disabled(!gameExists)
                        if let logsURL = model.logsURL(for: game) {
                            action("Show Engine Logs", symbol: "doc.text") {
                                guard FileManager.default.fileExists(atPath: logsURL.path) else { return }
                                NSWorkspace.shared.activateFileViewerSelecting([logsURL])
                            }
                        }
                        Text("Controller, graphics and save support are managed by KytyPS5. Available features vary by game.")
                            .font(.caption).foregroundStyle(.secondary)
                    }

                    if let error {
                        Label(error, systemImage: "exclamationmark.circle")
                            .font(.caption).foregroundStyle(.red).textSelection(.enabled)
                    }
                    Divider()
                    Button(role: .destructive) { confirmRemove = true } label: {
                        Label("Remove from Collection…", systemImage: "minus.circle")
                            .font(.system(size: 12)).frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .buttonStyle(.plain).disabled(busy || running)
                }
                .padding(18)
            }
            .frame(maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
        }
        .frame(width: 320)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(.primary.opacity(0.10))
                .allowsHitTesting(false)
        }
        .shadow(color: .black.opacity(0.10), radius: 12, y: 5)
        .alert("Remove " + game.name + "?", isPresented: $confirmRemove) {
            Button("Cancel", role: .cancel) {}
            Button("Remove from Collection", role: .destructive) {
                do { try model.removeGame(game); onClose() }
                catch { self.error = error.localizedDescription }
            }
        } message: {
            Text("Removes its entry from Caskrun. Your original game files and engine files stay in place.")
        }
        .onChange(of: game.id) { _, _ in error = nil; showEngineDetails = false }
    }

    private func action(_ title: String, symbol: String, perform: @escaping () -> Void) -> some View {
        Button(action: perform) {
            Label(title, systemImage: symbol).font(.system(size: 12))
                .frame(maxWidth: .infinity, minHeight: 26, alignment: .leading)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    @MainActor private func chooseEngine() async {
        guard !busy, model.runningGameIDs.isEmpty else { return }
        selectingEngine = true
        defer { selectingEngine = false }
        guard let url = await PS5CollectionPicker.engine() else { return }
        do { try await model.configureEngine(path: url); error = nil }
        catch { self.error = error.localizedDescription }
    }

    @MainActor private func openGame() {
        guard !busy else { return }
        error = nil
        if running {
            if !model.focus(game) { error = "The engine window is not available yet. It may still be starting." }
        } else {
            do { try model.launch(game) }
            catch { self.error = error.localizedDescription }
        }
    }

    @MainActor private func stopGame() async {
        guard !busy, running else { return }
        stopping = true
        defer { stopping = false }
        error = nil
        do { try await model.stop(game) }
        catch { self.error = error.localizedDescription }
    }
}

private struct PS5ExperimentalBadge: View {
    var body: some View {
        Text("Experimental").font(.system(size: 10, weight: .medium))
            .foregroundStyle(.secondary).padding(.horizontal, 8).padding(.vertical, 4)
            .background(.primary.opacity(0.06), in: Capsule())
    }
}
