import Foundation

@main
struct BottlePreferencesTests {
    static func main() {
        var checks = 0
        func expect(_ value: @autoclosure () -> Bool, _ message: String) {
            precondition(value(), message)
            checks += 1
        }

        // Reproduce an inspector's old read arriving after a saved change.
        // The persisted fixture remains DXMT; a late UI response must not turn
        // its displayed selection back to Automatic or remove the cache.
        var requests = BottlePreferenceRequests()
        var displayed = "auto"
        let oldRead = requests.beginRead()!
        let write = requests.beginWrite()!
        expect(!requests.acceptsRead(oldRead), "beginning a change invalidates an older read")
        expect(requests.beginRead() == nil, "reopening the inspector cannot read through a pending write")
        expect(requests.beginWrite() == nil, "only one preference write may be pending")
        expect(requests.acceptsWrite(write), "the current writer may publish its confirmed value")
        let persisted = "dxmt"
        if requests.acceptsWrite(write) { displayed = persisted }
        requests.finishWrite(write)
        if requests.acceptsRead(oldRead) { displayed = "auto" }
        expect(displayed == persisted, "a late Automatic snapshot cannot overwrite saved DXMT")
        var cachePresent = true
        if requests.acceptsRead(oldRead) { cachePresent = false }
        expect(cachePresent, "an old failed read cannot erase the newly confirmed cache")

        // A fresh reopen reads persistence rather than manufacturing defaults.
        let reopen = requests.beginRead()!
        expect(requests.acceptsRead(reopen), "a fresh read is allowed once the change finishes")
        if requests.acceptsRead(reopen) { displayed = persisted }
        expect(displayed == "dxmt", "a later inspector open retains the saved renderer")
        let newest = requests.beginRead()!
        expect(!requests.acceptsRead(reopen) && requests.acceptsRead(newest),
               "reordered refresh responses cannot replace the latest snapshot")

        // A refused write must keep the previous saved value. The model's
        // readback happens within the write ticket and does not start a second
        // asynchronous refresh that can race the next setting change.
        let refusal = requests.beginWrite()!
        if requests.acceptsWrite(refusal) { displayed = persisted }
        requests.finishWrite(refusal)
        expect(displayed == "dxmt", "a busy-bottle refusal never resets the saved renderer")
        expect(!requests.acceptsWrite(refusal), "a completed writer cannot publish late results")
        let later = requests.beginWrite()!
        requests.finishWrite(refusal)
        expect(requests.acceptsWrite(later), "an earlier completion cannot unlock a newer write")
        requests.finishWrite(later)

        var sibling = BottlePreferenceRequests()
        let siblingRead = sibling.beginRead()!
        let active = requests.beginWrite()!
        expect(sibling.acceptsRead(siblingRead), "one bottle's changes do not invalidate a sibling")
        requests.finishWrite(active)

        // Display mode uses a separate instance of the same ordering policy.
        var resolution = BottlePreferenceRequests()
        let oldDPI = resolution.beginRead()!
        let retina = resolution.beginWrite()!
        expect(resolution.acceptsWrite(retina) && !resolution.acceptsRead(oldDPI),
               "a stale standard-DPI response cannot overwrite a confirmed Retina change")
        resolution.finishWrite(retina)
        expect(resolution.beginRead() != nil, "display reads resume after saving")
        expect(BottlePreferenceRequests.refreshFailure(hasConfirmedValue: true, detail: "Unavailable")
            .contains("Showing the last confirmed values."),
               "a preserved cached selection is clearly labeled after a read failure")
        expect(!BottlePreferenceRequests.refreshFailure(hasConfirmedValue: false, detail: "Unavailable")
            .contains("last confirmed"), "a first read failure does not invent a saved selection")
        print("Bottle preferences: \(checks) checks passed")
    }
}
