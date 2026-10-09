import AppKit
import ImageIO
import SwiftUI

struct BrowserGame: Codable, Identifiable, Sendable {
    let id: String
    let name: String
    let url: String
    let created_at: UInt64

    static func checkedURL(_ value: String) -> URL? { BrowserGameLink.validatedURL(value) }
}

struct BrowserGames: Decodable, Sendable {
    let games: [BrowserGame]
}

enum BrowserGameCatalogKind: String, Decodable, Sendable {
    case game, collection, unverified
}

struct BrowserGameCatalogEntry: Decodable, Identifiable, Sendable {
    let id: String
    let name: String
    let detail: String
    let url: URL
    let sourceURL: URL
    let symbol: String
    let requiresOwnFiles: Bool
    let artworkURL: URL?
    let artworkRedirectURLs: [URL]
    let artworkFallbackURLs: [URL]
    let kind: BrowserGameCatalogKind

    private enum CodingKeys: String, CodingKey {
        case id, name, detail, url, sourceURL, symbol, requiresOwnFiles, artworkURL, artworkRedirectURLs, artworkFallbackURLs, kind
    }

    init(from decoder: Decoder) throws {
        let fields = try decoder.container(keyedBy: CodingKeys.self)
        id = try fields.decode(String.self, forKey: .id)
        name = try fields.decode(String.self, forKey: .name)
        detail = try fields.decode(String.self, forKey: .detail)
        url = try fields.decode(URL.self, forKey: .url)
        sourceURL = try fields.decode(URL.self, forKey: .sourceURL)
        symbol = try fields.decode(String.self, forKey: .symbol)
        requiresOwnFiles = try fields.decode(Bool.self, forKey: .requiresOwnFiles)
        artworkURL = try fields.decodeIfPresent(URL.self, forKey: .artworkURL)
        artworkRedirectURLs = fields.contains(.artworkRedirectURLs)
            ? try fields.decode([URL].self, forKey: .artworkRedirectURLs) : []
        artworkFallbackURLs = fields.contains(.artworkFallbackURLs)
            ? try fields.decode([URL].self, forKey: .artworkFallbackURLs) : []
        kind = fields.contains(.kind) ? try fields.decode(BrowserGameCatalogKind.self, forKey: .kind) : .game
        guard BrowserGameCoverPolicy.validArtworkMetadata(url: artworkURL, redirects: artworkRedirectURLs, fallbacks: artworkFallbackURLs) else {
            throw DecodingError.dataCorruptedError(forKey: .artworkURL, in: fields,
                                                   debugDescription: "Invalid browser game artwork URLs")
        }
    }
}

enum BrowserGameCatalog {
    private struct Document: Decodable {
        let schemaVersion: Int
        let entries: [BrowserGameCatalogEntry]
    }

    static let loaded: (entries: [BrowserGameCatalogEntry], error: String?) = {
        guard let url = Bundle.main.url(forResource: "browser-games", withExtension: "json", subdirectory: "catalog") else {
            return ([], "The browser game catalog is missing. You can still add your own game link.")
        }
        do {
            let bytes = try Data(contentsOf: url)
            guard bytes.count <= 128 * 1024 else { throw CocoaError(.fileReadTooLarge) }
            let document = try JSONDecoder().decode(Document.self, from: bytes)
            guard document.schemaVersion == 1, document.entries.count <= 64,
                  Set(document.entries.map(\.id)).count == document.entries.count,
                  document.entries.allSatisfy({ entry in
                      !entry.id.isEmpty && entry.id.utf8.count <= 64 && !entry.name.isEmpty && entry.name.utf8.count <= 160
                          && entry.detail.utf8.count <= 512 && !entry.symbol.isEmpty
                          && BrowserGameLink.validatedURL(entry.url.absoluteString) != nil
                          && BrowserGameLink.validatedURL(entry.sourceURL.absoluteString) != nil
                          && BrowserGameCoverPolicy.validArtworkMetadata(url: entry.artworkURL, redirects: entry.artworkRedirectURLs, fallbacks: entry.artworkFallbackURLs)
                  }) else { throw CocoaError(.coderInvalidValue) }
            return (document.entries, nil)
        } catch {
            return ([], "The browser game catalog could not be read. You can still add your own game link.")
        }
    }()

    static func entry(forSavedURL url: String, in entries: [BrowserGameCatalogEntry] = loaded.entries) -> BrowserGameCatalogEntry? {
        // A custom bookmark with a similar host or path does not inherit the
        // catalogue's artwork authority. Only the exact public game URL does.
        entries.first { $0.url.absoluteString == url }
    }
}

/// Artwork comes only from reviewed, explicit catalogue metadata. Adding a
/// custom bookmark never causes Caskrun to crawl that site or load its images.
struct BrowserGameCoverSource: Sendable {
    let url: URL
    let allowedURLs: Set<String>
    let fallbackURLs: [URL]

    init?(entry: BrowserGameCatalogEntry, catalog: [BrowserGameCatalogEntry]) {
        guard let approved = catalog.first(where: { $0.id == entry.id }),
              approved.url == entry.url, approved.artworkURL == entry.artworkURL,
              approved.artworkRedirectURLs == entry.artworkRedirectURLs,
              approved.artworkFallbackURLs == entry.artworkFallbackURLs,
              let url = approved.artworkURL,
              BrowserGameCoverPolicy.validArtworkMetadata(url: url, redirects: approved.artworkRedirectURLs, fallbacks: approved.artworkFallbackURLs) else { return nil }
        self.url = url
        allowedURLs = Set(([url] + approved.artworkRedirectURLs).map(\.absoluteString))
        fallbackURLs = approved.artworkFallbackURLs
    }

    private init(url: URL) { self.url = url; allowedURLs = [url.absoluteString]; fallbackURLs = [] }

    var chain: [BrowserGameCoverSource] { [self] + fallbackURLs.map { BrowserGameCoverSource(url: $0) } }
    var cacheKey: String {
        (["primary", url.absoluteString, "allowed"] + allowedURLs.sorted()
         + ["fallbacks"] + fallbackURLs.map(\.absoluteString)).joined(separator: "\n")
    }
}

enum BrowserGameCoverPolicy {
    static let maximumBytes = 2 * 1024 * 1024
    static let maximumRedirects = 3
    static let maximumDimension = 8192
    static let maximumPixels = 32_000_000
    static let thumbnailDimension = 960
    static let acceptedTypes: [String: String] = [
        "image/png": "public.png", "image/jpeg": "public.jpeg", "image/webp": "org.webmproject.webp", "image/avif": "public.avif"
    ]
    static let maximumAttempts = 3

    static func shouldRetry(_ error: Error) -> Bool {
        if let failure = error as? BrowserGameCoverDownloadError, case let .httpStatus(status) = failure {
            return status == 408 || status == 429 || (500...599).contains(status)
        }
        guard let network = error as? URLError else { return false }
        return [.timedOut, .cannotFindHost, .cannotConnectToHost, .networkConnectionLost,
                .dnsLookupFailed, .notConnectedToInternet, .resourceUnavailable].contains(network.code)
    }

    static func isSecurityFailure(_ error: Error) -> Bool {
        if let failure = error as? BrowserGameCoverDownloadError {
            switch failure {
            case .authenticationDenied: return true
            case let .httpStatus(status): return status == 401 || status == 403
            default: return false
            }
        }
        guard let network = error as? URLError else { return false }
        return [.serverCertificateUntrusted, .serverCertificateHasBadDate, .serverCertificateHasUnknownRoot,
                .serverCertificateNotYetValid, .secureConnectionFailed, .clientCertificateRejected,
                .clientCertificateRequired, .userAuthenticationRequired, .userCancelledAuthentication,
                .appTransportSecurityRequiresSecureConnection].contains(network.code)
    }

    static func retryDelay(afterAttempt attempt: Int) -> UInt64 { attempt == 0 ? 350_000_000 : 900_000_000 }

    static func checkedArtworkURL(_ url: URL) -> Bool {
        url.fragment == nil && BrowserGameLink.validatedURL(url.absoluteString) == url
    }

    static func validArtworkMetadata(url: URL?, redirects: [URL], fallbacks: [URL] = []) -> Bool {
        guard redirects.count <= maximumRedirects, Set(redirects).count == redirects.count,
              redirects.allSatisfy(checkedArtworkURL), fallbacks.count <= 2,
              Set(fallbacks).count == fallbacks.count, fallbacks.allSatisfy(checkedArtworkURL),
              Set(redirects).isDisjoint(with: Set(fallbacks)) else { return false }
        guard let url else { return redirects.isEmpty && fallbacks.isEmpty }
        return checkedArtworkURL(url) && !redirects.contains(url) && !fallbacks.contains(url)
    }

    static func request(for url: URL, allowedURLs: Set<String>) -> URLRequest? {
        guard checkedArtworkURL(url), allowedURLs.contains(url.absoluteString) else { return nil }
        // Rebuilding each redirect drops cookies, authorization and a Referer
        // which may have been attached to the server's redirect request.
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalAndRemoteCacheData, timeoutInterval: 10)
        request.httpMethod = "GET"
        request.httpShouldHandleCookies = false
        request.setValue("image/png,image/jpeg,image/webp,image/avif", forHTTPHeaderField: "Accept")
        request.setValue("Caskrun/0.1", forHTTPHeaderField: "User-Agent")
        return request
    }

    static func sessionConfiguration() -> URLSessionConfiguration {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpShouldSetCookies = false
        configuration.httpCookieStorage = nil
        configuration.urlCredentialStorage = nil
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalAndRemoteCacheData
        configuration.timeoutIntervalForRequest = 10
        configuration.timeoutIntervalForResource = 15
        configuration.httpMaximumConnectionsPerHost = 2
        return configuration
    }

    static func accepts(_ response: HTTPURLResponse, source: BrowserGameCoverSource) -> Bool {
        guard response.statusCode == 200, let url = response.url,
              checkedArtworkURL(url), source.allowedURLs.contains(url.absoluteString),
              response.expectedContentLength < 0 || response.expectedContentLength <= maximumBytes,
              let mime = response.mimeType?.lowercased(), acceptedTypes[mime] != nil else { return false }
        return true
    }

    static func boundedDimensions(width: Int, height: Int) -> Bool {
        width >= 64 && height >= 64 && width <= maximumDimension && height <= maximumDimension
            && width * height <= maximumPixels
    }

    static func thumbnail(_ data: Data, mimeType: String) -> Data? {
        guard !data.isEmpty, data.count <= maximumBytes,
              let expectedType = acceptedTypes[mimeType.lowercased()],
              let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              let actualType = CGImageSourceGetType(source) as String?, actualType == expectedType,
              CGImageSourceGetCount(source) > 0,
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int,
              boundedDimensions(width: width, height: height) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: thumbnailDimension
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(output, "public.png" as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else { return nil }
        return output as Data
    }
}

enum BrowserGameCoverDownloadError: Error {
    case rejectedResponse, overflow, authenticationDenied, httpStatus(Int)
}

/// URLSession delivers chunks; reject the whole response before retaining a
/// chunk which would cross the encoded-image limit.
struct BrowserGameCoverBuffer {
    private(set) var data = Data()

    mutating func append(_ chunk: Data) throws {
        guard chunk.count <= BrowserGameCoverPolicy.maximumBytes - data.count else {
            throw BrowserGameCoverDownloadError.overflow
        }
        data.append(chunk)
    }
}

private final class BrowserGameCoverDownload: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private let source: BrowserGameCoverSource
    private let lock = NSLock()
    private var redirects = 0
    private var buffer = BrowserGameCoverBuffer()
    private var acceptedResponse = false
    private var terminal: Result<Data, Error>?
    private var continuation: CheckedContinuation<Data, Error>?
    private var task: URLSessionDataTask?

    init(source: BrowserGameCoverSource) { self.source = source }

    func read(session: URLSession, request: URLRequest) async throws -> Data {
        try await withCheckedThrowingContinuation { continuation in
            lock.lock()
            if let terminal {
                lock.unlock()
                continuation.resume(with: terminal)
                return
            }
            self.continuation = continuation
            lock.unlock()
            let task = session.dataTask(with: request)
            lock.lock()
            let alreadyFinished = terminal != nil
            if !alreadyFinished { self.task = task }
            lock.unlock()
            if alreadyFinished { task.cancel() }
            else { task.resume() }
        }
    }

    private func finish(_ result: Result<Data, Error>) {
        lock.lock()
        guard terminal == nil else { lock.unlock(); return }
        terminal = result
        let continuation = continuation
        self.continuation = nil
        let task = task
        self.task = nil
        lock.unlock()
        if case .failure = result { task?.cancel() }
        continuation?.resume(with: result)
    }

    func cancel() { finish(.failure(CancellationError())) }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse,
                    completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
        guard let http = response as? HTTPURLResponse else {
            finish(.failure(BrowserGameCoverDownloadError.rejectedResponse)); completionHandler(.cancel); return
        }
        guard http.statusCode == 200 else {
            finish(.failure(BrowserGameCoverDownloadError.httpStatus(http.statusCode))); completionHandler(.cancel); return
        }
        guard BrowserGameCoverPolicy.accepts(http, source: source) else {
            finish(.failure(BrowserGameCoverDownloadError.rejectedResponse)); completionHandler(.cancel); return
        }
        lock.lock()
        let unfinished = terminal == nil
        if unfinished {
            acceptedResponse = true
            responseMIME = http.mimeType
        }
        lock.unlock()
        completionHandler(unfinished ? .allow : .cancel)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        lock.lock()
        guard terminal == nil, acceptedResponse else { lock.unlock(); return }
        do { try buffer.append(data); lock.unlock() }
        catch { lock.unlock(); finish(.failure(error)) }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        if let error { finish(.failure(error)); return }
        lock.lock()
        let result: Result<Data, Error> = acceptedResponse
            ? .success(buffer.data) : .failure(BrowserGameCoverDownloadError.rejectedResponse)
        lock.unlock()
        finish(result)
    }

    func urlSession(_ session: URLSession, task: URLSessionTask,
                    willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
                    completionHandler: @escaping (URLRequest?) -> Void) {
        lock.lock()
        redirects += 1
        let withinLimit = redirects <= BrowserGameCoverPolicy.maximumRedirects
        lock.unlock()
        completionHandler(withinLimit ? request.url.flatMap { BrowserGameCoverPolicy.request(for: $0, allowedURLs: source.allowedURLs) } : nil)
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didReceive challenge: URLAuthenticationChallenge,
                    completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        // TLS trust uses the system verifier; website or proxy account challenges
        // cannot draw on a user's saved credentials.
        if challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust {
            completionHandler(.performDefaultHandling, nil)
        } else {
            finish(.failure(BrowserGameCoverDownloadError.authenticationDenied))
            completionHandler(.cancelAuthenticationChallenge, nil)
        }
    }

    static func thumbnail(source: BrowserGameCoverSource) async -> BrowserGameCoverResult {
        guard let request = BrowserGameCoverPolicy.request(for: source.url, allowedURLs: source.allowedURLs), !Task.isCancelled else {
            return .failure(retryable: false)
        }
        let receiver = BrowserGameCoverDownload(source: source)
        let session = URLSession(configuration: BrowserGameCoverPolicy.sessionConfiguration(), delegate: receiver, delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        return await withTaskCancellationHandler {
            do {
                let data = try await receiver.read(session: session, request: request)
                guard !Task.isCancelled else { return .failure(retryable: false) }
                // Only a validated response can finish successfully. MIME is
                // checked once more against the actual encoded source type.
                guard let mimeType = receiver.mimeType else { return .failure(retryable: false) }
                let thumbnail = await Task.detached(priority: .utility) {
                    BrowserGameCoverPolicy.thumbnail(data, mimeType: mimeType)
                }.value
                guard !Task.isCancelled, let thumbnail else { return .failure(retryable: false) }
                return .success(thumbnail)
            } catch {
                return .failure(retryable: !Task.isCancelled && BrowserGameCoverPolicy.shouldRetry(error),
                                securityFailure: BrowserGameCoverPolicy.isSecurityFailure(error))
            }
        } onCancel: {
            receiver.cancel()
            session.invalidateAndCancel()
        }
    }

    private var responseMIME: String?
    private var mimeType: String? {
        lock.lock(); defer { lock.unlock() }
        return responseMIME
    }
}

enum BrowserGameCoverResult: Sendable {
    case success(Data), failure(retryable: Bool, securityFailure: Bool = false)
}

actor BrowserGameCoverStore {
    static let shared = BrowserGameCoverStore()
    private struct Flight {
        let id: UUID
        let task: Task<Void, Never>
        var waiters: [UUID: CheckedContinuation<Data?, Never>]
    }
    private let catalog: @Sendable () -> [BrowserGameCatalogEntry]
    private let transport: @Sendable (BrowserGameCoverSource) async -> BrowserGameCoverResult
    private var cache: [String: Data] = [:]
    private var cacheOrder: [String] = []
    private var cacheBytes = 0
    private let maximumCacheBytes = 24 * 1024 * 1024
    private var active = 0
    private var flights: [String: Flight] = [:]
    var waitingCount: Int { flights.values.reduce(0) { $0 + $1.waiters.count } }

    init(catalog: @escaping @Sendable () -> [BrowserGameCatalogEntry] = { BrowserGameCatalog.loaded.entries },
         transport: @escaping @Sendable (BrowserGameCoverSource) async -> BrowserGameCoverResult = { await BrowserGameCoverDownload.thumbnail(source: $0) }) {
        self.catalog = catalog
        self.transport = transport
    }

    private func remember(_ data: Data, key: String) {
        guard data.count <= maximumCacheBytes else { return }
        if let previous = cache.removeValue(forKey: key) { cacheBytes -= previous.count }
        cacheOrder.removeAll { $0 == key }
        while cacheBytes + data.count > maximumCacheBytes || cacheOrder.count >= 40 {
            let oldest = cacheOrder.removeFirst()
            if let removed = cache.removeValue(forKey: oldest) { cacheBytes -= removed.count }
        }
        cache[key] = data
        cacheOrder.append(key)
        cacheBytes += data.count
    }

    func load(_ entry: BrowserGameCatalogEntry) async -> Data? {
        guard let source = BrowserGameCoverSource(entry: entry, catalog: catalog()) else { return nil }
        let key = source.cacheKey
        guard !Task.isCancelled else { return nil }
        if let data = cache[key] {
            cacheOrder.removeAll { $0 == key }
            cacheOrder.append(key)
            return data
        }
        let waiter = UUID()
        return await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                guard !Task.isCancelled else { continuation.resume(returning: nil); return }
                if var flight = flights[key] {
                    flight.waiters[waiter] = continuation
                    flights[key] = flight
                } else {
                    let id = UUID()
                    let task = Task {
                        let data = await fetch(source)
                        complete(key: key, flight: id, data: data)
                    }
                    flights[key] = Flight(id: id, task: task, waiters: [waiter: continuation])
                }
            }
        } onCancel: {
            Task { await self.cancel(key: key, waiter: waiter) }
        }
    }

    private func cancel(key: String, waiter: UUID) {
        guard var flight = flights[key], let continuation = flight.waiters.removeValue(forKey: waiter) else { return }
        if flight.waiters.isEmpty {
            flights.removeValue(forKey: key)
            flight.task.cancel()
        } else { flights[key] = flight }
        continuation.resume(returning: nil)
    }

    private func complete(key: String, flight id: UUID, data: Data?) {
        guard let flight = flights[key], flight.id == id else { return }
        flights.removeValue(forKey: key)
        if let data { remember(data, key: key) }
        for continuation in flight.waiters.values { continuation.resume(returning: data) }
    }

    private func fetch(_ source: BrowserGameCoverSource) async -> Data? {
        var blockedHosts: Set<String> = []
        for asset in source.chain {
            guard let host = asset.url.host?.lowercased(), !blockedHosts.contains(host) else { continue }
            attempts: for attempt in 0..<BrowserGameCoverPolicy.maximumAttempts {
                guard !Task.isCancelled else { return nil }
                switch await download(asset) {
                case let .success(data): return data
                case let .failure(retryable, securityFailure):
                    guard !Task.isCancelled else { return nil }
                    if securityFailure {
                        // Do not revisit a host involved in a failed TLS or
                        // authentication request, even through another image.
                        blockedHosts.formUnion(asset.allowedURLs.compactMap { URL(string: $0)?.host?.lowercased() })
                        break attempts
                    }
                    if retryable, attempt + 1 < BrowserGameCoverPolicy.maximumAttempts {
                        do { try await Task.sleep(nanoseconds: BrowserGameCoverPolicy.retryDelay(afterAttempt: attempt)) }
                        catch { return nil }
                    } else {
                        // A different explicitly reviewed public asset can
                        // succeed when this site's response is unavailable.
                        break attempts
                    }
                }
            }
        }
        return nil
    }

    private func download(_ source: BrowserGameCoverSource) async -> BrowserGameCoverResult {
        while active >= 3 {
            do { try await Task.sleep(for: .milliseconds(40)) } catch { return .failure(retryable: false) }
        }
        guard !Task.isCancelled else { return .failure(retryable: false) }
        active += 1
        defer { active -= 1 }
        return await transport(source)
    }
}

/// Landscape pictures for Discover, separate from the compact Collection shortcut.
struct BrowserGameCover: View {
    let entry: BrowserGameCatalogEntry
    var compact = false
    var cornerRadius: CGFloat? = nil
    @State private var image: NSImage?
    @State private var loading = true

    var body: some View {
        Rectangle()
            .fill(Color(nsColor: .controlBackgroundColor))
            .aspectRatio(16.0 / 9.0, contentMode: .fit)
            .overlay {
                GeometryReader { geometry in
                    if let image {
                        Image(nsImage: image).resizable().scaledToFit()
                            .frame(width: geometry.size.width, height: geometry.size.height)
                    } else {
                        VStack(spacing: compact ? 3 : 8) {
                            if loading { ProgressView().controlSize(.small) }
                            Text(loading ? "Loading preview…" : compact ? "Preview unavailable" : "Picture temporarily unavailable")
                                .font(.system(size: compact ? 9 : 12)).foregroundStyle(.secondary)
                                .multilineTextAlignment(.center).padding(.horizontal, compact ? 4 : 12)
                        }
                        .frame(width: geometry.size.width, height: geometry.size.height)
                    }
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: cornerRadius ?? (compact ? 8 : 12), style: .continuous))
            .accessibilityHidden(true)
            .task(id: ([entry.id, entry.artworkURL?.absoluteString ?? "", "redirects"]
                       + entry.artworkRedirectURLs.map(\.absoluteString) + ["fallbacks"]
                       + entry.artworkFallbackURLs.map(\.absoluteString)).joined(separator: "\n")) {
                image = nil
                loading = true
                defer { if !Task.isCancelled { loading = false } }
                guard let bytes = await BrowserGameCoverStore.shared.load(entry), !Task.isCancelled else { return }
                image = NSImage(data: bytes)
            }
    }
}

private struct BrowserGameCollectionArtwork: View {
    let game: BrowserGame

    var body: some View {
        if let entry = BrowserGameCatalog.entry(forSavedURL: game.url) {
            BrowserGameCover(entry: entry, compact: true).frame(width: 96, height: 54)
        } else {
            BrowserGameArtwork(seed: game.url)
        }
    }
}

/// Mirrors the core's conservative bookmark policy. The core remains the
/// authority when saving. This check also prevents opening altered on-disk
/// data through a file URL, custom application scheme or credentialed URL.
enum BrowserGameLink {
    static func validatedURL(_ input: String) -> URL? {
        let value = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard value.hasPrefix("https://"), !value.isEmpty, value.utf8.count <= 2048,
              value.utf8.allSatisfy({ $0 > 0x20 && $0 < 0x7f && ![UInt8(0x5c), 0x22, 0x3c, 0x3e, 0x60, 0x5e, 0x7b, 0x7d, 0x7c].contains($0) }) else { return nil }
        let rest = value.dropFirst(8)
        let authority = String(rest.prefix { !"/?#".contains($0) }).lowercased()
        guard !authority.isEmpty, authority.utf8.count <= 253,
              !authority.contains(where: { "@:%[]".contains($0) }) else { return nil }
        let labels = authority.split(separator: ".", omittingEmptySubsequences: false)
        guard labels.count >= 2, labels.allSatisfy({ label in
            !label.isEmpty && label.utf8.count <= 63 && !label.hasPrefix("-") && !label.hasSuffix("-")
                && label.utf8.allSatisfy { ($0 >= 97 && $0 <= 122) || ($0 >= 48 && $0 <= 57) || $0 == 45 }
        }), let last = labels.last, last.count >= 2,
              last.utf8.allSatisfy({ $0 >= 97 && $0 <= 122 }),
              !["localhost", "local", "internal", "lan", "home", "invalid", "test", "example", "onion"].contains(String(last)) else { return nil }
        let bytes = Array(value.utf8)
        var index = 0
        while index < bytes.count {
            if bytes[index] == 37 {
                guard index + 2 < bytes.count,
                      let decoded = UInt8(String(bytes: bytes[(index + 1)...(index + 2)], encoding: .utf8) ?? "", radix: 16),
                      decoded >= 0x20, decoded != 0x7f, decoded != 0x5c else { return nil }
                index += 3
            } else { index += 1 }
        }
        let suffix = rest.dropFirst(authority.count)
        for parameters in suffix.split(omittingEmptySubsequences: false, whereSeparator: { $0 == "?" || $0 == "#" }).dropFirst() {
            for parameter in parameters.split(whereSeparator: { $0 == "&" || $0 == ";" }) {
                let key = String(parameter.prefix { $0 != "=" }).removingPercentEncoding ?? ""
                let plain = key.prefix { $0 != "[" }
                let normalized = plain.lowercased().filter { $0.isASCII && ($0.isLetter || $0.isNumber) }
                if ["token", "accesstoken", "idtoken", "refreshtoken", "auth", "authtoken", "bearer", "bearertoken", "authorization", "authorizationcode", "code", "clientsecret", "jwt", "oauthtoken", "oauthtokensecret", "oauthverifier", "session", "sessionid", "sid", "apikey", "apitoken", "key", "password", "passwd", "pwd", "email"].contains(normalized) { return nil }
            }
        }
        guard let url = URL(string: value), url.scheme == "https", url.user == nil, url.password == nil,
              url.port == nil, url.host?.lowercased() == authority else { return nil }
        return url
    }
}

struct AddBrowserGameSheet: View {
    let onSave: (String, String) async throws -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var name: String
    @State private var link: String
    @State private var saving = false
    @State private var error: String?
    @State private var showWebsite: Bool
    private let beganWithCatalogEntry: Bool

    init(initialName: String = "", initialURL: String = "", onSave: @escaping (String, String) async throws -> Void) {
        self.onSave = onSave
        _name = State(initialValue: initialName)
        _link = State(initialValue: initialURL)
        let isCatalogEntry = BrowserGameCatalog.entry(forSavedURL: initialURL) != nil
        beganWithCatalogEntry = isCatalogEntry
        _showWebsite = State(initialValue: !isCatalogEntry)
    }

    private var catalogEntry: BrowserGameCatalogEntry? {
        BrowserGameCatalog.entry(forSavedURL: link.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    private var validName: Bool {
        let value = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return !value.isEmpty && value.unicodeScalars.count <= 80
            && !value.unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            if let entry = catalogEntry {
                BrowserGameCover(entry: entry, cornerRadius: 10)
            }
            VStack(alignment: .leading, spacing: 4) {
                Text(catalogEntry == nil ? "Add a browser game" : "Add to Collection")
                    .font(.system(size: 18, weight: .semibold))
                Text("A saved shortcut, ready to play in your browser.")
                    .font(.system(size: 12)).foregroundStyle(.secondary)
            }

            VStack(alignment: .leading, spacing: 14) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Name").font(.system(size: 12, weight: .medium))
                    TextField("Game name", text: $name).textFieldStyle(.roundedBorder)
                        .accessibilityLabel("Game name").disabled(saving)
                }
                if beganWithCatalogEntry {
                    DisclosureGroup(isExpanded: $showWebsite) {
                        websiteField.padding(.top, 6)
                    } label: {
                        HStack(spacing: 8) {
                            Text("Website").font(.system(size: 12, weight: .medium))
                            Spacer(minLength: 8)
                            Text(BrowserGameLink.validatedURL(link)?.host ?? "Edit link")
                                .font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    .disabled(saving)
                } else {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Website").font(.system(size: 12, weight: .medium))
                        websiteField
                    }
                }
                if !link.isEmpty && BrowserGameLink.validatedURL(link) == nil {
                    Text("Enter a public HTTPS link without sign-in details.")
                        .font(.system(size: 11)).foregroundStyle(.orange)
                }
                Text(catalogEntry?.requiresOwnFiles == true
                     ? "This game needs files you own. Your browser handles the game and sign-in."
                     : "Use a site you trust. Your browser handles the game and sign-in.")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if let error {
                    Text(error).font(.system(size: 11)).foregroundStyle(.red)
                        .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                }
            }

            HStack(spacing: 10) {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).disabled(saving)
                Button(action: save) {
                    HStack(spacing: 6) {
                        if saving { ProgressView().controlSize(.small) }
                        Text(saving ? "Adding…" : "Add to Collection")
                    }
                    .frame(minWidth: 112)
                }
                .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                .disabled(saving || !validName || BrowserGameLink.validatedURL(link) == nil)
            }
        }
        .padding(22).frame(width: 430)
        .interactiveDismissDisabled(saving)
    }

    private var websiteField: some View {
        TextField("https://game-website.com", text: $link).textFieldStyle(.roundedBorder)
            .accessibilityLabel("HTTPS game link").disabled(saving)
    }

    private func save() {
        guard !saving, validName, BrowserGameLink.validatedURL(link) != nil else { return }
        saving = true
        error = nil
        Task { @MainActor in
            defer { saving = false }
            do {
                try await onSave(name.trimmingCharacters(in: .whitespacesAndNewlines), link.trimmingCharacters(in: .whitespacesAndNewlines))
                dismiss()
            } catch { self.error = error.localizedDescription }
        }
    }
}

struct BrowserGameArtwork: View {
    let seed: String
    var size: CGFloat = 56

    private var tint: Color {
        let value = seed.utf8.reduce(UInt64(5381)) { ($0 &* 33) &+ UInt64($1) }
        return Color(hue: Double(value % 360) / 360, saturation: 0.48, brightness: 0.73)
    }

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size * 0.23, style: .continuous)
                .fill(LinearGradient(colors: [tint.opacity(0.72), tint], startPoint: .topLeading, endPoint: .bottomTrailing))
            Image(systemName: "safari.fill").font(.system(size: size * 0.57, weight: .light))
                .foregroundStyle(.white.opacity(0.95))
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

struct BrowserGameTile: View {
    let game: BrowserGame
    var isSelected = false
    var opensOnSingleClick = false
    let onSelect: () -> Void
    let onOpen: () -> Void
    var onRemove: (() -> Void)? = nil
    @State private var hovered = false
    @State private var confirmRemove = false

    var body: some View {
        Button {
            onSelect()
            let clicks = NSApp.currentEvent?.clickCount ?? 0
            if opensOnSingleClick ? clicks < 2 : clicks >= 2 { onOpen() }
        } label: {
            VStack(spacing: 8) {
                BrowserGameCollectionArtwork(game: game)
                Text(game.name).font(.system(size: 13, weight: .semibold)).lineLimit(2)
                    .multilineTextAlignment(.center)
                    .foregroundStyle(isSelected || hovered ? Color.accentColor : Color.primary)
                Text("Browser").font(.system(size: 10)).foregroundStyle(.secondary)
            }
            .padding(12).frame(width: 120).contentShape(Rectangle())
        }
        .buttonStyle(.plain).frame(maxWidth: .infinity)
        .onHover { hovered = $0 }
        .help(opensOnSingleClick ? "Play \(game.name) in your browser." : "Click to select \(game.name). Double-click to play in your browser.")
        .accessibilityLabel(game.name)
        .accessibilityHint(opensOnSingleClick ? "Opens in your browser." : "Browser game. Double-click to open.")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .contextMenu {
            Button("Play in browser", action: onOpen)
            if onRemove != nil { Button("Remove from Collection", role: .destructive) { confirmRemove = true } }
        }
        .confirmationDialog("Remove \(game.name) from Collection?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button("Remove shortcut", role: .destructive) { onRemove?() }
            Button("Cancel", role: .cancel) {}
        } message: { Text("This removes the saved link. Your browser's game data stays in your browser.") }
    }
}

enum LibraryScope: String, CaseIterable, Identifiable {
    case all, steam, epic, gog, apps, browser
    var id: String { rawValue }
    var title: String {
        switch self { case .all: "All"; case .steam: "Steam"; case .epic: "Epic"; case .gog: "GOG"; case .apps: "Apps"; case .browser: "Browser" }
    }
    var symbol: String {
        switch self { case .all: "square.grid.2x2"; case .steam: "gamecontroller"; case .epic: "gamecontroller"; case .gog: "gamecontroller"; case .apps: "app"; case .browser: "safari" }
    }
}

struct LibraryScopeBar: View {
    @Binding var selection: LibraryScope
    var counts: [LibraryScope: Int] = [:]
    @Namespace private var highlight
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: 3) {
            ForEach(LibraryScope.allCases) { scope in
                Button {
                    withAnimation(reduceMotion ? nil : .easeOut(duration: 0.16)) { selection = scope }
                } label: {
                    HStack(spacing: 5) {
                Text(scope.title)
                        if let count = counts[scope] { Text("\(count)").font(.system(size: 10)).monospacedDigit().opacity(0.65) }
                    }
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(selection == scope ? Color.primary : Color.secondary)
                    .padding(.horizontal, 10).frame(height: 28)
                    .background {
                        if selection == scope {
                            Capsule().fill(.primary.opacity(0.09)).matchedGeometryEffect(id: "scope-selection", in: highlight)
                        }
                    }
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(scope.title)\(counts[scope].map { ", \($0) items" } ?? "")")
                .accessibilityAddTraits(selection == scope ? .isSelected : [])
            }
        }
        .padding(4).background(.quaternary.opacity(0.40), in: Capsule())
        .fixedSize()
    }
}

struct GameLibraryEntry: Identifiable {
    let id: String
    let name: String
    let source: LibraryScope
    let subtitle: String
    var compatibilityText = "Compatibility unknown"
    var compatibilityDetail = "This game has not been tested with Caskrun."
    var compatibility: CompatibilityGame? = nil
    var icon: NSImage? = nil
    var artworkRevision = ""
    var browserURL: String? = nil
    var canPlay = true
    var canManageBottle = false
    var availableRenderers: Set<String>? = nil
    var graphicsDescription: String? = nil

    var actionTitle: String {
        switch source {
        case .browser: "Play in browser"
        case .apps: "Open app"
        case .epic, .gog: "Open store"
        case .all, .steam: "Play"
        }
    }

    var cardHelp: String {
        let details = "Click for \(name) details."
        guard canPlay else { return details }
        let action: String
        switch source {
        case .browser: action = "play in your browser"
        case .apps: action = "open the app"
        case .epic, .gog: action = "open its store"
        case .all, .steam: action = "play"
        }
        return "\(details) Double-click to \(action)."
    }
}

// Keep the first tile's identity across the grid reflow caused by its inspector.
// The sequence is independent of the view under the second mouse event.
struct LibraryClickSequence {
    enum Outcome: Equatable { case pass, consume, open(String) }
    private struct Target {
        let id: String
        let windowNumber: Int
        let point: NSPoint
        let timestamp: TimeInterval
    }
    private var target: Target?
    private var consumedWindowNumber: Int?
    var hasTarget: Bool { target != nil }

    mutating func remember(id: String, windowNumber: Int, point: NSPoint, timestamp: TimeInterval) {
        target = Target(id: id, windowNumber: windowNumber, point: point, timestamp: timestamp)
    }

    mutating func cancel() { target = nil }

    mutating func route(type: NSEvent.EventType, windowNumber: Int, point: NSPoint,
                        timestamp: TimeInterval, clickCount: Int, doubleClickInterval: TimeInterval,
                        isBlocked: Bool = false) -> Outcome {
        if isBlocked {
            cancel()
            consumedWindowNumber = nil
            return .pass
        }
        if type == .leftMouseUp, consumedWindowNumber == windowNumber {
            consumedWindowNumber = nil
            return .consume
        }
        guard type == .leftMouseDown else {
            if type != .leftMouseUp { cancel() }
            return .pass
        }
        consumedWindowNumber = nil
        defer { cancel() }
        guard clickCount == 2, let target, target.windowNumber == windowNumber else { return .pass }
        let elapsed = timestamp - target.timestamp
        let distance = hypot(point.x - target.point.x, point.y - target.point.y)
        guard elapsed >= 0, elapsed <= doubleClickInterval, distance <= 8 else { return .pass }
        consumedWindowNumber = windowNumber
        return .open(target.id)
    }
}

@MainActor
final class LibraryClickRouter: ObservableObject {
    private var monitor: Any?
    private weak var firstWindow: NSWindow?
    private var sequence = LibraryClickSequence()
    private var launch: (@MainActor @Sendable (String) -> Void)?

    init() {
        monitor = NSEvent.addLocalMonitorForEvents(matching: [
            .leftMouseDown, .leftMouseUp, .rightMouseDown, .scrollWheel, .keyDown
        ]) { [weak self] event in
            guard let self else { return event }
            return self.route(event)
        }
    }

    deinit {
        if let monitor { NSEvent.removeMonitor(monitor) }
    }

    func remember(_ event: NSEvent?, id: String, launch: @escaping @MainActor @Sendable (String) -> Void) {
        cancel()
        guard let event, event.type == .leftMouseUp, event.clickCount == 1,
              let window = event.window, window.attachedSheet == nil, window.sheetParent == nil,
              NSApp.modalWindow == nil else { return }
        sequence.remember(id: id, windowNumber: window.windowNumber,
                          point: event.locationInWindow, timestamp: event.timestamp)
        firstWindow = window
        self.launch = launch
    }

    func cancel() {
        sequence.cancel()
        firstWindow = nil
        launch = nil
    }

    func route(_ event: NSEvent) -> NSEvent? {
        // clickCount and locationInWindow are mouse-event properties. Reading
        // clickCount on a scroll or key event raises an AppKit exception before
        // the sequence can pass that event through to its original control.
        guard event.type == .leftMouseDown || event.type == .leftMouseUp else {
            cancel()
            return event
        }
        if event.type == .leftMouseDown, event.window !== firstWindow { cancel() }
        let outcome = sequence.route(type: event.type, windowNumber: event.windowNumber,
                                     point: event.locationInWindow, timestamp: event.timestamp,
                                     clickCount: event.clickCount, doubleClickInterval: NSEvent.doubleClickInterval,
                                     isBlocked: NSApp.modalWindow != nil || event.window?.attachedSheet != nil || event.window?.sheetParent != nil)
        switch outcome {
        case .open(let id):
            let action = launch
            cancel()
            if let action { DispatchQueue.main.async { action(id) } }
            return nil
        case .consume: return nil
        case .pass:
            if !sequence.hasTarget { cancel() }
            return event
        }
    }
}

struct GameLibraryView: View {
    let entries: [GameLibraryEntry]
    let device: CompatibilityDevice
    var isLoading = false
    var warnings: [String] = []
    var query = ""
    let onRefresh: () -> Void
    let onPlay: (GameLibraryEntry) -> Void
    let onManage: (GameLibraryEntry) -> Void
    let onRemoveBrowser: (GameLibraryEntry) -> Void
    let onAddBrowser: () -> Void
    var onLoadArtwork: (GameLibraryEntry) -> Void = { _ in }
    var onInspect: (GameLibraryEntry) -> Void = { _ in }
    @State private var scope: LibraryScope = .all
    @State private var selectedID: String?
    @State private var confirmRemove = false
    @StateObject private var clickRouter = LibraryClickRouter()

    private var filtered: [GameLibraryEntry] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        return entries.filter { item in
            (scope == .all || item.source == scope) && (trimmed.isEmpty || item.name.localizedCaseInsensitiveContains(trimmed)
                || item.subtitle.localizedCaseInsensitiveContains(trimmed))
        }
    }
    private var selected: GameLibraryEntry? { filtered.first { $0.id == selectedID } }
    private var counts: [LibraryScope: Int] {
        var result: [LibraryScope: Int] = [.all: entries.count]
        for item in entries { result[item.source, default: 0] += 1 }
        for item in LibraryScope.allCases where result[item] == nil { result[item] = 0 }
        return result
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                LibraryScopeBar(selection: $scope, counts: counts)
                Spacer(minLength: 12)
                if isLoading { ProgressView().controlSize(.small) }
                Button(action: onRefresh) { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.borderless).help("Refresh installed games").accessibilityLabel("Refresh library")
                    .disabled(isLoading)
                Button(action: onAddBrowser) { Label("Add link", systemImage: "plus") }
                    .font(.system(size: 11, weight: .medium)).buttonStyle(.borderless)
            }
            .padding(.horizontal, 24).padding(.top, 12).padding(.bottom, 14)
            HStack(alignment: .top, spacing: 0) {
                ScrollView {
                    VStack(alignment: .leading, spacing: 18) {
                        if entries.isEmpty && isLoading {
                            ProgressView("Reading your installed games…").frame(maxWidth: .infinity, minHeight: 160)
                        } else if filtered.isEmpty {
                            emptyState
                        } else {
                            LazyVGrid(columns: [GridItem(.adaptive(minimum: 132, maximum: 158), spacing: 14)], alignment: .leading, spacing: 14) {
                                ForEach(filtered) { item in
                                    GameLibraryCard(item: item, isSelected: selectedID == item.id) {
                                        select(item)
                                    } open: { onPlay(item) }
                                        .task(id: item.id + ":" + item.artworkRevision) { onLoadArtwork(item) }
                                }
                            }
                        }
                        if !warnings.isEmpty {
                            DisclosureGroup("Some library information could not be read") {
                                ForEach(Array(warnings.enumerated()), id: \.offset) { _, warning in
                                    Text(warning).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                                        .frame(maxWidth: .infinity, alignment: .leading).padding(.top, 4)
                                }
                            }.font(.caption).foregroundStyle(.orange)
                        }
                        Text("Installed games stay with their store and bottle. Community reports are guidance; Caskrun gameplay remains unverified.")
                            .font(.system(size: 11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.horizontal, 24).padding(.bottom, 24)
                }
                .frame(maxWidth: .infinity)
                if let selected {
                    detail(selected).frame(width: 296).padding(.trailing, 16).padding(.bottom, 16)
                }
            }
        }
        .onChange(of: scope) { _, _ in clickRouter.cancel(); selectedID = nil }
        .onChange(of: query) { _, _ in clickRouter.cancel() }
        .onChange(of: selectedID) { _, _ in
            if let selected { onInspect(selected) } else { clickRouter.cancel() }
        }
        .onChange(of: entries.map(\.id)) { _, ids in
            clickRouter.cancel()
            if let selectedID, !ids.contains(selectedID) { self.selectedID = nil }
        }
        .onChange(of: entries.filter(\.canPlay).map(\.id)) { _, _ in clickRouter.cancel() }
        .onDisappear { clickRouter.cancel() }
        .confirmationDialog("Remove this browser game?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button("Remove shortcut", role: .destructive) { if let selected { onRemoveBrowser(selected) } }
            Button("Cancel", role: .cancel) {}
        } message: { Text("Only the saved Caskrun link is removed. Your browser's game data stays in your browser.") }
    }

    private func select(_ item: GameLibraryEntry) {
        clickRouter.remember(NSApp.currentEvent, id: item.id) { id in
            guard selectedID == id, let current = filtered.first(where: { $0.id == id }), current.canPlay else { return }
            onPlay(current)
        }
        selectedID = item.id
    }

    @ViewBuilder private var emptyState: some View {
        if !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            ContentUnavailableView.search(text: query).frame(maxWidth: .infinity, minHeight: 190)
        } else {
            VStack(alignment: .leading, spacing: 12) {
                Image(systemName: scope == .browser ? "safari" : "gamecontroller").font(.system(size: 25)).foregroundStyle(.secondary)
                Text(scope == .browser ? "Your browser games" : "Your library, together").font(.title3.weight(.semibold))
                Text(scope == .browser ? "Save a game link here and play in your browser." : "Installed Steam, Epic and GOG games appear here alongside saved apps. Sign in and launch publisher games through their store.")
                    .font(.subheadline).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                Button("Add a browser game", action: onAddBrowser).buttonStyle(.bordered)
            }
            .padding(20).frame(maxWidth: .infinity, minHeight: 190, alignment: .leading)
            .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 18))
        }
    }

    private func detail(_ item: GameLibraryEntry) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text(item.source == .browser ? "BROWSER GAME" : item.source == .apps ? "SAVED APP" : "IN YOUR LIBRARY")
                        .font(.system(size: 10, weight: .semibold, design: .monospaced)).foregroundStyle(.secondary)
                    Spacer()
                    Button { selectedID = nil } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .semibold)).frame(width: 24, height: 24) }
                        .buttonStyle(.plain).help("Close game details").accessibilityLabel("Close game details")
                }
                GameLibraryArtwork(item: item, size: 64)
                VStack(alignment: .leading, spacing: 5) {
                    Text(item.name).font(.system(size: 21, weight: .semibold)).fixedSize(horizontal: false, vertical: true)
                    Text(item.subtitle).font(.caption).foregroundStyle(.secondary)
                }
                Button { onPlay(item) } label: {
                    Label(item.actionTitle, systemImage: item.source == .browser ? "arrow.up.right" : "play.fill")
                        .font(.system(size: 13, weight: .semibold)).frame(maxWidth: .infinity).padding(.vertical, 4)
                }
                .buttonStyle(.borderedProminent).disabled(!item.canPlay)
                if item.canManageBottle {
                    Button { onManage(item) } label: { Label("Bottle controls", systemImage: "slider.horizontal.3") }
                        .font(.caption).buttonStyle(.borderless)
                }
                if let graphics = item.graphicsDescription {
                    Label(graphics, systemImage: "display").font(.caption).foregroundStyle(.secondary)
                }
                Divider()
                if item.source == .browser {
                    Label("Opens in your default browser", systemImage: "safari").font(.caption.weight(.medium))
                    if let link = item.browserURL, let url = BrowserGameLink.validatedURL(link) {
                        Text(url.host ?? "").font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                    Text("Browser requirements and game data are handled by the website. Windows graphics settings do not apply.")
                        .font(.caption).foregroundStyle(.secondary)
                    Button("Remove from Collection", role: .destructive) { confirmRemove = true }
                        .font(.caption).buttonStyle(.borderless)
                } else if item.source == .apps {
                    Label("Saved Windows shortcut", systemImage: "app").font(.caption.weight(.medium))
                    Text(item.compatibilityDetail).font(.caption).foregroundStyle(.secondary)
                    Text("Opens using this bottle's saved program, arguments and working folder.")
                        .font(.caption).foregroundStyle(.secondary)
                } else if item.source == .epic || item.source == .gog {
                    Label("Installed game", systemImage: "internaldrive").font(.caption.weight(.medium))
                    Text(item.compatibilityDetail).font(.caption).foregroundStyle(.secondary)
                    Text(item.canPlay ? "Open its store to play. Your sign-in and game ownership stay with the publisher." : "Save this store's Windows client in the bottle to open it here.")
                        .font(.caption).foregroundStyle(.secondary)
                } else {
                    CompatibilityEvidenceView(game: item.compatibility, device: device, availableRenderers: item.availableRenderers)
                    if item.compatibility == nil {
                        Text(item.compatibilityDetail).font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            .padding(18)
        }
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay { RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(.primary.opacity(0.10)) }
    }
}

/// Original local artwork while a publisher's installed icon is unavailable.
/// A stable seed gives the same title the same colour without any web request.
struct GameArtwork: View {
    let seed: String
    let name: String
    var size: CGFloat = 56
    var symbol: String? = nil

    private var tint: Color {
        let value = seed.utf8.reduce(UInt64(5381)) { ($0 &* 33) &+ UInt64($1) }
        return Color(hue: Double(value % 360) / 360, saturation: 0.42, brightness: 0.64)
    }
    private var initials: String {
        let words = name.split { !$0.isLetter && !$0.isNumber }
        return String(words.prefix(2).compactMap(\.first)).uppercased()
    }
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size * 0.23, style: .continuous)
                .fill(LinearGradient(colors: [tint.opacity(0.65), tint], startPoint: .topLeading, endPoint: .bottomTrailing))
            if let symbol {
                Image(systemName: symbol).font(.system(size: size * 0.5, weight: .light)).foregroundStyle(.white.opacity(0.94))
            } else if !initials.isEmpty {
                Text(initials).font(.system(size: size * 0.34, weight: .semibold, design: .rounded))
                    .foregroundStyle(.white.opacity(0.94)).lineLimit(1)
            } else {
                Image(systemName: "gamecontroller.fill").font(.system(size: size * 0.5, weight: .light)).foregroundStyle(.white.opacity(0.94))
            }
        }
        .frame(width: size, height: size).accessibilityHidden(true)
    }
}

private struct GameLibraryArtwork: View {
    let item: GameLibraryEntry
    let size: CGFloat
    var body: some View {
        Group {
            if let icon = item.icon { Image(nsImage: icon).resizable().interpolation(.high).scaledToFit() }
            else if item.source == .browser { BrowserGameArtwork(seed: item.browserURL ?? item.id, size: size) }
            else {
                GameArtwork(seed: item.name.lowercased(), name: item.name, size: size,
                            symbol: item.source == .apps ? "app.fill" : nil)
            }
        }
        .frame(width: size, height: size).accessibilityHidden(true)
    }
}

private struct GameLibraryCard: View {
    let item: GameLibraryEntry
    let isSelected: Bool
    let select: () -> Void
    let open: () -> Void
    @State private var hovered = false
    var body: some View {
        Button(action: select) {
            VStack(spacing: 8) {
                GameLibraryArtwork(item: item, size: 54).frame(height: 64)
                Text(item.name).font(.system(size: 12, weight: .semibold)).lineLimit(2)
                    .multilineTextAlignment(.center).frame(height: 32, alignment: .top)
                Text(item.source.title).font(.system(size: 10)).foregroundStyle(.secondary)
                if item.source != .browser {
                    Text(item.compatibilityText).font(.system(size: 9)).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            .padding(12).frame(maxWidth: .infinity)
            .foregroundStyle(isSelected ? Color.accentColor : Color.primary)
            .background(isSelected ? Color.accentColor.opacity(0.09) : Color.primary.opacity(hovered ? 0.035 : 0), in: RoundedRectangle(cornerRadius: 15))
            .overlay { RoundedRectangle(cornerRadius: 15).strokeBorder(isSelected ? Color.accentColor.opacity(0.22) : .clear) }
            .contentShape(RoundedRectangle(cornerRadius: 15))
        }
        .buttonStyle(.plain).onHover { hovered = $0 }
        .help(item.cardHelp)
        .accessibilityLabel("\(item.name), \(item.source.title), \(item.compatibilityText)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .accessibilityAction(named: Text(item.actionTitle)) { if item.canPlay { open() } }
    }
}
