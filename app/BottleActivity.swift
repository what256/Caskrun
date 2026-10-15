import Foundation

struct BottleActivitySnapshot: Decodable, Sendable {
    let bottle_id: String
    let running: Bool
    let owner_count: Int
}

struct BottleQuitRequestResult: Decodable, Sendable {
    let bottle_id: String
    let request_submitted: Bool
}

/// State for one bottle. A completed request never proves that its apps quit;
/// only a fresh activity check can turn a running bottle into an idle one.
struct BottleSessionState: Equatable, Sendable {
    enum Activity: Equatable, Sendable { case unknown, idle, running }
    enum Phase: Equatable, Sendable { case ready, requesting, waiting, forcing }

    private(set) var activity: Activity = .unknown
    private(set) var phase: Phase = .ready
    private(set) var operationID: UUID?
    private(set) var startedAt: Date?
    private var activityError: String?
    private var quitError: String?
    var error: String? { quitError ?? activityError }

    var isStopping: Bool { phase != .ready }
    var showsQuitControl: Bool { activity == .running || isStopping || quitError != nil }

    func canForceQuit(at now: Date) -> Bool {
        guard phase != .forcing, showsQuitControl else { return false }
        if error != nil { return true }
        return startedAt.map { now.timeIntervalSince($0) >= 4 } ?? false
    }

    mutating func beginQuit(at now: Date = Date()) -> UUID? {
        guard activity == .running, !isStopping else { return nil }
        let token = UUID()
        operationID = token
        phase = .requesting
        startedAt = now
        activityError = nil
        quitError = nil
        return token
    }

    mutating func beginForceQuit(at now: Date = Date()) -> UUID {
        let token = UUID()
        operationID = token
        phase = .forcing
        startedAt = now
        activityError = nil
        quitError = nil
        return token
    }

    mutating func requestFinished(_ token: UUID, submitted: Bool) {
        guard operationID == token, phase == .requesting else { return }
        phase = .waiting
        if !submitted { activity = .unknown }
        // Even an idle preflight is only a snapshot. The next activity check
        // confirms the bottle is idle, including apps opened in the meantime.
    }

    mutating func forceFinished(_ token: UUID) {
        guard operationID == token, phase == .forcing else { return }
        phase = .waiting
    }

    mutating func failed(_ token: UUID, message: String) {
        guard operationID == token else { return }
        phase = .ready
        operationID = nil
        startedAt = nil
        quitError = message
    }

    mutating func interrupted(_ token: UUID) {
        guard isStopping else { return }
        failed(token, message: "The quit request was interrupted. Check the remaining apps or try again.")
    }

    mutating func checked(running: Bool) {
        activity = running ? .running : .idle
        activityError = nil
        if !running, phase == .waiting || phase == .ready {
            quitError = nil
            phase = .ready
            operationID = nil
            startedAt = nil
        }
    }

    mutating func checkFailed(_ message: String) {
        // Unknown must never be displayed as idle or erase a pending shutdown.
        activity = .unknown
        activityError = message
    }
}
