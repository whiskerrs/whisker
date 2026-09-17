import UIKit
import WhiskerModule

final class TextMeasurementCache {
    // Compare owned, platform-neutral inputs before constructing UIFont,
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
    }

    struct Plain {
        let size: CGSize
        let ascender: Float
        let descender: Float
    }

    struct Paragraph {
        let layout: WhiskerParagraphLayout
        let size: CGSize
        let firstBaseline: CGFloat
        let lastBaseline: CGFloat
        let geometry: WhiskerValue

        init(layout: WhiskerParagraphLayout, paragraph: WhiskerParagraph) {
            self.layout = layout
            size = layout.size
            let baselines = layout.baselines
            firstBaseline = baselines.first
            lastBaseline = baselines.last
            geometry = layout.measurementGeometry(paragraph)
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

    private var entries: [Key: Entry] = [:]
    private let capacity = 256

    func removeAll() { entries.removeAll(keepingCapacity: true) }

    func paragraph(node: UInt64, input: Input, build: () throws -> Paragraph) rethrows -> Paragraph {
        let key = Key(node: node, width: input.width)
        if let entry = entries[key], entry.input == input, case .paragraph(let value) = entry.value {
            return value
        }
        let value = try build()
        insert(.paragraph(value), key: key, input: input)
        return value
    }

    func plain(node: UInt64, input: Input, build: () -> Plain) -> Plain {
        let key = Key(node: node, width: input.width)
        if let entry = entries[key], entry.input == input, case .plain(let value) = entry.value {
            return value
        }
        let value = build()
        insert(.plain(value), key: key, input: input)
        return value
    }

    private func insert(_ value: Value, key: Key, input: Input) {
        if entries.count >= capacity { removeAll() }
        entries[key] = Entry(input: input, value: value)
    }
}
