import Foundation

@main
struct BottleActivityTests {
    static func main() throws {
        var checks = 0
        func expect(_ value: @autoclosure () -> Bool, _ message: String) {
            precondition(value(), message)
            checks += 1
        }
        let start = Date(timeIntervalSince1970: 100)
        var first = BottleSessionState()
        expect(!first.showsQuitControl, "unknown activity does not invent a running app")
        first.checkFailed("inspection unavailable")
        expect(first.activity == .unknown && first.error != nil, "failed inspection is unknown, never idle")
        first.checked(running: false)
        expect(!first.showsQuitControl && first.error == nil, "idle bottle hides Quit Apps")
        expect(first.beginQuit(at: start) == nil, "idle bottles cannot start a shutdown")
        first.checked(running: true)
        expect(first.showsQuitControl, "running apps reveal Quit Apps")
        let request = first.beginQuit(at: start)!
        expect(first.phase == .requesting && first.isStopping, "polite request starts progress")
        expect(first.beginQuit(at: start) == nil, "repeated clicks do not start duplicate requests")
        expect(!first.canForceQuit(at: start.addingTimeInterval(3.9)), "force stays hidden during normal startup")
        expect(first.canForceQuit(at: start.addingTimeInterval(4)), "a slow request offers explicit Force Quit")
        first.checked(running: false)
        expect(first.phase == .requesting, "a racing idle probe cannot clear an in-flight request")
        first.requestFinished(request, submitted: true)
        expect(first.isStopping, "request success is not proof that apps exited")
        first.checked(running: true)
        expect(first.phase == .waiting, "apps which refuse clean exit keep progress and escalation available")
        first.checked(running: false)
        expect(first.phase == .ready && !first.showsQuitControl, "fresh idle check ends progress")

        first.checked(running: true)
        let old = first.beginQuit(at: start)!
        let force = first.beginForceQuit(at: start.addingTimeInterval(5))
        first.failed(old, message: "late cancellation")
        first.requestFinished(old, submitted: true)
        expect(first.phase == .forcing && first.operationID == force && first.error == nil,
               "late graceful results cannot overwrite a newer Force Quit")
        expect(!first.canForceQuit(at: start.addingTimeInterval(30)), "Force Quit cannot run twice concurrently")
        first.forceFinished(force)
        first.checked(running: false)
        expect(!first.showsQuitControl && !first.isStopping, "successful force plus an idle check resets controls")

        first.checked(running: true)
        let failed = first.beginQuit(at: start)!
        first.failed(failed, message: "request failed")
        first.checked(running: true)
        expect(first.error == "request failed" && first.canForceQuit(at: start),
               "activity refresh preserves a failed quit and its explicit escape hatch")
        first.checkFailed("inspection failed")
        expect(first.activity == .unknown && first.showsQuitControl && first.canForceQuit(at: start),
               "an inspection error cannot hide an unresolved shutdown")
        first.checked(running: false)
        expect(first.error == nil && !first.showsQuitControl, "confirmed idle clears errors")
        var second = BottleSessionState()
        second.checked(running: true)
        let retry = first.beginForceQuit(at: start)
        expect(second.activity == .running && !second.isStopping, "stopping one bottle does not change a sibling")
        first.failed(retry, message: "force failed")
        expect(!first.isStopping && first.error != nil, "force failure leaves a retryable state")

        first.checked(running: true)
        let interruptedQuit = first.beginQuit(at: start)!
        first.interrupted(interruptedQuit)
        expect(!first.isStopping && first.canForceQuit(at: start),
               "full app Quit failure cannot strand a canceled polite request")
        let interruptedForce = first.beginForceQuit(at: start)
        first.interrupted(interruptedForce)
        expect(first.phase == .ready && first.canForceQuit(at: start),
               "full app Quit failure leaves a canceled Force Quit retryable")
        let newer = first.beginForceQuit(at: start)
        first.interrupted(interruptedForce)
        expect(first.operationID == newer && first.phase == .forcing,
               "old cancellation cannot reset an operation which is still running")
        first.forceFinished(newer)
        first.checked(running: false)
        expect(!first.isStopping && !first.showsQuitControl,
               "recovered shutdown re-enables opening after apps exit")

        let json = Data(#"{"bottle_id":"fixture","running":true,"owner_count":2}"#.utf8)
        let snapshot = try JSONDecoder().decode(BottleActivitySnapshot.self, from: json)
        expect(snapshot.bottle_id == "fixture" && snapshot.running && snapshot.owner_count == 2,
               "activity response decodes the bottle-bound Core contract")
        print("Bottle activity: \(checks) checks passed")
    }
}
