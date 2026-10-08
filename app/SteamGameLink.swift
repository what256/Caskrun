import Foundation

enum SteamGameLink {
    /// Extract only a typed AppID. The supplied address is never launched,
    /// saved, or passed to Steam; query parameters and fragments stay local.
    static func appID(_ input: String) -> UInt32? {
        let value = input.trimmingCharacters(in: .whitespacesAndNewlines)
        if value.utf8.count <= 10, value.allSatisfy({ $0.isASCII && $0.isNumber }),
           let id = UInt32(value), id > 0 { return id }
        guard value.utf8.count <= 2048, !value.contains("\\"),
              !value.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }),
              let url = URLComponents(string: value), url.scheme == "https",
              url.host == "store.steampowered.com", url.user == nil, url.password == nil,
              url.port == nil else { return nil }
        let parts = url.path.split(separator: "/")
        guard parts.count >= 2, parts[0] == "app", parts[1].allSatisfy({ $0.isASCII && $0.isNumber }),
              let id = UInt32(parts[1]), id > 0 else { return nil }
        return id
    }
}
