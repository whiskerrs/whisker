import UIKit

public struct WhiskerInlineAttachment {
    public let node: UInt64
    public let offset: Int
    public let size: CGSize
    public let baseline: CGFloat
    public let alignment: Int
    public let shift: CGFloat
    public var truncation: Bool = false
    public var label: String? = nil

    func resolvedBaseline(font: UIFont) -> CGFloat {
        switch alignment {
        case 1: font.ascender
        case 2: (size.height + font.xHeight) / 2
        case 3: size.height + font.descender
        case 4: baseline + shift
        default: baseline
        }
    }
}

extension WhiskerParagraphLayout {
    public func inlinePlacements(_ attachments: [WhiskerInlineAttachment], visibleEnd: Int? = nil) -> WhiskerValue {
        let visible = manager.glyphRange(for: container)
        return .array(attachments.map { attachment in
            let offset = attachment.truncation ? visibleEnd ?? storage.length : attachment.offset
            let visibleAttachment = offset < storage.length && (!attachment.truncation || visibleEnd != nil) && (attachment.truncation || visibleEnd == nil || offset < visibleEnd!)
            guard visibleAttachment else { return .map(["node": .int(Int64(attachment.node)), "origin": .null]) }
            let glyph = manager.glyphIndexForCharacter(at: offset)
            var origin: WhiskerValue = .null
            if NSLocationInRange(glyph, visible) {
                let truncated = manager.truncatedGlyphRange(inLineFragmentForGlyphAt: glyph)
                if truncated.location == NSNotFound || !NSLocationInRange(glyph, truncated) {
                    let line = manager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
                    let location = manager.location(forGlyphAt: glyph)
                    origin = .array([.float(Double(line.minX + location.x)), .float(Double(line.minY + location.y - attachment.size.height))])
                }
            }
            return .map(["node": .int(Int64(attachment.node)), "origin": origin])
        })
    }
}

final class WhiskerTruncationAttachment: NSTextAttachment {
    override func attachmentBounds(for textContainer: NSTextContainer?, proposedLineFragment lineFrag: CGRect, glyphPosition position: CGPoint, characterIndex charIndex: Int) -> CGRect {
        var result = bounds
        if let textContainer { result.size.width = min(result.width, textContainer.size.width) }
        return result
    }
}
