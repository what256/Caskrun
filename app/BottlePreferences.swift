import Foundation

/// The Core contract permits either verified view for Automatic, while a
/// manual choice must be returned unchanged. Never display an unavailable
/// automatic DXMT selection as a confirmed result.
enum BottleGraphicsContract {
    static func accepts(selected: String, effective: String, dxmtAvailable: Bool) -> Bool {
        guard ["auto", "wined3d", "dxmt"].contains(selected),
              ["wined3d", "dxmt"].contains(effective) else { return false }
        return (selected == "auto" || selected == effective)
            && (selected != "auto" || effective != "dxmt" || dxmtAvailable)
    }

    static func automaticDescription(reason: String?) -> String {
        switch reason {
        case "d3d11-detected": "Automatic chose DXMT from the installed apps’ DirectX 10/11 imports."
        case "mixed-or-incomplete-evidence": "Automatic kept WineD3D because graphics requirements are mixed or could not be read completely."
        case "engine-unavailable": "Automatic kept WineD3D because the verified DXMT engine is unavailable."
        case "no-supported-evidence": "Automatic kept WineD3D; no supported DirectX 10/11 requirement was detected."
        default: "Automatic checks installed apps at the next idle launch."
        }
    }

    static func canTuneDXMT(selected: String, effective: String, dxmtAvailable: Bool) -> Bool {
        dxmtAvailable && effective == "dxmt"
            && accepts(selected: selected, effective: effective, dxmtAvailable: dxmtAvailable)
    }
}

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
