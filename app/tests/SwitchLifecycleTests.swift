import AppKit
import Foundation
@main struct SwitchLifecycleTests {
    @MainActor static func main() async throws {
        guard NSRunningApplication.runningApplications(withBundleIdentifier: SwitchCollectionStorage.bundleID).isEmpty else {
            print("Switch lifecycle test skipped: Suyu is already running")
            return
        }
        guard let base = ProcessInfo.processInfo.environment["CASKRUN_TEST_ROOT"],
              let fixture = ProcessInfo.processInfo.environment["CASKRUN_TEST_SWITCH_FIXTURE"] else {
            throw SwitchCollectionError.invalid("Test fixture paths are required.")
        }
        let root = URL(fileURLWithPath: base).appendingPathComponent("switch-life-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let game = root.appendingPathComponent("my game file with spaces.nsp")
        try Data([1]).write(to: game)
        let model = SwitchCollectionModel(storageDirectory: root.appendingPathComponent("data"))
        try model.configureEngine(URL(fileURLWithPath: fixture))
        let imported = try model.importGame(name: "Fixture", url: game)
        do {
            try await model.launch(imported)
            let record = root.appendingPathComponent("launch-arguments.json")
            var arguments: [String]?
            for _ in 0..<100 {
                if let data = try? Data(contentsOf: record), let read = try? JSONDecoder().decode([String].self, from: data) { arguments = read; break }
                try await Task.sleep(for: .milliseconds(100))
            }
            guard arguments?.count == 2, arguments?[1] == game.path else {
                throw SwitchCollectionError.invalid("Fixture did not receive exact game argument.")
            }
            do { try await model.launch(imported); throw SwitchCollectionError.invalid("Duplicate launch was permitted.") }
            catch let error as SwitchCollectionError {
                if error.localizedDescription == "Duplicate launch was permitted." { throw error }
            }
            guard await model.shutdownForQuit() else { throw SwitchCollectionError.invalid("Owned fixture did not quit.") }
            guard NSRunningApplication.runningApplications(withBundleIdentifier: SwitchCollectionStorage.bundleID).isEmpty else {
                throw SwitchCollectionError.invalid("Fixture application was left running.")
            }
            do { try await model.launch(imported); throw SwitchCollectionError.invalid("Launch while quitting was permitted.") }
            catch let error as SwitchCollectionError {
                if error.localizedDescription == "Launch while quitting was permitted." { throw error }
            }
            model.resumeAfterQuitFailure()
            let options = NSWorkspace.OpenConfiguration()
            options.arguments = [game.path]; options.activates = false; options.createsNewApplicationInstance = true
            let external = try await NSWorkspace.shared.openApplication(at: URL(fileURLWithPath: fixture), configuration: options)
            do {
                do { try await model.launch(imported); throw SwitchCollectionError.invalid("Existing Suyu was not detected.") }
                catch let error as SwitchCollectionError {
                    if error.localizedDescription == "Existing Suyu was not detected." { throw error }
                }
                guard await model.shutdownForQuit(), !external.isTerminated else {
                    throw SwitchCollectionError.invalid("Quit touched an independently opened Suyu.")
                }
                guard external.terminate() else { throw SwitchCollectionError.invalid("Could not stop test fixture.") }
                for _ in 0..<100 where !external.isTerminated { try await Task.sleep(for: .milliseconds(100)) }
                guard external.isTerminated else { throw SwitchCollectionError.invalid("Test fixture remained open.") }
            } catch {
                if !external.isTerminated { _ = external.forceTerminate() }
                throw error
            }
            print("Switch lifecycle: exact argument, duplicate guard, owned quit, quit lock and unrelated-instance preservation passed")
        } catch {
            _ = await model.shutdownForQuit()
            throw error
        }
    }
}
