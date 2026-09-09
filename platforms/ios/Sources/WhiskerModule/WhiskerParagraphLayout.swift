import UIKit

public final class WhiskerParagraphLayout {
    public let storage: NSTextStorage
    public let manager: NSLayoutManager
    public let container: NSTextContainer

    public init(text: NSAttributedString, width: CGFloat, maxLines: Int, overflow: WhiskerTextOverflow) {
        storage = NSTextStorage(attributedString: text)
        manager = WhiskerParagraphLayoutManager()
        container = NSTextContainer(size: CGSize(width: max(0, width), height: .greatestFiniteMagnitude))
        container.lineFragmentPadding = 0
        container.maximumNumberOfLines = maxLines
        container.lineBreakMode = overflow == .ellipsis ? .byTruncatingTail : .byWordWrapping
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
        manager.ensureLayout(for: container)
        resolveVerticalAlignment()
    }

    init(storage: NSTextStorage, manager: NSLayoutManager, container: NSTextContainer) {
        self.storage = storage
        self.manager = manager
        self.container = container
    }

    public var size: CGSize { manager.usedRect(for: container).size }

    public var baselines: (first: CGFloat, last: CGFloat) {
        var first: CGFloat?
        var last: CGFloat = 0
        manager.enumerateLineFragments(forGlyphRange: manager.glyphRange(for: container)) { _, _, _, range, _ in
            let baseline = self.baseline(at: range.location)
            first = first ?? baseline
            last = baseline
        }
        return (first ?? 0, last)
    }

    public func draw(at origin: CGPoint) {
        let range = manager.glyphRange(for: container)
        manager.drawBackground(forGlyphRange: range, at: origin)
        manager.drawGlyphs(forGlyphRange: range, at: origin)
    }
}

final class WhiskerParagraphLayoutManager: NSLayoutManager {
    private var paragraph: WhiskerParagraphLayout? {
        guard let textStorage, let container = textContainers.first else { return nil }
        return WhiskerParagraphLayout(storage: textStorage, manager: self, container: container)
    }
    override func drawBackground(forGlyphRange glyphsToShow: NSRange, at origin: CGPoint) {
        paragraph?.drawInlineBackgrounds(at: origin)
        super.drawBackground(forGlyphRange: glyphsToShow, at: origin)
    }
    override func drawGlyphs(forGlyphRange glyphsToShow: NSRange, at origin: CGPoint) {
        super.drawGlyphs(forGlyphRange: glyphsToShow, at: origin)
        paragraph?.drawInlineWaves(at: origin)
    }
}
