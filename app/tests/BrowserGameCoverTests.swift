import AppKit
import Foundation
import ImageIO

@main
struct BrowserGameCoverTests {
    static func main() async throws {
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
        expect(plain.artworkURL == nil && plain.artworkRedirectURLs.isEmpty && plain.artworkFallbackURLs.isEmpty, "old catalog entries retain default metadata")
        expect(plain.kind == .game, "old catalogue entries remain individual games")
        expect(try entry(["kind": "collection"]).kind == .collection, "collection pages decode as collections")
        expect(try entry(["kind": "unverified"]).kind == .unverified, "unverified links do not claim playable status")
        expect((try? entry(["kind": "launcher"])) == nil, "unknown catalogue kind refused")
        expect((try? entry(["kind": NSNull()])) == nil, "explicit null kind is refused like the catalogue validator")
        expect((try? entry(["artworkRedirectURLs": NSNull()])) == nil, "explicit null redirects are refused like the catalogue validator")
        expect((try? entry(["artworkFallbackURLs": NSNull()])) == nil, "explicit null fallback array is refused")
        let approved = try entry([
            "artworkURL": "https://games.example.org/images/cover.png",
            "artworkRedirectURLs": ["https://cdn.example.org/covers/game.png"]
        ])
        let source = BrowserGameCoverSource(entry: approved, catalog: [approved])!
        let fallbackEntry = try entry([
            "artworkURL": source.url.absoluteString,
            "artworkRedirectURLs": approved.artworkRedirectURLs.map(\.absoluteString),
            "artworkFallbackURLs": ["https://backup.example.org/cover.png", "https://second.example.org/cover.png"]
        ])
        let fallbackSource = BrowserGameCoverSource(entry: fallbackEntry, catalog: [fallbackEntry])!
        let securityEntry = try entry([
            "artworkURL": source.url.absoluteString,
            "artworkRedirectURLs": approved.artworkRedirectURLs.map(\.absoluteString),
            "artworkFallbackURLs": ["https://games.example.org/images/backup.png", "https://second.example.org/cover.png"]
        ])
        expect(fallbackSource.chain.count == 3, "primary and two explicitly reviewed fallback assets form a bounded chain")
        expect(fallbackSource.chain[0].allowedURLs == source.allowedURLs, "primary retains only its explicitly approved redirects")
        expect(fallbackSource.chain[1].allowedURLs == [fallbackEntry.artworkFallbackURLs[0].absoluteString]
               && fallbackSource.chain[2].allowedURLs == [fallbackEntry.artworkFallbackURLs[1].absoluteString],
               "each fallback admits only its exact reviewed URL")
        expect(BrowserGameCoverSource(entry: fallbackEntry, catalog: [approved]) == nil, "altered fallback metadata cannot reuse a catalogue id")
        expect(source.cacheKey != fallbackSource.cacheKey, "changed fallback metadata invalidates flight and cache identity")
        let differentScope = try entry([
            "artworkURL": source.url.absoluteString,
            "artworkRedirectURLs": approved.artworkRedirectURLs.map(\.absoluteString) + fallbackEntry.artworkFallbackURLs.map(\.absoluteString)
        ])
        expect(BrowserGameCoverSource(entry: differentScope, catalog: [differentScope])!.cacheKey != fallbackSource.cacheKey,
               "redirect and fallback authority cannot share an ambiguous cache key")
        for additions: [String: Any] in [
            ["artworkFallbackURLs": ["https://backup.example.org/cover.png"]],
            ["artworkURL": source.url.absoluteString, "artworkFallbackURLs": [source.url.absoluteString]],
            ["artworkURL": source.url.absoluteString, "artworkFallbackURLs": ["https://backup.example.org/cover.png", "https://backup.example.org/cover.png"]],
            ["artworkURL": source.url.absoluteString, "artworkFallbackURLs": (1...3).map { "https://backup.example.org/cover\($0).png" }],
            ["artworkURL": source.url.absoluteString, "artworkFallbackURLs": ["http://backup.example.org/cover.png"]],
            ["artworkURL": source.url.absoluteString, "artworkFallbackURLs": ["https://backup.example.org/cover.png#part"]],
            ["artworkURL": source.url.absoluteString, "artworkFallbackURLs": ["https://backup.example.org/cover.png?token=private"]],
            ["artworkURL": source.url.absoluteString, "artworkRedirectURLs": approved.artworkRedirectURLs.map(\.absoluteString),
             "artworkFallbackURLs": approved.artworkRedirectURLs.map(\.absoluteString)]
        ] {
            expect((try? entry(additions)) == nil, "orphaned, duplicate, insecure, credentialed or overlapping fallback metadata refused")
        }
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
        for status in [408, 429, 500, 502, 503, 504] {
            expect(BrowserGameCoverPolicy.shouldRetry(BrowserGameCoverDownloadError.httpStatus(status)), "transient HTTP response can retry")
        }
        for status in [301, 302, 400, 401, 403, 404, 410] {
            expect(!BrowserGameCoverPolicy.shouldRetry(BrowserGameCoverDownloadError.httpStatus(status)), "redirect, account and missing-picture responses do not retry")
        }
        for code in [URLError.Code.timedOut, .networkConnectionLost, .cannotConnectToHost] {
            expect(BrowserGameCoverPolicy.shouldRetry(URLError(code)), "transient network failure can retry")
        }
        for code in [URLError.Code.cancelled, .serverCertificateUntrusted, .serverCertificateHasBadDate, .secureConnectionFailed] {
            expect(!BrowserGameCoverPolicy.shouldRetry(URLError(code)), "cancellation and TLS trust errors never retry")
        }
        for error: Error in [BrowserGameCoverDownloadError.authenticationDenied,
                             BrowserGameCoverDownloadError.httpStatus(401), BrowserGameCoverDownloadError.httpStatus(403),
                             URLError(.serverCertificateUntrusted), URLError(.secureConnectionFailed)] {
            expect(BrowserGameCoverPolicy.isSecurityFailure(error), "failed TLS and account challenges block that source host")
        }
        for error: Error in [BrowserGameCoverDownloadError.overflow, BrowserGameCoverDownloadError.httpStatus(404),
                             URLError(.timedOut), URLError(.cancelled)] {
            expect(!BrowserGameCoverPolicy.isSecurityFailure(error), "size, availability and cancellation errors do not disable an independent image at that host")
        }
        expect(!BrowserGameCoverPolicy.shouldRetry(BrowserGameCoverDownloadError.overflow), "oversized content does not retry")
        expect(!BrowserGameCoverPolicy.shouldRetry(BrowserGameCoverDownloadError.rejectedResponse), "unapproved response does not retry")
        var buffer = BrowserGameCoverBuffer()
        try buffer.append(Data(repeating: 1, count: BrowserGameCoverPolicy.maximumBytes - 1))
        try buffer.append(Data([2]))
        expect(buffer.data.count == BrowserGameCoverPolicy.maximumBytes && buffer.data.last == 2, "chunk accumulation accepts the exact encoded byte boundary")
        expect((try? buffer.append(Data([3]))) == nil && buffer.data.count == BrowserGameCoverPolicy.maximumBytes,
               "overflow chunk is refused without retaining it")
        var wholeChunk = BrowserGameCoverBuffer()
        expect((try? wholeChunk.append(Data(repeating: 0, count: BrowserGameCoverPolicy.maximumBytes + 1))) == nil && wholeChunk.data.isEmpty,
               "an oversized first chunk is refused before append")
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
        expect(BrowserGameCoverPolicy.acceptedTypes["image/avif"] == "public.avif", "AVIF uses only the exact ImageIO source type")
        expect(BrowserGameCoverPolicy.request(for: source.url, allowedURLs: source.allowedURLs)?.value(forHTTPHeaderField: "Accept")?.contains("image/avif") == true,
               "reviewed AVIF previews can be requested")
        if (CGImageDestinationCopyTypeIdentifiers() as NSArray).contains("public.avif") {
            let avif = NSMutableData()
            let destination = CGImageDestinationCreateWithData(avif, "public.avif" as CFString, 1, nil)!
            let image = CGImageSourceCreateImageAtIndex(CGImageSourceCreateWithData(png as CFData, nil)!, 0, nil)!
            CGImageDestinationAddImage(destination, image, nil)
            expect(CGImageDestinationFinalize(destination), "local no-network AVIF fixture encodes")
            expect(BrowserGameCoverPolicy.thumbnail(avif as Data, mimeType: "image/avif") != nil, "actual bounded AVIF source decodes as a preview")
            expect(BrowserGameCoverPolicy.thumbnail(avif as Data, mimeType: "image/png") == nil, "AVIF cannot masquerade as another MIME type")
        }
        checks += await sharingChecks(entry: approved, fallbackEntry: fallbackEntry, securityEntry: securityEntry)
        print("\(checks) browser game cover policy checks passed")
    }

    private static func sharingChecks(entry: BrowserGameCatalogEntry, fallbackEntry: BrowserGameCatalogEntry,
                                      securityEntry: BrowserGameCatalogEntry) async -> Int {
        var checks = 0
        func expect(_ value: Bool, _ message: String) { precondition(value, message); checks += 1 }
        let image = Data([1, 2, 3])
        let probe = CoverFlightProbe()
        let store = BrowserGameCoverStore(catalog: { [entry] }, transport: { _ in await probe.read() })
        let first = Task { await store.load(entry) }
        let second = Task { await store.load(entry) }
        await waitUntil {
            let waiters = await store.waitingCount, starts = await probe.starts
            return waiters == 2 && starts == 1
        }
        first.cancel()
        expect(await first.value == nil, "cancelled view returns promptly while another view still waits")
        expect(await probe.cancellations == 0, "one cancelled view does not abort the shared picture request")
        await probe.finish(.success(image))
        expect(await second.value == image, "remaining view receives the shared picture")
        let cached = await store.load(entry), starts = await probe.starts
        expect(cached == image && starts == 1, "memory cache prevents a new request after success")
        expect(await store.waitingCount == 0, "successful shared request removes all waiters")

        let cancelProbe = CoverFlightProbe()
        let cancelStore = BrowserGameCoverStore(catalog: { [entry] }, transport: { _ in await cancelProbe.read() })
        let third = Task { await cancelStore.load(entry) }
        let fourth = Task { await cancelStore.load(entry) }
        await waitUntil {
            let waiters = await cancelStore.waitingCount, starts = await cancelProbe.starts
            return waiters == 2 && starts == 1
        }
        third.cancel(); fourth.cancel()
        let thirdValue = await third.value, fourthValue = await fourth.value
        expect(thirdValue == nil && fourthValue == nil, "all cancelled views finish without waiting for the resource deadline")
        await waitUntil { await cancelProbe.cancellations == 1 }
        expect(await cancelStore.waitingCount == 0, "last cancellation removes the flight and aborts its request once")

        let retry = CoverRetryProbe(results: [.failure(retryable: true), .failure(retryable: true), .success(image)])
        let retryStore = BrowserGameCoverStore(catalog: { [entry] }, transport: { _ in await retry.read() })
        let recovered = await retryStore.load(entry), retryStarts = await retry.starts
        expect(recovered == image && retryStarts == 3, "bounded automatic retries recover a transient request")
        let exhausted = CoverRetryProbe(results: [.failure(retryable: true)])
        let exhaustedStore = BrowserGameCoverStore(catalog: { [entry] }, transport: { _ in await exhausted.read() })
        let failed = await exhaustedStore.load(entry), exhaustedStarts = await exhausted.starts
        expect(failed == nil && exhaustedStarts == 3, "persistent transient failure stops after three attempts")
        let permanent = CoverRetryProbe(results: [.failure(retryable: false)])
        let permanentStore = BrowserGameCoverStore(catalog: { [entry] }, transport: { _ in await permanent.read() })
        let rejected = await permanentStore.load(entry), permanentStarts = await permanent.starts
        expect(rejected == nil && permanentStarts == 1, "permanent failure is not repeated")
        let emptyStore = BrowserGameCoverStore(catalog: { [] }, transport: { _ in await permanent.read() })
        let unknown = await emptyStore.load(entry), startsAfterUnknown = await permanent.starts
        expect(unknown == nil && startsAfterUnknown == 1, "custom or unapproved entries never reach the transport")

        let independent = CoverRetryProbe(results: [.failure(retryable: false), .success(image)])
        let independentStore = BrowserGameCoverStore(catalog: { [fallbackEntry] }, transport: { await independent.read($0) })
        expect(await independentStore.load(fallbackEntry) == image, "an independent reviewed fallback can recover after a permanent primary failure")
        expect(await independent.requestedURLs == [fallbackEntry.artworkURL!.absoluteString, fallbackEntry.artworkFallbackURLs[0].absoluteString],
               "permanent or security failure is not retried at the failed primary host")
        let primaryRetries = CoverRetryProbe(results: [.failure(retryable: true), .failure(retryable: true), .failure(retryable: true), .success(image)])
        let primaryRetryStore = BrowserGameCoverStore(catalog: { [fallbackEntry] }, transport: { await primaryRetries.read($0) })
        expect(await primaryRetryStore.load(fallbackEntry) == image, "fallback follows bounded transient attempts at the primary")
        expect(await primaryRetries.requestedURLs == Array(repeating: fallbackEntry.artworkURL!.absoluteString, count: 3) + [fallbackEntry.artworkFallbackURLs[0].absoluteString],
               "primary retry count remains bounded before trying the separate reviewed source")
        let allFailed = CoverRetryProbe(results: [.failure(retryable: false)])
        let allFailedStore = BrowserGameCoverStore(catalog: { [fallbackEntry] }, transport: { await allFailed.read($0) })
        expect(await allFailedStore.load(fallbackEntry) == nil, "exhausted reviewed fallbacks return the unavailable state")
        expect(await allFailed.requestedURLs == [fallbackEntry.artworkURL!.absoluteString] + fallbackEntry.artworkFallbackURLs.map(\.absoluteString),
               "fallback exhaustion never invents another URL")
        let denied = CoverRetryProbe(results: [.failure(retryable: true, securityFailure: true), .success(image)])
        let deniedStore = BrowserGameCoverStore(catalog: { [securityEntry] }, transport: { await denied.read($0) })
        expect(await deniedStore.load(securityEntry) == image, "security failure permits only an independent reviewed host")
        expect(await denied.requestedURLs == [securityEntry.artworkURL!.absoluteString, securityEntry.artworkFallbackURLs[1].absoluteString],
               "TLS or auth failure overrides retry and skips a fallback at the failed host")

        let failedPrimary = CoverRetryProbe(results: [.failure(retryable: false)])
        let stalledFallback = CoverFlightProbe()
        let cancelChain = BrowserGameCoverStore(catalog: { [fallbackEntry] }, transport: { source in
            if source.url == fallbackEntry.artworkURL { return await failedPrimary.read(source) }
            return await stalledFallback.read()
        })
        let awaitingFallback = Task { await cancelChain.load(fallbackEntry) }
        await waitUntil { await stalledFallback.starts == 1 }
        awaitingFallback.cancel()
        expect(await awaitingFallback.value == nil, "cancellation interrupts a stalled fallback immediately")
        await waitUntil { await stalledFallback.cancellations == 1 }
        expect(await stalledFallback.starts == 1, "cancellation halts the whole chain before a later fallback")
        return checks
    }

    private static func waitUntil(_ predicate: @escaping () async -> Bool) async {
        for _ in 0..<200 {
            if await predicate() { return }
            try? await Task.sleep(nanoseconds: 5_000_000)
        }
        preconditionFailure("Cover concurrency test exceeded its one-second synchronization deadline")
    }
}

private actor CoverFlightProbe {
    private(set) var starts = 0
    private(set) var cancellations = 0
    private var pending: [UUID: CheckedContinuation<BrowserGameCoverResult, Never>] = [:]

    func read() async -> BrowserGameCoverResult {
        starts += 1
        let id = UUID()
        return await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                if Task.isCancelled { continuation.resume(returning: .failure(retryable: false)) }
                else { pending[id] = continuation }
            }
        } onCancel: { Task { await self.cancel(id) } }
    }

    private func cancel(_ id: UUID) {
        guard let continuation = pending.removeValue(forKey: id) else { return }
        cancellations += 1
        continuation.resume(returning: .failure(retryable: false))
    }

    func finish(_ result: BrowserGameCoverResult) {
        let continuations = Array(pending.values)
        pending.removeAll()
        for continuation in continuations { continuation.resume(returning: result) }
    }
}

private actor CoverRetryProbe {
    private(set) var starts = 0
    private(set) var requestedURLs: [String] = []
    private var results: [BrowserGameCoverResult]
    init(results: [BrowserGameCoverResult]) { self.results = results }
    func read(_ source: BrowserGameCoverSource? = nil) -> BrowserGameCoverResult {
        starts += 1
        if let source { requestedURLs.append(source.url.absoluteString) }
        return results.count > 1 ? results.removeFirst() : results[0]
    }
}
