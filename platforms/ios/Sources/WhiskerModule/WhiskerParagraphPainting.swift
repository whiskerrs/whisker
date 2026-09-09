import UIKit

extension NSAttributedString.Key {
    static let whiskerInlineBackground = Self("rs.whisker.inline.background")
    static let whiskerInlineWave = Self("rs.whisker.inline.wave")
}

final class WhiskerInlineBackground: NSObject {
    let color: UIColor
    let radii: [[CGFloat]]
    init(color: UIColor, radii: [[CGFloat]]) { self.color = color; self.radii = radii }
}

final class WhiskerInlineWave: NSObject {
    let color: UIColor
    let underline: Bool
    let strike: Bool
    init(color: UIColor, underline: Bool, strike: Bool) {
        self.color = color; self.underline = underline; self.strike = strike
    }
}

extension WhiskerParagraphLayout {
    func drawInlineBackgrounds(at origin: CGPoint) {
        fragments(attribute: .whiskerInlineBackground) { value, rect, _, _ in
            guard let background = value as? WhiskerInlineBackground else { return }
            background.color.setFill()
            let rect = rect.offsetBy(dx: origin.x, dy: origin.y)
            inlineRoundedPath(rect, radii: background.radii).fill()
        }
    }

    func drawInlineWaves(at origin: CGPoint) {
        fragments(attribute: .whiskerInlineWave) { value, rect, glyph, character in
            guard let wave = value as? WhiskerInlineWave,
                  let font = self.storage.attribute(.font, at: character, effectiveRange: nil) as? UIFont else { return }
            let baseline = self.baseline(at: glyph) + origin.y
            let stroke = max(1, font.pointSize / 16)
            wave.color.setStroke()
            if wave.underline { inlineWave(from: rect.minX + origin.x, to: rect.maxX + origin.x, y: baseline + stroke * 1.5, stroke: stroke) }
            if wave.strike { inlineWave(from: rect.minX + origin.x, to: rect.maxX + origin.x, y: baseline - font.ascender * 0.35, stroke: stroke) }
        }
    }

    private func fragments(attribute: NSAttributedString.Key, draw: @escaping (Any, CGRect, Int, Int) -> Void) {
        let visible = manager.glyphRange(for: container)
        storage.enumerateAttribute(attribute, in: NSRange(location: 0, length: storage.length)) { value, characters, _ in
            guard let value else { return }
            let glyphs = NSIntersectionRange(visible, self.manager.glyphRange(forCharacterRange: characters, actualCharacterRange: nil))
            self.manager.enumerateLineFragments(forGlyphRange: glyphs) { _, _, _, lineGlyphs, _ in
                var range = NSIntersectionRange(glyphs, lineGlyphs)
                let truncated = self.manager.truncatedGlyphRange(inLineFragmentForGlyphAt: lineGlyphs.location)
                if truncated.location != NSNotFound {
                    range.length = max(0, min(NSMaxRange(range), truncated.location) - range.location)
                }
                guard range.length > 0 else { return }
                self.manager.enumerateEnclosingRects(forGlyphRange: range, withinSelectedGlyphRange: NSRange(location: NSNotFound, length: 0), in: self.container) { rect, _ in
                    draw(value, rect, range.location, characters.location)
                }
            }
        }
    }
}

private func inlineRoundedPath(_ rect: CGRect, radii: [[CGFloat]]) -> UIBezierPath {
    var corners = radii.map { CGSize(width: $0[0] + $0[1] * rect.width, height: $0[2] + $0[3] * rect.height) }
    let scale = min(1,
        radiusScale(rect.width, corners[0].width + corners[1].width),
        radiusScale(rect.width, corners[3].width + corners[2].width),
        radiusScale(rect.height, corners[0].height + corners[3].height),
        radiusScale(rect.height, corners[1].height + corners[2].height))
    corners = corners.map { CGSize(width: $0.width * scale, height: $0.height * scale) }
    let path = UIBezierPath()
    let k: CGFloat = 0.5522847498307936
    let (tl, tr, br, bl) = (corners[0], corners[1], corners[2], corners[3])
    path.move(to: CGPoint(x: rect.minX + tl.width, y: rect.minY))
    path.addLine(to: CGPoint(x: rect.maxX - tr.width, y: rect.minY))
    path.addCurve(to: CGPoint(x: rect.maxX, y: rect.minY + tr.height), controlPoint1: CGPoint(x: rect.maxX - tr.width * (1 - k), y: rect.minY), controlPoint2: CGPoint(x: rect.maxX, y: rect.minY + tr.height * (1 - k)))
    path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY - br.height))
    path.addCurve(to: CGPoint(x: rect.maxX - br.width, y: rect.maxY), controlPoint1: CGPoint(x: rect.maxX, y: rect.maxY - br.height * (1 - k)), controlPoint2: CGPoint(x: rect.maxX - br.width * (1 - k), y: rect.maxY))
    path.addLine(to: CGPoint(x: rect.minX + bl.width, y: rect.maxY))
    path.addCurve(to: CGPoint(x: rect.minX, y: rect.maxY - bl.height), controlPoint1: CGPoint(x: rect.minX + bl.width * (1 - k), y: rect.maxY), controlPoint2: CGPoint(x: rect.minX, y: rect.maxY - bl.height * (1 - k)))
    path.addLine(to: CGPoint(x: rect.minX, y: rect.minY + tl.height))
    path.addCurve(to: CGPoint(x: rect.minX + tl.width, y: rect.minY), controlPoint1: CGPoint(x: rect.minX, y: rect.minY + tl.height * (1 - k)), controlPoint2: CGPoint(x: rect.minX + tl.width * (1 - k), y: rect.minY))
    path.close()
    return path
}

private func radiusScale(_ extent: CGFloat, _ sum: CGFloat) -> CGFloat { sum > 0 ? extent / sum : 1 }

private func inlineWave(from left: CGFloat, to right: CGFloat, y: CGFloat, stroke: CGFloat) {
    let path = UIBezierPath()
    path.lineWidth = stroke
    path.move(to: CGPoint(x: left, y: y))
    var x = left
    var up = true
    while x < right {
        x = min(right, x + stroke * 2)
        path.addLine(to: CGPoint(x: x, y: y + (up ? -stroke : stroke)))
        up.toggle()
    }
    path.stroke()
}


extension WhiskerParagraphLayout {
    func applyPaint(_ text: NSAttributedString) {
        let keys: Set<NSAttributedString.Key> = [.foregroundColor, .backgroundColor, .underlineStyle, .underlineColor,
            .strikethroughStyle, .strikethroughColor, .shadow, .whiskerInlineBackground, .whiskerInlineWave]
        storage.beginEditing()
        for key in keys { storage.removeAttribute(key, range: NSRange(location: 0, length: storage.length)) }
        text.enumerateAttributes(in: NSRange(location: 0, length: text.length)) { attributes, range, _ in
            storage.addAttributes(attributes.filter { keys.contains($0.key) }, range: range)
        }
        storage.endEditing()
    }
}
