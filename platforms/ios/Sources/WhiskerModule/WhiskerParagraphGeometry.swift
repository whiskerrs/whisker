import UIKit

extension WhiskerParagraphLayout {
    public func measurementGeometry(_ paragraph: WhiskerParagraph) -> WhiskerValue {
        let visible = manager.glyphRange(for: container)
        var lines: [WhiskerValue] = []
        var visibleGlyphs: [NSRange] = []
        manager.enumerateLineFragments(forGlyphRange: visible) { rect, _, _, glyphs, _ in
            var characters = self.manager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
            let truncated = self.manager.truncatedGlyphRange(inLineFragmentForGlyphAt: glyphs.location)
            var ellipsis = 0
            if truncated.location != NSNotFound {
                let start = self.manager.characterIndexForGlyph(at: truncated.location)
                ellipsis = self.storage.length - start
                characters.length = self.storage.length - characters.location
                visibleGlyphs.append(NSRange(location: glyphs.location, length: max(0, truncated.location - glyphs.location)))
            } else {
                visibleGlyphs.append(glyphs)
            }
            if let end = paragraph.visibleEnd {
                let last = NSMaxRange(glyphs) >= NSMaxRange(visible)
                let sourceEnd = last ? (paragraph.source as NSString).length : min(NSMaxRange(characters), end)
                characters.length = max(0, sourceEnd - characters.location)
                if last { ellipsis = sourceEnd - end }
            }
            lines.append(.map([
                "start": .int(Int64(characters.location)), "end": .int(Int64(NSMaxRange(characters))),
                "ellipsis": .int(Int64(ellipsis)), "bounds": paragraphRect(rect),
                "baseline": .float(Double(self.baseline(at: glyphs.location))),
            ]))
        }
        var fragments: [WhiskerValue] = []
        let sources = paragraph.sourceRanges.isEmpty ? [NSRange(location: 0, length: storage.length)] : paragraph.sourceRanges
        for source in sources {
            let glyphs = manager.glyphRange(forCharacterRange: source, actualCharacterRange: nil)
            for line in visibleGlyphs {
                let range = NSIntersectionRange(glyphs, line)
                guard range.length > 0 else { continue }
                let characters = manager.characterRange(forGlyphRange: range, actualGlyphRange: nil)
                manager.enumerateEnclosingRects(forGlyphRange: range, withinSelectedGlyphRange: NSRange(location: NSNotFound, length: 0), in: container) { rect, _ in
                    guard rect.width > 0, rect.height > 0 else { return }
                    fragments.append(.map([
                        "start": .int(Int64(characters.location)), "end": .int(Int64(NSMaxRange(characters))),
                        "bounds": paragraphRect(rect),
                    ]))
                }
            }
        }
        return .map(["placements": inlinePlacements(paragraph.attachments, visibleEnd: paragraph.visibleEnd), "lines": .array(lines), "fragments": .array(fragments)])
    }

    func baseline(at glyph: Int) -> CGFloat {
        let character = manager.characterIndexForGlyph(at: glyph)
        let attachment = storage.attribute(.attachment, at: character, effectiveRange: nil) as? NSTextAttachment
        return manager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil).minY + manager.location(forGlyphAt: glyph).y + (attachment?.bounds.origin.y ?? 0)
    }
}

private func paragraphRect(_ rect: CGRect) -> WhiskerValue {
    .array([.float(Double(rect.minX)), .float(Double(rect.minY)), .float(Double(rect.width)), .float(Double(rect.height))])
}
