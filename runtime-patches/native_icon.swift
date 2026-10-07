// SPDX-License-Identifier: MIT
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
        Darwin.openat(directory, $0, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
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

private enum IconComposition {
    case artwork, finishedShape, opaqueSquare, supportedMark
}

private struct IconPlan {
    let image: CGImage
    let composition: IconComposition
    let colour: (red: CGFloat, green: CGFloat, blue: CGFloat)
    let luminance: CGFloat
}

// Inspect a fixed 128px thumbnail, independent of input size. Work in a known
// premultiplied RGBA buffer; transparent pixels do not influence colour choices.
// Geometry is deliberately conservative: an uncertain publisher silhouette
// remains a silhouette, rather than acquiring an invented background.
private func iconPlan(_ source: CGImage) throws -> IconPlan {
    guard source.width > 0, source.height > 0,
          source.width <= 4096, source.height <= 4096 else { throw IconFailure.cannotDecode }
    let longest = CGFloat(max(source.width, source.height))
    let width = max(1, Int((CGFloat(source.width) * 128 / longest).rounded()))
    let height = max(1, Int((CGFloat(source.height) * 128 / longest).rounded()))
    var pixels = [UInt8](repeating: 0, count: width * height * 4)
    let rendered = pixels.withUnsafeMutableBytes { bytes -> Bool in
        guard let context = CGContext(data: bytes.baseAddress, width: width, height: height,
                                      bitsPerComponent: 8, bytesPerRow: width * 4,
                                      space: CGColorSpaceCreateDeviceRGB(),
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue |
                                          CGBitmapInfo.byteOrder32Big.rawValue) else { return false }
        context.interpolationQuality = .high
        context.draw(source, in: CGRect(x: 0, y: 0, width: width, height: height))
        return true
    }
    guard rendered else { throw IconFailure.cannotRender }
    var left = width, right = -1, top = height, bottom = -1
    for y in 0..<height {
        for x in 0..<width where pixels[(y * width + x) * 4 + 3] >= 24 {
            left = min(left, x); right = max(right, x)
            top = min(top, y); bottom = max(bottom, y)
        }
    }
    guard right >= left, bottom >= top else { throw IconFailure.cannotRender }
    let boundWidth = right - left + 1, boundHeight = bottom - top + 1
    var weight = 0.0, red = 0.0, green = 0.0, blue = 0.0
    var redSquared = 0.0, greenSquared = 0.0, blueSquared = 0.0
    var luminanceSum = 0.0, luminanceSquared = 0.0, saturation = 0.0
    var circleError = 0.0, edgeWeight = 0.0, edgeCount = 0
    for y in top...bottom {
        for x in left...right {
            let offset = (y * width + x) * 4
            let alpha = Double(pixels[offset + 3]) / 255
            weight += alpha
            if alpha >= 0.1 {
                let r = min(1, Double(pixels[offset]) / 255 / alpha)
                let g = min(1, Double(pixels[offset + 1]) / 255 / alpha)
                let b = min(1, Double(pixels[offset + 2]) / 255 / alpha)
                let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b
                red += r * alpha; green += g * alpha; blue += b * alpha
                redSquared += r * r * alpha
                greenSquared += g * g * alpha
                blueSquared += b * b * alpha
                luminanceSum += luma * alpha
                luminanceSquared += luma * luma * alpha
                saturation += (max(r, g, b) - min(r, g, b)) * alpha
            }
            let nx = (Double(x - left) + 0.5) / Double(boundWidth)
            let ny = (Double(y - top) + 0.5) / Double(boundHeight)
            let radius = hypot((nx - 0.5) * 2, (ny - 0.5) * 2)
            // Leave the antialiased perimeter out of the shape comparison.
            if radius < 0.96 { circleError += 1 - alpha }
            if radius > 1.04 { circleError += alpha }
            if (nx < 0.035 || nx > 0.965) && ny > 0.25 && ny < 0.75 ||
                (ny < 0.035 || ny > 0.965) && nx > 0.25 && nx < 0.75 {
                edgeWeight += alpha
                edgeCount += 1
            }
        }
    }
    guard weight > 0 else { throw IconFailure.cannotRender }
    let coverage = weight / Double(boundWidth * boundHeight)
    let luma = luminanceSum / weight
    let variance = max(0, luminanceSquared / weight - luma * luma)
    let colourVariance = max(0, redSquared / weight - pow(red / weight, 2),
                             greenSquared / weight - pow(green / weight, 2),
                             blueSquared / weight - pow(blue / weight, 2))
    let square = Double(min(boundWidth, boundHeight)) / Double(max(boundWidth, boundHeight)) > 0.9
    let circle = square && coverage > 0.69 && coverage < 0.84 &&
        circleError / Double(boundWidth * boundHeight) < 0.035
    let finishedTile = square && coverage >= 0.84 &&
        edgeCount > 0 && edgeWeight / Double(edgeCount) > 0.82
    let singleTone = variance < 0.012 && colourVariance < 0.012
    let lowContrast = saturation / weight < 0.18 || luma > 0.82 || luma < 0.08
    // A sparse achromatic or very bright/dark mark is hard to see on at least
    // one desktop theme. Per-channel variance keeps equally bright but distinct
    // publisher colours from being mistaken for a single pale mark.
    // Multicolour artwork, distinct circles, and publisher plates are retained.
    let needsSupport = coverage < 0.46 && singleTone && lowContrast
    let composition: IconComposition
    if square && coverage > 0.985 { composition = .opaqueSquare }
    else if circle || finishedTile { composition = .finishedShape }
    else if needsSupport { composition = .supportedMark }
    else { composition = .artwork }

    var image = source
    // Remove excessive transparent padding only. Keep one sampled pixel of
    // breathing room so antialiased edge pixels are not clipped by the crop.
    if Double(boundWidth) / Double(width) < 0.88 || Double(boundHeight) / Double(height) < 0.88 {
        let x = max(0, left - 1), y = max(0, top - 1)
        let maxX = min(width, right + 2), maxY = min(height, bottom + 2)
        let crop = CGRect(x: CGFloat(x) * CGFloat(source.width) / CGFloat(width),
                          y: CGFloat(y) * CGFloat(source.height) / CGFloat(height),
                          width: CGFloat(maxX - x) * CGFloat(source.width) / CGFloat(width),
                          height: CGFloat(maxY - y) * CGFloat(source.height) / CGFloat(height)).integral
        guard let cropped = source.cropping(to: crop) else { throw IconFailure.cannotRender }
        image = cropped
    }
    return IconPlan(image: image, composition: composition,
                    colour: (CGFloat(red / weight), CGFloat(green / weight), CGFloat(blue / weight)),
                    luminance: CGFloat(luma))
}

private func pngImage(_ plan: IconPlan, size: Int) throws -> Data {
    guard size > 0, size <= 1024 else { throw IconFailure.cannotRender }
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
    let available: CGFloat
    switch plan.composition {
    case .supportedMark:
        // Muted publisher-derived colour, with lightness opposite the mark.
        // The original glyph is drawn unchanged; only its support is new.
        let base: CGFloat = plan.luminance > 0.55 ? 0.16 : 0.90
        let colour = plan.colour
        let mixed = (base * 0.94 + colour.red * 0.06,
                     base * 0.94 + colour.green * 0.06,
                     base * 0.94 + colour.blue * 0.06)
        context.setFillColor(CGColor(red: mixed.0, green: mixed.1, blue: mixed.2, alpha: 1))
        context.addPath(tilePath)
        context.fillPath()
        available = tile.width * 0.72
    case .opaqueSquare:
        // The publisher's existing background fills the Mac mask itself;
        // adding another tile would create the unwanted tile-within-a-tile.
        context.addPath(tilePath)
        context.clip()
        available = tile.width
    case .finishedShape:
        available = tile.width
    case .artwork:
        available = tile.width * 0.95
    }
    let source = plan.image
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

// Shared by the offscreen command and the native Discover artwork loader.
// Call outside the UI thread; no application or window is created here.
func caskrunAdaptiveIconPNG(_ source: CGImage, size: Int) throws -> Data {
    try pngImage(iconPlan(source), size: size)
}

private func appendBigEndian(_ value: UInt32, to data: inout Data) {
    var bigEndian = value.bigEndian
    withUnsafeBytes(of: &bigEndian) { data.append(contentsOf: $0) }
}

private func icnsImage(_ images: [Int: Data]) throws -> Data {
    // Native sizes and explicit Retina representations. PNG chunks retain the
    // publisher's colours, aspect ratio, and adaptive alpha geometry.
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
    let plan = try iconPlan(source)
    var images: [Int: Data] = [:]
    for size in [16, 32, 64, 128, 256, 512, 1024] { images[size] = try pngImage(plan, size: size) }
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

#if !CASKRUN_ICON_LIBRARY
@main
private struct CaskrunNativeIconCLI {
    static func main() {
        autoreleasepool {
            do { try run() }
            catch {
                fputs("Caskrun native icon conversion failed: \(error).\n", stderr)
                exit(1)
            }
        }
    }
}
#endif
