import Foundation
import ImageIO
import Darwin

/// Steam artwork uses a public app ID only. No account, bottle, filesystem path,
/// cookie or library manifest is sent to the publisher's image servers.
struct SteamGameArtworkSource: Sendable {
    let appID: UInt32
    let urls: [URL]
    var allowedURLs: Set<String> { Set(urls.map(\.absoluteString)) }
    var cacheName: String { "steam-\(appID)-cover-v1.png" }

    init?(appID: UInt32) {
        guard appID > 0 else { return nil }
        self.appID = appID
        urls = [
            "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/\(appID)/header.jpg",
            "https://cdn.akamai.steamstatic.com/steam/apps/\(appID)/header.jpg"
        ].compactMap(URL.init(string:))
    }
}

enum SteamGameArtworkPolicy {
    static let maximumBytes = 2 * 1024 * 1024
    static let maximumLocalBytes = 8 * 1024 * 1024
    static let maximumCacheBytes = 4 * 1024 * 1024
    static let maximumRedirects = 3
    static let maximumCacheEntries = 256
    static let acceptedTypes = ["image/png": "public.png", "image/jpeg": "public.jpeg",
                                "image/webp": "org.webmproject.webp", "image/avif": "public.avif"]

    static func request(for url: URL, source: SteamGameArtworkSource) -> URLRequest? {
        // Exact generated URLs prevent redirects to other games, local services,
        // credential-bearing addresses, tracking queries or arbitrary hosts.
        guard source.allowedURLs.contains(url.absoluteString), url.scheme == "https",
              url.user == nil, url.password == nil, url.port == nil,
              url.query == nil, url.fragment == nil else { return nil }
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalAndRemoteCacheData,
                                 timeoutInterval: 10)
        request.httpMethod = "GET"
        request.httpShouldHandleCookies = false
        request.setValue("image/jpeg,image/png,image/webp,image/avif", forHTTPHeaderField: "Accept")
        request.setValue("Caskrun/0.1", forHTTPHeaderField: "User-Agent")
        return request
    }

    static func sessionConfiguration() -> URLSessionConfiguration {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpShouldSetCookies = false
        configuration.httpCookieStorage = nil
        configuration.urlCredentialStorage = nil
        configuration.urlCache = nil
        configuration.timeoutIntervalForRequest = 10
        configuration.timeoutIntervalForResource = 15
        configuration.httpMaximumConnectionsPerHost = 2
        return configuration
    }

    static func accepts(_ response: HTTPURLResponse, source: SteamGameArtworkSource) -> Bool {
        response.statusCode == 200 && response.url.flatMap { request(for: $0, source: source) } != nil
            && (response.expectedContentLength < 0 || response.expectedContentLength <= maximumBytes)
            && response.mimeType.map { acceptedTypes[$0.lowercased()] != nil } == true
    }

    static func mayTryFallback(after error: Error) -> Bool {
        if let failure = error as? SteamGameArtworkError {
            switch failure {
            case let .httpStatus(status): return status == 404 || status == 410 || status == 408 || status == 429 || (500...599).contains(status)
            case .invalidImage, .overflow: return true
            case .rejectedResponse: return false
            }
        }
        guard let network = error as? URLError else { return false }
        return [.timedOut, .cannotFindHost, .cannotConnectToHost, .networkConnectionLost,
                .dnsLookupFailed, .notConnectedToInternet, .resourceUnavailable].contains(network.code)
    }

    /// Preserve the complete image and its aspect ratio. Game covers must never
    /// pass through the square Dock-icon background/cropping renderer.
    static func thumbnail(_ data: Data, mimeType: String? = nil, local: Bool = false) -> Data? {
        guard !data.isEmpty, data.count <= (local ? maximumLocalBytes : maximumBytes),
              let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              let type = CGImageSourceGetType(source) as String?, acceptedTypes.values.contains(type),
              mimeType == nil || acceptedTypes[mimeType!.lowercased()] == type,
              CGImageSourceGetCount(source) == 1,
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int,
              width >= 64, height >= 64, width <= 8192, height <= 8192,
              width * height <= 32_000_000 else { return nil }
        let options: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
                                     kCGImageSourceCreateThumbnailWithTransform: true,
                                     kCGImageSourceShouldCacheImmediately: true,
                                     kCGImageSourceThumbnailMaxPixelSize: 960]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(output, "public.png" as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination), output.length <= maximumCacheBytes else { return nil }
        return output as Data
    }
}

private final class SteamArtworkRedirectGuard: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    let source: SteamGameArtworkSource
    private let lock = NSLock()
    private var redirects = 0
    init(source: SteamGameArtworkSource) { self.source = source }

    func urlSession(_ session: URLSession, task: URLSessionTask,
                    willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
                    completionHandler: @escaping (URLRequest?) -> Void) {
        lock.lock(); redirects += 1; let withinLimit = redirects <= SteamGameArtworkPolicy.maximumRedirects; lock.unlock()
        completionHandler(withinLimit ? request.url.flatMap { SteamGameArtworkPolicy.request(for: $0, source: source) } : nil)
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didReceive challenge: URLAuthenticationChallenge,
                    completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        completionHandler(challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust
                          ? .performDefaultHandling : .cancelAuthenticationChallenge, nil)
    }
}

enum SteamGameArtworkError: Error { case rejectedResponse, overflow, invalidImage, httpStatus(Int) }

enum SteamGameArtworkDownload {
    static func read(_ source: SteamGameArtworkSource, url: URL) async throws -> Data {
        guard let request = SteamGameArtworkPolicy.request(for: url, source: source) else {
            throw SteamGameArtworkError.rejectedResponse
        }
        let delegate = SteamArtworkRedirectGuard(source: source)
        let session = URLSession(configuration: SteamGameArtworkPolicy.sessionConfiguration(), delegate: delegate, delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        return try await withTaskCancellationHandler {
            let (stream, response) = try await session.bytes(for: request)
            guard let http = response as? HTTPURLResponse else { throw SteamGameArtworkError.rejectedResponse }
            guard http.statusCode == 200 else { throw SteamGameArtworkError.httpStatus(http.statusCode) }
            guard SteamGameArtworkPolicy.accepts(http, source: source),
                  let mime = http.mimeType else { throw SteamGameArtworkError.rejectedResponse }
            var data = Data()
            data.reserveCapacity(min(max(0, Int(http.expectedContentLength)), SteamGameArtworkPolicy.maximumBytes))
            for try await byte in stream {
                try Task.checkCancellation()
                guard data.count < SteamGameArtworkPolicy.maximumBytes else { throw SteamGameArtworkError.overflow }
                data.append(byte)
            }
            try Task.checkCancellation()
            let thumbnail = await Task.detached(priority: .utility) {
                SteamGameArtworkPolicy.thumbnail(data, mimeType: mime)
            }.value
            guard let thumbnail else { throw SteamGameArtworkError.invalidImage }
            return thumbnail
        } onCancel: { session.invalidateAndCancel() }
    }
}

/// Cache descriptors stay anchored to Caskrun's own private directory. Symlinks
/// and special files are refused, writes are atomic, and the cache is bounded.
enum SteamGameArtworkCache {
    static var defaultRoot: URL? {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first?
            .appendingPathComponent("Caskrun/game-artwork", isDirectory: true)
    }

    private static func directory(_ root: URL, create: Bool) -> Int32? {
        guard root.isFileURL, root.standardizedFileURL.path == root.path else { return nil }
        // Create components relative to already opened directories. A symlink
        // in an ancestor cannot redirect even the directory-creation step.
        var fd = Darwin.open("/", O_RDONLY | O_CLOEXEC | O_DIRECTORY | O_NOFOLLOW)
        guard fd >= 0 else { return nil }
        for component in root.pathComponents.dropFirst() {
            var next = openat(fd, component, O_RDONLY | O_CLOEXEC | O_DIRECTORY | O_NOFOLLOW)
            if next < 0, errno == ENOENT, create {
                guard mkdirat(fd, component, 0o700) == 0 || errno == EEXIST else { Darwin.close(fd); return nil }
                next = openat(fd, component, O_RDONLY | O_CLOEXEC | O_DIRECTORY | O_NOFOLLOW)
            }
            Darwin.close(fd)
            guard next >= 0 else { return nil }
            fd = next
        }
        var metadata = stat()
        guard fstat(fd, &metadata) == 0, metadata.st_uid == geteuid(),
              (metadata.st_mode & S_IFMT) == S_IFDIR,
              (!create || fchmod(fd, 0o700) == 0), (create || metadata.st_mode & 0o077 == 0) else {
            Darwin.close(fd); return nil
        }
        return fd
    }

    static func read(_ source: SteamGameArtworkSource, root: URL) -> Data? {
        guard let directory = directory(root, create: false) else { return nil }
        defer { Darwin.close(directory) }
        let fd = openat(directory, source.cacheName, O_RDONLY | O_CLOEXEC | O_NONBLOCK | O_NOFOLLOW)
        guard fd >= 0 else { return nil }
        defer { Darwin.close(fd) }
        var metadata = stat()
        guard fstat(fd, &metadata) == 0, (metadata.st_mode & S_IFMT) == S_IFREG,
              metadata.st_uid == geteuid(), metadata.st_mode & 0o077 == 0,
              metadata.st_size > 0, metadata.st_size <= SteamGameArtworkPolicy.maximumCacheBytes else { return nil }
        var data = Data(count: Int(metadata.st_size))
        let complete = data.withUnsafeMutableBytes { bytes in
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.read(fd, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if count < 0, errno == EINTR { continue }
                guard count > 0 else { return false }; offset += count
            }
            return true
        }
        // A cached cover is generated PNG, never a stored HTTP response.
        var after = stat()
        guard complete, fstat(fd, &after) == 0, metadata.st_size == after.st_size,
              metadata.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
              metadata.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
              metadata.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
              metadata.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec,
              data.starts(with: [137, 80, 78, 71, 13, 10, 26, 10]) else { return nil }
        return SteamGameArtworkPolicy.thumbnail(data, mimeType: "image/png", local: true)
    }

    @discardableResult
    static func store(_ data: Data, source: SteamGameArtworkSource, root: URL) -> Bool {
        guard data.count <= SteamGameArtworkPolicy.maximumCacheBytes,
              data.starts(with: [137, 80, 78, 71, 13, 10, 26, 10]),
              let directory = directory(root, create: true) else { return false }
        defer { Darwin.close(directory) }
        let temporary = ".steam-artwork-\(UUID().uuidString).tmp"
        let fd = openat(directory, temporary, O_WRONLY | O_CLOEXEC | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
        guard fd >= 0 else { return false }
        defer { Darwin.close(fd); _ = unlinkat(directory, temporary, 0) }
        let complete = data.withUnsafeBytes { bytes in
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.write(fd, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if count < 0, errno == EINTR { continue }
                guard count > 0 else { return false }; offset += count
            }
            return true
        }
        guard complete, renameat(directory, temporary, directory, source.cacheName) == 0 else { return false }
        let files = ((try? FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: [.contentModificationDateKey])) ?? [])
            .filter { $0.lastPathComponent.range(of: #"^steam-[0-9]{1,10}-cover-v1\.png$"#, options: .regularExpression) != nil }
            .sorted { ((try? $0.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast)
                > ((try? $1.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast) }
        for file in files.dropFirst(SteamGameArtworkPolicy.maximumCacheEntries) {
            _ = unlinkat(directory, file.lastPathComponent, 0)
        }
        return true
    }
}

actor SteamGameArtworkStore {
    static let shared = SteamGameArtworkStore()
    typealias Transport = @Sendable (SteamGameArtworkSource, URL) async throws -> Data
    private let cacheRoot: URL?
    private let transport: Transport
    private var flights: [UInt32: Task<Data?, Never>] = [:]
    private var failedUntil: [UInt32: Date] = [:]
    private var active = 0
    private var waiting: [CheckedContinuation<Void, Never>] = []

    init(cacheRoot: URL? = SteamGameArtworkCache.defaultRoot,
         transport: @escaping Transport = { source, url in try await SteamGameArtworkDownload.read(source, url: url) }) {
        self.cacheRoot = cacheRoot; self.transport = transport
    }

    func load(appID: UInt32, localPath: String?) async -> Data? {
        guard let source = SteamGameArtworkSource(appID: appID), !Task.isCancelled else { return nil }
        // Named local covers are authoritative and available offline. Older
        // tiny icons/logos remain a last fallback after a full publisher cover.
        let localIsCover = localPath.map {
            let name = URL(fileURLWithPath: $0).lastPathComponent
            return ["header", "library_header", "library_hero", "library_600x900", "capsule"].contains(where: name.contains)
        } ?? false
        if localIsCover, let localPath, let local = await SteamArtworkReader.shared.read(localPath) { return local }
        if let flight = flights[appID] { return await flight.value }
        if failedUntil[appID].map({ $0 > Date() }) == true {
            if let localPath { return await SteamArtworkReader.shared.read(localPath) }
            return nil
        }
        let flight = Task { await self.resolve(source, localPath: localPath) }
        flights[appID] = flight
        let data = await flight.value
        flights[appID] = nil
        if data == nil { failedUntil[appID] = Date().addingTimeInterval(60) }
        else { failedUntil[appID] = nil }
        return Task.isCancelled ? nil : data
    }

    private func resolve(_ source: SteamGameArtworkSource, localPath: String?) async -> Data? {
        if active >= 2 { await withCheckedContinuation { waiting.append($0) } } else { active += 1 }
        defer { if waiting.isEmpty { active -= 1 } else { waiting.removeFirst().resume() } }
        if let cacheRoot, let cached = await Task.detached(priority: .utility, operation: {
            SteamGameArtworkCache.read(source, root: cacheRoot)
        }).value { return cached }
        for url in source.urls {
            guard !Task.isCancelled else { return nil }
            do {
                let data = try await transport(source, url)
                guard !Task.isCancelled else { return nil }
                if let cacheRoot {
                    _ = await Task.detached(priority: .utility) {
                        SteamGameArtworkCache.store(data, source: source, root: cacheRoot)
                    }.value
                }
                return data
            } catch {
                // Availability failures may try the second official endpoint;
                // account/TLS/redirect trust failures must never be worked around.
                if error is CancellationError { return nil }
                if !SteamGameArtworkPolicy.mayTryFallback(after: error) { break }
            }
        }
        if let localPath { return await SteamArtworkReader.shared.read(localPath) }
        return nil
    }
}
