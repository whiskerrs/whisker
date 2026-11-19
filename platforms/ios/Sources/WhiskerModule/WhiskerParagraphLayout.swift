#if canImport(UIKit)
import UIKit
typealias WhiskerTextEditActions = NSTextStorage.EditActions
#else
import AppKit
typealias WhiskerTextEditActions = NSTextStorageEditActions
#endif

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

final class WhiskerParagraphLayoutManager: NSLayoutManager, NSLayoutManagerDelegate {
    override init() {
        super.init()
        delegate = self
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        delegate = self
    }

    /// Set while the storage changes only attributes that leave glyphs and line breaks alone.
    var editingPaintOnly = false

    override func processEditing(
        for textStorage: NSTextStorage,
        edited editMask: WhiskerTextEditActions,
        range newCharRange: NSRange,
        changeInLength delta: Int,
        invalidatedRange invalidatedCharRange: NSRange
    ) {
        if editingPaintOnly { return }
        super.processEditing(
            for: textStorage, edited: editMask, range: newCharRange,
            changeInLength: delta, invalidatedRange: invalidatedCharRange
        )
    }

    /// TextKit keeps a fixed line height's slack above the text, while CSS and
    /// the Android Host split it evenly around the content area (half-leading).
    func layoutManager(
        _ layoutManager: NSLayoutManager,
        shouldSetLineFragmentRect lineFragmentRect: UnsafeMutablePointer<CGRect>,
        lineFragmentUsedRect: UnsafeMutablePointer<CGRect>,
        baselineOffset: UnsafeMutablePointer<CGFloat>,
        in textContainer: NSTextContainer,
        forGlyphRange glyphRange: NSRange
    ) -> Bool {
        guard let textStorage, glyphRange.length > 0 else { return false }
        let characters = characterRange(forGlyphRange: glyphRange, actualGlyphRange: nil)
        guard let paragraph = textStorage.attribute(.paragraphStyle, at: characters.location, effectiveRange: nil) as? NSParagraphStyle,
              paragraph.minimumLineHeight > 0,
              paragraph.minimumLineHeight == paragraph.maximumLineHeight else { return false }
        var ascent: CGFloat = 0
        var descent: CGFloat = 0
        textStorage.enumerateAttribute(.font, in: characters) { value, _, _ in
            guard let font = value as? WhiskerPlatformFont else { return }
            ascent = max(ascent, font.ascender)
            descent = max(descent, -font.descender)
        }
        guard ascent + descent > 0 else { return false }
        baselineOffset.pointee = (lineFragmentRect.pointee.height - ascent - descent) / 2 + ascent
        return true
    }

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
