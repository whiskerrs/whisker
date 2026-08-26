import CoreGraphics
import Foundation
import ImageIO

/// Encoded raster input acquired by the native Host outside frame transactions.
public enum WhiskerRasterResourceSource {
    case bytes(mediaType: String, data: Data)
    case url(String)
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
}

typealias HostResourceURLLoader = (
    URL,
    @escaping (Result<Data, Error>) -> Void
) -> () -> Void

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
        urlLoader: @escaping HostResourceURLLoader = HostResourceService.loadURL
    ) {
        self.store = store
        self.decodeQueue = decodeQueue
        self.urlLoader = urlLoader
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
                failIfCurrent(id: id, generation: generation)
                return true
            }
            decode(data, id: id, generation: generation)
        case let .url(value):
            guard let url = URL(string: value), let scheme = url.scheme?.lowercased() else {
                failIfCurrent(id: id, generation: generation)
                return true
            }
            if scheme == "data" {
                guard let data = Self.pngData(from: value) else {
                    failIfCurrent(id: id, generation: generation)
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
                                self.failIfCurrent(id: id, generation: generation)
                                return
                            }
                            self.decode(data, id: id, generation: generation)
                        case .failure:
                            self.failIfCurrent(id: id, generation: generation)
                        }
                    }
                }
            } else {
                failIfCurrent(id: id, generation: generation)
            }
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
                    self.failIfCurrent(id: id, generation: generation)
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

    private func failIfCurrent(id: UInt64, generation: UInt64) {
        guard currentGenerations[id] == generation else { return }
        pendingCancellations.removeValue(forKey: id)
        setState(.failed, id: id, generation: generation)
    }

    private func setState(_ state: WhiskerRasterResourceState, id: UInt64, generation: UInt64) {
        states[Key(id: id, generation: generation)] = state
        eventHandler?(WhiskerRasterResourceEvent(id: id, generation: generation, state: state))
    }

    private nonisolated static func pngData(from value: String) -> Data? {
        let prefix = "data:image/png;base64,"
        guard value.hasPrefix(prefix) else { return nil }
        return Data(base64Encoded: String(value.dropFirst(prefix.count)), options: [])
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
                completion(.failure(URLError(.badServerResponse)))
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
}
