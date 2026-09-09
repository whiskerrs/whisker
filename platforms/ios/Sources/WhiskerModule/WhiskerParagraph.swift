import CoreText
import UIKit

public struct WhiskerParagraph {
    public let attachments: [WhiskerInlineAttachment]
    private let runs: [ParagraphRun]
    public let source: String
    public private(set) var visibleEnd: Int?
    public var sourceRanges: [NSRange] {
        let visible = NSRange(location: 0, length: visibleEnd ?? (source as NSString).length)
        return runs.map { NSIntersectionRange($0.range, visible) }.filter { $0.length > 0 }
    }
    public var accessibleText: String { (source as NSString).substring(to: visibleEnd ?? (source as NSString).length).replacingOccurrences(of: "\u{FFFC}", with: "") }
    var accessibleActions: [(UInt64, String)] {
        var actions: [(UInt64, String)] = []
        let visible = NSRange(location: 0, length: visibleEnd ?? (source as NSString).length)
        for run in runs {
            guard let action = run.action else { continue }
            let range = NSIntersectionRange(run.range, visible)
            guard range.length > 0 else { continue }
            let label = (source as NSString).substring(with: range).replacingOccurrences(of: "\u{FFFC}", with: "")
            guard !label.isEmpty else { continue }
            if let index = actions.firstIndex(where: { $0.0 == action }) { actions[index].1 += label }
            else { actions.append((action, label)) }
        }
        return actions
    }
    public func withVisibleEnd(_ end: Int) -> WhiskerParagraph { var result = self; result.visibleEnd = end; return result }
    public func copyText(_ range: NSRange) -> String {
        let source = source as NSString
        let limit = visibleEnd ?? source.length
        let first = min(range.location, limit)
        let last = min(NSMaxRange(range), limit)
        var result = ""
        var cursor = first
        for attachment in attachments where !attachment.truncation && attachment.offset >= first && attachment.offset < last {
            result += source.substring(with: NSRange(location: cursor, length: attachment.offset - cursor))
            result += attachment.label ?? ""
            cursor = attachment.offset + 1
        }
        return result + source.substring(with: NSRange(location: cursor, length: max(0, last - cursor)))
    }

    public init(value: WhiskerValue, text: String) throws {
        source = text
        let fields = try value.paragraphFields()
        visibleEnd = fields["visibleEnd"]?.asInt.map(Int.init)
        guard try fields.integer("version") == 1 else { throw ParagraphError.invalidContent }
        let units = Array(text.utf16)
        var previousEnd = 0
        attachments = try (fields["attachments"] ?? .array([])).paragraphItems().map { raw in
            let fields = try raw.paragraphFields()
            let start = try fields.integer("start")
            let end = try fields.integer("end")
            let width = try fields.number("width")
            let height = try fields.number("height")
            let alignment = try fields.integer("alignment")
            let truncation = fields["truncation"]?.asBool == true
            let validRange = truncation ? start == units.count && end == start : start >= 0 && start < units.count && end == start + 1 && units[start] == 0xFFFC
            guard validRange,
                  width >= 0, height >= 0, alignment <= 4,
                  case .int(let node) = try fields.required("node"), node > 0 else { throw ParagraphError.invalidContent }
            return WhiskerInlineAttachment(node: UInt64(node), offset: start, size: CGSize(width: width, height: height), baseline: try fields.number("baseline"), alignment: alignment, shift: try fields.number("shift"), truncation: truncation, label: fields["label"]?.asString)
        }
        guard visibleEnd == nil || (visibleEnd! >= 0 && visibleEnd! <= units.count),
              Set(attachments.map(\.node)).count == attachments.count,
              Set(attachments.map(\.offset)).count == attachments.count else { throw ParagraphError.invalidContent }
        runs = try fields.required("runs").paragraphItems().map { value in
            let fields = try value.paragraphFields()
            let start = try fields.integer("start")
            let end = try fields.integer("end")
            guard start >= previousEnd, end > start, end <= units.count,
                  start == 0 || !(0xDC00...0xDFFF).contains(units[start]),
                  end == units.count || !(0xDC00...0xDFFF).contains(units[end]) else {
                throw ParagraphError.invalidContent
            }
            previousEnd = end
            let families = try fields.required("families").paragraphItems().map { value in
                guard let name = value.asString, !name.isEmpty else { throw ParagraphError.invalidContent }
                return name
            }
            let size = try fields.number("size")
            let weight = try fields.integer("weight")
            guard !families.isEmpty, size > 0, (1...1000).contains(weight),
                  let italic = fields["italic"]?.asBool else { throw ParagraphError.invalidContent }
            let base = resolveWhiskerBaseFont(fontFamilies: families, fontSize: size, fontWeight: weight, fontStyle: italic ? .italic : .normal).font
            var descriptor: [CFString: Any] = [:]
            let features = try fields.required("features").paragraphFields().map { tag, value -> [CFString: Any] in
                guard let value = value.asInt else { throw ParagraphError.invalidContent }
                return [kCTFontOpenTypeFeatureTag: try fontCode(tag), kCTFontOpenTypeFeatureValue: value]
            }
            if !features.isEmpty { descriptor[kCTFontFeatureSettingsAttribute] = features }
            var variations = [NSNumber: NSNumber]()
            for (tag, value) in try fields.required("variations").paragraphFields() {
                guard let value = value.asDouble, value.isFinite else { throw ParagraphError.invalidContent }
                variations[NSNumber(value: try fontCode(tag))] = NSNumber(value: value)
            }
            if fields["optical"]?.asBool == true, variations[NSNumber(value: try fontCode("opsz"))] == nil {
                variations[NSNumber(value: try fontCode("opsz"))] = NSNumber(value: Double(size))
            }
            if !variations.isEmpty { descriptor[kCTFontVariationAttribute] = variations }
            let font = descriptor.isEmpty ? base : CTFontCreateCopyWithAttributes(base, size, nil, CTFontDescriptorCreateWithAttributes(descriptor as CFDictionary)) as UIFont
            var attributes: [NSAttributedString.Key: Any] = [.font: font, .kern: try fields.number("spacing")]
            let alignment = fields["alignment"]?.asInt ?? 0
            guard alignment >= 0 && alignment <= 4 else { throw ParagraphError.invalidContent }
            attributes[.whiskerVerticalAlignment] = Int(alignment)
            attributes[.whiskerBaselineShift] = fields["shift"] == nil ? CGFloat(0) : try fields.number("shift")
            if let paint = fields["paint"], paint != .null {
                attributes.merge(try paintAttributes(paint)) { _, new in new }
            }
            return ParagraphRun(range: NSRange(location: start, length: end - start), attributes: attributes, action: fields["action"]?.asInt.flatMap { $0 > 0 ? UInt64($0) : nil })
        }
    }

    public func attributedString(_ text: String, attributes: [NSAttributedString.Key: Any]) -> NSAttributedString {
        let token = attachments.contains { $0.truncation }
        let display = token && visibleEnd != nil ? (text as NSString).substring(to: visibleEnd!) + "\u{FFFC}" : text
        let result = NSMutableAttributedString(string: display, attributes: attributes)
        if let font = attributes[.font] { result.addAttribute(.whiskerParagraphFont, value: font, range: NSRange(location: 0, length: result.length)) }
        let visible = NSRange(location: 0, length: token ? visibleEnd ?? result.length : result.length)
        for run in runs {
            let range = NSIntersectionRange(run.range, visible)
            if range.length > 0 { result.addAttributes(run.attributes, range: range) }
        }
        for attachment in attachments {
            let offset = attachment.truncation ? visibleEnd ?? result.length : attachment.offset
            guard offset < result.length, (attachment.truncation ? visibleEnd != nil : offset < NSMaxRange(visible)) else { continue }
            let font = result.attribute(.font, at: offset, effectiveRange: nil) as? UIFont ?? UIFont.systemFont(ofSize: 14)
            let inline = attachment.truncation ? WhiskerTruncationAttachment() : NSTextAttachment()
            inline.bounds = CGRect(x: 0, y: attachment.resolvedBaseline(font: font) - attachment.size.height, width: attachment.size.width, height: attachment.size.height)
            result.addAttributes([.attachment: inline, .whiskerInlineAttachment: attachment], range: NSRange(location: offset, length: 1))
        }
        return result
    }
}

private struct ParagraphRun {
    let range: NSRange
    let attributes: [NSAttributedString.Key: Any]
    let action: UInt64?
}

private enum ParagraphError: Error { case invalidContent }

private func fontCode(_ tag: String) throws -> UInt32 {
    guard tag.utf8.count == 4, tag.utf8.allSatisfy({ (32...126).contains($0) }) else { throw ParagraphError.invalidContent }
    return tag.utf8.reduce(0) { ($0 << 8) | UInt32($1) }
}

private func paintAttributes(_ value: WhiskerValue) throws -> [NSAttributedString.Key: Any] {
    let fields = try value.paragraphFields()
    var attributes: [NSAttributedString.Key: Any] = [.foregroundColor: try paragraphColor(fields.required("color"))]
    if let background = fields["background"], background != .null {
        let radii = try (fields["radii"] ?? .array(Array(repeating: .array(Array(repeating: .float(0), count: 4)), count: 4))).paragraphItems().map { value in
            let values = try value.paragraphItems().map { value -> CGFloat in
                guard let number = value.asDouble, number.isFinite, number >= 0 else { throw ParagraphError.invalidContent }
                return CGFloat(number)
            }
            guard values.count == 4 else { throw ParagraphError.invalidContent }
            return values
        }
        guard radii.count == 4 else { throw ParagraphError.invalidContent }
        attributes[.whiskerInlineBackground] = WhiskerInlineBackground(color: try paragraphColor(background), radii: radii)
    }
    let decorationStyle = try fields.integer("decorationStyle")
    guard decorationStyle <= 4 else { throw ParagraphError.invalidContent }
    let style: NSUnderlineStyle = switch decorationStyle {
    case 1: .double
    case 2: [.single, .patternDot]
    case 3: [.single, .patternDash]
    default: .single
    }
    attributes[.underlineStyle] = fields["underline"]?.asBool == true ? style.rawValue : 0
    attributes[.strikethroughStyle] = fields["strike"]?.asBool == true ? style.rawValue : 0
    let color = try paragraphColor(fields.required("decorationColor"))
    attributes[.underlineColor] = color
    attributes[.strikethroughColor] = color
    if decorationStyle == 4 {
        attributes[.underlineStyle] = 0
        attributes[.strikethroughStyle] = 0
        attributes[.whiskerInlineWave] = WhiskerInlineWave(color: color, underline: fields["underline"]?.asBool == true, strike: fields["strike"]?.asBool == true)
    }
    if let shadowValue = try fields.required("shadows").paragraphItems().first {
        let fields = try shadowValue.paragraphFields()
        let shadow = NSShadow()
        shadow.shadowOffset = CGSize(width: try fields.number("x"), height: try fields.number("y"))
        shadow.shadowBlurRadius = try fields.number("blur")
        shadow.shadowColor = try paragraphColor(fields.required("color"))
        attributes[.shadow] = shadow
    } else {
        attributes[.shadow] = NSShadow()
    }
    return attributes
}

private func paragraphColor(_ value: WhiskerValue) throws -> UIColor {
    let channels = try value.paragraphItems()
    guard channels.count == 4 else { throw ParagraphError.invalidContent }
    let values = try channels.map { value -> CGFloat in
        guard let number = value.asDouble, number.isFinite else { throw ParagraphError.invalidContent }
        return CGFloat(number)
    }
    return UIColor(red: values[0] / 255, green: values[1] / 255, blue: values[2] / 255, alpha: values[3])
}

private extension WhiskerValue {
    func paragraphFields() throws -> [String: WhiskerValue] {
        guard case .map(let fields) = self else { throw ParagraphError.invalidContent }
        return fields
    }
    func paragraphItems() throws -> [WhiskerValue] {
        guard case .array(let items) = self else { throw ParagraphError.invalidContent }
        return items
    }
}

private extension Dictionary where Key == String, Value == WhiskerValue {
    func required(_ name: String) throws -> WhiskerValue {
        guard let value = self[name] else { throw ParagraphError.invalidContent }
        return value
    }
    func integer(_ name: String) throws -> Int {
        guard case .int(let value) = try required(name), value >= 0, let integer = Int(exactly: value) else { throw ParagraphError.invalidContent }
        return integer
    }
    func number(_ name: String) throws -> CGFloat {
        guard let value = try required(name).asDouble, value.isFinite else { throw ParagraphError.invalidContent }
        return CGFloat(value)
    }
}
