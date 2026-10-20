import CoreGraphics
import Foundation
import ImageIO
import RsEditorCore

/// Loads the pictures of image nodes the editor reports missing: fetched
/// with URLSession (http, https, file and data addresses), decoded by
/// ImageIO with the EXIF orientation applied, at most `maxPixels` on the
/// long side, then handed to the editor as premultiplied RGBA.
final class ImageLoader {
    /// Resolves relative addresses (`/foto.jpg`).
    var baseURL: URL?
    /// The app's own loading (uploads in progress, authenticated servers):
    /// return the image's bytes, or nil to load the address as usual.
    var load: ((String) async -> Data?)?
    /// The app's storage for images dropped or pasted in (an upload): returns the
    /// stored image's address.
    var store: ((Data) async -> String?)?
    private let maxPixels = 2048
    private weak var engine: EditorEngine?

    init(engine: EditorEngine) {
        self.engine = engine
    }

    func load(_ sources: [String]) {
        for src in sources {
            Task { [weak self] in
                guard let self else { return }
                let image = await self.decode(src)
                await MainActor.run {
                    guard let engine = self.engine else { return }
                    if let image {
                        _ = engine.editor.addImage(
                            src: src, rgba: image.pixels, width: UInt32(image.width), height: UInt32(image.height),
                            scale: image.scale, premultiplied: true)
                    } else {
                        engine.editor.imageFailed(src: src)
                    }
                    engine.afterInput()
                }
            }
        }
    }

    private struct Decoded {
        var pixels: Data
        var width: Int
        var height: Int
        /// Pixels per point: below 1 when the picture was scaled down.
        var scale: Float
    }

    private func decode(_ src: String) async -> Decoded? {
        guard let data = await bytes(src),
              let source = CGImageSourceCreateWithData(data as CFData, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        else { return nil }
        var natural = (
            (properties[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue ?? 0,
            (properties[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue ?? 0
        )
        // Orientations 5–8 turn the picture sideways.
        if let orientation = (properties[kCGImagePropertyOrientation] as? NSNumber)?.intValue, orientation >= 5 {
            natural = (natural.1, natural.0)
        }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: min(maxPixels, max(natural.0, natural.1, 1)),
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
            return nil
        }
        let width = image.width, height = image.height
        var pixels = Data(count: width * height * 4)
        let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard let context = CGContext(
                data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
            else { return false }
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
            return true
        }
        guard drawn else { return nil }
        let scale = natural.0 > 0 ? Float(width) / Float(natural.0) : 1
        return Decoded(pixels: pixels, width: width, height: height, scale: scale)
    }

    private func bytes(_ src: String) async -> Data? {
        if let load, let data = await load(src) {
            return data
        }
        guard let url = URL(string: src, relativeTo: baseURL) else { return nil }
        if url.isFileURL {
            return try? Data(contentsOf: url)
        }
        guard let (data, response) = try? await URLSession.shared.data(from: url) else { return nil }
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            return nil
        }
        return data
    }
}
