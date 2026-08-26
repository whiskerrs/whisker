import Foundation
import WhiskerModule

enum HostResourceKind: UInt32 {
    case rasterImage = 1
    case vectorImage = 2
    case font = 3
    case cursor = 4
    case paintServer = 5
}

enum HostResourceCommand {
    case load(
        id: UInt64,
        generation: UInt64,
        kind: HostResourceKind,
        source: WhiskerRasterResourceSource
    )
    case release(id: UInt64, generation: UInt64)
}

/// Copies every borrowed command member before the Rust callback returns.
func hostResourceCommand(_ raw: WhiskerMobileResourceCommand) -> HostResourceCommand? {
    guard raw.resource != 0, raw.generation != 0, raw._reserved == 0 else { return nil }
    switch raw.command {
    case UInt32(WHISKER_RESOURCE_COMMAND_LOAD):
        guard let kind = HostResourceKind(rawValue: raw.kind) else { return nil }
        let source: WhiskerRasterResourceSource
        switch raw.source {
        case UInt32(WHISKER_RESOURCE_SOURCE_URL):
            guard raw.data.len == 0, let value = strictHostString(raw.identifier), !value.isEmpty else {
                return nil
            }
            source = .url(value)
        case UInt32(WHISKER_RESOURCE_SOURCE_BUNDLED_ASSET):
            guard raw.data.len == 0, let value = strictHostString(raw.identifier), !value.isEmpty else {
                return nil
            }
            source = .bundledAsset(value)
        case UInt32(WHISKER_RESOURCE_SOURCE_BYTES):
            guard let mediaType = strictHostString(raw.identifier), !mediaType.isEmpty,
                  let data = copiedHostBytes(raw.data), !data.isEmpty else { return nil }
            source = .bytes(mediaType: mediaType, data: data)
        default:
            return nil
        }
        return .load(id: raw.resource, generation: raw.generation, kind: kind, source: source)
    case UInt32(WHISKER_RESOURCE_COMMAND_RELEASE):
        guard raw.kind == 0, raw.source == UInt32(WHISKER_RESOURCE_SOURCE_NONE),
              raw.identifier.len == 0, raw.data.len == 0 else { return nil }
        return .release(id: raw.resource, generation: raw.generation)
    default:
        return nil
    }
}

func withMobileResourceEvent<Result>(
    _ event: WhiskerRasterResourceEvent,
    _ body: (inout WhiskerMobileResourceEvent) -> Result
) -> Result? {
    var raw = WhiskerMobileResourceEvent()
    raw.resource = event.id
    raw.generation = event.generation
    switch event.state {
    case let .ready(width, height):
        raw.status = UInt32(WHISKER_RESOURCE_EVENT_READY)
        raw.failure_code = UInt32(WHISKER_RESOURCE_FAILURE_NONE)
        raw.width = Float(width)
        raw.height = Float(height)
        raw.scale = 1
        raw.dimensions_mask = UInt32(WHISKER_RESOURCE_DIMENSIONS_PRESENT)
        return body(&raw)
    case .failed:
        guard let failureCode = event.failureCode else { return nil }
        raw.status = UInt32(WHISKER_RESOURCE_EVENT_FAILED)
        raw.failure_code = mobileFailureCode(failureCode)
        raw.dimensions_mask = 0
        if let diagnostic = event.diagnostic, !diagnostic.isEmpty {
            return diagnostic.utf8CString.withUnsafeBufferPointer { buffer in
                raw.diagnostic = WhiskerStringRef(
                    ptr: buffer.baseAddress,
                    len: max(0, buffer.count - 1)
                )
                return body(&raw)
            }
        }
        return body(&raw)
    case .loading, .released:
        return nil
    }
}

private func strictHostString(_ value: WhiskerStringRef) -> String? {
    guard value.len > 0 else { return "" }
    guard let pointer = value.ptr else { return nil }
    return String(
        bytes: UnsafeBufferPointer(
            start: UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self),
            count: value.len
        ),
        encoding: .utf8
    )
}

private func copiedHostBytes(_ value: WhiskerBytesRef) -> Data? {
    guard value.len > 0 else { return Data() }
    guard let pointer = value.ptr else { return nil }
    return Data(bytes: pointer, count: value.len)
}

private func mobileFailureCode(_ code: WhiskerRasterResourceFailureCode) -> UInt32 {
    switch code {
    case .notFound: return UInt32(WHISKER_RESOURCE_FAILURE_NOT_FOUND)
    case .denied: return UInt32(WHISKER_RESOURCE_FAILURE_DENIED)
    case .network: return UInt32(WHISKER_RESOURCE_FAILURE_NETWORK)
    case .decode: return UInt32(WHISKER_RESOURCE_FAILURE_DECODE)
    case .cancelled: return UInt32(WHISKER_RESOURCE_FAILURE_CANCELLED)
    case .unsupported: return UInt32(WHISKER_RESOURCE_FAILURE_UNSUPPORTED)
    }
}
