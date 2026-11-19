#if canImport(UIKit)
import UIKit
import WhiskerModule

private final class OverflowMaskLayer: CAShapeLayer {
    // The clipping viewport must track native scrolling without implicit interpolation.
    override func action(forKey event: String) -> CAAction? { nil }
}

private final class HostPaintLayer: CALayer {
    // A layer outside a view animates every property change unless told otherwise.
    override func action(forKey event: String) -> CAAction? { nil }
}

/// Common iOS wrapper for every built-in or custom Whisker element.
///
/// The scene owner controls hierarchy and geometry. Element modules only own
/// the mounted content view placed inside this wrapper.
final class WhiskerNodeView: UIView {
    let element: String
    var backingLayer: CALayer { layer }
    var contentFrame = CGRect.zero
    var paint: HostBoxPaint?
    let boxPainter = HostBoxPainter()
    var mountedElement: WhiskerMountedElement?
    private var defaultChildrenHost: UIView?
    private var viewContent: WhiskerContainerView?
    private var hostsChildrenInContent = false
    private var boxPresentation: HostBoxPresentation?
    private var paintedRevision: UInt64?
    private var paintedContentBox = CGRect.null
    private var uniformClipHost: UIView?
    private var uniformClipRestoresMasksToBounds = false
    private var contentCornerClipRestoresMasksToBounds: Bool?
    private let overflowMask = OverflowMaskLayer()
    private var overflowMaskScrollOrigin = CGPoint.zero
    private var overflowMaskCompositionBounds = CGRect.zero
    private var ancestorScrollObservations: [NSKeyValueObservation] = []
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
    private var pointerDelegate: AnyObject?
    private var pointerInteraction: AnyObject?
    private var scrollOffsetObservation: NSKeyValueObservation?
    private var whiskerVisible = true

    init(element: String) {
        self.element = element
        super.init(frame: .zero)
        isOpaque = false
        layer.anchorPoint = .zero
    }

    required init?(coder: NSCoder) { nil }

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

        accessibilityLabel = label
        accessibilityHint = hint
        accessibilityIdentifier = identifier
        accessibilityElementsHidden = hidden
        accessibilityViewIsModal = modal
        shouldGroupAccessibilityChildren = role == "group"
        isAccessibilityElement = !hidden && hasSemantics && role != "group"

        var traits: UIAccessibilityTraits = []
        switch role {
        case "button": traits.insert(.button)
        case "link": traits.insert(.link)
        case "image": traits.insert(.image)
        case "text": traits.insert(.staticText)
        case "header": traits.formUnion([.staticText, .header])
        case "checkbox", "radio", "switch": traits.insert(.button)
        case "adjustable": traits.insert(.adjustable)
        case "searchbox": traits.insert(.searchField)
        case "tab": traits.insert(.button)
        default: break
        }
        if state["disabled"]?.asBool == true { traits.insert(.notEnabled) }
        if state["selected"]?.asBool == true { traits.insert(.selected) }
        accessibilityTraits = traits

        if let checked = state["checked"]?.asString {
            accessibilityValue = checked == "mixed" ? "Mixed" : (checked == "true" ? "Checked" : "Unchecked")
        } else if let expanded = state["expanded"]?.asBool {
            accessibilityValue = expanded ? "Expanded" : "Collapsed"
        } else {
            accessibilityValue = nil
        }
    }

    override func willMove(toSuperview newSuperview: UIView?) {
        invalidateAncestorScrollExtent()
        super.willMove(toSuperview: newSuperview)
    }

    override func didMoveToSuperview() {
        super.didMoveToSuperview()
        invalidateAncestorScrollExtent()
        updateAncestorScrollObservations()
        updateOverflowMask()
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        updateAncestorScrollObservations()
        updateOverflowMask()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        if let mountedElement, viewContent == nil {
            mountedElement.view.frame = contentFrame
        }
        boxPresentation?.setFrame(bounds)
        updateBoxPaint()
        updateBackdropBlurGeometry()
        updateBoxShadowLayers()
        updateOverflowMask()
        updateClipPathMask()
        updateContentClipMask()
    }

    func setLayoutFrame(_ frame: CGRect) {
        // `UIView.frame` is undefined while a non-identity transform is
        // installed. Keep layout geometry and presentation transform
        // independent by positioning the zero-anchor layer directly.
        bounds = CGRect(origin: .zero, size: frame.size)
        layer.position = frame.origin
        boxPresentation?.setFrame(bounds)
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
        layer.transform = transform
        invalidateAncestorScrollExtent()
    }

    func setOverflowClip(horizontal: Bool, vertical: Bool) {
        clipsOverflowHorizontally = horizontal
        clipsOverflowVertically = vertical
        clipsToBounds = false
        updateAncestorScrollObservations()
        updateOverflowMask()
        invalidateAncestorScrollExtent()
    }

    func sceneChildrenHost() -> UIView {
        if let host = mountedElement?.childrenHost() { return host }
        if let viewContent { return hostsChildrenInContent ? viewContent : self }
        if let defaultChildrenHost { return defaultChildrenHost }
        let host = WhiskerChildrenHostView(frame: bounds)
        host.isOpaque = false
        host.layer.zPosition = 2
        addSubview(host)
        defaultChildrenHost = host
        updateOverflowMask()
        return host
    }

    /// The children host only when one exists, so a childless element never creates one.
    private func existingChildrenHost() -> UIView? {
        if let host = mountedElement?.childrenHost() { return host }
        if let viewContent { return hostsChildrenInContent ? viewContent : self }
        return defaultChildrenHost
    }

    func installMountedContent() {
        mountedElement?.view.frame = bounds
        mountedElement?.view.layer.zPosition = 2
        if let mountedElement, mountedElement.registration.name == WhiskerBuiltInElements.viewName,
           let content = mountedElement.view as? WhiskerContainerView {
            // The built-in View's content view draws nothing, so it stays out of the
            // hierarchy until `updateChildrenHosting` needs it between this view and the children.
            content.removeFromSuperview()
            content.autoresizingMask = []
            content.isHidden = false
            content.layer.mask = nil
            content.layer.cornerRadius = 0
            content.layer.masksToBounds = false
            viewContent = content
            hostsChildrenInContent = false
            updateOverflowMask()
        } else if let content = mountedElement?.view {
            content.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            addSubview(content)
        }
        if let defaultChildrenHost { bringSubviewToFront(defaultChildrenHost) }
        (mountedElement?.view as? WhiskerScrollContainerView)?
            .installWhiskerContentExtentSource { [weak self] in
                self?.scrollableChildrenExtent() ?? .zero
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
        guard !(mountedElement?.view is UIScrollView) else { return bounds }
        layoutIfNeeded()
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
            let shadowLayer = CAShapeLayer()
            boxShadowLayers.append(shadowLayer)
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
                layer.addSublayer(shadowLayer)
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
            blurView.layer.zPosition = -0.5
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
        if #available(iOS 13.4, *) {
            if let interaction = pointerInteraction as? UIPointerInteraction {
                interaction.invalidate()
            } else if value > 1 {
                let delegate = HostNodePointerDelegate(node: self)
                let interaction = UIPointerInteraction(delegate: delegate)
                addInteraction(interaction)
                pointerDelegate = delegate
                pointerInteraction = interaction
            }
        }
    }

    /// Applies computed CSS visibility to this element's own presentation.
    /// Descendant node views stay mounted so an explicit `visibility: visible`
    /// can override a hidden ancestor.
    func setWhiskerVisibility(_ visible: Bool) {
        guard whiskerVisible != visible else { return }
        whiskerVisible = visible
        if !visible { endEditing(true) }
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

    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        if !whiskerVisible {
            guard hitTestBehavior != 1, hitTestBehavior != 2,
                  let result = super.hitTest(point, with: event), result !== self
            else { return nil }
            var ancestor: UIView? = result
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
            return self.point(inside: point, with: event) ? self : nil
        case 3:
            let result = super.hitTest(point, with: event)
            return result === self ? nil : result
        default:
            return super.hitTest(point, with: event)
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
            maskLayer.fillColor = UIColor.white.cgColor
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
        shadowLayer.shadowOffset = .zero
        shadowLayer.shadowRadius = shadow.blurRadius / 2

        let maskPath = CGMutablePath()
        maskPath.addRect(CGRect(origin: .zero, size: layerFrame.size))
        if let borderPath = boxPainter.borderBoxPath(in: bounds).copy(using: &translation) {
            maskPath.addPath(borderPath)
        }
        maskLayer.frame = CGRect(origin: .zero, size: layerFrame.size)
        maskLayer.path = maskPath
        maskLayer.fillColor = UIColor.white.cgColor
        maskLayer.fillRule = .evenOdd
        shadowLayer.mask = maskLayer
    }

    private func updateBoxPaint() {
        switch boxPainter.paintPlan(in: bounds) {
        case .none:
            boxPresentation?.showLayerPaint(HostLayerBoxPaint())
        case .layer(let layerPaint):
            ensureBoxPresentation().showLayerPaint(layerPaint)
        case .draw:
            let presentation = ensureBoxPresentation()
            // A resized draw view redraws itself, so only the paint inputs are compared here.
            let stale = paintedRevision != boxPainter.revision || paintedContentBox != contentFrame
            presentation.showDrawnPaint(in: self, contentBox: contentFrame, redraw: stale)
            paintedRevision = boxPainter.revision
            paintedContentBox = contentFrame
        }
        updateChildrenHosting()
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
            viewContent.bounds = bounds
            viewContent.layer.position = CGPoint(x: bounds.midX, y: bounds.midY)
            addSubview(viewContent)
            for child in subviews where child is WhiskerNodeView { viewContent.addSubview(child) }
        } else {
            for child in viewContent.subviews where child is WhiskerNodeView { addSubview(child) }
            viewContent.layer.mask = nil
            removeUniformClip(from: viewContent)
            viewContent.removeFromSuperview()
        }
        hostsChildrenInContent = needsContent
    }

    private func ensureBoxPresentation() -> HostBoxPresentation {
        if let boxPresentation { return boxPresentation }
        let presentation = HostBoxPresentation(painter: boxPainter)
        presentation.setFrame(bounds)
        presentation.setHidden(!whiskerVisible)
        layer.insertSublayer(presentation.layer, at: 0)
        boxPresentation = presentation
        return presentation
    }

    private func updateOverflowMask() {
        updateChildrenHosting()
        guard let host = overflowClipHost() else { return }
        guard clipsOverflowHorizontally || clipsOverflowVertically else {
            host.layer.mask = nil
            scrollOffsetObservation = nil
            removeUniformClip(from: host)
            return
        }
        if clipsOverflowHorizontally, clipsOverflowVertically,
           let clip = boxPainter.uniformOverflowClip(in: bounds),
           installUniformClip(clip, on: host) {
            host.layer.mask = nil
            scrollOffsetObservation = nil
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
        overflowMask.backgroundColor = UIColor.clear.cgColor
        overflowMask.fillColor = UIColor.white.cgColor
        host.layer.mask = overflowMask
        if scrollOffsetObservation == nil, let scrollView = host as? UIScrollView {
            // A mask lives in the scrolling coordinate space, so it has to follow each offset.
            scrollOffsetObservation = scrollView.observe(\.contentOffset, options: [.new]) {
                [weak self] scrollView, _ in self?.updateOverflowMaskPosition(scrollView)
            }
        }
        overflowMaskScrollOrigin = (host as? UIScrollView)?.bounds.origin ?? .zero
        overflowMaskCompositionBounds = compositionBounds
    }

    /// `UIScrollView` scrolls by changing `bounds.origin`. A layer mask uses
    /// that moving coordinate space, so keep the already-built path aligned
    /// with the viewport without rebuilding its rounded geometry per event.
    private func updateOverflowMaskPosition(_ scrollView: UIScrollView) {
        let next = scrollView.bounds.origin
        defer { overflowMaskScrollOrigin = next }
        guard scrollView.layer.mask === overflowMask else { return }
        overflowMask.frame.origin.x += next.x - overflowMaskScrollOrigin.x
        overflowMask.frame.origin.y += next.y - overflowMaskScrollOrigin.y
    }

    private func updateAncestorScrollObservations() {
        ancestorScrollObservations.removeAll()
        guard clipsOverflowHorizontally != clipsOverflowVertically else { return }
        var ancestor = superview
        while let current = ancestor {
            if let scrollView = current as? UIScrollView {
                ancestorScrollObservations.append(scrollView.observe(
                    \.contentOffset,
                    options: [.new]
                ) { [weak self] _, _ in
                    self?.updateOverflowMaskForAncestorScroll()
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
        overflowMask.frame = frame
        overflowMaskCompositionBounds = next
    }

    /// Returns the stationary view that owns the element's clipping viewport.
    ///
    /// A children host is allowed to move inside its element root. In
    /// particular, `UIScrollView` translates its content view as the user
    /// scrolls. Attaching the overflow mask to that moving content view would
    /// pin the mask to the initial content coordinates and permanently hide
    /// every child that started outside the first viewport. The mounted root
    /// remains stationary and is therefore the correct clip coordinate space.
    private func overflowClipHost() -> UIView? {
        if let mountedElement, mountedElement.childrenHost() != nil {
            return mountedElement.view
        }
        if let viewContent { return hostsChildrenInContent ? viewContent : nil }
        return defaultChildrenHost
    }

    /// A mask layer is rendered offscreen on every frame its subtree is on screen;
    /// clipping to the host's own bounds is not.
    private func installUniformClip(_ clip: HostUniformClip, on host: UIView) -> Bool {
        if host === defaultChildrenHost || host === viewContent {
            // The bounds origin keeps children in this element's coordinates while
            // the host itself shrinks to the padding box it clips to.
            host.bounds = clip.rect
            host.layer.position = CGPoint(x: clip.rect.midX, y: clip.rect.midY)
        } else {
            // A module's own view keeps its geometry, so it can only clip to itself.
            let frame = host.frame
            guard abs(frame.minX - clip.rect.minX) < 0.01, abs(frame.minY - clip.rect.minY) < 0.01,
                  abs(frame.width - clip.rect.width) < 0.01,
                  abs(frame.height - clip.rect.height) < 0.01
            else { return false }
        }
        if uniformClipHost !== host {
            uniformClipRestoresMasksToBounds = host.layer.masksToBounds
            uniformClipHost = host
        }
        host.layer.cornerRadius = clip.cornerRadius
        host.layer.masksToBounds = true
        return true
    }

    private func removeUniformClip(from host: UIView) {
        if uniformClipHost === host {
            host.layer.cornerRadius = 0
            host.layer.masksToBounds = uniformClipRestoresMasksToBounds
            uniformClipHost = nil
        }
        if host === defaultChildrenHost || host === viewContent {
            host.bounds = bounds
            host.layer.position = CGPoint(x: bounds.midX, y: bounds.midY)
        }
    }

    private func updateClipPathMask() {
        guard let clipPath else {
            layer.mask = nil
            return
        }
        clipPathMask.frame = bounds
        clipPathMask.path = clipPath.path(
            in: bounds,
            contentBox: contentFrame,
            painter: boxPainter
        )
        clipPathMask.fillColor = UIColor.white.cgColor
        clipPathMask.fillRule = clipPath.fillRule
        layer.mask = clipPathMask
    }

    /// Rounds a childless element's native content like a CSS replaced element.
    private func updateContentClipMask() {
        guard let mountedElement else { return }
        let contentLayer = mountedElement.view.layer
        let childless = mountedElement.registration.childPolicy == .none
        if childless,
           let radius = boxPainter.uniformContentClipRadius(in: bounds, contentBox: contentFrame),
           radius > 0 {
            if contentLayer.mask === contentClipMask { contentLayer.mask = nil }
            if contentCornerClipRestoresMasksToBounds == nil {
                contentCornerClipRestoresMasksToBounds = contentLayer.masksToBounds
            }
            contentLayer.cornerRadius = radius
            contentLayer.masksToBounds = true
            return
        }
        if let restore = contentCornerClipRestoresMasksToBounds {
            contentLayer.cornerRadius = 0
            contentLayer.masksToBounds = restore
            contentCornerClipRestoresMasksToBounds = nil
        }
        guard childless,
              let path = boxPainter.contentBoxClipPath(in: bounds, contentBox: contentFrame)
        else {
            if contentLayer.mask === contentClipMask { contentLayer.mask = nil }
            return
        }
        var translation = CGAffineTransform(
            translationX: -contentFrame.minX,
            y: -contentFrame.minY
        )
        contentClipMask.frame = CGRect(origin: .zero, size: contentFrame.size)
        contentClipMask.path = path.copy(using: &translation)
        contentClipMask.fillColor = UIColor.white.cgColor
        contentLayer.mask = contentClipMask
    }

    private func hostCompositionBounds() -> CGRect {
        var host: UIView = self
        while let parent = host.superview { host = parent }
        return convert(host.bounds, from: host)
    }
}

/// UIKit can draw beam, crosshair, and resize pointer shapes, but does not
/// expose the platform glyphs for semantic CSS cursors such as link, grab,
/// help, wait, copy, forbidden, and zoom.
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

@available(iOS 13.4, *)
private final class HostNodePointerDelegate: NSObject, UIPointerInteractionDelegate {
    private unowned let node: WhiskerNodeView

    init(node: WhiskerNodeView) {
        self.node = node
    }

    func pointerInteraction(
        _ interaction: UIPointerInteraction,
        styleFor region: UIPointerRegion
    ) -> UIPointerStyle? {
        switch node.cursorPresentation {
        case .system, .unsupportedSystemFallback:
            nil
        case .hidden:
            .hidden()
        case .verticalBeam:
            UIPointerStyle(shape: .verticalBeam(length: 20))
        case .horizontalBeam:
            UIPointerStyle(shape: .horizontalBeam(length: 20))
        case .crosshair:
            UIPointerStyle(shape: .path(crosshairPointerPath()))
        case .horizontalResize:
            UIPointerStyle(shape: .path(resizePointerPath(angle: 0)))
        case .verticalResize:
            UIPointerStyle(shape: .path(resizePointerPath(angle: .pi / 2)))
        case .northwestSoutheastResize:
            UIPointerStyle(shape: .path(resizePointerPath(angle: .pi / 4)))
        case .northeastSouthwestResize:
            UIPointerStyle(shape: .path(resizePointerPath(angle: -.pi / 4)))
        }
    }
}

@available(iOS 13.4, *)
private func crosshairPointerPath() -> UIBezierPath {
    let path = UIBezierPath(rect: CGRect(x: -1, y: -9, width: 2, height: 18))
    path.append(UIBezierPath(rect: CGRect(x: -9, y: -1, width: 18, height: 2)))
    return path
}

@available(iOS 13.4, *)
private func resizePointerPath(angle: CGFloat) -> UIBezierPath {
    let path = UIBezierPath()
    path.move(to: CGPoint(x: -10, y: 0))
    path.addLine(to: CGPoint(x: -5, y: -5))
    path.addLine(to: CGPoint(x: -5, y: -2))
    path.addLine(to: CGPoint(x: 5, y: -2))
    path.addLine(to: CGPoint(x: 5, y: -5))
    path.addLine(to: CGPoint(x: 10, y: 0))
    path.addLine(to: CGPoint(x: 5, y: 5))
    path.addLine(to: CGPoint(x: 5, y: 2))
    path.addLine(to: CGPoint(x: -5, y: 2))
    path.addLine(to: CGPoint(x: -5, y: 5))
    path.close()
    path.apply(CGAffineTransform(rotationAngle: angle))
    return path
}

private final class WhiskerChildrenHostView: UIView {
    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        let result = super.hitTest(point, with: event)
        return result === self ? nil : result
    }
}

/// Presents an element's box paint from layer properties, and rasterizes only
/// the paints those cannot express: a `draw(_:)` view owns a bitmap as large as
/// the element, which most elements never need.
private final class HostBoxPresentation {
    let layer = HostPaintLayer()
    private var drawView: HostNodeDrawView?
    private unowned let painter: HostBoxPainter
    var drawsInView: Bool { drawView != nil }

    init(painter: HostBoxPainter) {
        self.painter = painter
        // Below child elements, above the drawn paint so inset shadows stay over the background.
        layer.zPosition = -0.25
    }

    func setFrame(_ frame: CGRect) {
        layer.frame = frame
        drawView?.frame = frame
    }

    func showLayerPaint(_ paint: HostLayerBoxPaint) {
        drawView?.removeFromSuperview()
        drawView = nil
        applyLayerPaint(paint)
    }

    func showDrawnPaint(in owner: UIView, contentBox: CGRect, redraw: Bool) {
        applyLayerPaint(HostLayerBoxPaint())
        if let drawView {
            drawView.contentBox = contentBox
            if redraw { drawView.setNeedsDisplay() }
            return
        }
        let view = HostNodeDrawView(painter: painter)
        view.frame = layer.frame
        view.contentBox = contentBox
        view.isHidden = layer.isHidden
        owner.insertSubview(view, at: 0)
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

private final class HostNodeDrawView: UIView {
    private unowned let painter: HostBoxPainter
    var contentBox = CGRect.zero

    init(painter: HostBoxPainter) {
        self.painter = painter
        super.init(frame: .zero)
        isOpaque = false
        isUserInteractionEnabled = false
        backgroundColor = .clear
        contentMode = .redraw
        layer.zPosition = -0.3
    }

    required init?(coder: NSCoder) { nil }

    override func draw(_ rect: CGRect) {
        painter.draw(in: bounds, contentBox: contentBox)
    }
}
#endif
