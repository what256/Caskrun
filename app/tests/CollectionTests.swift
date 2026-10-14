import AppKit
import Foundation

@main
struct CollectionTests {
    static func main() {
        var checks = 0
        func expect(_ value: Bool, _ message: String) {
            precondition(value, message)
            checks += 1
        }
        func item(_ id: String, _ name: String, _ platform: CollectionPlatform,
                  _ kind: CollectionItemKind, _ subtitle: String = "") -> CollectionItem {
            CollectionItem(id: id, name: name, platform: platform, kind: kind, subtitle: subtitle)
        }
        let entries = [
            item("steam:one", "Puzzle Game", .windows, .game, "Steam · Example bottle"),
            item("app:one", "Steam", .windows, .app, "Example bottle"),
            item("ps5:one", "Imported Adventure", .ps5, .game, "Engine required"),
            item("browser:one", "Browser Puzzle", .browser, .game, "Community port"),
            item("app:two", "Drawing App", .windows, .app, "Second bottle")
        ]
        func filter(_ platform: CollectionPlatform = .all, _ kind: CollectionItemKind? = nil,
                    _ query: String = "") -> [String] {
            CollectionFilter.entries(entries, platform: platform, kind: kind, query: query).map(\.id)
        }
        expect(CollectionPlatform.allCases.map(\.title) == ["All", "Windows", "PS5", "Browser"], "compact platform choices")
        expect(filter() == entries.map(\.id), "all items preserves input order")
        expect(filter(.windows) == ["steam:one", "app:one", "app:two"], "Windows includes games and apps")
        expect(filter(.ps5) == ["ps5:one"], "imported PS5 games have their own source")
        expect(filter(.browser) == ["browser:one"], "browser filter excludes installed apps")
        expect(filter(.all, .game) == ["steam:one", "ps5:one", "browser:one"], "game filter combines platforms")
        expect(filter(.all, .app) == ["app:one", "app:two"], "app filter includes launchers")
        expect(filter(.windows, .game) == ["steam:one"], "source and type filters intersect")
        expect(filter(.browser, .app).isEmpty, "empty intersections do not invent entries")
        expect(filter(.all, nil, "  pUzZlE\n") == ["steam:one", "browser:one"], "query trims whitespace and matches case-insensitively")
        expect(filter(.all, nil, "SECOND BOTTLE") == ["app:two"], "query also matches the source subtitle")
        expect(filter(.windows, .game, "Community").isEmpty, "query does not escape the source and type filters")
        expect(filter(.all, nil, " \n ") == entries.map(\.id), "whitespace query does not hide items")
        let counts = CollectionFilter.counts(entries, kind: nil)
        expect(counts == [.all: 5, .windows: 3, .ps5: 1, .browser: 1], "counts describe all platforms")
        expect(CollectionFilter.counts(entries, kind: .game) == [.all: 3, .windows: 1, .ps5: 1, .browser: 1], "counts respect item type")
        expect(CollectionFilter.counts(entries, kind: .app) == [.all: 2, .windows: 2, .ps5: 0, .browser: 0], "zero-count platforms remain discoverable")
        expect(CollectionFilter.counts([], kind: nil).values.allSatisfy { $0 == 0 }, "empty collection has real zero counts")

        var steamApp = entries[1]; steamApp.bottleID = "steam-original"
        var importedSteam = entries[1]; importedSteam.bottleID = "steam-imported"
        importedSteam = CollectionItem(id: "app:imported", name: "Steam", platform: .windows, kind: .app,
                                       subtitle: "Steam (imported)", bottleID: "steam-imported")
        var installedGame = entries[0]; installedGame.bottleID = "steam-imported"
        let folderEntries = [installedGame, steamApp, importedSteam, entries[2], entries[3]]
        let folders = [CollectionFolder(id: "steam-original", name: "Steam"),
                       CollectionFolder(id: "steam-imported", name: "Steam (imported)"),
                       CollectionFolder(id: "empty", name: "Empty bottle")]
        func browse(_ open: String? = nil, _ platform: CollectionPlatform = .all,
                    _ kind: CollectionItemKind? = nil, _ query: String = "") -> CollectionBrowseResult {
            CollectionGrouping.browse(folderEntries, folders: folders, openFolderID: open,
                                      platform: platform, kind: kind, query: query)
        }
        expect(browse().folders.map(\.id) == folders.map(\.id), "root presents every bottle, including an empty one")
        expect(browse().entries.map(\.id) == ["ps5:one", "browser:one"], "installed games and clients stay in their bottles")
        expect(browse().count == 5, "root count describes visible folders and non-Windows entries")
        expect(browse("steam-imported").entries.map(\.id) == ["app:imported", "steam:one"],
               "opening an imported folder shows its launcher first and only its own game")
        expect(browse("steam-original").entries.map(\.id) == ["app:one"],
               "same-named clients in different bottles are not merged")
        expect(browse("steam-imported").folders.isEmpty, "folder contents do not recursively repeat the bottle list")
        expect(browse("empty").entries.isEmpty && browse("missing").entries.isEmpty,
               "empty and removed bottle routes never leak another bottle's children")
        expect(browse(nil, .browser).folders.isEmpty && browse(nil, .browser).entries.map(\.id) == ["browser:one"],
               "browser filter retains only browser items")
        expect(browse(nil, .windows, .game).folders.map(\.id) == ["steam-imported"],
               "game filter includes only bottles containing visible games")
        expect(browse(nil, .windows, nil, "puzzle").folders.map(\.id) == ["steam-imported"],
               "search finds a game's containing bottle without flattening the game")
        expect(browse(nil, .windows, nil, "imported").folders.map(\.id) == ["steam-imported"],
               "bottle names remain searchable")
        expect(browse("steam-imported", .all, .game, "puzzle").entries.map(\.id) == ["steam:one"],
               "search and type filters still apply inside a bottle")
        expect(CollectionGrouping.contents(folderEntries, folderID: "steam-imported").count == 2,
               "folder summary uses exact bottle identity")

        let identities = [
            CollectionItem.namespacedID(platform: .windows, source: "steam", rawID: "one"),
            CollectionItem.namespacedID(platform: .browser, source: "steam", rawID: "one"),
            CollectionItem.namespacedID(platform: .windows, source: "epic", rawID: "one"),
            CollectionItem.namespacedID(platform: .windows, source: "a:b", rawID: "c"),
            CollectionItem.namespacedID(platform: .windows, source: "a", rawID: "b:c"),
            CollectionItem.namespacedID(platform: .windows, source: "é", rawID: "one")
        ]
        expect(Set(identities).count == identities.count, "same local ids and separator-containing components cannot collide")
        expect(identities[0] == CollectionItem.namespacedID(platform: .windows, source: "steam", rawID: "one"), "identity is stable")
        expect(entries[0].actionTitle == "Play" && entries[1].actionTitle == "Open app"
               && entries[3].actionTitle == "Play in browser", "action describes the right platform and item type")
        expect(entries[0].cardHelp.contains("Double-click") && entries[1].cardHelp.contains("open the app")
               && entries[3].cardHelp.contains("your browser"), "click hint describes actual routing")
        var unavailable = entries[2]
        unavailable.canOpen = false
        expect(!unavailable.cardHelp.contains("Double-click"), "unavailable PS5 launch is not advertised")
        expect(unavailable.cardHelp.contains("details"), "unavailable game can still be inspected")
        var publisherGame = entries[0]
        publisherGame.openActionTitle = "Open store"
        expect(publisherGame.actionTitle == "Open store", "publisher menu and accessibility actions describe store routing")
        expect(publisherGame.cardHelp.contains("open its store") && !publisherGame.cardHelp.contains("to play"),
               "publisher double-click does not advertise an unimplemented direct launch")
        publisherGame.canOpen = false
        expect(!publisherGame.cardHelp.contains("Double-click"), "missing store remains unavailable after customising its action")
        var runningPS5 = entries[2]
        runningPS5.openActionTitle = "Show window"
        expect(runningPS5.actionTitle == "Show window" && runningPS5.cardHelp.contains("show its window"),
               "running PS5 title advertises focus rather than another launch")
        runningPS5.openActionTitle = "Open game"
        expect(runningPS5.cardHelp.contains("open the game"), "experimental PS5 opening matches its action label")

        // The Collection uses the same event router as the established Library
        // grid. An inspector reflow must never make the second click open a
        // neighbouring item, or deliver its mouse-up to another control.
        var clicks = LibraryClickSequence()
        clicks.remember(id: "browser:one", windowNumber: 7, point: NSPoint(x: 100, y: 100), timestamp: 10)
        expect(clicks.route(type: .leftMouseDown, windowNumber: 7, point: NSPoint(x: 100, y: 100),
                            timestamp: 10.2, clickCount: 2, doubleClickInterval: 0.5) == .open("browser:one"),
               "inspector reflow retains the clicked Collection item identity")
        expect(clicks.route(type: .leftMouseUp, windowNumber: 7, point: .zero,
                            timestamp: 10.21, clickCount: 2, doubleClickInterval: 0.5) == .consume,
               "second mouse-up cannot activate a reflowed control")
        clicks.remember(id: "folder:steam-imported", windowNumber: 7, point: NSPoint(x: 100, y: 100), timestamp: 15)
        expect(clicks.route(type: .leftMouseDown, windowNumber: 7, point: NSPoint(x: 100, y: 100),
                            timestamp: 15.2, clickCount: 2, doubleClickInterval: 0.5) == .open("folder:steam-imported"),
               "folder double-click keeps the first bottle identity after content reflow")
        expect(clicks.route(type: .leftMouseUp, windowNumber: 7, point: .zero,
                            timestamp: 15.21, clickCount: 2, doubleClickInterval: 0.5) == .consume,
               "folder second mouse-up cannot launch the game beneath it")
        clicks.remember(id: "steam:one", windowNumber: 7, point: .zero, timestamp: 20)
        clicks.cancel()
        expect(clicks.route(type: .leftMouseDown, windowNumber: 7, point: .zero,
                            timestamp: 20.2, clickCount: 2, doubleClickInterval: 0.5) == .pass,
               "filter or selection cancellation prevents stale launches")
        clicks.remember(id: "ps5:one", windowNumber: 7, point: .zero, timestamp: 30)
        expect(clicks.route(type: .leftMouseDown, windowNumber: 7, point: .zero,
                            timestamp: 30.2, clickCount: 2, doubleClickInterval: 0.5, isBlocked: true) == .pass,
               "PS5 import sheet receives its own double-click")
        print("\(checks) Collection model and interaction checks passed")
    }
}
