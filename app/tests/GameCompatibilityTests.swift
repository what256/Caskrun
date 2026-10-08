import Foundation

@main
struct GameCompatibilityTests {
    static func main() throws {
        var checks = 0
        func check(_ condition: Bool, _ name: String) {
            precondition(condition, name)
            checks += 1
        }
        let fixture: [String: Any] = [
            "format": "caskrun-game-compatibility", "version": 1,
            "source": ["name": "Highball community reports", "repository": GameCompatibilityCatalog.sourceRepository,
                       "revision": GameCompatibilityCatalog.reviewedRevision, "license": "CC0-1.0",
                       "license_url": "\(GameCompatibilityCatalog.sourceRepository)/blob/\(GameCompatibilityCatalog.reviewedRevision)/LICENSE"],
            "games": [["steam_app_id": 620, "title": "Example Game", "reports": [
                ["rating": 5, "renderer": "dxmt", "chip": "Apple M4", "macos": "26.0.0",
                 "memory_gib": NSNull(), "date": "2026-09-16", "engine": "x64-sikarugir10.0_6-r3",
                 "source_url": "\(GameCompatibilityCatalog.sourceRepository)/issues/92"]
            ]]]
        ]
        func catalog(_ value: [String: Any]) throws -> GameCompatibilityCatalog {
            try GameCompatibilityCatalog.load(data: JSONSerialization.data(withJSONObject: value))
        }
        func refuses(_ value: [String: Any], _ name: String) {
            do { _ = try catalog(value); preconditionFailure(name) } catch { checks += 1 }
        }
        func replacingReport(_ fields: [String: Any]) -> [String: Any] {
            var value = fixture
            var games = value["games"] as! [[String: Any]]
            var reports = games[0]["reports"] as! [[String: Any]]
            for (key, field) in fields { reports[0][key] = field }
            games[0]["reports"] = reports
            value["games"] = games
            return value
        }
        let loaded = try catalog(fixture)
        let game = loaded.game(appID: 620)!
        check(game.title == "Example Game" && game.reports.count == 1, "Valid evidence must load")
        check(loaded.game(appID: 1) == nil, "Unknown game must remain unknown")
        check(loaded.search("example").count == 1 && loaded.search("620").count == 1, "Title and exact app-ID search")
        check(loaded.search("", limit: 0).isEmpty && loaded.search(String(repeating: "a", count: 513)).isEmpty,
              "Search input and result bounds")
        var unreviewed = fixture
        var source = fixture["source"] as! [String: Any]
        source["revision"] = String(repeating: "a", count: 40)
        unreviewed["source"] = source
        refuses(unreviewed, "Unreviewed source revision must be refused")
        var unknown = fixture
        unknown["notes"] = "not allowed"
        refuses(unknown, "Unknown top-level fields must be refused")
        refuses(replacingReport(["user": "not allowed"]), "Contributor identities must not be accepted")
        refuses(replacingReport(["model_identifier": "not allowed"]), "Device identifiers must not be accepted")
        refuses(replacingReport(["source_url": "https://example.invalid/issues/92"]), "Unknown source host must be refused")
        refuses(replacingReport(["source_url": "https://github.com/gauthierpiarrette/highball-db/issues/92?token=secret"]),
                "Queries and credentials must not enter source URLs")
        refuses(replacingReport(["source_url": "https://github.com/gauthierpiarrette/highball-db/issues/92\n"]),
                "Trailing source URL controls must be refused")
        refuses(replacingReport(["date": "2026-02-30"]), "Invalid calendar date must be refused")
        refuses(replacingReport(["rating": 6]), "Unrecognized rating must be refused")
        refuses(replacingReport(["engine": "/untrusted/local/path"]), "Engine must be a generic public package ID")
        refuses(replacingReport(["renderer": "custom-script"]), "Renderer must be a known data value")
        var duplicate = fixture
        duplicate["games"] = (fixture["games"] as! [[String: Any]]) + (fixture["games"] as! [[String: Any]])
        refuses(duplicate, "Duplicate app IDs must be refused")
        do {
            _ = try GameCompatibilityCatalog.load(data: Data(repeating: 32, count: 1_048_577))
            preconditionFailure("Oversized data must be refused")
        } catch { checks += 1 }
        let sameDevice = CompatibilityDevice(chip: "Apple M4", memoryGiB: 16, macOSVersion: "26.0.0")
        let same = GameCompatibilityCatalog.comparison(for: game.reports[0], device: sameDevice)
        check(same.chip == .same && same.memory == .unknown && same.operatingSystem == .same,
              "Same chip must not manufacture a RAM match")
        check(same.details.contains { $0.contains("Caskrun compatibility is untested") }, "External engine never verifies Caskrun")
        check(same.rendererAvailable == nil && same.details.contains { $0.contains("has not been checked") },
              "Unprobed local renderer availability stays unknown")
        let checked = GameCompatibilityCatalog.comparison(for: game.reports[0], device: sameDevice,
                                                          availableRenderers: ["wined3d", "dxmt"])
        check(checked.rendererAvailable == true && !checked.details.contains { $0.contains("has not been checked") },
              "Known availability is distinct from an unprobed bottle")
        let otherDevice = CompatibilityDevice(chip: "Apple M1", memoryGiB: 8, macOSVersion: "15.0.0")
        let different = GameCompatibilityCatalog.comparison(for: game.reports[0], device: otherDevice)
        check(different.chip == .different && different.operatingSystem == .different, "Different hardware and OS remain explicit")
        let unavailable = try catalog(replacingReport(["renderer": "d3dmetal"]))
        let comparison = GameCompatibilityCatalog.comparison(for: unavailable.games[0].reports[0], device: sameDevice,
                                                             availableRenderers: ["wined3d", "dxmt"])
        check(comparison.rendererAvailable == false && comparison.details.contains { $0.contains("not available") },
              "Unavailable renderer must be visible")
        let noRenderer = try catalog(replacingReport(["renderer": NSNull()]))
        let missingMode = GameCompatibilityCatalog.comparison(for: noRenderer.games[0].reports[0], device: sameDevice,
                                                              availableRenderers: ["wined3d", "dxmt"])
        check(missingMode.rendererAvailable == nil && missingMode.details.contains { $0.contains("does not record") }
              && !missingMode.details.contains { $0.contains("has not been checked") },
              "Missing report mode is distinct from unprobed local availability")
        let unknownDevice = CompatibilityDevice(chip: nil, memoryGiB: nil, macOSVersion: nil)
        check(GameCompatibilityCatalog.comparison(for: game.reports[0], device: unknownDevice).chip == .unknown,
              "Missing local hardware stays unknown")
        if CommandLine.arguments.count > 1 {
            let production = try GameCompatibilityCatalog.load(data: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
            check(production.games.count == 373, "Reviewed game count")
            check(production.games.reduce(0) { $0 + $1.reports.count } == 71, "Reviewed report count")
            check(production.games.flatMap(\.reports).allSatisfy { $0.memoryGiB == nil }, "Source lacks RAM; preserve uncertainty")
        }
        print("Game compatibility: \(checks) checks passed")
    }
}
