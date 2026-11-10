#if !canImport(UIKit) && canImport(AppKit)
import AppKit
import CoreText

/** Native container whose child geometry is supplied entirely by Rust. */
public class WhiskerContainerView: NSView {
    public override var isFlipped: Bool { true }

    public override init(frame: CGRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.masksToBounds = false
    }

    public required init?(coder: NSCoder) { nil }
}

/** Native scroll container with a dedicated multi-child document view. */
public final class WhiskerScrollContainerView: NSScrollView, WhiskerEventSource {
    public let whiskerContentView = WhiskerContainerView(frame: .zero)
    private var eventSink: ((String, WhiskerValue) -> Void)?
    private var presentationSink: ((CGPoint) -> Void)?
    private var contentExtentSource: (() -> CGSize)?
    private var cachedContentExtent = CGSize.zero
    private var contentExtentNeedsUpdate = true
    private var previousChildExtent = CGRect.null
    private var horizontal = false
    private var chromeVisible = true
    private var snapFactor: CGFloat?
    private var snapOffset: CGFloat = 0
    private var snapStopAlways = false
    private var liveScrollStart: CGPoint?
    private var reportingDrag = false
    private var lastScrollOffset = CGPoint.zero
    private var lastScrollSample: (offset: CGPoint, time: TimeInterval)?
    private var releaseVelocity = CGPoint.zero
    private var pendingLogicalOffset: (offset: CGFloat, smooth: Bool)?

    public var isScrollEnabled = true {
        didSet {
            if !isScrollEnabled { finishDrag(cancelled: true) }
        }
    }

    public override var isFlipped: Bool { true }

    public override init(frame: CGRect) {
        super.init(frame: frame)
        wantsLayer = true
        drawsBackground = false
        borderType = .noBorder
        scrollerStyle = .overlay
        autohidesScrollers = true
        horizontalScrollElasticity = .allowed
        verticalScrollElasticity = .allowed
        automaticallyAdjustsContentInsets = false
        contentInsets = .zero
        contentView.drawsBackground = false
        documentView = whiskerContentView
        contentView.postsBoundsChangedNotifications = true
        updateIndicatorVisibility()
        let center = NotificationCenter.default
        center.addObserver(self, selector: #selector(clipBoundsChanged), name: NSView.boundsDidChangeNotification, object: contentView)
        center.addObserver(self, selector: #selector(liveScrollStarted), name: NSScrollView.willStartLiveScrollNotification, object: self)
        center.addObserver(self, selector: #selector(liveScrollEnded), name: NSScrollView.didEndLiveScrollNotification, object: self)
    }

    public required init?(coder: NSCoder) { nil }

    deinit { NotificationCenter.default.removeObserver(self) }

    public func installWhiskerEventSink(_ sink: ((String, WhiskerValue) -> Void)?) {
        eventSink = sink
    }

    /** Installs the Host-internal scroll mirror, independent of app listeners. */
    public func installWhiskerPresentationSink(_ sink: ((CGPoint) -> Void)?) {
        presentationSink = sink
    }

    /// Installs the Host's overflow measurement without exposing its scene wrappers to native modules.
    public func installWhiskerContentExtentSource(_ source: (() -> CGSize)?) {
        contentExtentSource = source
        invalidateWhiskerContentExtent()
    }

    public func invalidateWhiskerContentExtent() {
        contentExtentNeedsUpdate = true
        needsLayout = true
    }

    public func setScrollOrientation(_ value: String) {
        horizontal = value == "horizontal"
        updateIndicatorVisibility()
    }

    public func setWhiskerChromeVisible(_ visible: Bool) {
        chromeVisible = visible
        updateIndicatorVisibility()
    }

    private func updateIndicatorVisibility() {
        hasHorizontalScroller = chromeVisible && horizontal
        hasVerticalScroller = chromeVisible && !horizontal
    }

    public func setItemSnap(factor: Double, offset: Double) {
        snapFactor = CGFloat(factor).clamped(to: 0...1)
        snapOffset = CGFloat(offset)
    }

    public func clearItemSnap() {
        snapFactor = nil
        snapOffset = 0
    }

    public func setScrollSnapStop(_ value: String) {
        snapStopAlways = value == "always"
    }

    public var contentOffset: CGPoint { contentView.bounds.origin }

    public var documentSize: CGSize { whiskerContentView.frame.size }

    public func scrollToLogicalOffset(_ offset: Double, smooth: Bool) {
        let logical = CGFloat(offset)
        // The document view is resized on the next layout pass; an offset
        // beyond the current content is re-applied there instead of clamped.
        pendingLogicalOffset = logical > maximumLogicalOffset + 0.5 ? (logical, smooth) : nil
        applyLogicalOffset(logical, smooth: smooth)
    }

    private var maximumLogicalOffset: CGFloat {
        horizontal
            ? max(0, documentSize.width - contentView.bounds.width)
            : max(0, documentSize.height - contentView.bounds.height)
    }

    private func applyLogicalOffset(_ offset: CGFloat, smooth: Bool) {
        var target = contentOffset
        if horizontal { target.x = min(offset, maximumLogicalOffset) }
        else { target.y = min(offset, maximumLogicalOffset) }
        target.x = max(0, target.x)
        target.y = max(0, target.y)
        if smooth {
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.3
                contentView.animator().setBoundsOrigin(target)
            } completionHandler: { [weak self] in
                guard let self else { return }
                reflectScrolledClipView(contentView)
            }
        } else {
            contentView.scroll(to: target)
            reflectScrolledClipView(contentView)
        }
    }

    public func scrollByLogicalOffset(_ offset: Double, smooth: Bool) {
        let current = horizontal ? contentOffset.x : contentOffset.y
        scrollToLogicalOffset(Double(current) + offset, smooth: smooth)
    }

    public override func scrollWheel(with event: NSEvent) {
        guard isScrollEnabled else {
            nextResponder?.scrollWheel(with: event)
            return
        }
        super.scrollWheel(with: event)
    }

    @objc private func liveScrollStarted() {
        pendingLogicalOffset = nil
        liveScrollStart = contentOffset
        releaseVelocity = .zero
        reportingDrag = true
        emitScroll()
    }

    @objc private func liveScrollEnded() {
        let snapped = snapToItem()
        finishDrag(cancelled: !isScrollEnabled)
        if snapped { liveScrollStart = nil }
    }

    private func finishDrag(cancelled: Bool) {
        guard reportingDrag else { return }
        reportingDrag = false
        let velocity = cancelled ? CGPoint.zero : releaseVelocity
        releaseVelocity = .zero
        emitScroll(velocity: velocity, cancelled: cancelled)
    }

    @objc private func clipBoundsChanged() {
        let now = ProcessInfo.processInfo.systemUptime
        if reportingDrag, let sample = lastScrollSample, now > sample.time {
            let elapsed = CGFloat(now - sample.time)
            releaseVelocity = CGPoint(
                x: horizontal ? (contentOffset.x - sample.offset.x) / elapsed : 0,
                y: horizontal ? 0 : (contentOffset.y - sample.offset.y) / elapsed
            )
        }
        lastScrollSample = (contentOffset, now)
        emitScroll()
    }

    private func emitScroll(velocity: CGPoint = .zero, cancelled: Bool = false) {
        let delta = CGPoint(x: contentOffset.x - lastScrollOffset.x, y: contentOffset.y - lastScrollOffset.y)
        lastScrollOffset = contentOffset
        presentationSink?(contentOffset)
        eventSink?("scroll", .map([
            "deltaX": .float(Double(delta.x)),
            "deltaY": .float(Double(delta.y)),
            "isDragging": .bool(reportingDrag),
            "velocityX": .float(Double(velocity.x)),
            "velocityY": .float(Double(velocity.y)),
            "isDragCancelled": .bool(cancelled),
            "scrollLeft": .float(Double(contentOffset.x)),
            "scrollTop": .float(Double(contentOffset.y)),
            "scrollWidth": .float(Double(documentSize.width)),
            "scrollHeight": .float(Double(documentSize.height)),
            "viewportWidth": .float(Double(contentView.bounds.width)),
            "viewportHeight": .float(Double(contentView.bounds.height)),
        ]))
    }

    /// Settles on the nearest item boundary once trackpad momentum ends; AppKit has no target-offset hook.
    private func snapToItem() -> Bool {
        guard let factor = snapFactor, !whiskerContentView.subviews.isEmpty else { return false }
        let viewport = horizontal ? contentView.bounds.width : contentView.bounds.height
        let contentExtent = horizontal ? documentSize.width : documentSize.height
        let proposed = horizontal ? contentOffset.x : contentOffset.y
        let maximum = max(0, contentExtent - viewport)
        let targets = whiskerContentView.subviews
            .map { child -> CGFloat in
                let frame = child.frame
                let start = horizontal ? frame.minX : frame.minY
                let size = horizontal ? frame.width : frame.height
                return (start + size * factor - viewport * factor + snapOffset)
                    .clamped(to: 0...maximum)
            }
            .sorted()
        let startPoint = liveScrollStart ?? contentOffset
        let start = horizontal ? startPoint.x : startPoint.y
        let target: CGFloat
        if snapStopAlways, proposed > start + .ulpOfOne {
            target = targets.first(where: { $0 > start + .ulpOfOne }) ?? targets.last ?? proposed
        } else if snapStopAlways, proposed < start - .ulpOfOne {
            target = targets.last(where: { $0 < start - .ulpOfOne }) ?? targets.first ?? proposed
        } else {
            target = targets.min(by: { abs($0 - proposed) < abs($1 - proposed) }) ?? proposed
        }
        guard abs(target - proposed) > 0.5 else { return true }
        applyLogicalOffset(target, smooth: true)
        return true
    }

    public override func layout() {
        super.layout()
        let directExtent = whiskerContentView.subviews.reduce(CGRect.zero) { result, child in
            result.union(child.frame)
        }
        if directExtent != previousChildExtent {
            previousChildExtent = directExtent
            contentExtentNeedsUpdate = true
        }
        let extent: CGSize
        if let contentExtentSource {
            if contentExtentNeedsUpdate {
                contentExtentNeedsUpdate = false
                cachedContentExtent = contentExtentSource()
            }
            extent = cachedContentExtent
        } else {
            extent = CGSize(width: directExtent.maxX, height: directExtent.maxY)
        }
        let viewport = contentView.bounds.size
        let size = CGSize(
            width: max(viewport.width, extent.width),
            height: max(viewport.height, extent.height)
        )
        if whiskerContentView.frame.size != size {
            whiskerContentView.frame = CGRect(origin: .zero, size: size)
        }
        if let pending = pendingLogicalOffset, pending.offset <= maximumLogicalOffset + 0.5 {
            pendingLogicalOffset = nil
            applyLogicalOffset(pending.offset, smooth: pending.smooth)
        }
    }
}

/** Mirrors UIKit's semantic content attribute for the shared Text factory. */
public enum WhiskerSemanticContentAttribute {
    case unspecified
    case forceLeftToRight
    case forceRightToLeft
}

/** Native text element implementing Whisker's single-line decoration contract. */
public final class WhiskerTextLabel: NSView, WhiskerEventSource {
    private var explicitAccessibilityLabel: String?
    var selectionController: WhiskerTextSelection?
    var textEventSink: ((String, WhiskerValue) -> Void)?
    var preparedContent: UInt64 = 0
    public func installWhiskerEventSink(_ sink: ((String, WhiskerValue) -> Void)?) { textEventSink = sink }
    private var whiskerIndent = WhiskerTextIndent()
    private var richLayout: WhiskerParagraphLayout?
    private var plainLayout: WhiskerParagraphLayout?
    var richParagraph: WhiskerParagraph?
    public var font: NSFont = .systemFont(ofSize: 14) { didSet { needsDisplay = true } }
    public var textColor: NSColor = .labelColor { didSet { needsDisplay = true } }
    public var textAlignment: NSTextAlignment = .natural { didSet { needsDisplay = true } }
    public var numberOfLines = 1 { didSet { needsDisplay = true } }
    public var lineBreakMode: NSLineBreakMode = .byTruncatingTail { didSet { needsDisplay = true } }
    public var semanticContentAttribute: WhiskerSemanticContentAttribute = .unspecified
    public var effectiveUserInterfaceLayoutDirection: NSUserInterfaceLayoutDirection {
        switch semanticContentAttribute {
        case .unspecified: userInterfaceLayoutDirection
        case .forceLeftToRight: .leftToRight
        case .forceRightToLeft: .rightToLeft
        }
    }
    public var attributedText: NSAttributedString? {
        didSet { plainLayout = nil; needsDisplay = true }
    }
    public var text: String? { attributedText?.string }
    public var richContent: NSAttributedString? {
        didSet { richLayout = nil; needsDisplay = true }
    }
    public var richOverflow: WhiskerTextOverflow = .clip
    private var appliedIndent: CGFloat?
    public internal(set) var whiskerFontFeatures: [WhiskerFontFeature] = []
    public internal(set) var whiskerFontVariations: [WhiskerFontVariation] = []
    public internal(set) var whiskerFontOpticalSizing: WhiskerFontOpticalSizing = .none
    public internal(set) var whiskerFontFamilies: [String] = ["system"]
    public internal(set) var whiskerResolvedFontFamily = ""
    public internal(set) var whiskerFontWeight = 400
    public internal(set) var whiskerFontStyle: WhiskerTextFontStyle = .normal
    public internal(set) var whiskerLineHeight: CGFloat?
    public internal(set) var whiskerLetterSpacing: CGFloat = 0
    public internal(set) var whiskerDirection: WhiskerTextDirection = .auto

    public override var isFlipped: Bool { true }

    public override init(frame: CGRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
        setAccessibilityElement(true)
        setAccessibilityRole(.staticText)
    }

    public required init?(coder: NSCoder) { nil }

    public override func accessibilityLabel() -> String? {
        explicitAccessibilityLabel ?? richParagraph?.accessibleText ?? text
    }

    public override func setAccessibilityLabel(_ accessibilityLabel: String?) {
        explicitAccessibilityLabel = accessibilityLabel
    }

    public override func accessibilityCustomActions() -> [NSAccessibilityCustomAction]? {
        let revision = preparedContent
        return richParagraph?.accessibleActions.map { span, label in
            NSAccessibilityCustomAction(name: label) { [weak self] in
                guard let self, self.preparedContent == revision else { return false }
                self.textEventSink?("textactivate", .map(["span": .int(Int64(span)), "revision": .int(Int64(revision))]))
                return true
            }
        }
    }

    public func setWhiskerIndent(_ indent: WhiskerTextIndent) {
        whiskerIndent = indent
        appliedIndent = nil
        applyWhiskerIndent()
    }

    public override func layout() {
        super.layout()
        applyWhiskerIndent()
        if let richLayout, richLayout.container.size.width != bounds.width { self.richLayout = nil }
        selectionController?.synchronize()
    }

    public override func setFrameSize(_ newSize: NSSize) {
        let changed = newSize != frame.size
        super.setFrameSize(newSize)
        if changed { needsLayout = true; needsDisplay = true }
    }

    private func applyWhiskerIndent() {
        let resolved = whiskerIndent.resolve(width: bounds.width)
        guard appliedIndent != resolved, let attributedText else { return }
        appliedIndent = resolved
        let mutable = NSMutableAttributedString(attributedString: attributedText)
        let existingParagraph = attributedText.length > 0
            ? attributedText.attribute(.paragraphStyle, at: 0, effectiveRange: nil)
                as? NSParagraphStyle
            : nil
        let paragraph = existingParagraph?.mutableCopy() as? NSMutableParagraphStyle
            ?? NSMutableParagraphStyle()
        paragraph.alignment = textAlignment
        paragraph.lineBreakMode = lineBreakMode
        paragraph.firstLineHeadIndent = resolved
        mutable.addAttribute(.paragraphStyle, value: paragraph, range: NSRange(location: 0, length: mutable.length))
        self.attributedText = mutable
        if richContent != nil { richContent = mutable }
    }

    public var whiskerDecoration: WhiskerTextDecoration? {
        didSet { needsDisplay = true }
    }

    public func installPreparedParagraph(_ layout: WhiskerParagraphLayout) {
        applyWhiskerIndent()
        if let richContent {
            guard richContent.string == layout.storage.string else { return }
            layout.applyPaint(richContent)
            richLayout = layout
        } else {
            guard let attributedText, attributedText.string == layout.storage.string else { return }
            layout.applyPaint(attributedText)
            plainLayout = layout
        }
        selectionController?.synchronize()
        needsDisplay = true
    }

    func preparedParagraphLayout(width: CGFloat) -> WhiskerParagraphLayout? {
        guard let richContent else { return nil }
        if let richLayout, richLayout.container.size.width == width { return richLayout }
        let layout = WhiskerParagraphLayout(text: richContent, width: width, maxLines: numberOfLines, overflow: richOverflow)
        richLayout = layout
        return layout
    }

    public override func draw(_ dirtyRect: NSRect) {
        if selectionController != nil { return }
        let rect = bounds
        if let layout = preparedParagraphLayout(width: rect.width) {
            layout.draw(at: rect.origin)
            return
        }
        if let plainLayout, drawPlain(plainLayout, in: rect) { return }
        guard let attributedText else { return }
        let overflow: WhiskerTextOverflow = lineBreakMode == .byTruncatingTail ? .ellipsis : .clip
        let fallback = WhiskerParagraphLayout(text: attributedText, width: rect.width, maxLines: numberOfLines, overflow: overflow)
        _ = drawPlain(fallback, in: rect)
    }

    /// Draws the measured plain layout, or returns `false` when `rect` would break its lines elsewhere.
    private func drawPlain(_ layout: WhiskerParagraphLayout, in rect: CGRect) -> Bool {
        let manager = layout.manager
        let glyphs = manager.glyphRange(for: layout.container)
        var lines: [(glyphs: NSRange, used: CGRect)] = []
        manager.enumerateLineFragments(forGlyphRange: glyphs) { _, used, _, range, _ in
            lines.append((range, used))
        }
        let widest = lines.map(\.used.maxX).max() ?? 0
        guard widest <= rect.width + 0.5 else { return false }
        if rect.width > layout.container.size.width + 0.5 {
            let shown = manager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
            let truncated = glyphs.length > 0
                && manager.truncatedGlyphRange(inLineFragmentForGlyphAt: NSMaxRange(glyphs) - 1).location != NSNotFound
            guard lines.count <= 1, shown.length == layout.storage.length, !truncated else { return false }
        }
        let flush: CGFloat = switch textAlignment {
        case .center: 0.5
        case .right: 1
        default: 0
        }
        let wave = whiskerDecoration.flatMap { $0.style == .wavy ? $0 : nil }
        for line in lines {
            let origin = CGPoint(x: rect.minX + flush * (rect.width - line.used.maxX), y: rect.minY)
            manager.drawGlyphs(forGlyphRange: line.glyphs, at: origin)
            guard let wave, line.used.width > 0 else { continue }
            let baseline = origin.y + layout.baseline(at: line.glyphs.location)
            drawWave(wave, from: origin.x + line.used.minX, to: origin.x + line.used.maxX, baseline: baseline)
        }
        return true
    }

    private func drawWave(_ decoration: WhiskerTextDecoration, from start: CGFloat, to end: CGFloat, baseline: CGFloat) {
        guard let context = whiskerCurrentGraphicsContext() else { return }
        let stroke = max(1, font.pointSize / 16)
        let y = decoration.line == .underline
            ? baseline + stroke * 1.5
            : baseline - font.xHeight * 0.45
        context.saveGState()
        context.setStrokeColor(decoration.color.cgColor)
        context.setLineWidth(stroke)
        context.move(to: CGPoint(x: start, y: y))
        var x = start
        var up = true
        while x < end {
            x = min(x + stroke * 2, end)
            context.addLine(to: CGPoint(x: x, y: y + (up ? -stroke : stroke)))
            up.toggle()
        }
        context.strokePath()
        context.restoreGState()
    }
}

private extension Comparable {
    func clamped(to range: ClosedRange<Self>) -> Self {
        min(max(self, range.lowerBound), range.upperBound)
    }
}
#endif
