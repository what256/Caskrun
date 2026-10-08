import Darwin
import Foundation
import SwiftUI

struct CompatibilityDevice: Equatable {
    let chip: String?
    let memoryGiB: Int?
    let macOSVersion: String?

    static func current() -> Self {
        var size = 0
        var chip: String?
        if sysctlbyname("machdep.cpu.brand_string", nil, &size, nil, 0) == 0,
           size > 0, size <= 256 {
            var bytes = [CChar](repeating: 0, count: size)
            if sysctlbyname("machdep.cpu.brand_string", &bytes, &size, nil, 0) == 0,
               bytes.contains(0) {
                let value = String(cString: bytes).trimmingCharacters(in: .whitespacesAndNewlines)
                if CompatibilityValidation.matches(value, "Apple M[1-9][0-9]?(?: Pro| Max| Ultra)?") {
                    chip = value
                }
            }
        }
        let version = ProcessInfo.processInfo.operatingSystemVersion
        let memory = ProcessInfo.processInfo.physicalMemory
        return Self(chip: chip,
                    memoryGiB: memory > 0 ? Int((memory + 1_073_741_823) / 1_073_741_824) : nil,
                    macOSVersion: "\(version.majorVersion).\(version.minorVersion).\(version.patchVersion)")
    }

    var summary: String {
        [chip, memoryGiB.map { "\($0) GB" }, macOSVersion.map { "macOS \($0)" }]
            .compactMap { $0 }.joined(separator: " · ")
    }
}

struct CompatibilityReport: Decodable, Identifiable {
    let rating: Int
    let renderer: String?
    let chip: String?
    let macOS: String?
    let memoryGiB: Int?
    let testedDate: String
    let engine: String?
    let sourceURL: URL

    enum CodingKeys: String, CodingKey {
        case rating, renderer, chip, engine
        case macOS = "macos", memoryGiB = "memory_gib", testedDate = "date", sourceURL = "source_url"
    }

    var id: String { sourceURL.absoluteString }

    var rendererName: String {
        switch renderer {
        case "wined3d": "WineD3D"
        case "dxmt": "DXMT"
        case "dxvk": "DXVK"
        case "d3dmetal": "D3DMetal"
        case "vkd3d": "VKD3D"
        default: "Graphics mode not recorded"
        }
    }
}

struct CompatibilityGame: Decodable, Identifiable {
    let steamAppID: UInt32
    let title: String
    let reports: [CompatibilityReport]

    enum CodingKeys: String, CodingKey {
        case steamAppID = "steam_app_id", title, reports
    }

    var id: UInt32 { steamAppID }
    var hasMixedReports: Bool {
        reports.contains { $0.rating >= 4 } && reports.contains { $0.rating < 4 }
    }
    var summary: String {
        if reports.isEmpty { return "Not tested in Caskrun" }
        if hasMixedReports { return "Mixed external reports" }
        return "\(reports.count) external \(reports.count == 1 ? "report" : "reports")"
    }
}

struct CompatibilityComparison {
    enum Match: Equatable { case same, different, unknown }
    let chip: Match
    let memory: Match
    let operatingSystem: Match
    let rendererAvailable: Bool?
    let label: String
    let details: [String]
}

struct GameCompatibilityCatalog {
    struct Source: Decodable {
        let name: String
        let repository: URL
        let revision: String
        let license: String
        let licenseURL: URL
        enum CodingKeys: String, CodingKey { case name, repository, revision, license; case licenseURL = "license_url" }
    }
    private struct Envelope: Decodable {
        let format: String
        let version: Int
        let source: Source
        let games: [CompatibilityGame]
    }

    static let reviewedRevision = "d8d165834cd0b77613ac0f00300e8fac0b98b143"
    static let sourceRepository = "https://github.com/gauthierpiarrette/highball-db"
    static let shared: Self = {
        guard let url = Bundle.main.url(forResource: "game-compatibility", withExtension: "json", subdirectory: "catalog") else {
            return Self(games: [], source: nil, error: "The compatibility catalog is missing. Installed games can still be opened.")
        }
        do {
            let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
            guard let bytes = attributes[.size] as? NSNumber, bytes.uint64Value <= 1_048_576 else {
                throw CompatibilityValidation.Failure.invalid
            }
            return try load(data: Data(contentsOf: url, options: .mappedIfSafe))
        } catch {
            return Self(games: [], source: nil, error: "The compatibility catalog could not be verified. Installed games can still be opened.")
        }
    }()

    let games: [CompatibilityGame]
    let source: Source?
    let error: String?

    static func load(data: Data) throws -> Self {
        guard data.count <= 1_048_576 else { throw CompatibilityValidation.Failure.invalid }
        let object = try JSONSerialization.jsonObject(with: data)
        try CompatibilityValidation.structure(object)
        let decoded = try JSONDecoder().decode(Envelope.self, from: data)
        guard decoded.format == "caskrun-game-compatibility", decoded.version == 1,
              decoded.source.name == "Highball community reports",
              decoded.source.repository.absoluteString == sourceRepository,
              decoded.source.revision == reviewedRevision,
              decoded.source.license == "CC0-1.0",
              decoded.source.licenseURL.absoluteString == "\(sourceRepository)/blob/\(reviewedRevision)/LICENSE",
              !decoded.games.isEmpty, decoded.games.count <= 1024 else {
            throw CompatibilityValidation.Failure.invalid
        }
        var seenApps = Set<UInt32>()
        var seenReports = Set<String>()
        var reportCount = 0
        for game in decoded.games {
            guard game.steamAppID > 0, seenApps.insert(game.steamAppID).inserted,
                  CompatibilityValidation.text(game.title, maximum: 200) else {
                throw CompatibilityValidation.Failure.invalid
            }
            for report in game.reports {
                reportCount += 1
                guard reportCount <= 4096, (1...5).contains(report.rating),
                      report.renderer.map({ CompatibilityValidation.renderers.contains($0) }) ?? true,
                      report.chip.map({ CompatibilityValidation.matches($0, "Apple M[1-9][0-9]?(?: Pro| Max| Ultra)?") }) ?? true,
                      report.macOS.map({ CompatibilityValidation.matches($0, "[1-9][0-9]?(?:\\.[0-9]{1,2}){0,2}") }) ?? true,
                      report.memoryGiB.map({ (1...1024).contains($0) }) ?? true,
                      report.engine.map({ CompatibilityValidation.matches($0, "x64-(?:sikarugir|crossover|wine)[A-Za-z0-9._+-]{1,72}") }) ?? true,
                      CompatibilityValidation.date(report.testedDate),
                      CompatibilityValidation.matches(report.sourceURL.absoluteString,
                          "https://github\\.com/gauthierpiarrette/highball-db/(?:issues/[1-9][0-9]{0,9}|blob/\(reviewedRevision)/db/reports/[a-z0-9]+(?:-[a-z0-9]+)*\\.jsonl#L[1-9][0-9]{0,5})"),
                      seenReports.insert(report.sourceURL.absoluteString).inserted else {
                    throw CompatibilityValidation.Failure.invalid
                }
            }
        }
        return Self(games: decoded.games, source: decoded.source, error: nil)
    }

    func game(appID: UInt32) -> CompatibilityGame? {
        games.first { $0.steamAppID == appID }
    }

    func search(_ query: String, limit: Int = 80) -> [CompatibilityGame] {
        let query = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard query.utf8.count <= 512 else { return [] }
        let limit = min(max(limit, 0), 1024)
        if query.isEmpty { return Array(games.prefix(limit)) }
        return Array(games.lazy.filter {
            $0.title.localizedStandardContains(query) || String($0.steamAppID) == query
        }.prefix(limit))
    }

    static func comparison(for report: CompatibilityReport, device: CompatibilityDevice,
                           availableRenderers: Set<String>? = nil) -> CompatibilityComparison {
        func match<T: Equatable>(_ reported: T?, _ local: T?) -> CompatibilityComparison.Match {
            guard let reported, let local else { return .unknown }
            return reported == local ? .same : .different
        }
        let chip = match(report.chip, device.chip)
        let memory = match(report.memoryGiB, device.memoryGiB)
        let operatingSystem = match(report.macOS, device.macOSVersion)
        var details: [String] = []
        switch chip {
        case .same: details.append("The report names the same chip. Memory, game versions and runtime differences still matter.")
        case .different: details.append("The report used a different chip; its performance does not predict this Mac's performance.")
        case .unknown: details.append("The report or this Mac does not identify a comparable chip.")
        }
        if memory == .unknown { details.append("Reported memory is unknown.") }
        else if memory == .different { details.append("Reported memory differs from this Mac.") }
        if operatingSystem != .same { details.append("The macOS version differs or is not fully recorded.") }
        // An installed renderer is known only after this bottle's capability
        // check. A catalog view alone cannot establish local runtime support.
        let rendererAvailable = report.renderer.flatMap { renderer in
            availableRenderers.map { $0.contains(renderer) }
        }
        if rendererAvailable == false {
            details.append("The reported \(report.rendererName) renderer is not available for this bottle.")
        } else if report.renderer == nil {
            details.append("The report does not record its graphics mode.")
        } else if rendererAvailable == nil {
            details.append("Renderer availability has not been checked for this bottle.")
        }
        details.append("This is an external Highball result using another runtime. Caskrun compatibility is untested.")
        return CompatibilityComparison(chip: chip, memory: memory, operatingSystem: operatingSystem,
                                       rendererAvailable: rendererAvailable,
                                       label: chip == .same ? "Reported on your chip" : "External Mac report",
                                       details: details)
    }
}

private enum CompatibilityValidation {
    enum Failure: Error { case invalid }
    static let renderers: Set<String> = ["wined3d", "dxmt", "d3dmetal", "dxvk", "vkd3d"]

    static func matches(_ value: String, _ expression: String) -> Bool {
        guard value.utf8.count <= 512 else { return false }
        return value.range(of: "\\A(?:\(expression))\\z", options: .regularExpression) != nil
    }
    static func text(_ value: String, maximum: Int) -> Bool {
        !value.isEmpty && value.unicodeScalars.count <= maximum &&
            value.unicodeScalars.allSatisfy { !CharacterSet.controlCharacters.contains($0) }
    }
    static func date(_ value: String) -> Bool {
        guard matches(value, "[0-9]{4}-[0-9]{2}-[0-9]{2}") else { return false }
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.dateFormat = "yyyy-MM-dd"
        formatter.isLenient = false
        return formatter.date(from: value).map { formatter.string(from: $0) == value } ?? false
    }
    static func exact(_ object: Any, _ fields: Set<String>) throws -> [String: Any] {
        guard let dictionary = object as? [String: Any], Set(dictionary.keys) == fields else {
            throw Failure.invalid
        }
        return dictionary
    }
    static func structure(_ object: Any) throws {
        let root = try exact(object, ["format", "version", "source", "games"])
        _ = try exact(root["source"] ?? NSNull(), ["name", "repository", "revision", "license", "license_url"])
        guard let games = root["games"] as? [Any], games.count <= 1024 else { throw Failure.invalid }
        var count = 0
        for game in games {
            let game = try exact(game, ["steam_app_id", "title", "reports"])
            guard let reports = game["reports"] as? [Any] else { throw Failure.invalid }
            count += reports.count
            guard count <= 4096 else { throw Failure.invalid }
            for report in reports {
                _ = try exact(report, ["rating", "renderer", "chip", "macos", "memory_gib", "date", "engine", "source_url"])
            }
        }
    }
}

struct CompatibilityEvidenceView: View {
    let game: CompatibilityGame?
    let device: CompatibilityDevice
    var availableRenderers: Set<String>? = nil
    @State private var showAllReports = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Label("Compatibility", systemImage: "desktopcomputer")
                .font(.headline)
            if !device.summary.isEmpty {
                Text(device.summary).font(.caption).foregroundStyle(.secondary)
            }
            Text("Not tested in Caskrun")
                .font(.subheadline.weight(.semibold))
            Text("Reports from another launcher are useful evidence, but do not confirm this game works in Caskrun.")
                .font(.caption).foregroundStyle(.secondary)
            if let game, !game.reports.isEmpty {
                if game.hasMixedReports {
                    Label("External reports have mixed outcomes", systemImage: "exclamationmark.bubble")
                        .font(.caption.weight(.medium))
                }
                ForEach(showAllReports ? game.reports : Array(game.reports.prefix(3))) { report in
                    reportView(report)
                }
                if game.reports.count > 3 {
                    Button(showAllReports ? "Show fewer reports" : "Show all \(game.reports.count) reports") {
                        showAllReports.toggle()
                    }.buttonStyle(.link).font(.caption)
                }
            } else {
                Text("No direct community report is included for this game. Missing evidence does not mean the game is incompatible.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Link("Highball data · CC0 · source snapshot",
                 destination: URL(string: "\(GameCompatibilityCatalog.sourceRepository)/tree/\(GameCompatibilityCatalog.reviewedRevision)")!)
                .font(.caption)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func reportView(_ report: CompatibilityReport) -> some View {
        let comparison = GameCompatibilityCatalog.comparison(for: report, device: device,
                                                             availableRenderers: availableRenderers)
        return VStack(alignment: .leading, spacing: 7) {
            Divider()
            HStack(alignment: .firstTextBaseline) {
                Text("Highball community · \(report.rating)/5")
                    .font(.caption.weight(.semibold))
                Spacer(minLength: 8)
                Text(report.testedDate).font(.caption2).foregroundStyle(.secondary)
            }
            Text(comparison.label).font(.caption.weight(.medium))
            Text([report.chip ?? "Chip unknown", report.macOS.map { "macOS \($0)" } ?? "macOS unknown",
                  report.rendererName].joined(separator: " · "))
                .font(.caption).foregroundStyle(.secondary)
            if let engine = report.engine {
                Text("Reported engine: \(engine)").font(.caption2).foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
            ForEach(comparison.details, id: \.self) { detail in
                Text(detail).font(.caption2).foregroundStyle(.secondary)
            }
            Link(destination: report.sourceURL) {
                Label("Read the source report", systemImage: "arrow.up.right")
                    .font(.caption)
            }
        }
    }
}
