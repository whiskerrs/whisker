import CoreText
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif
import WhiskerModule

final class TextMeasurementCache {
    // Compare owned, platform-neutral inputs before constructing WhiskerPlatformFont,
    // NSParagraphStyle or bridged attribute dictionaries. No FFI pointers live
    // beyond the request that supplied them.
    struct Style: Equatable {
        struct Feature: Equatable { let tag: [UInt8]; let value: UInt32 }
        struct Variation: Equatable { let tag: [UInt8]; let value: Float }
        let families: [String]
        let size: Float
        let weight: UInt16
        let fontStyle: UInt8
        let opticalSizing: UInt8
        let features: [Feature]
        let variations: [Variation]
        let lineHeight: Float
        let letterSpacing: Float
        let indent: CGFloat
        let direction: UInt8
        let alignment: UInt8

        init(_ request: WhiskerMobileMeasureRequest, families: [String], widthBasis: CGFloat) {
            self.families = families
            size = request.font_size
            weight = request.font_weight
            fontStyle = request.font_style
            opticalSizing = request.font_optical_sizing
            features = request.font_features.map { pointer in
                UnsafeBufferPointer(start: pointer, count: request.font_feature_count).map {
                    Feature(tag: withUnsafeBytes(of: $0.tag) { Array($0) }, value: $0.value)
                }
            } ?? []
            variations = request.font_variations.map { pointer in
                UnsafeBufferPointer(start: pointer, count: request.font_variation_count).map {
                    Variation(tag: withUnsafeBytes(of: $0.tag) { Array($0) }, value: $0.value)
                }
            } ?? []
            lineHeight = request.line_height
            letterSpacing = request.letter_spacing
            indent = CGFloat(request.indent_logical_pixels) + widthBasis * CGFloat(request.indent_percentage) / 100
            direction = request.direction
            alignment = request.alignment
        }
    }

    struct Input: Equatable {
        let source: String
        let payload: WhiskerValue?
        let style: Style
        let width: CGFloat
        let maxLines: Int
        let overflow: UInt8
        let wordBreak: UInt8
        let environmentEpoch: UInt64

        func differsOnlyInWidth(from other: Input) -> Bool {
            source == other.source && payload == other.payload && style == other.style
                && maxLines == other.maxLines && overflow == other.overflow
                && wordBreak == other.wordBreak && environmentEpoch == other.environmentEpoch
        }
    }

    struct Plain {
        let layout: WhiskerParagraphLayout?
        let size: CGSize
        let firstBaseline: CGFloat
        let lastBaseline: CGFloat

        /// Whether laying the same text out at `width` would break it into these same lines.
        func holds(at width: CGFloat) -> Bool {
            guard let layout else { return false }
            let manager = layout.manager
            if width <= layout.container.size.width {
                return manager.usedRect(for: layout.container).maxX <= width
            }
            let glyphs = manager.glyphRange(for: layout.container)
            guard glyphs.length > 0 else { return false }
            var lines = 0
            manager.enumerateLineFragments(forGlyphRange: glyphs) { _, _, _, _, stop in
                lines += 1
                if lines > 1 { stop.pointee = true }
            }
            let shown = manager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
            return lines == 1 && shown.length == layout.storage.length
                && manager.truncatedGlyphRange(inLineFragmentForGlyphAt: NSMaxRange(glyphs) - 1).location == NSNotFound
        }
    }

    struct Paragraph {
        let layout: WhiskerParagraphLayout
        let size: CGSize
        let firstBaseline: CGFloat
        let lastBaseline: CGFloat
        let geometry: RawGeometry

        init(layout: WhiskerParagraphLayout, paragraph: WhiskerParagraph) {
            self.layout = layout
            size = layout.size
            let baselines = layout.baselines
            firstBaseline = baselines.first
            lastBaseline = baselines.last
            geometry = RawGeometry(layout.measurementGeometry(paragraph))
        }
    }

    private struct Key: Hashable {
        let node: UInt64
        let width: CGFloat
    }

    private enum Value {
        case paragraph(Paragraph)
        case plain(Plain)
    }

    private struct Entry {
        let input: Input
        let value: Value
    }

    // WhiskerView empties this every frame, so a cap would only evict a large frame's own constraint passes.
    private var entries: [Key: Entry] = [:]
    private var plainWidths: [UInt64: [CGFloat]] = [:]

    private struct ProbeKey: Hashable {
        let input: Input

        func hash(into hasher: inout Hasher) {
            hasher.combine(input.source)
            hasher.combine(input.style.size)
            hasher.combine(input.style.weight)
        }
    }

    /// Min-content sizes by text and style: no node draws that layout, so one answer serves every node and frame.
    private var probes: [ProbeKey: Plain] = [:]
    private static let probeCapacity = 4_096
    private var registeredFontsObserver: NSObjectProtocol?

    init() {
        // A family name registered later resolves to another font than the one a probe was measured with.
        registeredFontsObserver = NotificationCenter.default.addObserver(
            forName: kCTFontManagerRegisteredFontsChangedNotification as NSNotification.Name,
            object: nil, queue: .main
        ) { [weak self] _ in self?.probes.removeAll() }
    }

    deinit {
        if let registeredFontsObserver { NotificationCenter.default.removeObserver(registeredFontsObserver) }
    }

    func removeAll() {
        entries.removeAll(keepingCapacity: true)
        plainWidths.removeAll(keepingCapacity: true)
    }

    func paragraph(node: UInt64, input: Input, build: () throws -> Paragraph) rethrows -> Paragraph {
        let key = Key(node: node, width: input.width)
        if let entry = entries[key], entry.input == input, case .paragraph(let value) = entry.value {
            return value
        }
        let value = try build()
        insert(.paragraph(value), key: key, input: input)
        return value
    }

    func plain(node: UInt64, input: Input, probe: Bool, build: () -> Plain) -> Plain {
        let key = Key(node: node, width: input.width)
        if let entry = entries[key], entry.input == input, case .plain(let value) = entry.value {
            return value
        }
        if probe {
            let probeKey = ProbeKey(input: input)
            if let value = probes[probeKey] { return value }
            let built = build()
            let value = Plain(layout: nil, size: built.size, firstBaseline: built.firstBaseline, lastBaseline: built.lastBaseline)
            if probes.count >= Self.probeCapacity { probes.removeAll(keepingCapacity: true) }
            probes[probeKey] = value
            return value
        }
        for width in plainWidths[node] ?? [] {
            if let entry = entries[Key(node: node, width: width)], case .plain(let value) = entry.value,
               entry.input.differsOnlyInWidth(from: input), value.holds(at: input.width) {
                return value
            }
        }
        let value = build()
        insert(.plain(value), key: key, input: input)
        plainWidths[node, default: []].append(input.width)
        return value
    }

    private func insert(_ value: Value, key: Key, input: Input) {
        entries[key] = Entry(input: input, value: value)
    }
}

/// A geometry tree encoded once; each response receives a copy Rust may release.
final class RawGeometry {
    private var raw: WhiskerValueRaw

    init(_ value: WhiskerValue) { raw = value.toRaw() }

    deinit { WhiskerValue.releaseRaw(&raw) }

    func copy() -> WhiskerValueRaw { copyRaw(raw) }
}

/// Deep-copies with the Swift allocator that `WhiskerValue.releaseRaw` frees.
private func copyRaw(_ raw: WhiskerValueRaw) -> WhiskerValueRaw {
    var out = raw
    switch Int(raw.type) {
    case Int(WHISKER_VALUE_STRING), Int(WHISKER_VALUE_ERROR):
        out.v.s = copyString(raw.v.s)
    case Int(WHISKER_VALUE_BYTES):
        if let source = raw.v.bytes.ptr {
            let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: raw.v.bytes.len)
            buffer.update(from: source, count: raw.v.bytes.len)
            out.v.bytes.ptr = UnsafePointer(buffer)
        }
    case Int(WHISKER_VALUE_ARRAY):
        if let source = raw.v.array.items {
            let buffer = UnsafeMutablePointer<WhiskerValueRaw>.allocate(capacity: max(raw.v.array.count, 1))
            for index in 0..<raw.v.array.count {
                buffer[index] = copyRaw(source[index])
            }
            out.v.array.items = buffer
        }
    case Int(WHISKER_VALUE_MAP):
        if let source = raw.v.map.entries {
            let buffer = UnsafeMutablePointer<WhiskerKeyValueRaw>.allocate(capacity: max(raw.v.map.count, 1))
            for index in 0..<raw.v.map.count {
                var entry = WhiskerKeyValueRaw()
                entry.key = copyString(source[index].key)
                entry.value = copyRaw(source[index].value)
                buffer[index] = entry
            }
            out.v.map.entries = buffer
        }
    default:
        break
    }
    return out
}

private func copyString(_ string: WhiskerStringRef) -> WhiskerStringRef {
    guard let source = string.ptr else { return string }
    let buffer = UnsafeMutablePointer<CChar>.allocate(capacity: string.len + 1)
    buffer.update(from: source, count: string.len)
    buffer[string.len] = 0
    var out = WhiskerStringRef()
    out.ptr = UnsafePointer(buffer)
    out.len = string.len
    return out
}
