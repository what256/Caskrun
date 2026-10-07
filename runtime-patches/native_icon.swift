// SPDX-License-Identifier: LGPL-2.1-or-later
// Original Caskrun offscreen icon converter. No application, window, or UI is created.
import Foundation
import CoreGraphics
import Darwin
import ImageIO
import UniformTypeIdentifiers

private enum IconFailure: Error {
    case invalidArguments, unsafePath, invalidInput, cannotDecode, cannotRender, cannotWrite
}

private let inputLimit = 16 * 1024 * 1024

private func checkedURL(_ path: String) throws -> URL {
    guard path.hasPrefix("/"), path.utf8.count < Int(PATH_MAX), !path.contains("\0") else {
        throw IconFailure.unsafePath
    }
    let url = URL(fileURLWithPath: path)
    guard url.standardizedFileURL.path == path else { throw IconFailure.unsafePath }
    return url
}

// Walk from an anchored root descriptor. Every directory open refuses symbolic
// links; later reads and writes use openat on the retained final descriptor.
private func openDirectory(_ url: URL) throws -> Int32 {
    var descriptor = Darwin.open("/", O_RDONLY | O_DIRECTORY | O_CLOEXEC)
    guard descriptor >= 0 else { throw IconFailure.unsafePath }
    do {
        for component in url.pathComponents.dropFirst() {
            let next = component.withCString {
                Darwin.openat(descriptor, $0, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            }
            guard next >= 0 else { throw IconFailure.unsafePath }
            Darwin.close(descriptor)
            descriptor = next
        }
        return descriptor
    } catch {
        Darwin.close(descriptor)
        throw error
    }
}

private func readInput(_ url: URL) throws -> Data {
    let directory = try openDirectory(url.deletingLastPathComponent())
    defer { Darwin.close(directory) }
    let descriptor = url.lastPathComponent.withCString {
        Darwin.openat(directory, $0, O_RDONLY | O_NOFOLLOW | O_CLOEXEC)
    }
    guard descriptor >= 0 else { throw IconFailure.invalidInput }
    defer { Darwin.close(descriptor) }
    var metadata = stat()
    guard fstat(descriptor, &metadata) == 0,
          (metadata.st_mode & S_IFMT) == S_IFREG,
          metadata.st_size > 0, metadata.st_size <= inputLimit else {
        throw IconFailure.invalidInput
    }
    var data = Data(count: Int(metadata.st_size))
    try data.withUnsafeMutableBytes { bytes in
        var offset = 0
        while offset < bytes.count {
            let count = Darwin.read(descriptor, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
            if count < 0, errno == EINTR { continue }
            guard count > 0 else { throw IconFailure.invalidInput }
            offset += count
        }
    }
    var after = stat()
    guard fstat(descriptor, &after) == 0, after.st_size == metadata.st_size,
          after.st_mtimespec.tv_sec == metadata.st_mtimespec.tv_sec,
          after.st_mtimespec.tv_nsec == metadata.st_mtimespec.tv_nsec else {
        throw IconFailure.invalidInput
    }
    return data
}

private func pngImage(_ source: CGImage, size: Int) throws -> Data {
    guard let context = CGContext(data: nil, width: size, height: size,
                                  bitsPerComponent: 8, bytesPerRow: size * 4,
                                  space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
        throw IconFailure.cannotRender
    }
    let side = CGFloat(size)
    let inset = side * 0.045
    let tile = CGRect(x: inset, y: inset, width: side - inset * 2, height: side - inset * 2)
    let tilePath = CGPath(roundedRect: tile, cornerWidth: tile.width * 0.22,
                         cornerHeight: tile.height * 0.22, transform: nil)
    context.setFillColor(CGColor(red: 0.105, green: 0.12, blue: 0.145, alpha: 1))
    context.addPath(tilePath)
    context.fillPath()
    context.setStrokeColor(CGColor(gray: 1, alpha: 0.09))
    context.setLineWidth(max(0.5, side / 512))
    context.addPath(tilePath)
    context.strokePath()
    let available = tile.width * 0.76
    let scale = available / CGFloat(max(source.width, source.height))
    let width = CGFloat(source.width) * scale
    let height = CGFloat(source.height) * scale
    context.interpolationQuality = .high
    context.draw(source, in: CGRect(x: (side - width) / 2, y: (side - height) / 2,
                                   width: width, height: height))
    guard let rendered = context.makeImage() else { throw IconFailure.cannotRender }
    let data = NSMutableData()
    guard let destination = CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil) else {
        throw IconFailure.cannotRender
    }
    CGImageDestinationAddImage(destination, rendered, nil)
    guard CGImageDestinationFinalize(destination) else { throw IconFailure.cannotRender }
    return data as Data
}

private func appendBigEndian(_ value: UInt32, to data: inout Data) {
    var bigEndian = value.bigEndian
    withUnsafeBytes(of: &bigEndian) { data.append(contentsOf: $0) }
}

private func icnsImage(_ images: [Int: Data]) throws -> Data {
    // Native sizes and explicit Retina representations. PNG chunks retain the
    // publisher's colours, aspect ratio, and alpha; only the surrounding tile is new.
    let types: [(String, Int)] = [
        ("icp4", 16), ("icp5", 32), ("icp6", 64), ("ic07", 128),
        ("ic08", 256), ("ic09", 512), ("ic10", 1024),
        ("ic11", 32), ("ic12", 64), ("ic13", 256), ("ic14", 512)
    ]
    var chunks = Data()
    for (type, size) in types {
        guard let png = images[size], png.count <= inputLimit else { throw IconFailure.cannotRender }
        chunks.append(contentsOf: type.utf8)
        appendBigEndian(UInt32(png.count + 8), to: &chunks)
        chunks.append(png)
    }
    guard chunks.count <= inputLimit - 8 else { throw IconFailure.cannotRender }
    var output = Data("icns".utf8)
    appendBigEndian(UInt32(chunks.count + 8), to: &output)
    output.append(chunks)
    return output
}

private func writeNew(_ data: Data, name: String, directory: Int32) throws {
    let descriptor = name.withCString {
        Darwin.openat(directory, $0, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
    }
    guard descriptor >= 0 else { throw IconFailure.cannotWrite }
    do {
        try data.withUnsafeBytes { bytes in
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.write(descriptor, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if count < 0, errno == EINTR { continue }
                guard count > 0 else { throw IconFailure.cannotWrite }
                offset += count
            }
        }
        guard fsync(descriptor) == 0 else { throw IconFailure.cannotWrite }
        Darwin.close(descriptor)
    } catch {
        Darwin.close(descriptor)
        name.withCString { _ = Darwin.unlinkat(directory, $0, 0) }
        throw error
    }
}

private func run() throws {
    let arguments = Array(CommandLine.arguments.dropFirst())
    guard arguments.count == 6 else { throw IconFailure.invalidArguments }
    var options: [String: String] = [:]
    for index in stride(from: 0, to: arguments.count, by: 2) {
        let key = arguments[index]
        guard ["--input", "--output", "--png-output"].contains(key), options[key] == nil else {
            throw IconFailure.invalidArguments
        }
        options[key] = arguments[index + 1]
    }
    guard let inputPath = options["--input"], let iconPath = options["--output"],
          let pngPath = options["--png-output"] else { throw IconFailure.invalidArguments }
    let input = try checkedURL(inputPath)
    let icon = try checkedURL(iconPath)
    let png = try checkedURL(pngPath)
    guard icon.lastPathComponent == "app.icns", png.lastPathComponent == "app.png",
          icon.deletingLastPathComponent() == png.deletingLastPathComponent() else {
        throw IconFailure.invalidArguments
    }
    let directory = try openDirectory(icon.deletingLastPathComponent())
    defer { Darwin.close(directory) }
    var metadata = stat()
    guard fstat(directory, &metadata) == 0, metadata.st_uid == geteuid(),
          (metadata.st_mode & 0o022) == 0 else { throw IconFailure.unsafePath }
    let data = try readInput(input)
    let pngSignature = Data([137, 80, 78, 71, 13, 10, 26, 10])
    let isPNG = data.starts(with: pngSignature)
    let isICO = data.count >= 6 && data[0] == 0 && data[1] == 0 && data[2] == 1 && data[3] == 0
    guard isPNG || isICO,
          let decodedSource = CGImageSourceCreateWithData(data as CFData, nil),
          CGImageSourceGetCount(decodedSource) > 0,
          CGImageSourceGetCount(decodedSource) <= 64 else { throw IconFailure.cannotDecode }
    var bestIndex = 0
    var bestArea = 0
    for index in 0..<CGImageSourceGetCount(decodedSource) {
        guard let properties = CGImageSourceCopyPropertiesAtIndex(decodedSource, index, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? NSNumber,
              let height = properties[kCGImagePropertyPixelHeight] as? NSNumber,
              width.intValue > 0, height.intValue > 0,
              width.intValue <= 4096, height.intValue <= 4096 else { throw IconFailure.cannotDecode }
        let area = width.intValue * height.intValue
        if area > bestArea { bestArea = area; bestIndex = index }
    }
    let thumbnailOptions: [CFString: Any] = [
        kCGImageSourceCreateThumbnailFromImageAlways: true,
        kCGImageSourceThumbnailMaxPixelSize: 1024,
        kCGImageSourceCreateThumbnailWithTransform: true,
        kCGImageSourceShouldCacheImmediately: true
    ]
    guard let source = CGImageSourceCreateThumbnailAtIndex(decodedSource, bestIndex,
                                                          thumbnailOptions as CFDictionary),
          source.width > 0, source.height > 0,
          source.width <= 1024, source.height <= 1024 else { throw IconFailure.cannotDecode }
    var images: [Int: Data] = [:]
    for size in [16, 32, 64, 128, 256, 512, 1024] { images[size] = try pngImage(source, size: size) }
    let icns = try icnsImage(images)
    try writeNew(icns, name: "app.icns", directory: directory)
    do {
        try writeNew(images[1024]!, name: "app.png", directory: directory)
    } catch {
        // The first file was created by this invocation; remove it if its
        // companion could not be written. Pre-existing outputs are untouched.
        _ = Darwin.unlinkat(directory, "app.icns", 0)
        throw error
    }
}

autoreleasepool {
    do { try run() }
    catch {
        fputs("Caskrun native icon conversion failed: \(error).\n", stderr)
        exit(1)
    }
}
