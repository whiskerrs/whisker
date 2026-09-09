import UIKit

extension NSAttributedString.Key {
    static let whiskerVerticalAlignment = NSAttributedString.Key("WhiskerVerticalAlignment")
    static let whiskerBaselineShift = NSAttributedString.Key("WhiskerBaselineShift")
    static let whiskerParagraphFont = NSAttributedString.Key("WhiskerParagraphFont")
    static let whiskerInlineAttachment = NSAttributedString.Key("WhiskerInlineAttachment")
}

private struct AlignmentItem {
    let range: NSRange
    let font: UIFont
    let attachment: WhiskerInlineAttachment?
    let alignment: Int
    let shift: CGFloat
    var ascent: CGFloat { attachment?.baseline ?? font.ascender }
    var descent: CGFloat { attachment.map { $0.size.height - $0.baseline } ?? -font.descender }
    var height: CGFloat { ascent + descent }
}

extension WhiskerParagraphLayout {
    func resolveVerticalAlignment() {
        guard storage.length > 0 else { return }
        var lines = [NSRange]()
        manager.enumerateLineFragments(forGlyphRange: manager.glyphRange(for: container)) { _, _, _, glyphs, _ in
            lines.append(self.manager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil))
        }
        var updates = [(NSRange, CGFloat, WhiskerInlineAttachment?)]()
        for line in lines {
            let rootFont = storage.attribute(.whiskerParagraphFont, at: line.location, effectiveRange: nil) as? UIFont
                ?? storage.attribute(.font, at: line.location, effectiveRange: nil) as? UIFont
                ?? UIFont.systemFont(ofSize: 14)
            var items = [AlignmentItem]()
            storage.enumerateAttributes(in: line) { attributes, range, _ in
                let font = attributes[.font] as? UIFont ?? rootFont
                let attachment = attributes[.whiskerInlineAttachment] as? WhiskerInlineAttachment
                items.append(AlignmentItem(range: range, font: font, attachment: attachment,
                    alignment: attachment?.alignment ?? attributes[.whiskerVerticalAlignment] as? Int ?? 0,
                    shift: attachment?.shift ?? attributes[.whiskerBaselineShift] as? CGFloat ?? 0))
            }
            guard items.contains(where: { $0.alignment != 0 }) else { continue }
            var ascent = rootFont.ascender
            var descent = -rootFont.descender
            for item in items where item.alignment != 1 && item.alignment != 3 {
                let shift = item.alignment == 2 ? (item.descent - item.ascent + rootFont.xHeight) / 2 : item.alignment == 4 ? item.shift : 0
                ascent = max(ascent, item.ascent + shift)
                descent = max(descent, item.descent - shift)
            }
            let height = max(ascent + descent, items.filter { $0.alignment == 1 || $0.alignment == 3 }.map(\.height).max() ?? 0)
            let leading = (height - ascent - descent) / 2
            ascent += leading
            descent += leading
            for item in items {
                let shift: CGFloat = switch item.alignment {
                case 1: ascent - item.ascent
                case 2: (item.descent - item.ascent + rootFont.xHeight) / 2
                case 3: item.descent - descent
                case 4: item.shift
                default: 0
                }
                updates.append((item.range, shift, item.attachment))
            }
        }
        guard !updates.isEmpty else { return }
        storage.beginEditing()
        for (range, shift, attachment) in updates {
            if let attachment, let native = storage.attribute(.attachment, at: range.location, effectiveRange: nil) as? NSTextAttachment {
                native.bounds.origin.y = attachment.baseline + shift - attachment.size.height
                storage.addAttribute(.baselineOffset, value: CGFloat(0), range: range)
            } else {
                storage.addAttribute(.baselineOffset, value: shift, range: range)
            }
        }
        storage.endEditing()
        manager.invalidateLayout(forCharacterRange: NSRange(location: 0, length: storage.length), actualCharacterRange: nil)
        manager.ensureLayout(for: container)
    }
}
