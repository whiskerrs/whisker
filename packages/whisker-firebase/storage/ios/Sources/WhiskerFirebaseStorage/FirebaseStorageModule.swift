import Foundation
import FirebaseStorage
import WhiskerModule

@WhiskerModule
public final class FirebaseStorageModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("FirebaseStorage")
            Function("useEmulator") { (args: [WhiskerValue]) -> WhiskerValue in
                guard args.count == 2, let host = args[0].asString,
                      let port = args[1].asInt, (1...65535).contains(port) else {
                    return Self.failure("invalid-argument", "Expected emulator host and port")
                }
                Storage.storage().useEmulator(withHost: host, port: Int(port))
                return Self.success(.null)
            }
            AsyncFunction("putBytes") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard args.count == 3, let reference = Self.reference(args[0]), case .bytes(let data) = args[1] else {
                    promise.resolve(Self.failure("invalid-argument", "Expected path, data, and metadata")); return
                }
                _ = reference.putData(data, metadata: Self.settable(args[2]), completion: Self.metadataCallback(promise))
            }
            AsyncFunction("putFile") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard args.count == 3, let reference = Self.reference(args[0]), let file = args[1].asString else {
                    promise.resolve(Self.failure("invalid-argument", "Expected path, file, and metadata")); return
                }
                _ = reference.putFile(from: URL(fileURLWithPath: file), metadata: Self.settable(args[2]),
                                  completion: Self.metadataCallback(promise))
            }
            AsyncFunction("getBytes") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard args.count == 2, let reference = Self.reference(args[0]), let max = args[1].asInt else {
                    promise.resolve(Self.failure("invalid-argument", "Expected path and size limit")); return
                }
                _ = reference.getData(maxSize: max) { data, error in
                    if let error { promise.resolve(Self.sdkFailure(error)); return }
                    promise.resolve(Self.success(.bytes(data ?? Data())))
                }
            }
            AsyncFunction("writeToFile") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard args.count == 2, let reference = Self.reference(args[0]), let file = args[1].asString else {
                    promise.resolve(Self.failure("invalid-argument", "Expected path and file")); return
                }
                _ = reference.write(toFile: URL(fileURLWithPath: file)) { _, error in
                    promise.resolve(error.map(Self.sdkFailure) ?? Self.success(.null))
                }
            }
            AsyncFunction("downloadUrl") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard let reference = args.first.flatMap(Self.reference) else {
                    promise.resolve(Self.failure("invalid-argument", "Expected a path")); return
                }
                reference.downloadURL { url, error in
                    if let error { promise.resolve(Self.sdkFailure(error)); return }
                    promise.resolve(Self.success(url.map { .string($0.absoluteString) } ?? .null))
                }
            }
            AsyncFunction("getMetadata") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard let reference = args.first.flatMap(Self.reference) else {
                    promise.resolve(Self.failure("invalid-argument", "Expected a path")); return
                }
                reference.getMetadata(completion: Self.metadataCallback(promise))
            }
            AsyncFunction("updateMetadata") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard args.count == 2, let reference = Self.reference(args[0]) else {
                    promise.resolve(Self.failure("invalid-argument", "Expected path and metadata")); return
                }
                // A fresh StorageMetadata would send `metadata: null` and clear custom metadata,
                // so apply the changes to the current metadata and let the SDK send the diff.
                let changes = args[1]
                reference.getMetadata { current, error in
                    if let error { promise.resolve(Self.sdkFailure(error)); return }
                    guard let current else { promise.resolve(Self.failure("-13000", "Missing metadata")); return }
                    reference.updateMetadata(Self.settable(changes, into: current), completion: Self.metadataCallback(promise))
                }
            }
            AsyncFunction("delete") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard let reference = args.first.flatMap(Self.reference) else {
                    promise.resolve(Self.failure("invalid-argument", "Expected a path")); return
                }
                reference.delete { error in promise.resolve(error.map(Self.sdkFailure) ?? Self.success(.null)) }
            }
            AsyncFunction("list") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard args.count == 3, let path = args[0].asString else {
                    promise.resolve(Self.failure("invalid-argument", "Expected path and paging")); return
                }
                let reference = path.isEmpty ? Storage.storage().reference() : Storage.storage().reference(withPath: path)
                let callback: (StorageListResult?, Error?) -> Void = { result, error in
                    if let error { promise.resolve(Self.sdkFailure(error)); return }
                    guard let result else { promise.resolve(Self.failure("-13000", "Missing list result")); return }
                    promise.resolve(Self.success(.map([
                        "items": .array(result.items.map { .string($0.fullPath) }),
                        "prefixes": .array(result.prefixes.map { .string($0.fullPath) }),
                        "next_page_token": result.pageToken.map { .string($0) } ?? .null,
                    ])))
                }
                guard let max = args[1].asInt else { reference.listAll(completion: callback); return }
                if let token = args[2].asString {
                    reference.list(maxResults: max, pageToken: token, completion: callback)
                } else {
                    reference.list(maxResults: max, completion: callback)
                }
            }
        }
    }

    private static func reference(_ wire: WhiskerValue) -> StorageReference? {
        guard let path = wire.asString, !path.isEmpty else { return nil }
        return Storage.storage().reference(withPath: path)
    }

    private static func settable(_ wire: WhiskerValue, into metadata: StorageMetadata = StorageMetadata()) -> StorageMetadata {
        guard case .map(let fields) = wire else { return metadata }
        if let value = fields["content_type"]?.asString { metadata.contentType = value }
        if let value = fields["cache_control"]?.asString { metadata.cacheControl = value }
        if let value = fields["content_disposition"]?.asString { metadata.contentDisposition = value }
        if let value = fields["content_encoding"]?.asString { metadata.contentEncoding = value }
        if let value = fields["content_language"]?.asString { metadata.contentLanguage = value }
        if case .map(let custom) = fields["custom_metadata"] {
            metadata.customMetadata = (metadata.customMetadata ?? [:]).merging(custom.compactMapValues(\.asString)) { $1 }
        }
        return metadata
    }

    private static func success(_ value: WhiskerValue) -> WhiskerValue { .map(["value": value]) }
    private static func failure(_ code: String, _ message: String) -> WhiskerValue {
        .map(["error": .map(["code": .string(code), "message": .string(message)])])
    }
    private static func sdkFailure(_ error: Error) -> WhiskerValue {
        let error = error as NSError
        let code = error.domain == StorageErrorDomain ? String(error.code) : "-13000"
        return failure(code, error.localizedDescription)
    }
    private static func metadataCallback(_ promise: WhiskerPromise) -> (StorageMetadata?, Error?) -> Void {
        { metadata, error in
            if let error { promise.resolve(sdkFailure(error)); return }
            guard let metadata else { promise.resolve(failure("-13000", "Missing metadata")); return }
            promise.resolve(success(encode(metadata)))
        }
    }

    private static func optional(_ value: String?) -> WhiskerValue { value.map { .string($0) } ?? .null }
    private static func millis(_ date: Date?) -> WhiskerValue {
        date.map { .int(Int64(($0.timeIntervalSince1970 * 1000).rounded())) } ?? .null
    }

    private static func encode(_ metadata: StorageMetadata) -> WhiskerValue {
        .map([
            "bucket": .string(metadata.bucket),
            "full_path": optional(metadata.path),
            "name": optional(metadata.name),
            "size": .int(metadata.size),
            "generation": .string(String(metadata.generation)),
            "md5_hash": optional(metadata.md5Hash),
            "time_created": millis(metadata.timeCreated),
            "updated": millis(metadata.updated),
            "content_type": optional(metadata.contentType),
            "cache_control": optional(metadata.cacheControl),
            "content_disposition": optional(metadata.contentDisposition),
            "content_encoding": optional(metadata.contentEncoding),
            "content_language": optional(metadata.contentLanguage),
            "custom_metadata": .map((metadata.customMetadata ?? [:]).mapValues { .string($0) }),
        ])
    }
}
