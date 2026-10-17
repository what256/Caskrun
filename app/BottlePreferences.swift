import Foundation

/// Orders a bottle preference's asynchronous reads and writes. Inspector
/// recreation must not let an older read overwrite a confirmed saved change.
struct BottlePreferenceRequests {
    private var revision: UInt64 = 0
    private var writing: UInt64?

    mutating func beginRead() -> UInt64? {
        guard writing == nil else { return nil }
        revision &+= 1
        return revision
    }

    mutating func beginWrite() -> UInt64? {
        guard writing == nil else { return nil }
        revision &+= 1
        writing = revision
        return revision
    }

    func acceptsRead(_ token: UInt64) -> Bool { writing == nil && revision == token }
    func acceptsWrite(_ token: UInt64) -> Bool { writing == token && revision == token }

    mutating func finishWrite(_ token: UInt64) {
        if writing == token { writing = nil }
    }

    static func refreshFailure(hasConfirmedValue: Bool, detail: String) -> String {
        let context = hasConfirmedValue ? " Showing the last confirmed values." : ""
        return "Could not refresh saved settings.\(context) \(detail)"
    }
}
