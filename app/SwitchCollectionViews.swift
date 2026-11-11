import AppKit
import SwiftUI
import UniformTypeIdentifiers

@MainActor
enum SwitchCollectionPicker {
    static func game() async -> URL? {
        let panel = NSOpenPanel()
        panel.title = "Add Switch game"; panel.message = "Choose your own game file. The original stays in place."
        panel.prompt = "Choose Game"; panel.canChooseFiles = true; panel.canChooseDirectories = false
        panel.allowedContentTypes = ["nsp", "xci", "nro"].compactMap { UTType(filenameExtension: $0) }
        return await selected(panel)
    }
    static func engine() async -> URL? {
        let panel = NSOpenPanel()
        panel.title = "Choose Suyu"; panel.message = "Choose your installed Suyu app for Apple Silicon."
        panel.prompt = "Choose App"; panel.canChooseFiles = true; panel.canChooseDirectories = false
        panel.allowedContentTypes = [.applicationBundle]
        return await selected(panel)
    }
    private static func selected(_ panel: NSOpenPanel) async -> URL? {
        await withCheckedContinuation { continuation in
            let completion: (NSApplication.ModalResponse) -> Void = { response in
                continuation.resume(returning: response == .OK ? panel.url : nil)
            }
            if let window = NSApp.keyWindow { panel.beginSheetModal(for: window, completionHandler: completion) }
            else { panel.begin(completionHandler: completion) }
        }
    }
}

struct ImportSwitchGameSheet: View {
    @ObservedObject var model: SwitchCollectionModel
    let onImported: (SwitchCollectionGame) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var gameURL: URL?
    @State private var name = ""
    @State private var error: String?
    @State private var choosing = false
    private var cleanName: String { name.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Add Switch game").font(.title2.weight(.semibold))
            Text("Keep your games together and open them with your own Suyu installation.")
                .font(.subheadline).foregroundStyle(.secondary)
            HStack(spacing: 12) {
                Image(systemName: "gamecontroller").font(.system(size: 23)).foregroundStyle(.secondary)
                VStack(alignment: .leading, spacing: 3) {
                    Text(gameURL?.lastPathComponent ?? "Choose a game file").font(.system(size: 13, weight: .medium)).lineLimit(2)
                    Text("NSP · XCI · NRO — originals stay in place").font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                Button(gameURL == nil ? "Choose…" : "Change…") { Task { await chooseGame() } }.disabled(choosing)
            }
            .padding(14).background(.primary.opacity(0.045), in: RoundedRectangle(cornerRadius: 12))
            TextField("Game name", text: $name).textFieldStyle(.roundedBorder).disabled(choosing)
            Divider()
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 5) {
                    Text("Suyu for Apple Silicon").font(.system(size: 13, weight: .medium))
                    Text(model.enginePath == nil ? "Choose Suyu now or after adding a game." : "Suyu is selected.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button(model.enginePath == nil ? "Choose App…" : "Change App…") { Task { await chooseEngine() } }.disabled(choosing)
            }
            Link("Get Suyu for macOS", destination: URL(string: "https://github.com/suyu-emu/suyu-main/releases/latest")!)
                .font(.caption)
            Text("Suyu may need your own console keys and firmware. Game compatibility varies and has not been verified by Caskrun.")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            if let error { Label(error, systemImage: "exclamationmark.circle").font(.caption).foregroundStyle(.red).textSelection(.enabled) }
            HStack {
                Spacer(); Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).disabled(choosing)
                Button("Add") { add() }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                    .disabled(choosing || gameURL == nil || cleanName.isEmpty)
            }
        }
        .padding(24).frame(width: 500).interactiveDismissDisabled(choosing)
    }
    @MainActor private func chooseGame() async {
        choosing = true; defer { choosing = false }
        guard let chosen = await SwitchCollectionPicker.game() else { return }
        if name.isEmpty || name == gameURL?.deletingPathExtension().lastPathComponent { name = chosen.deletingPathExtension().lastPathComponent }
        gameURL = chosen; error = nil
    }
    @MainActor private func chooseEngine() async {
        choosing = true; defer { choosing = false }
        guard let chosen = await SwitchCollectionPicker.engine() else { return }
        do { try model.configureEngine(chosen); error = nil } catch { self.error = error.localizedDescription }
    }
    private func add() {
        guard let gameURL else { return }
        do { let game = try model.importGame(name: cleanName, url: gameURL); onImported(game); dismiss() }
        catch { self.error = error.localizedDescription }
    }
}

struct SwitchGameInspector: View {
    @ObservedObject var model: SwitchCollectionModel
    let game: SwitchCollectionGame
    let onClose: () -> Void
    let isQuitting: Bool
    @State private var error: String?
    @State private var choosing = false
    @State private var confirmRemove = false
    private var exists: Bool { FileManager.default.fileExists(atPath: game.path) }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("SWITCH GAME").font(.system(size: 10, weight: .semibold, design: .monospaced)).tracking(1).foregroundStyle(.secondary)
                Spacer()
                Button(action: onClose) { Image(systemName: "xmark").font(.system(size: 11, weight: .semibold)).frame(width: 28, height: 28).contentShape(Rectangle()) }
                    .buttonStyle(.plain).keyboardShortcut(.escape, modifiers: []).help("Close game details")
            }.padding(.horizontal, 18).padding(.top, 12).padding(.bottom, 8)
            Divider().padding(.horizontal, 18)
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    HStack(spacing: 12) {
                        Image(systemName: "gamecontroller.fill").font(.system(size: 25)).frame(width: 64, height: 64)
                            .background(.primary.opacity(0.045), in: RoundedRectangle(cornerRadius: 12))
                        VStack(alignment: .leading, spacing: 5) {
                            Text(game.name).font(.system(size: 19, weight: .semibold, design: .rounded)).lineLimit(3)
                            Text("Switch · Suyu").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    Button { Task { await play() } } label: {
                        Label("Open " + game.name, systemImage: "play.fill").font(.system(size: 13, weight: .semibold))
                            .frame(maxWidth: .infinity).frame(height: 30)
                    }.buttonStyle(.borderedProminent).disabled(isQuitting || model.quitting || model.enginePath == nil || model.launching || !exists)
                    Text(exists ? "Game compatibility has not been verified in Caskrun." : "Game file is unavailable. Restore it to its original location.")
                        .font(.caption).foregroundStyle(.secondary)
                    Divider()
                    VStack(alignment: .leading, spacing: 10) {
                        HStack { Text("Engine").font(.system(size: 13, weight: .semibold)); Spacer(); Text(model.enginePath == nil ? "Not selected" : "Suyu").font(.caption).foregroundStyle(.secondary) }
                        Button(model.enginePath == nil ? "Choose Suyu…" : "Change Suyu…") { Task { await chooseEngine() } }
                            .disabled(choosing || model.launching)
                        Link("Get Suyu for macOS", destination: URL(string: "https://github.com/suyu-emu/suyu-main/releases/latest")!).font(.caption)
                        Text("Suyu manages its own settings, keys, firmware and saves. Quit Suyu before opening a different game here.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Divider()
                    Button { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: game.path)]) } label: {
                        Label("Show game file in Finder", systemImage: "folder").font(.system(size: 12))
                    }.buttonStyle(.plain).disabled(!exists)
                    if let error { Label(error, systemImage: "exclamationmark.circle").font(.caption).foregroundStyle(.red).textSelection(.enabled) }
                    Divider()
                    Button(role: .destructive) { confirmRemove = true } label: {
                        Label("Remove from Collection…", systemImage: "minus.circle").font(.system(size: 12))
                    }.buttonStyle(.plain)
                }.padding(18)
            }.frame(maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
        }
        .frame(width: 320).frame(maxHeight: .infinity, alignment: .top)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay { RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(.primary.opacity(0.10)).allowsHitTesting(false) }
        .shadow(color: .black.opacity(0.10), radius: 12, y: 5)
        .alert("Remove " + game.name + "?", isPresented: $confirmRemove) {
            Button("Cancel", role: .cancel) {}
            Button("Remove from Collection", role: .destructive) {
                do { try model.remove(game); onClose() } catch { self.error = error.localizedDescription }
            }
        } message: { Text("Only the Collection entry is removed. Your original game file and Suyu stay in place.") }
        .onChange(of: game.id) { _, _ in error = nil }
    }
    @MainActor private func chooseEngine() async {
        choosing = true; defer { choosing = false }
        guard let chosen = await SwitchCollectionPicker.engine() else { return }
        do { try model.configureEngine(chosen); error = nil } catch { self.error = error.localizedDescription }
    }
    @MainActor private func play() async {
        guard !isQuitting else { return }
        do { try await model.launch(game); error = nil } catch { self.error = error.localizedDescription }
    }
}
