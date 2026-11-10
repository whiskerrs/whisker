import Foundation
import CryptoKit
import ImageIO

final class ManagedImageCache {
    static let shared = ManagedImageCache()
    let queue = DispatchQueue(label: "rs.whisker.image-cache", qos: .userInitiated)
    private var limit = 64 * 1024 * 1024
    private var used = 0
    private var entries: [String: CGImage] = [:]
    private var order: [String] = []
    private let root: URL
    private let fetch: ((URL, [String: String]) throws -> Data)?

    init(root: URL = FileManager.default.temporaryDirectory.appendingPathComponent("whisker-images", isDirectory: true),
         fetch: ((URL, [String: String]) throws -> Data)? = nil) {
        self.root = root
        self.fetch = fetch
    }

    var retainedBytes: Int { used }
    private lazy var session: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.urlCache = nil
        config.timeoutIntervalForRequest = 60
        config.timeoutIntervalForResource = 60
        return URLSession(configuration: config)
    }()

    struct Options: Decodable {
        let decode_width: Int?
        let decode_height: Int?
    }

    private func hash(_ url: String) -> String {
        SHA256.hash(data: Data(url.utf8)).map { String(format: "%02x", $0) }.joined()
    }

    private func cost(_ image: CGImage) -> Int { image.bytesPerRow * image.height }

    func configure(_ bytes: String) throws {
        guard let bytes = Int(bytes), bytes >= 0 else { throw failure("invalid image cache budget") }
        limit = bytes
        trim()
    }

    private func trim() {
        while used > limit, let oldest = order.first {
            order.removeFirst()
            if let image = entries.removeValue(forKey: oldest) { used -= cost(image) }
        }
    }

    func remove(_ url: String) throws {
        let prefix = hash(url)
        for key in order.filter({ $0.hasPrefix(prefix + ":") }) {
            if let image = entries.removeValue(forKey: key) { used -= cost(image) }
        }
        order.removeAll { $0.hasPrefix(prefix + ":") }
        let file = root.appendingPathComponent(prefix)
        if FileManager.default.fileExists(atPath: file.path) { try FileManager.default.removeItem(at: file) }
    }

    func load(_ value: String, headers: [String: String], json: String) throws -> CGImage {
        let options = try JSONDecoder().decode(Options.self, from: Data(json.utf8))
        guard options.decode_width.map({ $0 > 0 }) ?? true,
              options.decode_height.map({ $0 > 0 }) ?? true else { throw failure("decode bounds must be positive") }
        guard let url = URL(string: value), ["http", "https"].contains(url.scheme?.lowercased() ?? "") else {
            throw failure("managed image cache requires an HTTP(S) URL")
        }
        let digest = hash(value)
        let key = "\(digest):\(options.decode_width ?? 0):\(options.decode_height ?? 0)"
        if let image = entries[key] {
            order.removeAll { $0 == key }
            order.append(key)
            return image
        }
        let file = root.appendingPathComponent(digest)
        var image: CGImage?
        if let bytes = try? Data(contentsOf: file) {
            image = try? decode(bytes, options: options)
            if image == nil { try FileManager.default.removeItem(at: file) }
        }
        if image == nil {
            let bytes = try fetch?(url, headers) ?? download(url, headers: headers)
            image = try decode(bytes, options: options)
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            try bytes.write(to: file, options: .atomic)
        }
        guard let image else { throw failure("image decode failed") }
        let size = cost(image)
        if size <= limit {
            while used > limit - size, let oldest = order.first {
                order.removeFirst()
                if let removed = entries.removeValue(forKey: oldest) { used -= cost(removed) }
            }
            entries[key] = image
            order.append(key)
            used += size
        }
        return image
    }

    private func download(_ url: URL, headers: [String: String]) throws -> Data {
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 60)
        for (name, value) in headers { request.setValue(value, forHTTPHeaderField: name) }
        let semaphore = DispatchSemaphore(value: 0)
        var result: Result<Data, Error> = .failure(failure("image download did not complete"))
        let task = session.dataTask(with: request) { data, response, error in
            defer { semaphore.signal() }
            if let error { result = .failure(error); return }
            guard let response = response as? HTTPURLResponse, (200..<300).contains(response.statusCode), let data else {
                result = .failure(self.failure("image HTTP request failed")); return
            }
            guard data.count <= 64 * 1024 * 1024 else {
                result = .failure(self.failure("image exceeds 64 MiB download limit")); return
            }
            result = .success(data)
        }
        task.resume()
        semaphore.wait()
        return try result.get()
    }

    private func decode(_ data: Data, options: Options) throws -> CGImage {
        guard let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let rawWidth = properties[kCGImagePropertyPixelWidth] as? Int,
              let rawHeight = properties[kCGImagePropertyPixelHeight] as? Int,
              rawWidth > 0, rawHeight > 0 else { throw failure("unsupported or corrupt image") }
        let orientation = properties[kCGImagePropertyOrientation] as? Int ?? 1
        let rotated = (5...8).contains(orientation)
        let width = rotated ? rawHeight : rawWidth
        let height = rotated ? rawWidth : rawHeight
        let scale = min(1.0, min(Double(options.decode_width ?? width) / Double(width), Double(options.decode_height ?? height) / Double(height)))
        let targetWidth = max(1, Int((Double(width) * scale).rounded(.down)))
        let targetHeight = max(1, Int((Double(height) * scale).rounded(.down)))
        let settings: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: max(targetWidth, targetHeight),
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, settings as CFDictionary) else {
            throw failure("image thumbnail decode failed")
        }
        return image
    }

    private func failure(_ message: String) -> NSError {
        NSError(domain: "WhiskerImageCache", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
    }
}
