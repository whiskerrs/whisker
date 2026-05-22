// WhiskerValue — Swift mirror of the Rust `whisker::native_module::
// WhiskerValue` tagged union. Used by `@WhiskerModule`-annotated
// classes' methods as the universal arg/return type, replacing the
// previous `NSArray` + Foundation type marshalling.
//
// ## Why a Swift enum
//
// Phase 7-Φ.F: the native_module bridge now exchanges typed Whisker
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
import WhiskerDriver

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
        switch Int(raw.type_) {
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
            return .error("WhiskerValueRaw carries unknown type \(raw.type_)")
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
            out.type_ = UInt8(WHISKER_VALUE_NULL.rawValue)
        case .bool(let b):
            out.type_ = UInt8(WHISKER_VALUE_BOOL.rawValue)
            out.v.b = b
        case .int(let i):
            out.type_ = UInt8(WHISKER_VALUE_INT.rawValue)
            out.v.i = i
        case .float(let f):
            out.type_ = UInt8(WHISKER_VALUE_FLOAT.rawValue)
            out.v.f = f
        case .string(let s):
            out.type_ = UInt8(WHISKER_VALUE_STRING.rawValue)
            out.v.s = encodeString(s)
        case .bytes(let b):
            out.type_ = UInt8(WHISKER_VALUE_BYTES.rawValue)
            out.v.bytes = encodeBytes(b)
        case .array(let items):
            out.type_ = UInt8(WHISKER_VALUE_ARRAY.rawValue)
            out.v.array = encodeArray(items)
        case .map(let entries):
            out.type_ = UInt8(WHISKER_VALUE_MAP.rawValue)
            out.v.map = encodeMap(entries)
        case .error(let msg):
            out.type_ = UInt8(WHISKER_VALUE_ERROR.rawValue)
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
