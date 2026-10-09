import AppKit
import Foundation
import ImageIO

@main
struct BrowserGameCoverTests {
    static func main() throws {
        var checks = 0
        func expect(_ value: Bool, _ message: String) {
            precondition(value, message)
            checks += 1
        }
        let original: [String: Any] = [
            "id": "example-game", "name": "Example Game", "detail": "Community browser game.",
            "url": "https://games.example.org/play", "sourceURL": "https://games.example.org/",
            "symbol": "gamecontroller", "requiresOwnFiles": false
        ]
        func entry(_ additions: [String: Any] = [:]) throws -> BrowserGameCatalogEntry {
            let document = original.merging(additions) { _, replacement in replacement }
            return try JSONDecoder().decode(BrowserGameCatalogEntry.self, from: JSONSerialization.data(withJSONObject: document))
        }
        let plain = try entry()
        expect(plain.artworkURL == nil && plain.artworkRedirectURLs.isEmpty, "old catalog entries retain title artwork")
        expect(plain.kind == .game, "old catalogue entries remain individual games")
        expect(try entry(["kind": "collection"]).kind == .collection, "collection pages decode as collections")
        expect(try entry(["kind": "unverified"]).kind == .unverified, "unverified links do not claim playable status")
        expect((try? entry(["kind": "launcher"])) == nil, "unknown catalogue kind refused")
        expect((try? entry(["kind": NSNull()])) == nil, "explicit null kind is refused like the catalogue validator")
        expect((try? entry(["artworkRedirectURLs": NSNull()])) == nil, "explicit null redirects are refused like the catalogue validator")
        let approved = try entry([
            "artworkURL": "https://games.example.org/images/cover.png",
            "artworkRedirectURLs": ["https://cdn.example.org/covers/game.png"]
        ])
        let source = BrowserGameCoverSource(entry: approved, catalog: [approved])!
        expect(BrowserGameCatalog.entry(forSavedURL: approved.url.absoluteString, in: [approved])?.id == approved.id,
               "Home can reuse the reviewed cover for the exact catalogue game URL")
        for different in ["https://games.example.org/", "https://games.example.org/play?level=1",
                          "https://games.example.org/play#level", "https://games.example.org/play/",
                          " https://games.example.org/play", "https://different.example.org/play"] {
            expect(BrowserGameCatalog.entry(forSavedURL: different, in: [approved]) == nil,
                   "custom or altered Home URLs never initiate catalogue picture requests")
        }
        expect(source.allowedURLs.count == 2, "explicit reviewed cover and redirect URLs")
        expect(BrowserGameCoverSource(entry: approved, catalog: []) == nil, "non-catalog entries never initiate artwork requests")
        expect(BrowserGameCoverSource(entry: approved, catalog: [plain]) == nil, "altered cover metadata cannot reuse a catalog id")
        let changedGame = try entry([
            "url": "https://other.example.org/play", "artworkURL": approved.artworkURL!.absoluteString,
            "artworkRedirectURLs": approved.artworkRedirectURLs.map(\.absoluteString)
        ])
        expect(BrowserGameCoverSource(entry: changedGame, catalog: [approved]) == nil, "altered saved game link cannot impersonate the catalog entry")
        for bad in [
            "http://games.example.org/cover.png", "file:///cover.png", "https://127.0.0.1/cover.png",
            "https://games.example.org:443/cover.png", "https://user:password@example.org/cover.png",
            "https://games.example.org/cover.png?token=private", "https://games.example.org/cover.png#anchor"
        ] {
            expect((try? entry(["artworkURL": bad])) == nil, "unsafe artwork URL refused")
        }
        for additions: [String: Any] in [
            ["artworkRedirectURLs": ["https://cdn.example.org/cover.png"]],
            ["artworkURL": source.url.absoluteString, "artworkRedirectURLs": [source.url.absoluteString]],
            ["artworkURL": source.url.absoluteString, "artworkRedirectURLs": ["https://cdn.example.org/cover.png", "https://cdn.example.org/cover.png"]],
            ["artworkURL": source.url.absoluteString, "artworkRedirectURLs": (1...4).map { "https://cdn.example.org/cover\($0).png" }],
            ["artworkURL": source.url.absoluteString, "artworkRedirectURLs": ["http://cdn.example.org/cover.png"]],
            ["artworkURL": source.url.absoluteString, "artworkRedirectURLs": "https://cdn.example.org/cover.png"]
        ] {
            expect((try? entry(additions)) == nil, "missing cover, duplicate, unbounded or malformed redirects refused")
        }
        let request = BrowserGameCoverPolicy.request(for: source.url, allowedURLs: source.allowedURLs)!
        expect(request.httpMethod == "GET" && request.httpBody == nil, "artwork uses a clean GET request")
        expect(!request.httpShouldHandleCookies, "request does not send stored cookies")
        for header in ["Authorization", "Proxy-Authorization", "Cookie", "Referer"] {
            expect(request.value(forHTTPHeaderField: header) == nil, "no account or referring-page header")
        }
        expect(BrowserGameCoverPolicy.request(for: approved.artworkRedirectURLs[0], allowedURLs: source.allowedURLs) != nil,
               "exact reviewed CDN redirect accepted")
        for bad in [
            "https://cdn.example.org/covers/other.png", "https://cdn.example.org/covers/game.png?download=1",
            "https://cdn.example.org/covers/game.png#x", "http://cdn.example.org/covers/game.png",
            "https://another.example.org/covers/game.png"
        ] {
            expect(BrowserGameCoverPolicy.request(for: URL(string: bad)!, allowedURLs: source.allowedURLs) == nil,
                   "unlisted, altered or insecure redirect refused")
        }
        let configuration = BrowserGameCoverPolicy.sessionConfiguration()
        expect(!configuration.httpShouldSetCookies && configuration.httpCookieStorage == nil, "session has no cookie persistence")
        expect(configuration.urlCredentialStorage == nil && configuration.urlCache == nil, "session has no account credential store or disk cache")
        expect(configuration.timeoutIntervalForRequest == 10 && configuration.timeoutIntervalForResource == 15, "requests have finite deadlines")
        func response(_ url: URL = source.url, status: Int = 200, type: String = "image/png", length: Int? = nil) -> HTTPURLResponse {
            var headers = ["Content-Type": type]
            if let length { headers["Content-Length"] = String(length) }
            return HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: headers)!
        }
        expect(BrowserGameCoverPolicy.accepts(response(), source: source), "unknown-length PNG stream accepted under incremental bound")
        expect(BrowserGameCoverPolicy.accepts(response(length: BrowserGameCoverPolicy.maximumBytes), source: source), "response byte boundary accepted")
        expect(!BrowserGameCoverPolicy.accepts(response(length: BrowserGameCoverPolicy.maximumBytes + 1), source: source), "oversized declared response refused")
        expect(!BrowserGameCoverPolicy.accepts(response(status: 302), source: source), "unfollowed redirect is not an image")
        expect(!BrowserGameCoverPolicy.accepts(response(status: 401), source: source), "account challenge is not an image")
        for type in ["text/html", "image/svg+xml", "application/octet-stream", "image/gif"] {
            expect(!BrowserGameCoverPolicy.accepts(response(type: type), source: source), "non-cover image or page MIME refused")
        }
        expect(!BrowserGameCoverPolicy.accepts(response(URL(string: "https://another.example.org/cover.png")!), source: source), "response URL must remain explicitly approved")
        for (width, height, accepted) in [(64, 64, true), (63, 64, false), (1280, 720, true), (8192, 64, true),
                                           (8193, 64, false), (8000, 4000, true), (8000, 4001, false), (-1, 100, false)] {
            expect(BrowserGameCoverPolicy.boundedDimensions(width: width, height: height) == accepted, "source dimension and pixel bounds")
        }
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 2048, pixelsHigh: 1024, bitsPerSample: 8,
                                      samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                                      bytesPerRow: 0, bitsPerPixel: 0)!
        bitmap.bitmapData!.update(repeating: 255, count: bitmap.bytesPerRow * bitmap.pixelsHigh)
        let png = bitmap.representation(using: .png, properties: [:])!
        let thumbnail = BrowserGameCoverPolicy.thumbnail(png, mimeType: "image/png")!
        let decoded = CGImageSourceCreateWithData(thumbnail as CFData, nil)!
        let properties = CGImageSourceCopyPropertiesAtIndex(decoded, 0, nil)! as NSDictionary
        expect(properties[kCGImagePropertyPixelWidth] as? Int == 960 && properties[kCGImagePropertyPixelHeight] as? Int == 480,
               "actual source raster is downsampled to the bounded landscape thumbnail")
        expect(BrowserGameCoverPolicy.thumbnail(png, mimeType: "image/jpeg") == nil, "declared MIME must match decoded source type")
        expect(BrowserGameCoverPolicy.thumbnail(Data("<html>not an image</html>".utf8), mimeType: "image/png") == nil, "page bytes are never decoded as a cover")
        expect(BrowserGameCoverPolicy.thumbnail(Data(repeating: 0, count: BrowserGameCoverPolicy.maximumBytes + 1), mimeType: "image/png") == nil,
               "encoded input byte bound also applies to the decoder")
        print("\(checks) browser game cover policy checks passed")
    }
}
