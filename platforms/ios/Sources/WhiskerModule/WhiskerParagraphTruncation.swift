import UIKit

extension WhiskerParagraph {
    public func prepare(attributes: [NSAttributedString.Key: Any], width: CGFloat, maxLines: Int, overflow: WhiskerTextOverflow) -> (WhiskerParagraphLayout, WhiskerParagraph) {
        func layout(_ paragraph: WhiskerParagraph, lines: Int, overflow: WhiskerTextOverflow) -> WhiskerParagraphLayout {
            WhiskerParagraphLayout(text: paragraph.attributedString(self.source, attributes: attributes), width: width, maxLines: lines, overflow: overflow)
        }
        guard attachments.contains(where: { $0.truncation }) else { return (layout(self, lines: maxLines, overflow: overflow), self) }
        func fits(_ candidate: WhiskerParagraphLayout) -> Bool {
            var count = 0
            candidate.manager.enumerateLineFragments(forGlyphRange: candidate.manager.glyphRange(for: candidate.container)) { _, _, _, _, _ in count += 1 }
            return (maxLines == 0 || count <= maxLines) && candidate.size.width <= width
        }
        let full = layout(self, lines: 0, overflow: .clip)
        if fits(full) { return (full, self) }
        let source = source as NSString
        var boundaries = [0]
        var offset = 0
        while offset < source.length {
            offset = NSMaxRange(source.rangeOfComposedCharacterSequence(at: offset))
            boundaries.append(offset)
        }
        var bestParagraph = withVisibleEnd(0)
        var best = layout(bestParagraph, lines: 0, overflow: .clip)
        var low = 0
        var high = boundaries.count - 1
        var probes = 0
        while low < high && probes < 60 {
            probes += 1
            let middle = low + (high - low) / 2
            let paragraph = withVisibleEnd(boundaries[middle])
            let candidate = layout(paragraph, lines: 0, overflow: .clip)
            if fits(candidate) { best = candidate; bestParagraph = paragraph; low = middle + 1 }
            else { high = middle }
        }
        return (best, bestParagraph)
    }
}
