import Foundation

@main
struct SteamGameLinkTests {
    static func main() {
        let accepted: [(String, UInt32)] = [
            ("480", 480), (" 480 ", 480), ("4294967295", UInt32.max),
            ("https://store.steampowered.com/app/480/Example/", 480),
            ("https://store.steampowered.com/app/480/?language=english#details", 480)
        ]
        let refused = ["", "0", "-1", "4294967296", "٤٨٠", "480 --exec", "steam://install/480",
            "http://store.steampowered.com/app/480", "https://store.steampowered.com.attacker.invalid/app/480",
            "https://" + "account:secret@store.steampowered.com/app/480", "https://store.steampowered.com:443/app/480",
            "https://store.steampowered.com/app/0", "https://store.steampowered.com/app/480evil",
            "https://store.steampowered.com/sub/480", "https://store.steampowered.com/app/480\n/",
            "https://store.steampowered.com\\@attacker.invalid/app/480", String(repeating: "4", count: 2049)]
        for (input, expected) in accepted { precondition(SteamGameLink.appID(input) == expected) }
        for input in refused { precondition(SteamGameLink.appID(input) == nil) }
        print("Steam game links: \(accepted.count + refused.count) checks passed")
    }
}
