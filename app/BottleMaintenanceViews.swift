import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct IconRepairResult: Decodable, Sendable {
    let bottle_id: String
    let removed_native_apps: Int
    let removed_artwork_entries: Int
    let message: String
}

struct RegisteredProgram: Decodable, Identifiable, Sendable {
    let name: String
    let version: String?
    let publisher: String?
    let sources: [String]
    var id: String { [name, version ?? "", publisher ?? ""].joined(separator: "\u{0}") }
}

struct ProgramInventory: Decodable, Sendable {
    let bottle_id: String
    let initialized: Bool
    let complete: Bool
    let count: Int
    let programs: [RegisteredProgram]
    let warnings: [String]
}

struct OneTimeProgram: Identifiable {
    let id = UUID()
    let bottle: Bottle
    let url: URL
    let savedLaunchers: [SavedLauncher]
}

struct BottleNotes: Decodable, Sendable {
    let bottle_id: String
    let notes: String
}

struct BottleWindowsVersion: Decodable, Sendable {
    let bottle_id: String
    let selected_version: String
    let effective_version: String?
    let warning: String?
}

struct BottleNotesView: View {
    @ObservedObject var model: CaskrunModel
    let bottle: Bottle
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var savedText: String?
    @State private var error: String?
    @State private var loading = true
    @State private var saving = false
    @State private var confirmDiscard = false
    private var changed: Bool { savedText != nil && text != savedText }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Notes for \(bottle.name)").font(.title2.weight(.semibold))
            Text("Keep install steps or game settings with this bottle. Notes are included in its backup.")
                .font(.caption).foregroundStyle(.secondary)
            if loading { ProgressView("Loading notes…").frame(maxWidth: .infinity, minHeight: 220) }
            else if savedText != nil {
                TextEditor(text: $text).font(.body).frame(height: 240)
                    .overlay { RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.15)) }
                    .accessibilityLabel("Bottle notes")
                    .disabled(saving)
                Text("\(text.utf8.count) / 16384 bytes").font(.caption).foregroundStyle(.secondary)
            }
            if let error { Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
            HStack {
                if savedText == nil && !loading { Button("Retry") { Task { await load() } } }
                Spacer()
                Button("Cancel") { if changed { confirmDiscard = true } else { dismiss() } }
                    .keyboardShortcut(.cancelAction).disabled(saving)
                Button("Save") { Task { await save() } }
                    .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                    .disabled(loading || saving || !changed || text.utf8.count > 16 * 1024 || text.contains("\0") || model.isQuitting || model.isManagingBottle || model.isLaunching)
            }
        }
        .padding(24).frame(width: 540)
        .interactiveDismissDisabled(changed || saving)
        .task { await load() }
        .alert("Discard changes?", isPresented: $confirmDiscard) {
            Button("Keep Editing", role: .cancel) {}
            Button("Discard", role: .destructive) { dismiss() }
        } message: { Text("Your unsaved note changes will be discarded.") }
    }

    @MainActor private func load() async {
        loading = true
        defer { loading = false }
        error = nil
        do {
            let result = try await model.notes(in: bottle)
            guard result.bottle_id == bottle.id else { throw CocoaError(.coderInvalidValue) }
            text = result.notes
            savedText = result.notes
        } catch { self.error = error.localizedDescription }
    }

    @MainActor private func save() async {
        guard !saving else { return }
        saving = true
        defer { saving = false }
        do { try await model.saveNotes(text, in: bottle); savedText = text; dismiss() }
        catch { self.error = error.localizedDescription }
    }
}

struct OneTimeRunSheet: View {
    let program: OneTimeProgram
    @ObservedObject var model: CaskrunModel
    @Environment(\.dismiss) private var dismiss
    @State private var argumentLines = ""
    @State private var selectedLauncherID: String

    init(program: OneTimeProgram, model: CaskrunModel) {
        self.program = program
        self.model = model
        _selectedLauncherID = State(initialValue: program.savedLaunchers.first?.id ?? "")
    }

    private var selectedLauncher: SavedLauncher? {
        program.savedLaunchers.first { $0.id == selectedLauncherID }
    }

    private var arguments: [String] { LaunchArgumentDraft.parse(argumentLines) }
    private var validation: String? {
        LaunchArgumentDraft.validation(arguments, saved: selectedLauncher?.saved_arguments ?? [])
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Run a program").font(.title2.weight(.semibold))
            Label(selectedLauncher?.name ?? program.url.lastPathComponent, systemImage: "app")
                .font(.headline).lineLimit(2)
            Text("Run once in \(program.bottle.name). This does not add a new Home icon.")
                .foregroundStyle(.secondary)
            if program.savedLaunchers.count > 1 {
                Picker("Saved launch options", selection: $selectedLauncherID) {
                    ForEach(program.savedLaunchers) { Text($0.name).tag($0.id) }
                }
            }
            if let directory = selectedLauncher?.working_directory {
                Text("Working folder: \(directory)").font(.caption).foregroundStyle(.secondary)
            }
            Text("Arguments — one per line").font(.subheadline.weight(.semibold))
            TextEditor(text: $argumentLines)
                .font(.system(.body, design: .monospaced)).frame(height: 140)
                .overlay { RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.15)) }
                .accessibilityLabel("Temporary arguments, one per line")
            Text("Leave empty to open normally. Each line is one argument; spaces are kept. Do not add shell quotes. Saved app options are kept automatically.")
                .font(.caption).foregroundStyle(.secondary)
            if let validation { Text(validation).font(.caption).foregroundStyle(.red) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Run") {
                    model.runOnce(program, arguments: arguments, launcher: selectedLauncher)
                    dismiss()
                }
                .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                .disabled(validation != nil || model.isQuitting || model.isLaunching || model.isManagingBottle)
            }
        }
        .padding(24).frame(width: 500)
    }
}

struct RenameLauncherSheet: View {
    let launcher: SavedLauncher
    @ObservedObject var model: CaskrunModel
    let onSave: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var name: String

    init(launcher: SavedLauncher, model: CaskrunModel, onSave: @escaping (String) -> Void) {
        self.launcher = launcher
        self.model = model
        self.onSave = onSave
        _name = State(initialValue: launcher.name)
    }

    private var valid: Bool {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return !trimmed.isEmpty && trimmed.unicodeScalars.count <= 80 && !trimmed.unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Rename app").font(.title2.weight(.semibold))
            TextField("App name", text: $name).textFieldStyle(.roundedBorder)
                .accessibilityLabel("App name")
            Text("Changes its name in Caskrun. The installed program and its launch options stay in place.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save") { onSave(name); dismiss() }
                    .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                    .disabled(!valid || model.isQuitting || model.isLaunching || model.isManagingBottle)
            }
        }
        .padding(24).frame(width: 420)
    }
}

struct InstalledProgramsView: View {
    @ObservedObject var model: CaskrunModel
    let bottle: Bottle
    @Environment(\.dismiss) private var dismiss
    @State private var inventory: ProgramInventory?
    @State private var error: String?
    @State private var loading = false
    @State private var query = ""

    private var programs: [RegisteredProgram] {
        let all = inventory?.programs ?? []
        return query.isEmpty ? all : all.filter {
            $0.name.localizedCaseInsensitiveContains(query) || ($0.publisher ?? "").localizedCaseInsensitiveContains(query)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Installed Programs").font(.title2.weight(.semibold))
                    Text(bottle.name).foregroundStyle(.secondary)
                }
                Spacer()
                Button("Refresh") { Task { await load() } }.disabled(loading || model.isQuitting)
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            Text("Programs registered by Windows installers. Portable apps and individual games may not appear here.")
                .font(.caption).foregroundStyle(.secondary)
            TextField("Search programs", text: $query).textFieldStyle(.roundedBorder)
            if loading && inventory == nil { ProgressView("Reading installed programs…").frame(maxWidth: .infinity, minHeight: 200) }
            else if let error {
                ContentUnavailableView("Could not read programs", systemImage: "exclamationmark.triangle", description: Text(error))
            } else if let inventory {
                if !inventory.complete {
                    Label("Some registry information could not be read.", systemImage: "exclamationmark.triangle")
                        .font(.caption).foregroundStyle(.orange)
                    ForEach(Array(inventory.warnings.enumerated()), id: \.offset) { _, warning in
                        Text(warning).font(.caption).foregroundStyle(.secondary)
                    }
                }
                if programs.isEmpty {
                    ContentUnavailableView(query.isEmpty ? "No registered programs" : "No matching programs",
                                           systemImage: "square.stack.3d.up",
                                           description: Text(inventory.initialized ? "Use Find installed apps to add a portable program to Home." : "Install an app to set up this bottle."))
                } else {
                    List(programs) { program in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(program.name).font(.body.weight(.medium)).textSelection(.enabled)
                            let details = [program.version, program.publisher].compactMap { $0 }.filter { !$0.isEmpty }
                            if !details.isEmpty { Text(details.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                        }
                        .padding(.vertical, 4)
                    }
                    .listStyle(.inset)
                }
                HStack {
                    Text("\(inventory.count) registered programs").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("Manage Installed Programs…") { model.openUninstaller(in: bottle); dismiss() }
                        .disabled(model.isQuitting || model.isLaunching || model.isManagingBottle || model.status.wine_path == nil)
                }
            }
        }
        .padding(24).frame(width: 600, height: 500)
        .task { await load() }
    }

    @MainActor private func load() async {
        guard !loading else { return }
        loading = true
        defer { loading = false }
        error = nil
        do {
            let result = try await model.installedPrograms(in: bottle)
            guard result.bottle_id == bottle.id else { throw CocoaError(.coderInvalidValue) }
            inventory = result
        } catch { self.error = error.localizedDescription }
    }
}

@MainActor
enum LocalReportSave {
    static func text(_ text: String, name: String, message: String, type: UTType = .plainText) throws {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = name
        panel.allowedContentTypes = [type]
        panel.message = message
        panel.canCreateDirectories = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        try Data(text.utf8).write(to: url, options: .atomic)
    }
}

struct BottleDiagnosticReport: Codable, Sendable {
    let format: String
    let version: Int
    let core_version: String
    let phase: String
    let bottle: BottleState
    let runtime: RuntimeState
    let graphics: GraphicsState
    let installer: InstallerState?
    let logs: LogState
    let issues: [String]

    struct BottleState: Codable, Sendable {
        let initialization: String
        let saved_program_count: Int?
        let visible_program_count: Int?
    }
    struct RuntimeState: Codable, Sendable {
        let availability: String
        let identity: String?
        let recorded_identity: String?
        let record_matches: Bool?
    }
    struct GraphicsState: Codable, Sendable {
        let selected_backend: String?
        let effective_backend: String?
        let dxmt_available: Bool
    }
    struct InstallerState: Codable, Sendable {
        let catalog_id: String
        let stage: String
        let has_issue: Bool
    }
    struct LogState: Codable, Sendable {
        let count: Int
        let scan_truncated: Bool
        let latest: LatestLog?
        struct LatestLog: Codable, Sendable {
            let kind: String
            let size_bytes: UInt64
            let age: String
        }
    }

    func text() throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        return String(decoding: try encoder.encode(self), as: UTF8.self) + "\n"
    }
}

struct BottleDiagnosticsView: View {
    @ObservedObject var model: CaskrunModel
    let bottle: Bottle
    @Environment(\.dismiss) private var dismiss
    @State private var report: BottleDiagnosticReport?
    @State private var error: String?
    @State private var loading = false
    @State private var exportError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Bottle Diagnostics").font(.title2.weight(.semibold))
                    Text(bottle.name).foregroundStyle(.secondary)
                }
                Spacer()
                Button("Refresh") { Task { await load() } }.disabled(loading || model.isQuitting)
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            Text("The report contains engine status and issue codes. Program names, local paths and raw logs are excluded.")
                .font(.caption).foregroundStyle(.secondary)
            if loading && report == nil {
                ProgressView("Checking bottle…").frame(maxWidth: .infinity, minHeight: 200)
            } else if let error {
                ContentUnavailableView("Could not check bottle", systemImage: "exclamationmark.triangle", description: Text(error))
            } else if let report {
                ScrollView {
                    VStack(alignment: .leading, spacing: 18) {
                        Grid(alignment: .leading, horizontalSpacing: 24, verticalSpacing: 10) {
                            diagnosticRow("Caskrun", "\(report.core_version) · \(report.phase)")
                            diagnosticRow("Bottle", title(report.bottle.initialization))
                            diagnosticRow("Engine", report.runtime.availability == "verified_bundle" && report.runtime.identity == nil
                                          ? "Unreviewed engine package" : title(report.runtime.availability))
                            diagnosticRow("Renderer", title(report.graphics.effective_backend ?? "unavailable"))
                            diagnosticRow("Saved apps", report.bottle.saved_program_count.map(String.init) ?? "Unavailable")
                            diagnosticRow("Logs", "\(report.logs.count)" + (report.logs.scan_truncated ? "+" : ""))
                            if let installer = report.installer { diagnosticRow("Last setup", title(installer.stage)) }
                        }
                        Divider()
                        if report.issues.isEmpty {
                            Label("No diagnostic issues found", systemImage: "checkmark.circle")
                                .foregroundStyle(.secondary)
                        } else {
                            ForEach(Array(report.issues.enumerated()), id: \.offset) { _, issue in
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(issueTitle(issue)).font(.subheadline.weight(.medium))
                                    Text(issue).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                HStack {
                    Button("Copy Report") { save(report, copy: true) }
                    Spacer()
                    Button("Save Report…") { save(report, copy: false) }.buttonStyle(.borderedProminent)
                }
            }
        }
        .padding(24).frame(width: 580, height: 520)
        .task { await load() }
        .alert("Could not save report", isPresented: Binding(
            get: { exportError != nil }, set: { if !$0 { exportError = nil } }
        )) { Button("OK") { exportError = nil } } message: { Text(exportError ?? "") }
    }

    private func diagnosticRow(_ name: String, _ value: String) -> some View {
        GridRow { Text(name).foregroundStyle(.secondary); Text(value).textSelection(.enabled) }
    }

    private func title(_ value: String) -> String {
        switch value {
        case "wined3d": return "WineD3D"
        case "dxmt": return "DXMT (experimental)"
        case "verified_bundle": return "Verified included engine"
        default: return value.replacingOccurrences(of: "_", with: " ").capitalized
        }
    }

    private func issueTitle(_ value: String) -> String {
        switch value {
        case "bottle_changed_during_report": return "The bottle changed during this check. Refresh the report."
        case "prefix_unavailable": return "Windows files could not be read. Check the bottle's files."
        case "initialization_incomplete": return "Windows setup is incomplete. Retry the installation."
        case "programs_unreadable", "program_limit_reached": return "The saved app list could not be read completely."
        case "runtime_missing", "runtime_unavailable": return "The included engine is unavailable. Reinstall a complete Caskrun build."
        case "runtime_unreviewed": return "This engine has not passed Caskrun's bundled-engine checks."
        case "runtime_record_unreadable", "runtime_record_mismatch": return "The bottle's saved engine record needs attention before launching."
        case "graphics_unreadable": return "Graphics preferences could not be read."
        case "dxmt_unavailable": return "DXMT is selected but unavailable in this engine. Close apps and choose WineD3D."
        case "experimental_graphics": return "DXMT is experimental. Test each game before relying on it."
        case "installer_state_unreadable": return "The last setup record could not be read."
        case "installer_failed": return "The last setup reported a problem. Open its logs and retry."
        case "logs_unreadable", "logs_partially_scanned": return "Some log metadata could not be read."
        default: return "A diagnostic check needs attention."
        }
    }

    @MainActor private func load() async {
        guard !loading else { return }
        loading = true
        defer { loading = false }
        error = nil
        do {
            let result = try await model.diagnostics(in: bottle)
            guard result.format == "caskrun-diagnostic-report", result.version == 1 else { throw CocoaError(.coderInvalidValue) }
            report = result
        } catch { self.error = error.localizedDescription }
    }

    private func save(_ report: BottleDiagnosticReport, copy: Bool) {
        do {
            let text = try report.text()
            if copy {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(text, forType: .string)
            } else {
                try LocalReportSave.text(text, name: "Caskrun-diagnostics.json",
                    message: "Save the structured diagnostic report. Raw logs and personal paths are excluded.", type: .json)
            }
        } catch { exportError = error.localizedDescription }
    }
}
