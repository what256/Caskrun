import AppKit
import ImageIO
import Darwin

// Shared with the offscreen Dock renderer; no window or Windows process is created.
enum MacAppArtwork {
    static func png(_ data: Data, crop: CGRect? = nil, svg: Bool = false) -> Data? {
        guard !data.isEmpty, data.count <= 16 * 1024 * 1024 else { return nil }
        let bitmap: CGImage
        if svg {
            guard let image = NSImage(data: data), image.isValid,
                  image.size.width > 0, image.size.height > 0,
                  image.size.width <= 2048, image.size.height <= 2048,
                  let decoded = image.cgImage(forProposedRect: nil, context: nil, hints: nil),
                  decoded.width <= 4096, decoded.height <= 4096 else { return nil }
            if let crop {
                let sx = CGFloat(decoded.width) / image.size.width
                let sy = CGFloat(decoded.height) / image.size.height
                let rect = CGRect(x: crop.minX * sx, y: crop.minY * sy,
                                  width: crop.width * sx, height: crop.height * sy).integral
                guard rect.minX >= 0, rect.minY >= 0,
                      rect.maxX <= CGFloat(decoded.width), rect.maxY <= CGFloat(decoded.height),
                      let cropped = decoded.cropping(to: rect) else { return nil }
                bitmap = cropped
            } else { bitmap = decoded }
        } else {
            guard let source = CGImageSourceCreateWithData(data as CFData, nil),
                  CGImageSourceGetCount(source) > 0, CGImageSourceGetCount(source) <= 64 else { return nil }
            var best = 0, area = 0
            for index in 0..<CGImageSourceGetCount(source) {
                guard let props = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any],
                      let width = props[kCGImagePropertyPixelWidth] as? NSNumber,
                      let height = props[kCGImagePropertyPixelHeight] as? NSNumber,
                      width.intValue > 0, height.intValue > 0,
                      width.intValue <= 4096, height.intValue <= 4096 else { return nil }
                if width.intValue * height.intValue > area {
                    best = index; area = width.intValue * height.intValue
                }
            }
            let options: [CFString: Any] = [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceThumbnailMaxPixelSize: 1024,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceShouldCacheImmediately: true
            ]
            guard let decoded = CGImageSourceCreateThumbnailAtIndex(source, best, options as CFDictionary) else { return nil }
            bitmap = decoded
        }
        return try? caskrunAdaptiveIconPNG(bitmap, size: 256)
    }

    static func read(_ path: String, maximum: Int) -> Data? {
        guard path.hasPrefix("/"), !path.contains("\0"),
              URL(fileURLWithPath: path).standardizedFileURL.path == path else { return nil }
        let fd = Darwin.open(path, O_RDONLY | O_CLOEXEC | O_NONBLOCK | 0x2000_0000)
        guard fd >= 0 else { return nil }
        defer { Darwin.close(fd) }
        var before = stat()
        guard fstat(fd, &before) == 0, (before.st_mode & S_IFMT) == S_IFREG,
              before.st_size > 0, before.st_size <= maximum else { return nil }
        var data = Data(count: Int(before.st_size))
        let readOK = data.withUnsafeMutableBytes { bytes in
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.read(fd, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if count < 0, errno == EINTR { continue }
                guard count > 0 else { return false }
                offset += count
            }
            return true
        }
        var after = stat()
        guard readOK, fstat(fd, &after) == 0, before.st_size == after.st_size,
              before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
              before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
              before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
              before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec else { return nil }
        return data
    }
}

// Decode only visible library artwork, with two bounded readers at a time.
actor SteamArtworkReader {
    static let shared = SteamArtworkReader()
    private var active = 0
    private var waiting: [CheckedContinuation<Void, Never>] = []
    func read(_ path: String) async -> Data? {
        if active >= 2 { await withCheckedContinuation { waiting.append($0) } }
        else { active += 1 }
        defer {
            if waiting.isEmpty { active -= 1 }
            else { waiting.removeFirst().resume() }
        }
        guard !Task.isCancelled else { return nil }
        return await Task.detached(priority: .utility) {
            guard let bytes = MacAppArtwork.read(path, maximum: 8 * 1024 * 1024) else { return nil as Data? }
            return SteamGameArtworkPolicy.thumbnail(bytes, local: true)
        }.value
    }
}
