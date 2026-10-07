import Foundation
import CoreFoundation
import FirebaseFirestore
import WhiskerModule

@WhiskerModule
public final class FirestoreModule: Module {
    private let lock = NSLock()
    private var hasOperations = false
    private var emulatorEndpoint: String?
    private var listeners: [Int64: ListenerRegistration] = [:]

    private func database() -> Firestore {
        lock.lock()
        defer { lock.unlock() }
        hasOperations = true
        return Firestore.firestore()
    }

    private func configureEmulator(_ host: String, _ port: Int) -> WhiskerValue {
        lock.lock()
        defer { lock.unlock() }
        let endpoint = "\(host):\(port)"
        if emulatorEndpoint == endpoint { return Self.success(.null) }
        guard !hasOperations && emulatorEndpoint == nil else {
            return Self.failure("failed-precondition", "Configure the emulator once, before the first Firestore operation")
        }
        let database = Firestore.firestore()
        let settings = database.settings
        settings.host = endpoint
        // The emulator speaks plaintext gRPC; Apple's useEmulator only sets the host.
        settings.isSSLEnabled = false
        database.settings = settings
        emulatorEndpoint = endpoint
        return Self.success(.null)
    }

    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Firestore")
            Events("snapshot")
            Function("useEmulator") { (args: [WhiskerValue]) -> WhiskerValue in
                guard args.count == 2, let host = args[0].asString,
                      let port = args[1].asInt, (1...65535).contains(port) else {
                    return Self.failure("invalid-argument", "Expected emulator host and port")
                }
                return self.configureEmulator(host, Int(port))
            }
            Function("documentId") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let path = args.first?.asString else {
                    return Self.failure("invalid-argument", "Missing collection path")
                }
                return Self.success(.string(self.database().collection(path).document().documentID))
            }
            AsyncFunction("getDocument") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard args.count == 2, let path = args[0].asString, let source = args[1].asString else {
                    promise.resolve(Self.failure("invalid-argument", "Expected document path and source")); return
                }
                let database = self.database()
                database.document(path).getDocument(source: Self.source(source)) { snapshot, error in
                    if let error { promise.resolve(Self.sdkFailure(error)); return }
                    guard let snapshot else {
                        promise.resolve(Self.failure("invalid-response", "Missing snapshot")); return
                    }
                    promise.resolve(Self.catching { .map(["value": try Self.encodeDocument(snapshot, database)]) })
                }
            }
            AsyncFunction("getQuery") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard args.count == 2, let source = args[1].asString else {
                    promise.resolve(Self.failure("invalid-argument", "Expected query and source")); return
                }
                let database = self.database()
                do {
                    try Self.query(args[0], database).getDocuments(source: Self.source(source)) { snapshot, error in
                        if let error { promise.resolve(Self.sdkFailure(error)); return }
                        guard let snapshot else {
                            promise.resolve(Self.failure("invalid-response", "Missing snapshot")); return
                        }
                        promise.resolve(Self.catching { .map(["value": try Self.encodeQuery(snapshot, database)]) })
                    }
                } catch { promise.resolve(Self.failure("invalid-argument", error.localizedDescription)) }
            }
            AsyncFunction("count") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard let spec = args.first else {
                    promise.resolve(Self.failure("invalid-argument", "Missing query")); return
                }
                do {
                    try Self.query(spec, self.database()).count.getAggregation(source: .server) { snapshot, error in
                        if let error { promise.resolve(Self.sdkFailure(error)); return }
                        promise.resolve(Self.success(.int(snapshot?.count.int64Value ?? 0)))
                    }
                } catch { promise.resolve(Self.failure("invalid-argument", error.localizedDescription)) }
            }
            AsyncFunction("commit") { (args: [WhiskerValue], promise: WhiskerPromise) in
                guard case .array(let ops) = args.first else {
                    promise.resolve(Self.failure("invalid-argument", "Missing write operations")); return
                }
                let database = self.database()
                do {
                    let batch = database.batch()
                    for op in ops { try Self.apply(op, to: batch, database) }
                    batch.commit { error in
                        promise.resolve(error.map(Self.sdkFailure) ?? Self.success(.null))
                    }
                } catch { promise.resolve(Self.failure("invalid-argument", error.localizedDescription)) }
            }
            Function("listen") { (args: [WhiskerValue]) -> WhiskerValue in
                guard args.count == 3, let id = args[0].asInt, case .map(let target) = args[1],
                      let kind = target["kind"]?.asString, let includeMetadata = args[2].asBool else {
                    return Self.failure("invalid-argument", "Expected listener id, target, and options")
                }
                let database = self.database()
                let registration: ListenerRegistration
                switch kind {
                case "document":
                    guard let path = target["path"]?.asString else {
                        return Self.failure("invalid-argument", "Missing document path")
                    }
                    registration = database.document(path).addSnapshotListener(includeMetadataChanges: includeMetadata) { [weak self] snapshot, error in
                        self?.emit(id, error: error) { try Self.encodeDocument(snapshot!, database) }
                    }
                case "query":
                    do {
                        let query = try Self.query(target["query"] ?? .null, database)
                        registration = query.addSnapshotListener(includeMetadataChanges: includeMetadata) { [weak self] snapshot, error in
                            self?.emit(id, error: error) { try Self.encodeQuery(snapshot!, database) }
                        }
                    } catch { return Self.failure("invalid-argument", error.localizedDescription) }
                default:
                    return Self.failure("invalid-argument", "Unknown listener target")
                }
                self.lock.lock()
                self.listeners[id] = registration
                self.lock.unlock()
                return Self.success(.null)
            }
            Function("unlisten") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let id = args.first?.asInt else {
                    return Self.failure("invalid-argument", "Missing listener id")
                }
                self.lock.lock()
                let registration = self.listeners.removeValue(forKey: id)
                self.lock.unlock()
                registration?.remove()
                return Self.success(.null)
            }
            Function("unlistenAll") { (_: [WhiskerValue]) -> WhiskerValue in
                self.lock.lock()
                let registrations = self.listeners.values
                self.listeners.removeAll()
                self.lock.unlock()
                registrations.forEach { $0.remove() }
                return Self.success(.null)
            }
        }
    }

    private func emit(_ id: Int64, error: Error?, encode: @escaping () throws -> WhiskerValue) {
        let send = { [weak self] in
            guard let self else { return }
            self.lock.lock()
            let active = self.listeners[id] != nil
            self.lock.unlock()
            guard active else { return }
            var payload: [String: WhiskerValue]
            if let error {
                guard case .map(let failure) = Self.sdkFailure(error) else { return }
                payload = failure
            } else {
                guard case .map(let result) = Self.catching({ .map(["value": try encode()]) }) else { return }
                payload = result
            }
            payload["id"] = .int(id)
            self.sendEvent("snapshot", .map(payload))
        }
        if Thread.isMainThread { send() } else { DispatchQueue.main.async(execute: send) }
    }

    // MARK: - Results

    private static func success(_ value: WhiskerValue) -> WhiskerValue { .map(["value": value]) }
    private static func failure(_ code: String, _ message: String) -> WhiskerValue {
        .map(["error": .map(["code": .string(code), "message": .string(message)])])
    }
    private static func catching(_ body: () throws -> WhiskerValue) -> WhiskerValue {
        do { return try body() } catch { return failure("unsupported-value", error.localizedDescription) }
    }
    private static func sdkFailure(_ error: Error) -> WhiskerValue {
        let codes = [1: "cancelled", 2: "unknown", 3: "invalid-argument", 4: "deadline-exceeded",
                     5: "not-found", 6: "already-exists", 7: "permission-denied", 8: "resource-exhausted",
                     9: "failed-precondition", 10: "aborted", 11: "out-of-range", 12: "unimplemented",
                     13: "internal", 14: "unavailable", 15: "data-loss", 16: "unauthenticated"]
        let error = error as NSError
        let code = error.domain == FirestoreErrorDomain ? codes[error.code] ?? "unknown" : "unknown"
        return failure(code, error.localizedDescription)
    }
    private static func invalid(_ message: String = "Unsupported or malformed Firestore value") -> NSError {
        NSError(domain: "WhiskerFirebaseFirestore", code: 3, userInfo: [NSLocalizedDescriptionKey: message])
    }
    private static func source(_ name: String) -> FirestoreSource {
        switch name {
        case "server": return .server
        case "cache": return .cache
        default: return .default
        }
    }

    // MARK: - Snapshots

    private static func encodeDocument(_ snapshot: DocumentSnapshot, _ db: Firestore) throws -> WhiskerValue {
        .map([
            "path": .string(snapshot.reference.path),
            "data": try snapshot.data().map { .map(try encodeFields($0, db)) } ?? .null,
            "from_cache": .bool(snapshot.metadata.isFromCache),
            "has_pending_writes": .bool(snapshot.metadata.hasPendingWrites),
        ])
    }

    private static func encodeQuery(_ snapshot: QuerySnapshot, _ db: Firestore) throws -> WhiskerValue {
        let index = { (value: UInt) -> WhiskerValue in value == UInt(NSNotFound) ? .int(-1) : .int(Int64(value)) }
        let changes = try snapshot.documentChanges.map { change -> WhiskerValue in
            let type: String
            switch change.type {
            case .added: type = "added"
            case .modified: type = "modified"
            case .removed: type = "removed"
            @unknown default: throw invalid("Unknown document change type")
            }
            return .map([
                "type": .string(type),
                "old_index": index(change.oldIndex),
                "new_index": index(change.newIndex),
                "doc": try encodeDocument(change.document, db),
            ])
        }
        return .map([
            "docs": .array(try snapshot.documents.map { try encodeDocument($0, db) }),
            "changes": .array(changes),
            "from_cache": .bool(snapshot.metadata.isFromCache),
            "has_pending_writes": .bool(snapshot.metadata.hasPendingWrites),
        ])
    }

    // MARK: - Queries and writes

    private static func query(_ wire: WhiskerValue, _ db: Firestore) throws -> Query {
        guard case .map(let spec) = wire else { throw invalid("Malformed query") }
        var query: Query
        if let path = spec["path"]?.asString {
            query = db.collection(path)
        } else if let group = spec["group"]?.asString {
            query = db.collectionGroup(group)
        } else { throw invalid("Malformed query target") }
        if case .array(let filters) = spec["filters"] {
            for filter in filters {
                guard case .map(let f) = filter, let field = f["field"]?.asString,
                      let op = f["op"]?.asString, let wireValue = f["value"] else { throw invalid("Malformed filter") }
                let value = try decode(wireValue, db)
                let list = { () throws -> [Any] in
                    guard let list = value as? [Any] else { throw invalid("Filter needs an array") }
                    return list
                }
                switch op {
                case "==": query = query.whereField(field, isEqualTo: value)
                case "!=": query = query.whereField(field, isNotEqualTo: value)
                case "<": query = query.whereField(field, isLessThan: value)
                case "<=": query = query.whereField(field, isLessThanOrEqualTo: value)
                case ">": query = query.whereField(field, isGreaterThan: value)
                case ">=": query = query.whereField(field, isGreaterThanOrEqualTo: value)
                case "array-contains": query = query.whereField(field, arrayContains: value)
                case "array-contains-any": query = query.whereField(field, arrayContainsAny: try list())
                case "in": query = query.whereField(field, in: try list())
                case "not-in": query = query.whereField(field, notIn: try list())
                default: throw invalid("Unknown filter operator")
                }
            }
        }
        if case .array(let orders) = spec["order_by"] {
            for order in orders {
                guard case .map(let o) = order, let field = o["field"]?.asString,
                      let descending = o["descending"]?.asBool else { throw invalid("Malformed order") }
                query = query.order(by: field, descending: descending)
            }
        }
        if let limit = spec["limit"]?.asInt {
            query = spec["limit_to_last"]?.asBool == true ? query.limit(toLast: Int(limit)) : query.limit(to: Int(limit))
        }
        let cursor = { (key: String) throws -> ([Any], Bool)? in
            guard case .map(let c) = spec[key], case .array(let values) = c["values"],
                  let inclusive = c["inclusive"]?.asBool else { return nil }
            return (try values.map { try decode($0, db) }, inclusive)
        }
        if let (values, inclusive) = try cursor("start") {
            query = inclusive ? query.start(at: values) : query.start(after: values)
        }
        if let (values, inclusive) = try cursor("end") {
            query = inclusive ? query.end(at: values) : query.end(before: values)
        }
        return query
    }

    private static func apply(_ wire: WhiskerValue, to batch: WriteBatch, _ db: Firestore) throws {
        guard case .map(let op) = wire, let kind = op["op"]?.asString,
              let path = op["path"]?.asString else { throw invalid("Malformed write") }
        let document = db.document(path)
        let data = { () throws -> [String: Any] in
            guard case .map(let fields) = op["data"] else { throw invalid("Missing write data") }
            return try fields.mapValues { try decode($0, db) }
        }
        switch kind {
        case "set":
            if case .array(let fields) = op["merge_fields"] {
                batch.setData(try data(), forDocument: document, mergeFields: fields.compactMap(\.asString))
            } else {
                batch.setData(try data(), forDocument: document, merge: op["merge"]?.asBool == true)
            }
        case "update": batch.updateData(try data(), forDocument: document)
        case "delete": batch.deleteDocument(document)
        default: throw invalid("Unknown write")
        }
    }

    // MARK: - Values

    private static func tag(_ type: String, _ value: WhiskerValue) -> WhiskerValue {
        .map(["type": .string(type), "value": value])
    }
    private static func encodeFields(_ fields: [String: Any], _ db: Firestore) throws -> [String: WhiskerValue] {
        try fields.mapValues { try encode($0, db) }
    }
    private static func encode(_ value: Any, _ db: Firestore) throws -> WhiskerValue {
        switch value {
        case is NSNull: return tag("null", .null)
        case let v as NSNumber:
            if CFGetTypeID(v) == CFBooleanGetTypeID() { return tag("bool", .bool(v.boolValue)) }
            let type = String(cString: v.objCType)
            return type == "f" || type == "d" ? tag("double", .float(v.doubleValue)) : tag("integer", .int(v.int64Value))
        case let v as String: return tag("string", .string(v))
        case let v as Data: return tag("bytes", .bytes(v))
        case let v as Timestamp: return tag("timestamp", .array([.int(v.seconds), .int(Int64(v.nanoseconds))]))
        case let v as GeoPoint: return tag("geo_point", .array([.float(v.latitude), .float(v.longitude)]))
        case let v as DocumentReference:
            guard v.firestore === db else { throw invalid("Cross-database references are unsupported") }
            return tag("reference", .string(v.path))
        case let v as [Any]: return tag("array", .array(try v.map { try encode($0, db) }))
        case let v as [String: Any]: return tag("map", .map(try encodeFields(v, db)))
        default: throw invalid()
        }
    }
    private static func decode(_ wire: WhiskerValue, _ db: Firestore) throws -> Any {
        guard case .map(let fields) = wire, let type = fields["type"]?.asString,
              let value = fields["value"] else { throw invalid() }
        let list = { () throws -> [Any] in
            guard case .array(let values) = value else { throw invalid() }
            return try values.map { try decode($0, db) }
        }
        switch (type, value) {
        case ("null", .null): return NSNull()
        case ("bool", .bool(let v)): return v
        case ("integer", .int(let v)): return v
        case ("double", .float(let v)): return v
        case ("string", .string(let v)): return v
        case ("bytes", .bytes(let v)): return v
        case ("reference", .string(let v)): return db.document(v)
        case ("array", .array): return try list()
        case ("map", .map(let v)): return try v.mapValues { try decode($0, db) }
        case ("timestamp", .array(let v)):
            guard v.count == 2, case .int(let seconds) = v[0], case .int(let nanos) = v[1],
                  (-62135596800...253402300799).contains(seconds), (0..<1000000000).contains(nanos) else { throw invalid() }
            return Timestamp(seconds: seconds, nanoseconds: Int32(nanos))
        case ("geo_point", .array(let v)):
            guard v.count == 2, let latitude = v[0].asDouble, let longitude = v[1].asDouble,
                  (-90.0...90.0).contains(latitude), (-180.0...180.0).contains(longitude) else { throw invalid() }
            return GeoPoint(latitude: latitude, longitude: longitude)
        case ("server_timestamp", .null): return FieldValue.serverTimestamp()
        case ("delete", .null): return FieldValue.delete()
        case ("increment", _):
            switch try decode(value, db) {
            case let v as Int64: return FieldValue.increment(v)
            case let v as Double: return FieldValue.increment(v)
            default: throw invalid()
            }
        case ("array_union", .array): return FieldValue.arrayUnion(try list())
        case ("array_remove", .array): return FieldValue.arrayRemove(try list())
        default: throw invalid()
        }
    }
}
