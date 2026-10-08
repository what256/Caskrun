import Foundation

@main
struct BrowserGameTests {
    static func main() throws {
        var checks = 0
        func expect(_ value: Bool, _ message: String) {
            precondition(value, message)
            checks += 1
        }
        expect(BrowserGame.checkedURL("https://games.example.org/")?.host == "games.example.org", "ordinary HTTPS game")
        expect(BrowserGame.checkedURL(" https://Games.EXAMPLE.org/play?q=one%20two#level ") != nil, "trimmed link and encoded spaces")
        for bad in ["file:///game", "http://games.example.org/", "javascript:alert(1)", "https://" + "name:secret@games.example.org/", "https://games.example.org:443/", "https://127.0.0.1/", "https://[::1]/", "https://localhost/", "https://game.local/", "https://game.test/", "https://game.example/", "https://games.example.org./", "https://" + "games.example.org\\@evil.org/", "https://games.example.org/%00", "https://games.example.org/%0a", "https://games.example.org/%5c", "https://games.example.org/%", "https://games.example.org/a b", "https://gáme.example.org/", "https://games.example.org/\"bad\""] {
            expect(BrowserGame.checkedURL(bad) == nil, "unsafe link refused: \(bad)")
        }
        expect(BrowserGame.checkedURL("https://games.example.org/" + String(repeating: "x", count: 2048)) == nil, "bounded URL")
        for suffix in ["?token=private", "?ACCESS_TOKEN=private", "?%74%6f%6b%65%6e=private", "?access%5ftoken=private", "?session[id]=private", "?email=person", "?api-key=private", "?auth_token=private", "?AUTH%5fTOKEN=private", "?bearer_token=private", "?authorization_code=private", "?code=private", "?client_secret=private", "?jwt=private", "?oauth_token=private", "?oauth_token_secret=private", "?oauth_verifier=private", "#id_token=private", "#auth[token]=private", "#client-secret=private", "#/play?authorization=private", "?game=1;Password=private"] {
            expect(BrowserGame.checkedURL("https://games.example.org/" + suffix) == nil, "account link refused: \(suffix)")
        }
        let game = try JSONDecoder().decode(BrowserGame.self, from: Data(#"{"id":"example","name":"Browser Game","url":"https://games.example.org/","created_at":1}"#.utf8))
        expect(game.name == "Browser Game" && game.created_at == 1, "backend DTO decode")
        expect(LibraryScope.allCases.map(\.title) == ["All", "Steam", "Epic", "GOG", "Apps", "Browser"], "compact scopes")
        print("\(checks) browser game checks passed")
    }
}
