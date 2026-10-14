import AppKit
import Foundation
import ImageIO

@main
struct SteamGameArtworkTests {
    static func main() async throws {
        var checks = 0
        func expect(_ value: Bool, _ message: String) { precondition(value, message); checks += 1 }
        let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
            .appendingPathComponent(".build/steam-artwork-tests/\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let source = SteamGameArtworkSource(appID: 480)!
        expect(SteamGameArtworkSource(appID: 0) == nil, "invalid Steam identity never initiates a fetch")
        expect(source.urls.count == 2 && source.allowedURLs.count == 2, "only two exact public Steam image endpoints")
        let request = SteamGameArtworkPolicy.request(for: source.urls[0], source: source)!
        expect(request.httpMethod == "GET" && request.httpBody == nil && !request.httpShouldHandleCookies,
               "image request cannot carry bottle data or cookies")
        for header in ["Authorization", "Proxy-Authorization", "Cookie", "Referer"] {
            expect(request.value(forHTTPHeaderField: header) == nil, "no account, proxy or filesystem-bearing header")
        }
        var credentialFixture = URLComponents(url: source.urls[0], resolvingAgainstBaseURL: false)!
        credentialFixture.user = "fixture"
        credentialFixture.password = "fixture"
        for url in ["http://cdn.akamai.steamstatic.com/steam/apps/480/header.jpg",
                    "https://cdn.akamai.steamstatic.com/steam/apps/481/header.jpg",
                    "https://cdn.akamai.steamstatic.com/steam/apps/480/header.jpg?token=x",
                    credentialFixture.string!,
                    "https://cdn.akamai.steamstatic.com:443/steam/apps/480/header.jpg",
                    "https://127.0.0.1/header.jpg", "https://example.org/header.jpg"] {
            expect(SteamGameArtworkPolicy.request(for: URL(string: url)!, source: source) == nil,
                   "altered image identity, tracking, credentials and foreign redirects refused")
        }
        let configuration = SteamGameArtworkPolicy.sessionConfiguration()
        expect(configuration.urlCredentialStorage == nil && configuration.httpCookieStorage == nil
               && configuration.urlCache == nil && !configuration.httpShouldSetCookies,
               "ephemeral transport has no credential, cookie or shared response storage")
        func response(status: Int = 200, mime: String = "image/jpeg", size: Int = 1024) -> HTTPURLResponse {
            HTTPURLResponse(url: source.urls[0], statusCode: status, httpVersion: "HTTP/1.1",
                            headerFields: ["Content-Type": mime, "Content-Length": String(size)])!
        }
        expect(SteamGameArtworkPolicy.accepts(response(), source: source), "public JPEG response accepted")
        expect(!SteamGameArtworkPolicy.accepts(response(status: 401), source: source), "account challenge refused")
        expect(!SteamGameArtworkPolicy.accepts(response(mime: "text/html"), source: source), "error page is never game artwork")
        expect(!SteamGameArtworkPolicy.accepts(response(size: SteamGameArtworkPolicy.maximumBytes + 1), source: source),
               "oversized HTTP response refused before streaming")
        expect(!SteamGameArtworkPolicy.mayTryFallback(after: URLError(.serverCertificateUntrusted)), "TLS trust failure cannot fall back around verification")
        expect(!SteamGameArtworkPolicy.mayTryFallback(after: SteamGameArtworkError.httpStatus(403)), "account refusal cannot fall back around verification")
        expect(!SteamGameArtworkPolicy.mayTryFallback(after: SteamGameArtworkError.httpStatus(302)), "unapproved redirect cannot try another host")
        expect(SteamGameArtworkPolicy.mayTryFallback(after: SteamGameArtworkError.httpStatus(404)), "missing public asset can try the second exact endpoint")

        let png = try fixture(width: 1280, height: 600)
        let cover = SteamGameArtworkPolicy.thumbnail(png, mimeType: "image/png")!
        let dimensions = imageDimensions(cover)!
        expect(dimensions == [960, 450], "landscape game artwork keeps its complete original aspect ratio")
        expect(SteamGameArtworkPolicy.thumbnail(png, mimeType: "image/jpeg") == nil, "MIME must match the actual encoded format")
        expect(SteamGameArtworkPolicy.thumbnail(Data("<html>not artwork</html>".utf8)) == nil, "invalid encoded image rejected")
        expect(SteamGameArtworkPolicy.thumbnail(try fixture(width: 8193, height: 64)) == nil, "oversized source dimension rejected before decoding")
        expect(SteamGameArtworkPolicy.thumbnail(try fixture(width: 63, height: 64)) == nil, "tiny images are not promoted to game covers")

        let cache = root.appendingPathComponent("cache")
        expect(SteamGameArtworkCache.store(cover, source: source, root: cache), "normalized PNG atomically stored in private cache")
        expect(SteamGameArtworkCache.read(source, root: cache) != nil, "private cached cover reused offline")
        let mode = (try FileManager.default.attributesOfItem(atPath: cache.appendingPathComponent(source.cacheName).path)[.posixPermissions] as! NSNumber).intValue
        expect(mode == 0o600, "cached artwork accessible to this user only")
        let alternate = SteamGameArtworkSource(appID: 481)!
        try FileManager.default.createSymbolicLink(at: cache.appendingPathComponent(alternate.cacheName), withDestinationURL: cache.appendingPathComponent(source.cacheName))
        expect(SteamGameArtworkCache.read(alternate, root: cache) == nil, "symlink cannot impersonate another cached game")
        let symlinkRoot = root.appendingPathComponent("linked")
        try FileManager.default.createSymbolicLink(at: symlinkRoot, withDestinationURL: cache)
        expect(!SteamGameArtworkCache.store(cover, source: alternate, root: symlinkRoot.appendingPathComponent("new")),
               "cache creation never follows a symlinked ancestor")
        expect(!FileManager.default.fileExists(atPath: cache.appendingPathComponent("new").path), "refused cache write creates nothing through that ancestor")
        try Data("corrupt cached data".utf8).write(to: cache.appendingPathComponent(source.cacheName))
        expect(SteamGameArtworkCache.read(source, root: cache) == nil, "corrupt cache does not suppress a new publisher fetch")
        let pruningCache = root.appendingPathComponent("pruning-cache")
        for appID: UInt32 in 1...257 {
            precondition(SteamGameArtworkCache.store(cover, source: SteamGameArtworkSource(appID: appID)!, root: pruningCache))
        }
        expect(try FileManager.default.contentsOfDirectory(atPath: pruningCache.path).count == SteamGameArtworkPolicy.maximumCacheEntries,
               "disk cache is capped at 256 complete normalized covers")

        let probe = ArtworkProbe(data: cover)
        let store = SteamGameArtworkStore(cacheRoot: root.appendingPathComponent("actor-cache"), transport: { source, url in
            try await probe.fetch(source: source, url: url)
        })
        async let first = store.load(appID: 480, localPath: nil)
        async let second = store.load(appID: 480, localPath: nil)
        let pair = await (first, second)
        expect(pair.0 != nil && pair.0 == pair.1, "missing local art resolves automatically to one shared downloaded cover")
        expect(await probe.calls == 1, "simultaneous views share one publisher request")
        let cached = await store.load(appID: 480, localPath: nil), callsAfterCache = await probe.count()
        expect(cached != nil && callsAfterCache == 1, "cached cover requires no repeat network request")
        let local = root.appendingPathComponent("header.jpg")
        try png.write(to: local)
        let localResult = await store.load(appID: 481, localPath: local.path), callsAfterLocal = await probe.count()
        expect(localResult != nil && callsAfterLocal == 1, "existing named local cover is preferred without network")
        let unavailable = SteamGameArtworkStore(cacheRoot: nil, transport: { _, _ in throw URLError(.notConnectedToInternet) })
        expect(await unavailable.load(appID: 480, localPath: local.path) != nil, "local cover still works with no internet")
        let fallbackProbe = ArtworkProbe(data: cover, firstFailure: .httpStatus(404))
        let fallback = SteamGameArtworkStore(cacheRoot: nil, transport: { source, url in
            try await fallbackProbe.fetch(source: source, url: url)
        })
        expect(await fallback.load(appID: 480, localPath: nil) != nil, "missing primary asset automatically uses the second official endpoint")
        expect(await fallbackProbe.calls == 2, "fallback makes exactly two bounded image requests")
        let deniedProbe = ArtworkProbe(data: cover, firstFailure: .httpStatus(403))
        let denied = SteamGameArtworkStore(cacheRoot: nil, transport: { source, url in
            try await deniedProbe.fetch(source: source, url: url)
        })
        expect(await denied.load(appID: 480, localPath: nil) == nil, "account denial does not request another host")
        expect(await denied.load(appID: 480, localPath: nil) == nil, "unavailable artwork safely stays unavailable during the retry backoff")
        expect(await deniedProbe.calls == 1, "retry backoff prevents repeated failures when views reappear")

        if CommandLine.arguments.contains("--live") {
            // Optional actual transport check using public game IDs only. No
            // requests use an account; all test files are removed by defer.
            let live = SteamGameArtworkStore(cacheRoot: root.appendingPathComponent("live-cache"))
            for appID: UInt32 in [945360, 1943950, 3241660, 1962700] {
                let data = await live.load(appID: appID, localPath: nil)
                expect(data.flatMap(imageDimensions)?.count == 2, "live official Steam header resolves and decodes")
                print("Steam public artwork \(appID): normalized cover verified")
            }
        }
        print("\(checks) Steam artwork checks passed")
    }

    static func fixture(width: Int, height: Int) throws -> Data {
        let image = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
                                     bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                     colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        image.bitmapData!.update(repeating: 150, count: image.bytesPerRow * image.pixelsHigh)
        return image.representation(using: .png, properties: [:])!
    }

    static func imageDimensions(_ data: Data) -> [Int]? {
        guard let image = CGImageSourceCreateWithData(data as CFData, nil),
              let props = CGImageSourceCopyPropertiesAtIndex(image, 0, nil) as? [CFString: Any],
              let width = props[kCGImagePropertyPixelWidth] as? Int,
              let height = props[kCGImagePropertyPixelHeight] as? Int else { return nil }
        return [width, height]
    }
}

private actor ArtworkProbe {
    let data: Data
    let firstFailure: SteamGameArtworkError?
    private(set) var calls = 0
    init(data: Data, firstFailure: SteamGameArtworkError? = nil) { self.data = data; self.firstFailure = firstFailure }
    func fetch(source: SteamGameArtworkSource, url: URL) async throws -> Data {
        calls += 1
        try await Task.sleep(nanoseconds: 50_000_000)
        if calls == 1, let firstFailure { throw firstFailure }
        return data
    }
    func count() -> Int { calls }
}
