// WhiskerValue — Swift mirror of the Rust `whisker::platform_module::
// WhiskerValue` tagged union. Used by `Module`-subclass methods as
// the universal arg/return type, replacing the previous `NSArray` +
// Foundation type marshalling.
//
// ## Why a Swift enum
//
// Phase 7-Φ.F: the platform_module bridge now exchanges typed Whisker
// values directly, not Foundation NSObject wrappers. Author code
// pattern-matches on enum cases instead of `as?` casting against
// NSNumber / NSString / NSData / NSArray / NSDictionary — fewer
// silent nil casts and exhaustive switch coverage.
//
// ## C ABI bridge
//
// The bridge layer (whisker_bridge_ios.mm) hands the Swift dispatch
// shim raw `WhiskerValueRaw` arrays. `WhiskerValue.decodeArray(_:
// count:)` walks the C struct array and produces `[WhiskerValue]`.
// `WhiskerValue.toRaw()` does the reverse, allocating heap-owned
// strings / bytes / nested arrays / maps via `malloc` (the bridge
// frees them after the dispatch returns).
//
// ## Discriminant alignment
//
// The `caseValue` mapping matches `WhiskerValueType` in
// `whisker_bridge.h`. Drift between the two would corrupt the
// payload union — the static `assert`s in `decode` / `toRaw` guard
// against silent regressions.

import Foundation
// `@_exported` so module-author Swift files can `import WhiskerRuntime`
// alone and still see the C ABI types (`WhiskerValueRaw`,
// `WhiskerStringRef`, `whisker_bridge_register_module_dispatch`, …)
// that the codegen-emitted `@_cdecl` dispatch shim references.
// Without re-exporting, every module .swift file would need its own
// `import WhiskerDriver`.
@_exported import WhiskerDriver

public enum WhiskerValue: Equatable {
    case null
    case bool(Bool)
    case int(Int64)
    case float(Double)
    case string(String)
    case bytes(Data)
    case array([WhiskerValue])
    case map([String: WhiskerValue])
    case error(String)
}

// MARK: - Convenience accessors (case ② arg destructuring)

/// Typed reads for module authors destructuring raw `WhiskerValue`
/// args (`args[0].asDouble()`, `value.asString()`, …). Numeric
/// reads coerce between `.int` / `.float`; everything else returns
/// `nil` on a case mismatch.
public extension WhiskerValue {
    var asString: String? {
        if case .string(let s) = self { return s }
        return nil
    }
    var asBool: Bool? {
        if case .bool(let b) = self { return b }
        return nil
    }
    var asInt: Int64? {
        switch self {
        case .int(let i): return i
        case .float(let f): return Int64(f)
        default: return nil
        }
    }
    var asDouble: Double? {
        switch self {
        case .float(let f): return f
        case .int(let i): return Double(i)
        default: return nil
        }
    }
    var asBytes: Data? {
        if case .bytes(let d) = self { return d }
        return nil
    }
}

// MARK: - WhiskerValueRaw <-> WhiskerValue

public extension WhiskerValue {
    /// Walk `count` `WhiskerValueRaw`s starting at `raw` and produce
    /// the corresponding Swift values. Returns `[]` on null pointer.
    ///
    /// Caller still owns the underlying C allocations — this method
    /// copies the data out into Swift-managed storage.
    static func decodeArray(
        _ raw: UnsafePointer<WhiskerValueRaw>?,
        count: Int
    ) -> [WhiskerValue] {
        guard let raw, count > 0 else { return [] }
        var out: [WhiskerValue] = []
        out.reserveCapacity(count)
        for i in 0..<count {
            out.append(WhiskerValue.from(raw: raw.advanced(by: i).pointee))
        }
        return out
    }

    /// Copy one `WhiskerValueRaw` into a Swift `WhiskerValue`.
    static func from(raw: WhiskerValueRaw) -> WhiskerValue {
        switch Int(raw.type) {
        case Int(WHISKER_VALUE_NULL.rawValue):
            return .null
        case Int(WHISKER_VALUE_BOOL.rawValue):
            return .bool(raw.v.b)
        case Int(WHISKER_VALUE_INT.rawValue):
            return .int(raw.v.i)
        case Int(WHISKER_VALUE_FLOAT.rawValue):
            return .float(raw.v.f)
        case Int(WHISKER_VALUE_STRING.rawValue):
            return .string(decodeString(raw.v.s))
        case Int(WHISKER_VALUE_BYTES.rawValue):
            return .bytes(decodeBytes(raw.v.bytes))
        case Int(WHISKER_VALUE_ARRAY.rawValue):
            return .array(decodeArray(raw.v.array.items, count: raw.v.array.count))
        case Int(WHISKER_VALUE_MAP.rawValue):
            return .map(decodeMap(raw.v.map))
        case Int(WHISKER_VALUE_ERROR.rawValue):
            return .error(decodeString(raw.v.s))
        default:
            return .error("WhiskerValueRaw carries unknown type \(raw.type)")
        }
    }

    /// Allocate a `WhiskerValueRaw` for this value. Heap allocations
    /// are bridge-owned — caller MUST eventually call
    /// `whisker_bridge_value_release` on the returned raw to free
    /// strings / bytes / nested arrays / maps.
    func toRaw() -> WhiskerValueRaw {
        var out = WhiskerValueRaw()
        switch self {
        case .null:
            out.type = UInt8(WHISKER_VALUE_NULL.rawValue)
        case .bool(let b):
            out.type = UInt8(WHISKER_VALUE_BOOL.rawValue)
            out.v.b = b
        case .int(let i):
            out.type = UInt8(WHISKER_VALUE_INT.rawValue)
            out.v.i = i
        case .float(let f):
            out.type = UInt8(WHISKER_VALUE_FLOAT.rawValue)
            out.v.f = f
        case .string(let s):
            out.type = UInt8(WHISKER_VALUE_STRING.rawValue)
            out.v.s = encodeString(s)
        case .bytes(let b):
            out.type = UInt8(WHISKER_VALUE_BYTES.rawValue)
            out.v.bytes = encodeBytes(b)
        case .array(let items):
            out.type = UInt8(WHISKER_VALUE_ARRAY.rawValue)
            out.v.array = encodeArray(items)
        case .map(let entries):
            out.type = UInt8(WHISKER_VALUE_MAP.rawValue)
            out.v.map = encodeMap(entries)
        case .error(let msg):
            out.type = UInt8(WHISKER_VALUE_ERROR.rawValue)
            out.v.s = encodeString(msg)
        }
        return out
    }
}

// MARK: - Encode helpers (Swift → WhiskerValueRaw heap-owned)

private func encodeString(_ s: String) -> WhiskerStringRef {
    let utf8 = s.utf8
    let count = utf8.count
    // `+1` for NUL terminator so the C side can also treat the buffer
    // as a C string when convenient.
    let buf = UnsafeMutablePointer<CChar>.allocate(capacity: count + 1)
    for (i, byte) in utf8.enumerated() {
        buf[i] = CChar(bitPattern: byte)
    }
    buf[count] = 0
    var out = WhiskerStringRef()
    out.ptr = UnsafePointer(buf)
    out.len = count
    return out
}

private func encodeBytes(_ data: Data) -> WhiskerBytesRef {
    let count = data.count
    let buf = UnsafeMutablePointer<UInt8>.allocate(capacity: count)
    data.copyBytes(to: buf, count: count)
    var out = WhiskerBytesRef()
    out.ptr = UnsafePointer(buf)
    out.len = count
    return out
}

private func encodeArray(_ items: [WhiskerValue]) -> WhiskerValueArray {
    let count = items.count
    let buf = UnsafeMutablePointer<WhiskerValueRaw>.allocate(capacity: max(count, 1))
    for (i, item) in items.enumerated() {
        buf[i] = item.toRaw()
    }
    var out = WhiskerValueArray()
    out.items = buf
    out.count = count
    return out
}

private func encodeMap(_ entries: [String: WhiskerValue]) -> WhiskerValueMap {
    // BTreeMap-like stable ordering — sort by key so two encodes of
    // equivalent maps produce byte-identical raw buffers (helps
    // snapshot tests + deterministic logs).
    let sorted = entries.sorted { $0.key < $1.key }
    let count = sorted.count
    let buf = UnsafeMutablePointer<WhiskerKeyValueRaw>.allocate(capacity: max(count, 1))
    for (i, (key, value)) in sorted.enumerated() {
        var entry = WhiskerKeyValueRaw()
        entry.key = encodeString(key)
        entry.value = value.toRaw()
        buf[i] = entry
    }
    var out = WhiskerValueMap()
    out.entries = buf
    out.count = count
    return out
}

// MARK: - Decode helpers (WhiskerValueRaw → Swift, copy)

private func decodeString(_ ref: WhiskerStringRef) -> String {
    guard let ptr = ref.ptr, ref.len > 0 else { return "" }
    let buf = UnsafeBufferPointer(
        start: UnsafeRawPointer(ptr).assumingMemoryBound(to: UInt8.self),
        count: ref.len
    )
    return String(decoding: buf, as: UTF8.self)
}

private func decodeBytes(_ ref: WhiskerBytesRef) -> Data {
    guard let ptr = ref.ptr, ref.len > 0 else { return Data() }
    return Data(bytes: ptr, count: ref.len)
}

private func decodeMap(_ map: WhiskerValueMap) -> [String: WhiskerValue] {
    guard let entries = map.entries, map.count > 0 else { return [:] }
    var out: [String: WhiskerValue] = [:]
    out.reserveCapacity(map.count)
    for i in 0..<map.count {
        let entry = entries.advanced(by: i).pointee
        let key = decodeString(entry.key)
        out[key] = WhiskerValue.from(raw: entry.value)
    }
    return out
}

// MARK: - NSDictionary <-> WhiskerValue (Phase 7-Φ.H.2)

public extension WhiskerValue {
    /// Decode the params `NSDictionary` Lynx's `LynxUIMethodProcessor`
    /// hands to a `@WhiskerUIMethod`-emitted dispatcher into the
    /// `[WhiskerValue]` shape user code expects.
    ///
    /// Convention: the C bridge packs the Rust-side `&[WhiskerValue]`
    /// into `{"args": [...]}` — a single key `args` holding an
    /// `NSArray` of positionally-encoded entries. Each entry decodes
    /// via [WhiskerValue.from(nsObject:)] (recursive). Missing key
    /// or non-array shape yields an empty list rather than an error
    /// so the user method still runs (with no args).
    ///
    /// The C bridge that produces this shape lives in
    /// `whisker_bridge_invoke_element_method` (Phase 7-Φ.H.2.5,
    /// tracked separately) — until that lands, callers receive an
    /// empty list.
    static func fromNSDictionary(_ params: NSDictionary?) -> [WhiskerValue] {
        guard let params, let raw = params["args"] else { return [] }
        guard let arr = raw as? [Any] else { return [] }
        return arr.map { WhiskerValue.from(nsObject: $0) }
    }

    /// Recursive `Any` -> `WhiskerValue` conversion for entries
    /// inside Lynx-supplied NSDictionary / NSArray payloads.
    /// `NSNumber` -> int / float / bool depending on its objCType;
    /// `NSDictionary` -> `.map`; `NSArray` -> `.array`; everything
    /// else falls through to `.error`.
    static func from(nsObject: Any) -> WhiskerValue {
        if nsObject is NSNull { return .null }
        if let n = nsObject as? NSNumber {
            // `NSNumber` doesn't distinguish bool / int / double at
            // the Swift bridging level — peek at its objCType to
            // choose the matching WhiskerValue variant. `c` is the
            // Obj-C bool encoding.
            let type = String(cString: n.objCType)
            switch type {
            case "c", "B": return .bool(n.boolValue)
            case "f", "d": return .float(n.doubleValue)
            default: return .int(n.int64Value)
            }
        }
        if let s = nsObject as? String { return .string(s) }
        if let d = nsObject as? Data { return .bytes(d) }
        if let arr = nsObject as? [Any] {
            return .array(arr.map { WhiskerValue.from(nsObject: $0) })
        }
        if let dict = nsObject as? [String: Any] {
            var out: [String: WhiskerValue] = [:]
            for (k, v) in dict { out[k] = WhiskerValue.from(nsObject: v) }
            return .map(out)
        }
        return .error("unsupported NSObject \(Swift.type(of: nsObject)) in @WhiskerUIMethod args")
    }

    /// Encode a `WhiskerValue` into an Obj-C-compatible `Any?`
    /// suitable for handing to Lynx's `LynxUIMethodCallbackBlock`
    /// (`callback(code, id _Nullable data)`). The bridge then passes
    /// the value back through Lynx's standard callback JNI path.
    ///
    /// `bytes` becomes `Data` (`NSData` on the Obj-C side). `error`
    /// becomes `["error": message]` since the callback already has
    /// the error-code channel for the failure signal.
    static func toAnyObject(_ value: WhiskerValue) -> Any? {
        switch value {
        case .null: return nil
        case .bool(let b): return NSNumber(value: b)
        case .int(let i): return NSNumber(value: i)
        case .float(let f): return NSNumber(value: f)
        case .string(let s): return s
        case .bytes(let d): return d
        case .array(let arr): return arr.map { WhiskerValue.toAnyObject($0) as Any }
        case .map(let map):
            var out: [String: Any] = [:]
            for (k, v) in map { out[k] = WhiskerValue.toAnyObject(v) as Any }
            return out
        case .error(let msg): return ["error": msg]
        }
    }
}
