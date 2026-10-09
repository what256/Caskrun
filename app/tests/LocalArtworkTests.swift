import AppKit
import Darwin
import Foundation
import ImageIO

@main
struct LocalArtworkTests {
    private struct Failure: Error { let message: String }

    static func main() throws {
        var checks = 0
        func expect(_ value: Bool, _ message: String) throws {
            guard value else { throw Failure(message: message) }
            checks += 1
        }
        let fileManager = FileManager.default
        let build = URL(fileURLWithPath: fileManager.currentDirectoryPath).resolvingSymlinksInPath()
            .appendingPathComponent(".build", isDirectory: true)
        let fixture = build.appendingPathComponent("local-artwork-fixture-" + UUID().uuidString, isDirectory: true)
        try fileManager.createDirectory(at: fixture, withIntermediateDirectories: false)
        defer { try? fileManager.removeItem(at: fixture) }

        let bytes = Data("artwork".utf8)
        let regular = fixture.appendingPathComponent("regular.bin")
        try bytes.write(to: regular)
        try expect(MacAppArtwork.read(regular.path, maximum: bytes.count) == bytes,
                   "ordinary local artwork reads successfully at the exact size cap")
        try expect(MacAppArtwork.read(regular.path, maximum: bytes.count - 1) == nil,
                   "oversized input is refused before allocation")
        try expect(MacAppArtwork.read(regular.path, maximum: 0) == nil,
                   "zero size allowance refuses nonempty input")
        try expect(MacAppArtwork.read("regular.bin", maximum: 128) == nil, "relative path is refused")
        try expect(MacAppArtwork.read(fixture.path + "/./regular.bin", maximum: 128) == nil,
                   "noncanonical path is refused")
        try expect(MacAppArtwork.read(regular.path + "\0suffix", maximum: 128) == nil,
                   "embedded NUL cannot truncate the selected path")
        try expect(MacAppArtwork.read(fixture.path, maximum: 128) == nil, "directory is not artwork")
        let empty = fixture.appendingPathComponent("empty.bin")
        try Data().write(to: empty)
        try expect(MacAppArtwork.read(empty.path, maximum: 128) == nil, "empty file is refused")
        try expect(MacAppArtwork.read(fixture.appendingPathComponent("missing.bin").path, maximum: 128) == nil,
                   "missing file is refused")

        let leafLink = fixture.appendingPathComponent("leaf-link.bin")
        try fileManager.createSymbolicLink(atPath: leafLink.path, withDestinationPath: regular.path)
        try expect(MacAppArtwork.read(leafLink.path, maximum: 128) == nil, "final-component symlink is refused")
        let directory = fixture.appendingPathComponent("real-directory", isDirectory: true)
        try fileManager.createDirectory(at: directory, withIntermediateDirectories: false)
        let nested = directory.appendingPathComponent("nested.bin")
        try bytes.write(to: nested)
        let ancestorLink = fixture.appendingPathComponent("ancestor-link", isDirectory: true)
        try fileManager.createSymbolicLink(atPath: ancestorLink.path, withDestinationPath: directory.path)
        try expect(MacAppArtwork.read(ancestorLink.appendingPathComponent("nested.bin").path, maximum: 128) == nil,
                   "symlink in an ancestor component is refused")
        try expect(MacAppArtwork.read(nested.path, maximum: 128) == bytes,
                   "the same ordinary file remains readable through its real directory")
        let fifo = fixture.appendingPathComponent("named-pipe")
        try expect(mkfifo(fifo.path, 0o600) == 0, "owned FIFO fixture created")
        let start = Date()
        try expect(MacAppArtwork.read(fifo.path, maximum: 128) == nil,
                   "FIFO is refused without waiting for a writer")
        try expect(Date().timeIntervalSince(start) < 1, "nonblocking FIFO rejection returns promptly")

        try expect(MacAppArtwork.png(Data()) == nil, "empty image is refused")
        try expect(MacAppArtwork.png(bytes) == nil, "ordinary bytes do not decode as an image")
        let imageBytes = try makePNG(width: 64, height: 32)
        let imageFile = fixture.appendingPathComponent("wide-game.png")
        try imageBytes.write(to: imageFile)
        guard let read = MacAppArtwork.read(imageFile.path, maximum: 8 * 1024 * 1024),
              let converted = MacAppArtwork.png(read),
              let source = CGImageSourceCreateWithData(converted as CFData, nil),
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw Failure(message: "local PNG read and adaptive compositing failed")
        }
        try expect(converted.starts(with: [137, 80, 78, 71, 13, 10, 26, 10]), "compositor emits PNG data")
        try expect(image.width == 256 && image.height == 256, "adaptive output includes native icon dimensions")
        let pixels = try pixelData(image)
        let center = (128 * image.width + 128) * 4
        try expect(pixels[center + 3] > 240 && pixels[center + 2] > pixels[center + 1]
                   && pixels[center + 1] > pixels[center], "publisher colour survives compositing at the centre")
        try expect(pixels[3] == 0, "native icon corners remain transparent")
        try expect(MacAppArtwork.png(try makePNG(width: 4097, height: 1)) == nil,
                   "decoded dimensions above the image bound are refused")
        print("\(checks) local artwork read and compositing checks passed")
    }

    private static func makePNG(width: Int, height: Int) throws -> Data {
        let pixel: [UInt8] = [40, 100, 210, 255]
        let pixels = Array(repeating: pixel, count: width * height).flatMap { $0 }
        guard let provider = CGDataProvider(data: Data(pixels) as CFData),
              let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                                  bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue |
                                      CGBitmapInfo.byteOrder32Big.rawValue), provider: provider,
                                  decode: nil, shouldInterpolate: true, intent: .defaultIntent) else {
            throw Failure(message: "could not make owned PNG fixture")
        }
        let bytes = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(bytes, "public.png" as CFString, 1, nil) else {
            throw Failure(message: "could not encode owned PNG fixture")
        }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else { throw Failure(message: "could not finish PNG fixture") }
        return bytes as Data
    }

    private static func pixelData(_ image: CGImage) throws -> [UInt8] {
        var pixels = [UInt8](repeating: 0, count: image.width * image.height * 4)
        let rendered = pixels.withUnsafeMutableBytes { bytes -> Bool in
            guard let context = CGContext(data: bytes.baseAddress, width: image.width, height: image.height,
                                          bitsPerComponent: 8, bytesPerRow: image.width * 4,
                                          space: CGColorSpaceCreateDeviceRGB(),
                                          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue |
                                              CGBitmapInfo.byteOrder32Big.rawValue) else { return false }
            context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
            return true
        }
        guard rendered else { throw Failure(message: "could not inspect composed pixels") }
        return pixels
    }
}
