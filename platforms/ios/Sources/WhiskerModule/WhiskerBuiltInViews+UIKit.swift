#if canImport(UIKit)
import CoreText
import UIKit

/** Native container whose child geometry is supplied entirely by Rust. */
public class WhiskerContainerView: UIView {
    public override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        clipsToBounds = false
    }

    public required init?(coder: NSCoder) { nil }
}

/** Vertical native scroll container with a dedicated multi-child content view. */
public final class WhiskerScrollContainerView: UIScrollView, UIScrollViewDelegate, WhiskerEventSource {
    public let contentView = WhiskerContainerView(frame: .zero)
    public var whiskerContentView: WhiskerContainerView { contentView }
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
    private var scrollSequenceStart: CGPoint?
    private var reportingDrag = false
    private var lastScrollOffset = CGPoint.zero
    private var releaseVelocity = CGPoint.zero
    private var pendingLogicalOffset: (offset: CGFloat, smooth: Bool)?
    private var refreshEnabled = false
    private var refreshRequested = false
    private lazy var whiskerRefreshControl: UIRefreshControl = {
        let control = UIRefreshControl()
        control.addTarget(self, action: #selector(refreshControlTriggered), for: .valueChanged)
        return control
    }()

    public override var isScrollEnabled: Bool {
        didSet {
            if !isScrollEnabled { finishDrag(cancelled: true) }
        }
    }

    public override init(frame: CGRect) {
        super.init(frame: frame)
        alwaysBounceVertical = false
        // Whisker lays out against the complete edge-to-edge viewport and
        // exposes safe-area insets separately through the SafeArea module.
        // UIKit's automatic adjustment would add those insets a second time
        // and make the Host disagree with Rust about every child coordinate.
        contentInsetAdjustmentBehavior = .never
        automaticallyAdjustsScrollIndicatorInsets = false
        addSubview(contentView)
        delegate = self
        panGestureRecognizer.addTarget(self, action: #selector(panStateChanged))
    }

    public required init?(coder: NSCoder) { nil }

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
        setNeedsLayout()
    }

    public func setScrollOrientation(_ value: String) {
        horizontal = value == "horizontal"
        updateIndicatorVisibility()
        updateRefreshControl()
    }

    /** Attaches the native pull-to-refresh control while the axis is vertical. */
    public func setRefreshEnabled(_ enabled: Bool) {
        refreshEnabled = enabled
        updateRefreshControl()
    }

    /** Shows or hides the refresh indicator; a user pull starts it natively. */
    public func setRefreshing(_ refreshing: Bool) {
        refreshRequested = refreshing
        guard let control = refreshControl else { return }
        if refreshing, !control.isRefreshing {
            control.beginRefreshing()
        } else if !refreshing, control.isRefreshing {
            control.endRefreshing()
        }
    }

    private func updateRefreshControl() {
        let active = refreshEnabled && !horizontal
        // A pull needs vertical bounce even when the content fits the viewport.
        alwaysBounceVertical = active
        if active {
            guard refreshControl == nil else { return }
            refreshControl = whiskerRefreshControl
            if refreshRequested { whiskerRefreshControl.beginRefreshing() }
        } else if let control = refreshControl {
            control.endRefreshing()
            refreshControl = nil
        }
    }

    @objc private func refreshControlTriggered() {
        eventSink?("refresh", .map([:]))
    }

    public func setWhiskerChromeVisible(_ visible: Bool) {
        chromeVisible = visible
        updateIndicatorVisibility()
    }

    private func updateIndicatorVisibility() {
        showsHorizontalScrollIndicator = chromeVisible && horizontal
        showsVerticalScrollIndicator = chromeVisible && !horizontal
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

    public func scrollToLogicalOffset(_ offset: Double, smooth: Bool) {
        let logical = CGFloat(offset)
        // An offset beyond the current content is reset to the start by
        // UIKit's next layout pass, before `layoutSubviews` assigns the
        // content size that would hold it; re-applied there instead.
        pendingLogicalOffset = logical > maximumLogicalOffset + 0.5 ? (logical, smooth) : nil
        applyLogicalOffset(logical, smooth: smooth)
    }

    private var maximumLogicalOffset: CGFloat {
        horizontal
            ? max(0, contentSize.width - bounds.width)
            : max(0, contentSize.height - bounds.height)
    }

    private func applyLogicalOffset(_ offset: CGFloat, smooth: Bool) {
        var target = contentOffset
        if horizontal { target.x = offset }
        else { target.y = offset }
        setContentOffset(target, animated: smooth)
    }

    public func scrollByLogicalOffset(_ offset: Double, smooth: Bool) {
        let current = horizontal ? contentOffset.x : contentOffset.y
        scrollToLogicalOffset(Double(current) + offset, smooth: smooth)
    }

    public func scrollViewWillBeginDragging(_ scrollView: UIScrollView) {
        pendingLogicalOffset = nil
        scrollSequenceStart = contentOffset
        releaseVelocity = .zero
        reportingDrag = true
        emitScroll()
    }

    @objc private func panStateChanged() {
        if panGestureRecognizer.state == .cancelled || panGestureRecognizer.state == .failed {
            finishDrag(cancelled: true)
        }
    }

    public func scrollViewDidEndDragging(_ scrollView: UIScrollView, willDecelerate decelerate: Bool) {
        finishDrag(cancelled: panGestureRecognizer.state == .cancelled || !isScrollEnabled)
    }

    private func finishDrag(cancelled: Bool) {
        guard reportingDrag else { return }
        reportingDrag = false
        let velocity = cancelled ? CGPoint.zero : releaseVelocity
        releaseVelocity = .zero
        emitScroll(velocity: velocity, cancelled: cancelled)
    }

    public func scrollViewDidScroll(_ scrollView: UIScrollView) {
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
            "scrollWidth": .float(Double(contentSize.width)),
            "scrollHeight": .float(Double(contentSize.height)),
            "viewportWidth": .float(Double(bounds.width)),
            "viewportHeight": .float(Double(bounds.height)),
        ]))
    }

    public func scrollViewWillEndDragging(
        _ scrollView: UIScrollView,
        withVelocity velocity: CGPoint,
        targetContentOffset: UnsafeMutablePointer<CGPoint>
    ) {
        // UIKit supplies content velocity in points/ms; Whisker uses logical px/s.
        releaseVelocity = CGPoint(x: horizontal ? velocity.x * 1000 : 0,
                                  y: horizontal ? 0 : velocity.y * 1000)
        guard let factor = snapFactor, !contentView.subviews.isEmpty else { return }
        let viewport = horizontal ? bounds.width : bounds.height
        let contentExtent = horizontal ? contentSize.width : contentSize.height
        let proposed = horizontal ? targetContentOffset.pointee.x : targetContentOffset.pointee.y
        let maximum = max(0, contentExtent - viewport)
        let targets = contentView.subviews
            .map { child -> CGFloat in
                let frame = child.frame
                let start = horizontal ? frame.minX : frame.minY
                let size = horizontal ? frame.width : frame.height
                return (start + size * factor - viewport * factor + snapOffset)
                    .clamped(to: 0...maximum)
            }
            .sorted()
        let startPoint = scrollSequenceStart ?? contentOffset
        let start = horizontal ? startPoint.x : startPoint.y
        let target: CGFloat
        if snapStopAlways, proposed > start + .ulpOfOne {
            target = targets.first(where: { $0 > start + .ulpOfOne }) ?? targets.last ?? proposed
        } else if snapStopAlways, proposed < start - .ulpOfOne {
            target = targets.last(where: { $0 < start - .ulpOfOne }) ?? targets.first ?? proposed
        } else {
            target = targets.min(by: { abs($0 - proposed) < abs($1 - proposed) }) ?? proposed
        }
        scrollSequenceStart = nil
        if horizontal { targetContentOffset.pointee.x = target }
        else { targetContentOffset.pointee.y = target }
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        let directExtent = contentView.subviews.reduce(CGRect.zero) { result, child in
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
        let size = CGSize(
            width: max(bounds.width, extent.width),
            height: max(bounds.height, extent.height)
        )
        contentSize = size
        contentView.frame = CGRect(origin: .zero, size: size)
        if let pending = pendingLogicalOffset, pending.offset <= maximumLogicalOffset + 0.5 {
            pendingLogicalOffset = nil
            applyLogicalOffset(pending.offset, smooth: pending.smooth)
        }
    }
}

/** Native text element implementing Whisker's single-line decoration contract. */
public final class WhiskerTextLabel: UILabel, WhiskerEventSource {
    private var explicitAccessibilityLabel: String?
    public override var accessibilityLabel: String? {
        get { explicitAccessibilityLabel ?? richParagraph?.accessibleText ?? super.accessibilityLabel }
        set { explicitAccessibilityLabel = newValue; super.accessibilityLabel = newValue }
    }
    public override var accessibilityCustomActions: [UIAccessibilityCustomAction]? {
        get {
            let revision = preparedContent
            let actions = richParagraph?.accessibleActions.map { span, label in
                UIAccessibilityCustomAction(name: label) { [weak self] _ in
                    guard let self, self.preparedContent == revision else { return false }
                    self.textEventSink?("textactivate", .map(["span": .int(Int64(span)), "revision": .int(Int64(revision))]))
                    return true
                }
            } ?? []
            return (super.accessibilityCustomActions ?? []) + actions
        }
        set { super.accessibilityCustomActions = newValue }
    }
    var selectionController: WhiskerTextSelection?
    var textEventSink: ((String, WhiskerValue) -> Void)?
    var preparedContent: UInt64 = 0
    public func installWhiskerEventSink(_ sink: ((String, WhiskerValue) -> Void)?) { textEventSink = sink }
    private var whiskerIndent = WhiskerTextIndent()
    private var richLayout: WhiskerParagraphLayout?
    private var plainLayout: WhiskerParagraphLayout?
    var richParagraph: WhiskerParagraph?
    public override var attributedText: NSAttributedString? {
        didSet { plainLayout = nil }
    }
    public var richContent: NSAttributedString? {
        didSet { richLayout = nil; setNeedsDisplay() }
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

    public func setWhiskerIndent(_ indent: WhiskerTextIndent) {
        whiskerIndent = indent
        appliedIndent = nil
        applyWhiskerIndent()
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        applyWhiskerIndent()
        if let richLayout, richLayout.container.size.width != bounds.width { self.richLayout = nil }
        selectionController?.synchronize()
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
        didSet { setNeedsDisplay() }
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
        setNeedsDisplay()
    }

    func preparedParagraphLayout(width: CGFloat) -> WhiskerParagraphLayout? {
        guard let richContent else { return nil }
        if let richLayout, richLayout.container.size.width == width { return richLayout }
        let layout = WhiskerParagraphLayout(text: richContent, width: width, maxLines: numberOfLines, overflow: richOverflow)
        richLayout = layout
        return layout
    }

    public override func drawText(in rect: CGRect) {
        if selectionController != nil { return }
        if let layout = preparedParagraphLayout(width: rect.width) {
            layout.draw(at: rect.origin)
            return
        }
        if let plainLayout, drawPlain(plainLayout, in: rect) { return }
        super.drawText(in: rect)
        guard let decoration = whiskerDecoration, decoration.style == .wavy else { return }
        let textRect = self.textRect(forBounds: rect, limitedToNumberOfLines: numberOfLines)
        let width = min(textRect.width, sizeThatFits(textRect.size).width)
        guard width > 0 else { return }
        drawWave(decoration, from: textRect.minX, to: textRect.minX + width, baseline: textRect.minY + font.ascender)
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
        guard let context = UIGraphicsGetCurrentContext() else { return }
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
