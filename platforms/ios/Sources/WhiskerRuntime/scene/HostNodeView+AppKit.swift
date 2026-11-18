#if !canImport(UIKit) && canImport(AppKit)
import AppKit
import WhiskerModule

private final class OverflowMaskLayer: CAShapeLayer {
    // The clipping viewport must track native scrolling without implicit interpolation.
    override func action(forKey event: String) -> CAAction? { nil }
}

private final class HostPaintLayer: CALayer {
    // A layer outside a view animates every property change unless told otherwise.
    override func action(forKey event: String) -> CAAction? { nil }
}

/// Common macOS wrapper for every built-in or custom Whisker element.
///
/// The scene owner controls hierarchy and geometry. Element modules only own
/// the mounted content view placed inside this wrapper.
final class WhiskerNodeView: NSView {
    let element: String
    var contentFrame = CGRect.zero
    var paint: HostBoxPaint?
    let boxPainter = HostBoxPainter()
    var mountedElement: WhiskerMountedElement?
    private var defaultChildrenHost: NSView?
    private var viewContent: WhiskerContainerView?
    private var hostsChildrenInContent = false
    private var boxPresentation: HostBoxPresentation?
    private var paintedRevision: UInt64?
    private var paintedContentBox = CGRect.null
    private var paintedSize = CGSize.zero
    private var uniformClipHost: NSView?
    private var uniformClipRestoresMasksToBounds = false
    private var contentCornerClipRestoresMasksToBounds: Bool?
    private let overflowMask = OverflowMaskLayer()
    private var overflowMaskScrollOrigin = CGPoint.zero
    private var overflowMaskCompositionBounds = CGRect.zero
    private var ancestorScrollObservers: [NSObjectProtocol] = []
    private var scrollOffsetObserver: NSObjectProtocol?
    private let clipPathMask = CAShapeLayer()
    private let contentClipMask = OverflowMaskLayer()
    private var clipPath: HostClipPath?
    private var boxShadows: [HostBoxShadow] = []
    private var boxShadowLayers: [CAShapeLayer] = []
    private var boxShadowMaskLayers: [CAShapeLayer] = []
    private var clipsOverflowHorizontally = false
    private var clipsOverflowVertically = false
    private var backdropBlurView: HostBackdropBlurView?
    private(set) var imageRendering: HostImageRendering = .auto
    private(set) var hitTestBehavior: Int32 = 0
    private(set) var cursorKeyword: Int32 = 0
    private var whiskerVisible = true

    var backingLayer: CALayer { layer! }

    override var isFlipped: Bool { true }

    override func makeBackingLayer() -> CALayer { HostNodeLayer() }

    init(element: String) {
        self.element = element
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        backingLayer.masksToBounds = false
    }

    required init?(coder: NSCoder) { nil }

    deinit {
        let center = NotificationCenter.default
        ancestorScrollObservers.forEach(center.removeObserver)
        scrollOffsetObserver.map(center.removeObserver)
    }

    func setAccessibility(_ raw: WhiskerValue) {
        guard case .map(let value) = raw else { return }
        let label = value["label"]?.asString
        let hint = value["hint"]?.asString
        let role = value["role"]?.asString
        let identifier = value["identifier"]?.asString
        let hidden = value["hidden"]?.asBool ?? false
        let modal = value["modal"]?.asBool ?? false
        let state: [String: WhiskerValue]
        if case .map(let map) = value["state"] { state = map } else { state = [:] }
        let hasSemantics = label != nil || hint != nil || role != nil
            || state["disabled"]?.asBool != nil || state["selected"]?.asBool != nil
            || state["checked"]?.asString != nil || state["expanded"]?.asBool != nil

        setAccessibilityLabel(label)
        setAccessibilityHelp(hint)
        setAccessibilityIdentifier(identifier ?? "")
        setAccessibilityModal(modal)
        setAccessibilityElement(!hidden && hasSemantics && role != "group")
        let accessibilityRole: NSAccessibility.Role? = switch role {
        case "button", "tab": .button
        case "link": .link
        case "image": .image
        case "text", "header": .staticText
        case "checkbox", "switch": .checkBox
        case "radio": .radioButton
        case "adjustable": .slider
        case "searchbox": .textField
        case "group": .group
        default: nil
        }
        setAccessibilityRole(accessibilityRole)
        setAccessibilityEnabled(state["disabled"]?.asBool != true)
        setAccessibilitySelected(state["selected"]?.asBool == true)
        if let checked = state["checked"]?.asString {
            setAccessibilityValue(checked == "mixed" ? 2 : (checked == "true" ? 1 : 0))
        } else if let expanded = state["expanded"]?.asBool {
            setAccessibilityExpanded(expanded)
            setAccessibilityValue(nil)
        } else {
            setAccessibilityValue(nil)
        }
    }

    override func viewWillMove(toSuperview newSuperview: NSView?) {
        invalidateAncestorScrollExtent()
        super.viewWillMove(toSuperview: newSuperview)
    }

    override func viewDidMoveToSuperview() {
        super.viewDidMoveToSuperview()
        invalidateAncestorScrollExtent()
        updateAncestorScrollObservations()
        updateOverflowMask()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        updateAncestorScrollObservations()
        updateOverflowMask()
    }

    override func layout() {
        super.layout()
        if let mountedElement, viewContent == nil {
            mountedElement.view.frame = contentFrame
        }
        updateBoxPaint()
        updateBackdropBlurGeometry()
        updateBoxShadowLayers()
        updateOverflowMask()
        updateClipPathMask()
        updateContentClipMask()
    }

    func setLayoutFrame(_ frame: CGRect) {
        setFrameSize(frame.size)
        setFrameOrigin(frame.origin)
        boxPresentation?.setFrame(bounds, in: self)
        updateBackdropBlurGeometry()
        updateOverflowMask()
        updateClipPathMask()
        invalidateAncestorScrollExtent()
    }

    func setPresentationTransform(_ values: UnsafeBufferPointer<Float>) {
        precondition(values.count == 16)
        var transform = CATransform3DIdentity
        transform.m11 = CGFloat(values[0])
        transform.m12 = CGFloat(values[1])
        transform.m13 = CGFloat(values[2])
        transform.m14 = CGFloat(values[3])
        transform.m21 = CGFloat(values[4])
        transform.m22 = CGFloat(values[5])
        transform.m23 = CGFloat(values[6])
        transform.m24 = CGFloat(values[7])
        transform.m31 = CGFloat(values[8])
        transform.m32 = CGFloat(values[9])
        transform.m33 = CGFloat(values[10])
        transform.m34 = CGFloat(values[11])
        transform.m41 = CGFloat(values[12])
        transform.m42 = CGFloat(values[13])
        transform.m43 = CGFloat(values[14])
        transform.m44 = CGFloat(values[15])
        (backingLayer as! HostNodeLayer).presentationTransform = transform
        invalidateAncestorScrollExtent()
    }

    func setOverflowClip(horizontal: Bool, vertical: Bool) {
        clipsOverflowHorizontally = horizontal
        clipsOverflowVertically = vertical
        updateAncestorScrollObservations()
        updateOverflowMask()
        invalidateAncestorScrollExtent()
    }

    func sceneChildrenHost() -> NSView {
        if let host = mountedElement?.childrenHost() { return host }
        if let viewContent { return hostsChildrenInContent ? viewContent : self }
        if let defaultChildrenHost { return defaultChildrenHost }
        let host = WhiskerChildrenHostView(frame: bounds)
        host.layer?.zPosition = 2
        addSubview(host)
        defaultChildrenHost = host
        updateOverflowMask()
        return host
    }

    /// The children host only when one exists, so a childless element never creates one.
    private func existingChildrenHost() -> NSView? {
        if let host = mountedElement?.childrenHost() { return host }
        if let viewContent { return hostsChildrenInContent ? viewContent : self }
        return defaultChildrenHost
    }

    func installMountedContent() {
        mountedElement?.view.frame = bounds
        mountedElement?.view.layer?.zPosition = 2
        if let mountedElement, mountedElement.registration.name == WhiskerBuiltInElements.viewName,
           let content = mountedElement.view as? WhiskerContainerView {
            // The built-in View's content view draws nothing, so it stays out of the
            // hierarchy until `updateChildrenHosting` needs it between this view and the children.
            content.removeFromSuperview()
            content.autoresizingMask = []
            content.isHidden = false
            content.layer?.mask = nil
            content.layer?.cornerRadius = 0
            content.layer?.masksToBounds = false
            viewContent = content
            hostsChildrenInContent = false
        } else if let content = mountedElement?.view {
            content.autoresizingMask = [.width, .height]
            addSubview(content)
        }
        if let defaultChildrenHost { bringSubviewToFront(defaultChildrenHost) }
        let scrollView = mountedElement?.view as? WhiskerScrollContainerView
        scrollView?.installWhiskerContentExtentSource { [weak self] in
            self?.scrollableChildrenExtent() ?? .zero
        }
        scrollOffsetObserver.map(NotificationCenter.default.removeObserver)
        scrollOffsetObserver = nil
        if let scrollView {
            scrollOffsetObserver = NotificationCenter.default.addObserver(
                forName: NSView.boundsDidChangeNotification,
                object: scrollView.contentView,
                queue: nil
            ) { [weak self, weak scrollView] _ in
                guard let scrollView else { return }
                MainActor.assumeIsolated { self?.updateOverflowMaskPosition(scrollView) }
            }
        }
        updateOverflowMask()
    }

    private func invalidateAncestorScrollExtent() {
        var ancestor = superview
        while let current = ancestor {
            if let scroll = current as? WhiskerScrollContainerView {
                scroll.invalidateWhiskerContentExtent()
                return
            }
            ancestor = current.superview
        }
    }

    private func scrollableChildrenExtent() -> CGSize {
        guard let host = existingChildrenHost() else { return .zero }
        var right: CGFloat = 0
        var bottom: CGFloat = 0
        for child in host.subviews {
            let childBounds = (child as? WhiskerNodeView)?.scrollableOverflowBounds() ?? child.bounds
            let extent = child.convert(childBounds, to: host)
            right = max(right, extent.maxX)
            bottom = max(bottom, extent.maxY)
        }
        return CGSize(width: right, height: bottom)
    }

    private func scrollableOverflowBounds() -> CGRect {
        // Nested scroll contents belong to their own viewport, not the outer scroll range.
        guard !(mountedElement?.view is NSScrollView) else { return bounds }
        layoutSubtreeIfNeeded()
        guard let host = existingChildrenHost() else { return bounds }
        let extent = scrollableChildrenExtent()
        let rect = host.convert(CGRect(origin: .zero, size: extent), to: self)
        return CGRect(
            origin: bounds.origin,
            size: CGSize(
                width: clipsOverflowHorizontally ? bounds.width : max(bounds.width, rect.maxX - bounds.minX),
                height: clipsOverflowVertically ? bounds.height : max(bounds.height, rect.maxY - bounds.minY)
            )
        )
    }

    func boxPaintDidChange() {
        updateBoxPaint()
        updateBackdropBlurGeometry()
        updateBoxShadowLayers()
        updateOverflowMask()
        updateClipPathMask()
        updateContentClipMask()
    }

    func setClipPath(_ clipPath: HostClipPath?) {
        self.clipPath = clipPath
        updateClipPathMask()
    }

    func setBoxShadows(_ shadows: [HostBoxShadow]) {
        while boxShadowLayers.count > shadows.count {
            boxShadowLayers.removeLast().removeFromSuperlayer()
            boxShadowMaskLayers.removeLast()
        }
        while boxShadowLayers.count < shadows.count {
            boxShadowLayers.append(CAShapeLayer())
            boxShadowMaskLayers.append(CAShapeLayer())
        }
        boxShadows = shadows
        boxShadowLayers.forEach { $0.removeFromSuperlayer() }
        // CSS lists the frontmost shadow first. Re-add back-to-front so that
        // the first entry remains visually above later entries.
        for index in shadows.indices.reversed() {
            let shadowLayer = boxShadowLayers[index]
            shadowLayer.isHidden = !whiskerVisible
            if shadows[index].inset {
                ensureBoxPresentation().layer.addSublayer(shadowLayer)
            } else {
                shadowLayer.zPosition = -1
                backingLayer.addSublayer(shadowLayer)
            }
        }
        updateBoxShadowLayers()
    }

    func setBackdropBlur(_ radius: CGFloat) {
        if radius <= 0 {
            backdropBlurView?.setBlurRadius(0)
            return
        }
        let blurView: HostBackdropBlurView
        if let existing = backdropBlurView {
            blurView = existing
        } else {
            blurView = HostBackdropBlurView()
            blurView.layer?.zPosition = -0.5
            blurView.isHidden = !whiskerVisible
            insertSubview(blurView, at: 0)
            backdropBlurView = blurView
            updateChildrenHosting()
        }
        blurView.setBlurRadius(radius)
        updateBackdropBlurGeometry()
    }

    func setImageRendering(_ value: HostImageRendering) {
        imageRendering = value
        boxPainter.setImageRendering(value)
        updateBoxPaint()
    }

    func setHitTestBehavior(_ value: Int32) {
        precondition((0...3).contains(value))
        hitTestBehavior = value
    }

    func setCursorKeyword(_ value: Int32) {
        precondition((0...34).contains(value))
        cursorKeyword = value
        window?.invalidateCursorRects(for: self)
    }

    override func resetCursorRects() {
        guard let cursor = hostCursor(keyword: cursorKeyword) else { return }
        addCursorRect(bounds, cursor: cursor)
    }

    /// Applies computed CSS visibility to this element's own presentation.
    /// Descendant node views stay mounted so an explicit `visibility: visible`
    /// can override a hidden ancestor.
    func setWhiskerVisibility(_ visible: Bool) {
        guard whiskerVisible != visible else { return }
        whiskerVisible = visible
        if !visible, let responder = window?.firstResponder as? NSView, responder.isDescendant(of: self) {
            window?.makeFirstResponder(nil)
        }
        boxPresentation?.setHidden(!visible)
        boxShadowLayers.forEach { $0.isHidden = !visible }
        backdropBlurView?.isHidden = !visible
        (mountedElement?.view as? WhiskerScrollContainerView)?
            .setWhiskerChromeVisible(visible)
        if let mountedElement, mountedElement.childrenHost() == nil, viewContent == nil {
            mountedElement.view.isHidden = !visible
        }
    }

    var cursorPresentation: HostCursorPresentation {
        hostCursorPresentation(keyword: cursorKeyword)
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        if !whiskerVisible {
            guard hitTestBehavior != 1, hitTestBehavior != 2,
                  let result = super.hitTest(point), result !== self
            else { return nil }
            var ancestor: NSView? = result
            while let current = ancestor, current !== self {
                if current is WhiskerNodeView { return result }
                ancestor = current.superview
            }
            return nil
        }
        switch hitTestBehavior {
        case 1:
            return nil
        case 2:
            let local = superview.map { convert(point, from: $0) } ?? point
            return bounds.contains(local) ? self : nil
        case 3:
            let result = super.hitTest(point)
            return result === self ? nil : result
        default:
            return super.hitTest(point)
        }
    }

    private func updateBackdropBlurGeometry() {
        guard let backdropBlurView else { return }
        backdropBlurView.frame = bounds
        backdropBlurView.setShape(boxPainter.borderBoxPath(in: bounds), in: bounds)
    }

    private func updateBoxShadowLayers() {
        for index in boxShadows.indices {
            updateBoxShadowLayer(
                boxShadowLayers[index],
                maskLayer: boxShadowMaskLayers[index],
                shadow: boxShadows[index]
            )
        }
    }

    private func updateBoxShadowLayer(
        _ shadowLayer: CAShapeLayer,
        maskLayer: CAShapeLayer,
        shadow: HostBoxShadow
    ) {
        if shadow.inset {
            guard let shadowPath = boxPainter.insetBoxShadowPath(in: bounds, shadow: shadow) else {
                shadowLayer.path = nil
                shadowLayer.mask = nil
                shadowLayer.shadowPath = nil
                return
            }
            shadowLayer.frame = bounds
            shadowLayer.path = shadowPath
            shadowLayer.fillColor = shadow.color.cgColor
            shadowLayer.fillRule = .evenOdd
            shadowLayer.strokeColor = nil
            // Let Core Animation derive the shadow from the even-odd ring;
            // `shadowPath` itself does not carry the layer's fill rule.
            shadowLayer.shadowPath = nil
            shadowLayer.shadowColor = shadow.color.withAlphaComponent(1).cgColor
            shadowLayer.shadowOpacity = Float(shadow.color.cgColor.alpha)
            shadowLayer.shadowOffset = .zero
            shadowLayer.shadowRadius = shadow.blurRadius / 2
            maskLayer.frame = bounds
            maskLayer.path = boxPainter.paddingBoxPath(in: bounds)
            maskLayer.fillColor = NSColor.white.cgColor
            maskLayer.fillRule = .nonZero
            shadowLayer.mask = maskLayer
            return
        }
        guard let shadowPath = boxPainter.hardBoxShadowPath(in: bounds, shadow: shadow) else {
            shadowLayer.path = nil
            shadowLayer.mask = nil
            shadowLayer.shadowPath = nil
            return
        }
        let blurExtent = shadow.blurRadius * 1.5
        let layerFrame = shadowPath.boundingBoxOfPath
            .insetBy(dx: -blurExtent, dy: -blurExtent)
            .union(bounds)
        var translation = CGAffineTransform(
            translationX: -layerFrame.minX,
            y: -layerFrame.minY
        )
        shadowLayer.frame = layerFrame
        shadowLayer.path = shadowPath.copy(using: &translation)
        shadowLayer.fillColor = shadow.color.cgColor
        shadowLayer.fillRule = .nonZero
        shadowLayer.strokeColor = nil
        shadowLayer.shadowPath = shadowPath.copy(using: &translation)
        shadowLayer.shadowColor = shadow.color.withAlphaComponent(1).cgColor
        shadowLayer.shadowOpacity = Float(shadow.color.cgColor.alpha)
        // AppKit's backing layers use bottom-left geometry, so a zero offset is the only orientation-free choice.
        shadowLayer.shadowOffset = .zero
        shadowLayer.shadowRadius = shadow.blurRadius / 2

        let maskPath = CGMutablePath()
        maskPath.addRect(CGRect(origin: .zero, size: layerFrame.size))
        if let borderPath = boxPainter.borderBoxPath(in: bounds).copy(using: &translation) {
            maskPath.addPath(borderPath)
        }
        maskLayer.frame = CGRect(origin: .zero, size: layerFrame.size)
        maskLayer.path = maskPath
        maskLayer.fillColor = NSColor.white.cgColor
        maskLayer.fillRule = .evenOdd
        shadowLayer.mask = maskLayer
    }

    private func updateBoxPaint() {
        boxPresentation?.setFrame(bounds, in: self)
        switch boxPainter.paintPlan(in: bounds) {
        case .none:
            boxPresentation?.showLayerPaint(HostLayerBoxPaint())
        case .layer(let layerPaint):
            ensureBoxPresentation().showLayerPaint(layerPaint)
        case .draw:
            let presentation = ensureBoxPresentation()
            let stale = paintedRevision != boxPainter.revision ||
                paintedContentBox != contentFrame || paintedSize != bounds.size
            presentation.showDrawnPaint(in: self, contentBox: contentFrame, redraw: stale)
            paintedRevision = boxPainter.revision
            paintedContentBox = contentFrame
            paintedSize = bounds.size
        }
        updateChildrenHosting()
    }

    private func ensureBoxPresentation() -> HostBoxPresentation {
        if let boxPresentation { return boxPresentation }
        let presentation = HostBoxPresentation(painter: boxPainter)
        presentation.setFrame(bounds, in: self)
        presentation.setHidden(!whiskerVisible)
        boxPresentation = presentation
        return presentation
    }

    /// Children of the built-in View are subviews of this view itself. Its content view
    /// comes between them only for a clip, or when this view holds painted subviews:
    /// the scene inserts children by index, which only counts them when they are alone.
    private func updateChildrenHosting() {
        guard let viewContent else { return }
        let needsContent = clipsOverflowHorizontally || clipsOverflowVertically ||
            backdropBlurView != nil || boxPresentation?.drawsInView == true
        guard needsContent != hostsChildrenInContent else { return }
        if needsContent {
            viewContent.frame = bounds
            viewContent.bounds = bounds
            addSubview(viewContent)
            for child in subviews where child is WhiskerNodeView { viewContent.addSubview(child) }
        } else {
            for child in viewContent.subviews where child is WhiskerNodeView { addSubview(child) }
            viewContent.layer?.mask = nil
            removeUniformClip(from: viewContent)
            viewContent.removeFromSuperview()
        }
        hostsChildrenInContent = needsContent
    }

    private func updateOverflowMask() {
        updateChildrenHosting()
        guard let host = overflowClipHost() else { return }
        guard clipsOverflowHorizontally || clipsOverflowVertically else {
            host.layer?.mask = nil
            removeUniformClip(from: host)
            return
        }
        if clipsOverflowHorizontally, clipsOverflowVertically,
           let clip = boxPainter.uniformOverflowClip(in: bounds),
           installUniformClip(clip, on: host) {
            host.layer?.mask = nil
            return
        }
        removeUniformClip(from: host)
        let compositionBounds = hostCompositionBounds()
        let nodePath = boxPainter.overflowClipPath(
            in: bounds,
            visibleBounds: compositionBounds,
            horizontal: clipsOverflowHorizontally,
            vertical: clipsOverflowVertically
        )
        let origin = host.convert(CGPoint.zero, from: self)
        let xUnit = host.convert(CGPoint(x: 1, y: 0), from: self)
        let yUnit = host.convert(CGPoint(x: 0, y: 1), from: self)
        var conversion = CGAffineTransform(
            a: xUnit.x - origin.x,
            b: xUnit.y - origin.y,
            c: yUnit.x - origin.x,
            d: yUnit.y - origin.y,
            tx: origin.x,
            ty: origin.y
        )
        guard let convertedPath = nodePath.copy(using: &conversion) else { return }
        let maskFrame = convertedPath.boundingBoxOfPath
        var maskTranslation = CGAffineTransform(
            translationX: -maskFrame.minX,
            y: -maskFrame.minY
        )
        overflowMask.frame = maskFrame
        overflowMask.path = convertedPath.copy(using: &maskTranslation)
        overflowMask.backgroundColor = NSColor.clear.cgColor
        overflowMask.fillColor = NSColor.white.cgColor
        host.layer?.mask = overflowMask
        overflowMaskScrollOrigin = .zero
        overflowMaskCompositionBounds = compositionBounds
    }

    /// The overflow mask lives on the stationary scroll view, so scrolling never moves it.
    private func updateOverflowMaskPosition(_ scrollView: NSScrollView) {
        overflowMaskScrollOrigin = scrollView.contentView.bounds.origin
    }

    private func updateAncestorScrollObservations() {
        let center = NotificationCenter.default
        ancestorScrollObservers.forEach(center.removeObserver)
        ancestorScrollObservers.removeAll()
        guard clipsOverflowHorizontally != clipsOverflowVertically else { return }
        var ancestor = superview
        while let current = ancestor {
            if let scrollView = current as? NSScrollView {
                ancestorScrollObservers.append(center.addObserver(
                    forName: NSView.boundsDidChangeNotification,
                    object: scrollView.contentView,
                    queue: nil
                ) { [weak self] _ in
                    MainActor.assumeIsolated { self?.updateOverflowMaskForAncestorScroll() }
                })
            }
            ancestor = current.superview
        }
    }

    private func updateOverflowMaskForAncestorScroll() {
        let next = hostCompositionBounds()
        var frame = overflowMask.frame
        if !clipsOverflowHorizontally {
            frame.origin.x += next.minX - overflowMaskCompositionBounds.minX
        }
        if !clipsOverflowVertically {
            frame.origin.y += next.minY - overflowMaskCompositionBounds.minY
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        overflowMask.frame = frame
        CATransaction.commit()
        overflowMaskCompositionBounds = next
    }

    /// Returns the stationary view that owns the element's clipping viewport.
    ///
    /// A children host is allowed to move inside its element root: the
    /// document view of an `NSScrollView` moves as the user scrolls, so the
    /// mask belongs on the mounted root, which stays put.
    private func overflowClipHost() -> NSView? {
        if let mountedElement, mountedElement.childrenHost() != nil {
            return mountedElement.view
        }
        if let viewContent { return hostsChildrenInContent ? viewContent : nil }
        return defaultChildrenHost
    }

    /// A mask layer is rendered offscreen on every frame its subtree is on screen;
    /// clipping to the host's own bounds is not.
    private func installUniformClip(_ clip: HostUniformClip, on host: NSView) -> Bool {
        if host === defaultChildrenHost || host === viewContent {
            // The bounds origin keeps children in this element's coordinates while
            // the host itself shrinks to the padding box it clips to.
            host.frame = clip.rect
            host.bounds = clip.rect
        } else {
            // A module's own view keeps its geometry, so it can only clip to itself.
            let frame = host.frame
            guard abs(frame.minX - clip.rect.minX) < 0.01, abs(frame.minY - clip.rect.minY) < 0.01,
                  abs(frame.width - clip.rect.width) < 0.01,
                  abs(frame.height - clip.rect.height) < 0.01
            else { return false }
        }
        host.wantsLayer = true
        guard let layer = host.layer else { return false }
        if uniformClipHost !== host {
            uniformClipRestoresMasksToBounds = layer.masksToBounds
            uniformClipHost = host
        }
        layer.cornerRadius = clip.cornerRadius
        layer.masksToBounds = true
        return true
    }

    private func removeUniformClip(from host: NSView) {
        if uniformClipHost === host {
            host.layer?.cornerRadius = 0
            host.layer?.masksToBounds = uniformClipRestoresMasksToBounds
            uniformClipHost = nil
        }
        if host === defaultChildrenHost || host === viewContent {
            host.frame = bounds
            host.bounds = bounds
        }
    }

    private func updateClipPathMask() {
        guard let clipPath else {
            backingLayer.mask = nil
            return
        }
        clipPathMask.frame = bounds
        clipPathMask.path = clipPath.path(
            in: bounds,
            contentBox: contentFrame,
            painter: boxPainter
        )
        clipPathMask.fillColor = NSColor.white.cgColor
        clipPathMask.fillRule = clipPath.fillRule
        backingLayer.mask = clipPathMask
    }

    /// Rounds a childless element's native content like a CSS replaced element.
    private func updateContentClipMask() {
        guard let mountedElement else { return }
        let contentView = mountedElement.view
        let childless = mountedElement.registration.childPolicy == .none
        if childless,
           let radius = boxPainter.uniformContentClipRadius(in: bounds, contentBox: contentFrame),
           radius > 0 {
            contentView.wantsLayer = true
            guard let contentLayer = contentView.layer else { return }
            if contentLayer.mask === contentClipMask { contentLayer.mask = nil }
            if contentCornerClipRestoresMasksToBounds == nil {
                contentCornerClipRestoresMasksToBounds = contentLayer.masksToBounds
            }
            contentLayer.cornerRadius = radius
            contentLayer.masksToBounds = true
            return
        }
        if let restore = contentCornerClipRestoresMasksToBounds {
            contentView.layer?.cornerRadius = 0
            contentView.layer?.masksToBounds = restore
            contentCornerClipRestoresMasksToBounds = nil
        }
        guard childless,
              let path = boxPainter.contentBoxClipPath(in: bounds, contentBox: contentFrame)
        else {
            if contentView.layer?.mask === contentClipMask { contentView.layer?.mask = nil }
            return
        }
        contentView.wantsLayer = true
        guard let contentLayer = contentView.layer else { return }
        // An unflipped content view's layer has a bottom-left origin.
        var translation = contentView.isFlipped
            ? CGAffineTransform(translationX: -contentFrame.minX, y: -contentFrame.minY)
            : CGAffineTransform(a: 1, b: 0, c: 0, d: -1, tx: -contentFrame.minX, ty: contentFrame.maxY)
        contentClipMask.frame = CGRect(origin: .zero, size: contentFrame.size)
        contentClipMask.path = path.copy(using: &translation)
        contentClipMask.fillColor = NSColor.white.cgColor
        contentLayer.mask = contentClipMask
    }

    private func hostCompositionBounds() -> CGRect {
        var host: NSView = self
        while let parent = host.superview { host = parent }
        return convert(host.bounds, from: host)
    }
}

enum HostCursorPresentation: Equatable {
    case system
    case hidden
    case verticalBeam
    case horizontalBeam
    case crosshair
    case horizontalResize
    case verticalResize
    case northwestSoutheastResize
    case northeastSouthwestResize
    case unsupportedSystemFallback
}

func hostCursorPresentation(keyword: Int32) -> HostCursorPresentation {
    switch keyword {
    case 0, 1:
        .system
    case 2:
        .hidden
    case 9:
        .crosshair
    case 10:
        .verticalBeam
    case 11:
        .horizontalBeam
    case 19, 22, 24, 29:
        .horizontalResize
    case 20, 21, 23, 30:
        .verticalResize
    case 26, 27, 32:
        .northwestSoutheastResize
    case 25, 28, 31:
        .northeastSouthwestResize
    default:
        .unsupportedSystemFallback
    }
}

/// Maps protocol `CursorKeyword` discriminants to AppKit's system cursors; `nil` keeps the inherited cursor.
private func hostCursor(keyword: Int32) -> NSCursor? {
    switch keyword {
    case 3: .contextualMenu
    case 5: .pointingHand
    case 9: .crosshair
    case 10: .iBeam
    case 11: .iBeamCursorForVerticalLayout
    case 12: .dragLink
    case 13: .dragCopy
    case 15, 16: .operationNotAllowed
    case 17: .openHand
    case 18: .closedHand
    case 19, 22, 24, 29: .resizeLeftRight
    case 20, 21, 23, 30: .resizeUpDown
    case 25, 28, 31:
        if #available(macOS 15, *) {
            .frameResize(position: .topRight, directions: .all)
        } else {
            nil
        }
    case 26, 27, 32:
        if #available(macOS 15, *) {
            .frameResize(position: .topLeft, directions: .all)
        } else {
            nil
        }
    case 33:
        if #available(macOS 15, *) { .zoomIn } else { nil }
    case 34:
        if #available(macOS 15, *) { .zoomOut } else { nil }
    case 0, 1: nil
    default: .arrow
    }
}

private final class WhiskerChildrenHostView: NSView {
    override var isFlipped: Bool { true }

    override init(frame: CGRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.masksToBounds = false
    }

    required init?(coder: NSCoder) { nil }

    override func hitTest(_ point: NSPoint) -> NSView? {
        let result = super.hitTest(point)
        return result === self ? nil : result
    }
}

/// AppKit resets a backing layer's transform whenever it rebuilds the layer
/// tree, so the Whisker transform is pinned on the layer itself.
private final class HostNodeLayer: CALayer {
    var presentationTransform = CATransform3DIdentity {
        didSet { super.transform = presentationTransform }
    }

    override var transform: CATransform3D {
        get { super.transform }
        set { super.transform = presentationTransform }
    }
}

/// Presents an element's box paint from layer properties, and rasterizes only
/// the paints those cannot express: a `draw(_:)` view owns a bitmap as large as
/// the element, which most elements never need.
private final class HostBoxPresentation {
    let layer = HostPaintLayer()
    private var drawView: HostNodePaintView?
    private unowned let painter: HostBoxPainter
    var drawsInView: Bool { drawView != nil }

    init(painter: HostBoxPainter) {
        self.painter = painter
        // Below child elements, above the drawn paint so inset shadows stay over the background.
        layer.zPosition = -0.25
    }

    /// AppKit may rebuild a view's sublayers, so the paint layer is re-attached when it went missing.
    func setFrame(_ frame: CGRect, in owner: WhiskerNodeView) {
        if layer.superlayer !== owner.backingLayer {
            owner.backingLayer.insertSublayer(layer, at: 0)
        }
        layer.frame = frame
        drawView?.frame = frame
    }

    func showLayerPaint(_ paint: HostLayerBoxPaint) {
        drawView?.removeFromSuperview()
        drawView = nil
        applyLayerPaint(paint)
    }

    func showDrawnPaint(in owner: NSView, contentBox: CGRect, redraw: Bool) {
        applyLayerPaint(HostLayerBoxPaint())
        if let drawView {
            drawView.contentBox = contentBox
            if redraw { drawView.needsDisplay = true }
            return
        }
        let view = HostNodePaintView(painter: painter)
        view.frame = layer.frame
        view.contentBox = contentBox
        view.isHidden = layer.isHidden
        owner.insertSubview(view, at: 0)
        view.needsDisplay = true
        drawView = view
    }

    func setHidden(_ hidden: Bool) {
        layer.isHidden = hidden
        drawView?.isHidden = hidden
    }

    private func applyLayerPaint(_ paint: HostLayerBoxPaint) {
        layer.backgroundColor = paint.background?.cgColor
        layer.cornerRadius = paint.cornerRadius
        layer.borderWidth = paint.borderWidth
        layer.borderColor = paint.borderColor?.cgColor
    }
}

private final class HostNodePaintView: NSView {
    private unowned let painter: HostBoxPainter
    var contentBox = CGRect.zero

    override var isFlipped: Bool { true }

    init(painter: HostBoxPainter) {
        self.painter = painter
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
        layer?.zPosition = -0.3
    }

    required init?(coder: NSCoder) { nil }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        painter.draw(in: bounds, contentBox: contentBox)
    }
}
#endif
