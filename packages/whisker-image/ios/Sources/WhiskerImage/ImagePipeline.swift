// Loads images for `WhiskerImageView` on UIKit: Kingfisher downloads and keeps
// the original bytes on disk, while decoding happens here, off the main
// thread, at the size the view displays.

#if canImport(UIKit)
import Foundation
import ImageIO
import Kingfisher
import UIKit

struct WhiskerImageRequest: Equatable {
    let url: URL
    let cacheKey: String
    let headers: [String: String]
    /// The view's size in pixels, or `.zero` to decode at the source's own size.
    let targetPixelSize: CGSize

    var memoryKey: NSString {
        "\(cacheKey)|\(Int(targetPixelSize.width))x\(Int(targetPixelSize.height))" as NSString
    }
}

final class WhiskerLoadedImage {
    let image: UIImage
    /// Pixel size of the source, whatever size it was decoded at.
    let naturalSize: CGSize

    init(image: UIImage, naturalSize: CGSize) {
        self.image = image
        self.naturalSize = naturalSize
    }

    var pixelSize: CGSize {
        CGSize(width: image.size.width * image.scale, height: image.size.height * image.scale)
    }

    fileprivate var cost: Int {
        let frames = max(1, image.images?.count ?? 1)
        return Int(pixelSize.width * pixelSize.height) * 4 * frames
    }
}

final class WhiskerImageTask {
    private let lock = NSLock()
    private var cancelledFlag = false
    fileprivate var download: DownloadTask?

    fileprivate var cancelled: Bool {
        lock.lock()
        defer { lock.unlock() }
        return cancelledFlag
    }

    func cancel() {
        dispatchPrecondition(condition: .onQueue(.main))
        lock.lock()
        cancelledFlag = true
        lock.unlock()
        download?.cancel()
        download = nil
    }
}

enum WhiskerImageError: LocalizedError {
    case undecodable

    var errorDescription: String? { "The image data could not be decoded." }
}

final class WhiskerImagePipeline {
    static let shared = WhiskerImagePipeline()

    private let decoded = NSCache<NSString, WhiskerLoadedImage>()
    private let work = DispatchQueue(
        label: "rs.whisker.image.decode",
        qos: .userInitiated,
        attributes: .concurrent
    )

    private init() {
        decoded.totalCostLimit = 64 << 20
    }

    func cached(_ request: WhiskerImageRequest) -> WhiskerLoadedImage? {
        decoded.object(forKey: request.memoryKey)
    }

    /// Calls back on the main thread, unless the task was cancelled first.
    func load(
        _ request: WhiskerImageRequest,
        scale: CGFloat,
        completion: @escaping (Result<WhiskerLoadedImage, Error>, _ downloaded: Bool) -> Void
    ) -> WhiskerImageTask {
        let task = WhiskerImageTask()
        work.async { [self] in
            if let data = storedData(for: request) {
                finish(request, data: data, scale: scale, downloaded: false, task: task, completion)
                return
            }
            DispatchQueue.main.async { [self] in
                guard !task.cancelled else { return }
                task.download = download(request) { [self] result in
                    switch result {
                    case .success(let data):
                        work.async { [self] in
                            finish(
                                request,
                                data: data,
                                scale: scale,
                                downloaded: true,
                                task: task,
                                completion
                            )
                        }
                    case .failure(let error):
                        DispatchQueue.main.async {
                            guard !task.cancelled else { return }
                            completion(.failure(error), true)
                        }
                    }
                }
            }
        }
        return task
    }

    func prefetch(urls: [URL], headers: [String: String]) {
        for url in urls {
            let request = WhiskerImageRequest(
                url: url,
                cacheKey: Self.cacheKey(for: url, headers: headers),
                headers: headers,
                targetPixelSize: .zero
            )
            work.async { [self] in
                guard !ImageCache.default.diskStorage.isCached(forKey: request.cacheKey) else { return }
                DispatchQueue.main.async { [self] in _ = download(request) { _ in } }
            }
        }
    }

    /// URL plus the headers that shape the answer, since Kingfisher's disk
    /// cache is otherwise keyed by URL alone.
    static func cacheKey(for url: URL, headers: [String: String]) -> String {
        guard !headers.isEmpty else { return url.absoluteString }
        let joined = headers
            .sorted { $0.key < $1.key }
            .map { "\($0.key)=\($0.value)" }
            .joined(separator: ";")
        return "\(url.absoluteString)|\(joined)"
    }

    private func storedData(for request: WhiskerImageRequest) -> Data? {
        if request.url.isFileURL {
            return try? Data(contentsOf: request.url, options: .mappedIfSafe)
        }
        return try? ImageCache.default.diskStorage.value(forKey: request.cacheKey)
    }

    private func download(
        _ request: WhiskerImageRequest,
        completion: @escaping (Result<Data, Error>) -> Void
    ) -> DownloadTask? {
        var options: KingfisherOptionsInfo = [.callbackQueue(.untouch)]
        if !request.headers.isEmpty {
            let headers = request.headers
            options.append(.requestModifier(AnyModifier { urlRequest in
                var urlRequest = urlRequest
                for (name, value) in headers {
                    urlRequest.setValue(value, forHTTPHeaderField: name)
                }
                return urlRequest
            }))
        }
        return ImageDownloader.default.downloadImage(with: request.url, options: options) { result in
            switch result {
            case .success(let loaded):
                ImageCache.default.storeToDisk(loaded.originalData, forKey: request.cacheKey)
                completion(.success(loaded.originalData))
            case .failure(let error):
                completion(.failure(error))
            }
        }
    }

    private func finish(
        _ request: WhiskerImageRequest,
        data: Data,
        scale: CGFloat,
        downloaded: Bool,
        task: WhiskerImageTask,
        _ completion: @escaping (Result<WhiskerLoadedImage, Error>, Bool) -> Void
    ) {
        guard !task.cancelled else { return }
        let loaded = Self.decode(data, targetPixelSize: request.targetPixelSize, scale: scale)
        if let loaded {
            decoded.setObject(loaded, forKey: request.memoryKey, cost: loaded.cost)
        }
        DispatchQueue.main.async {
            guard !task.cancelled else { return }
            completion(loaded.map(Result.success) ?? .failure(WhiskerImageError.undecodable), downloaded)
        }
    }

    private static func decode(
        _ data: Data,
        targetPixelSize: CGSize,
        scale: CGFloat
    ) -> WhiskerLoadedImage? {
        let sourceOptions = [kCGImageSourceShouldCache: false] as CFDictionary
        guard let source = CGImageSourceCreateWithData(data as CFData, sourceOptions) else {
            return nil
        }
        if CGImageSourceGetCount(source) > 1 {
            // Animated sources keep every frame at its own size.
            let options = ImageCreatingOptions(scale: scale)
            guard let image = KingfisherWrapper<UIImage>.image(data: data, options: options) else {
                return nil
            }
            return WhiskerLoadedImage(
                image: image,
                naturalSize: CGSize(
                    width: image.size.width * image.scale,
                    height: image.size.height * image.scale
                )
            )
        }
        let natural = naturalSize(of: source)
        // Enough pixels to cover the view in every mode, never more than the source has.
        var cover: CGFloat = 1
        if targetPixelSize.width > 0, targetPixelSize.height > 0,
           natural.width > 0, natural.height > 0 {
            cover = min(1, max(
                targetPixelSize.width / natural.width,
                targetPixelSize.height / natural.height
            ))
        }
        var options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            // Decodes here, on the work queue, instead of on the main thread at first display.
            kCGImageSourceShouldCacheImmediately: true,
        ]
        if cover < 1 {
            options[kCGImageSourceThumbnailMaxPixelSize] =
                (max(natural.width, natural.height) * cover).rounded(.up)
        }
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
            return nil
        }
        return WhiskerLoadedImage(
            image: UIImage(cgImage: image, scale: scale, orientation: .up),
            naturalSize: natural.width > 0
                ? natural
                : CGSize(width: image.width, height: image.height)
        )
    }

    private static func naturalSize(of source: CGImageSource) -> CGSize {
        guard let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? CGFloat,
              let height = properties[kCGImagePropertyPixelHeight] as? CGFloat
        else { return .zero }
        // EXIF orientations 5–8 display the image a quarter turn from its stored pixels.
        let orientation = properties[kCGImagePropertyOrientation] as? Int ?? 1
        return orientation >= 5 ? CGSize(width: height, height: width) : CGSize(width: width, height: height)
    }
}
#endif
