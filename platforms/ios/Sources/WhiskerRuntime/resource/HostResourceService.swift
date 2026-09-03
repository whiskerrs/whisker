import CoreGraphics
import Foundation
import ImageIO

/// Encoded raster input acquired by the native Host outside frame transactions.
public enum WhiskerRasterResourceSource {
    case bytes(mediaType: String, data: Data)
    case url(String)
    case bundledAsset(String)
}

public enum WhiskerRasterResourceFailureCode: Equatable {
    case notFound
    case denied
    case network
    case decode
    case cancelled
    case unsupported
}

/// Observable lifecycle state for one exact resource generation.
public enum WhiskerRasterResourceState: Equatable {
    case loading
    case ready(width: Int, height: Int)
    case failed
    case released
}

/// One generation-safe lifecycle notification emitted by the native Host.
public struct WhiskerRasterResourceEvent: Equatable {
    public let id: UInt64
    public let generation: UInt64
    public let state: WhiskerRasterResourceState
    public let failureCode: WhiskerRasterResourceFailureCode?
    public let diagnostic: String?
}

typealias HostResourceURLLoader = (
    URL,
    @escaping (Result<Data, Error>) -> Void
) -> () -> Void

typealias HostResourceAssetLoader = (String) -> Result<Data, Error>

private enum HostResourceAcquisitionError: Error {
    case notFound
    case denied
    case network
}

/// Acquires and decodes raster resources independently from frame application.
///
/// All retained state and store mutations are serialized on the main thread.
/// Acquisition and decoding run off-thread; their completions must still match
/// the latest generation before they can replace the paintable image.
@MainActor
final class HostResourceService {
    private struct Key: Hashable {
        let id: UInt64
        let generation: UInt64
    }

    private let store: HostResourceStore
    private let urlLoader: HostResourceURLLoader
    private let assetLoader: HostResourceAssetLoader
    private let decodeQueue: DispatchQueue
    private var latestGenerations = [UInt64: UInt64]()
    private var currentGenerations = [UInt64: UInt64]()
    private var installedGenerations = [UInt64: UInt64]()
    private var states = [Key: WhiskerRasterResourceState]()
    private var pendingCancellations = [UInt64: () -> Void]()

    var eventHandler: ((WhiskerRasterResourceEvent) -> Void)?

    init(
        store: HostResourceStore,
        decodeQueue: DispatchQueue = DispatchQueue(
            label: "dev.whisker.resource-decode",
            qos: .userInitiated
        ),
        urlLoader: @escaping HostResourceURLLoader = HostResourceService.loadURL,
        assetLoader: @escaping HostResourceAssetLoader = HostResourceService.loadBundledAsset
    ) {
        self.store = store
        self.decodeQueue = decodeQueue
        self.urlLoader = urlLoader
        self.assetLoader = assetLoader
    }

    @discardableResult
    func load(id: UInt64, generation: UInt64, source: WhiskerRasterResourceSource) -> Bool {
        guard id != 0, generation != 0,
              generation > latestGenerations[id, default: 0] else { return false }

        pendingCancellations.removeValue(forKey: id)?()
        latestGenerations[id] = generation
        currentGenerations[id] = generation
        setState(.loading, id: id, generation: generation)

        switch source {
        case let .bytes(mediaType, data):
            guard !mediaType.isEmpty, mediaType.lowercased().hasPrefix("image/"), !data.isEmpty else {
                failIfCurrent(id: id, generation: generation, code: .unsupported)
                return true
            }
            decode(data, id: id, generation: generation)
        case let .url(value):
            guard let url = URL(string: value), let scheme = url.scheme?.lowercased() else {
                failIfCurrent(id: id, generation: generation, code: .unsupported)
                return true
            }
            if scheme == "data" {
                guard let data = Self.imageData(fromDataURL: value) else {
                    failIfCurrent(id: id, generation: generation, code: .decode)
                    return true
                }
                decode(data, id: id, generation: generation)
            } else if scheme == "http" || scheme == "https" {
                pendingCancellations[id] = urlLoader(url) { [weak self] result in
                    DispatchQueue.main.async {
                        guard let self,
                              self.currentGenerations[id] == generation else { return }
                        self.pendingCancellations.removeValue(forKey: id)
                        switch result {
                        case let .success(data):
                            guard !data.isEmpty else {
                                self.failIfCurrent(id: id, generation: generation, code: .network)
                                return
                            }
                            self.decode(data, id: id, generation: generation)
                        case let .failure(error):
                            self.failIfCurrent(
                                id: id,
                                generation: generation,
                                code: Self.failureCode(for: error),
                                diagnostic: error.localizedDescription
                            )
                        }
                    }
                }
            } else {
                failIfCurrent(id: id, generation: generation, code: .unsupported)
            }
        case let .bundledAsset(identifier):
            guard !identifier.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                failIfCurrent(id: id, generation: generation, code: .notFound)
                return true
            }
            loadBundledAsset(identifier, id: id, generation: generation)
        }
        return true
    }

    @discardableResult
    func release(id: UInt64, generation: UInt64) -> Bool {
        let key = Key(id: id, generation: generation)
        guard id != 0, generation != 0, states[key] != nil else { return false }

        if currentGenerations[id] == generation {
            pendingCancellations.removeValue(forKey: id)?()
            currentGenerations.removeValue(forKey: id)
        }
        if installedGenerations[id] == generation {
            store.removeRasterImage(id: id)
            installedGenerations.removeValue(forKey: id)
        }
        setState(.released, id: id, generation: generation)
        return true
    }

    func state(id: UInt64, generation: UInt64) -> WhiskerRasterResourceState? {
        states[Key(id: id, generation: generation)]
    }

    private func decode(_ data: Data, id: UInt64, generation: UInt64) {
        decodeQueue.async { [weak self] in
            let image = Self.decodeImage(data)
            DispatchQueue.main.async {
                guard let self,
                      self.currentGenerations[id] == generation,
                      self.states[Key(id: id, generation: generation)] == .loading else { return }
                guard let image, self.store.registerRasterImage(image, id: id) else {
                    self.failIfCurrent(id: id, generation: generation, code: .decode)
                    return
                }
                self.installedGenerations[id] = generation
                self.setState(
                    .ready(width: image.width, height: image.height),
                    id: id,
                    generation: generation
                )
            }
        }
    }

    private func loadBundledAsset(_ identifier: String, id: UInt64, generation: UInt64) {
        let loader = assetLoader
        decodeQueue.async { [weak self] in
            let result = loader(identifier)
            DispatchQueue.main.async {
                guard let self, self.currentGenerations[id] == generation else { return }
                switch result {
                case let .success(data):
                    guard !data.isEmpty else {
                        self.failIfCurrent(id: id, generation: generation, code: .notFound)
                        return
                    }
                    self.decode(data, id: id, generation: generation)
                case let .failure(error):
                    self.failIfCurrent(
                        id: id,
                        generation: generation,
                        code: Self.failureCode(for: error),
                        diagnostic: error.localizedDescription
                    )
                }
            }
        }
    }

    private func failIfCurrent(
        id: UInt64,
        generation: UInt64,
        code: WhiskerRasterResourceFailureCode,
        diagnostic: String? = nil
    ) {
        guard currentGenerations[id] == generation else { return }
        pendingCancellations.removeValue(forKey: id)
        setState(
            .failed,
            id: id,
            generation: generation,
            failureCode: code,
            diagnostic: diagnostic
        )
    }

    private func setState(
        _ state: WhiskerRasterResourceState,
        id: UInt64,
        generation: UInt64,
        failureCode: WhiskerRasterResourceFailureCode? = nil,
        diagnostic: String? = nil
    ) {
        states[Key(id: id, generation: generation)] = state
        eventHandler?(WhiskerRasterResourceEvent(
            id: id,
            generation: generation,
            state: state,
            failureCode: failureCode,
            diagnostic: diagnostic
        ))
    }

    private nonisolated static func imageData(fromDataURL value: String) -> Data? {
        guard value.count > 5, value.prefix(5).lowercased() == "data:",
              let comma = value.firstIndex(of: ",") else { return nil }
        let metadata = value[value.index(value.startIndex, offsetBy: 5)..<comma]
        let fields = metadata.split(separator: ";", omittingEmptySubsequences: false)
        guard let mediaType = fields.first,
              mediaType.lowercased().hasPrefix("image/"),
              fields.dropFirst().contains(where: { $0.lowercased() == "base64" }) else {
            return nil
        }
        let encoded = String(value[value.index(after: comma)...])
        guard let data = Data(base64Encoded: encoded, options: [.ignoreUnknownCharacters]),
              !data.isEmpty else { return nil }
        return data
    }

    private nonisolated static func decodeImage(_ data: Data) -> CGImage? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        return CGImageSourceCreateImageAtIndex(source, 0, nil)
    }

    private nonisolated static func loadURL(
        _ url: URL,
        completion: @escaping (Result<Data, Error>) -> Void
    ) -> () -> Void {
        let task = URLSession.shared.dataTask(with: url) { data, response, error in
            if let error {
                completion(.failure(error))
                return
            }
            if let response = response as? HTTPURLResponse,
               !(200...299).contains(response.statusCode) {
                if response.statusCode == 404 {
                    completion(.failure(HostResourceAcquisitionError.notFound))
                } else if response.statusCode == 401 || response.statusCode == 403 {
                    completion(.failure(HostResourceAcquisitionError.denied))
                } else {
                    completion(.failure(HostResourceAcquisitionError.network))
                }
                return
            }
            guard let data else {
                completion(.failure(URLError(.zeroByteResource)))
                return
            }
            completion(.success(data))
        }
        task.resume()
        return { task.cancel() }
    }

    private nonisolated static func loadBundledAsset(_ identifier: String) -> Result<Data, Error> {
        let path = identifier as NSString
        let file = path.lastPathComponent as NSString
        let extensionName = file.pathExtension
        let name = extensionName.isEmpty ? file as String : file.deletingPathExtension
        let directory = path.deletingLastPathComponent
        guard let url = Bundle.main.url(
            forResource: name,
            withExtension: extensionName.isEmpty ? nil : extensionName,
            subdirectory: directory.isEmpty || directory == "." ? nil : directory
        ) else {
            return .failure(HostResourceAcquisitionError.notFound)
        }
        do {
            return .success(try Data(contentsOf: url, options: .mappedIfSafe))
        } catch {
            return .failure(error)
        }
    }

    private nonisolated static func failureCode(
        for error: Error
    ) -> WhiskerRasterResourceFailureCode {
        if let error = error as? HostResourceAcquisitionError {
            switch error {
            case .notFound: return .notFound
            case .denied: return .denied
            case .network: return .network
            }
        }
        if let error = error as? URLError, error.code == .cancelled { return .cancelled }
        return .network
    }
}
