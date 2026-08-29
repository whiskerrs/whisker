import UIKit
import WhiskerModule

/// Common iOS wrapper for every built-in or custom Whisker element.
///
/// The scene owner controls hierarchy and geometry. Element modules only own
/// the mounted content view placed inside this wrapper.
final class WhiskerNodeView: UIView {
    let element: String
    var contentFrame = CGRect.zero
    var paint: HostBoxPaint?
    let boxPainter = HostBoxPainter()
    var mountedElement: WhiskerMountedElement?
    private let defaultChildrenHost = WhiskerChildrenHostView(frame: .zero)
    private lazy var paintView = HostNodePaintView(painter: boxPainter)
    private let overflowMask = CAShapeLayer()
    private var overflowMaskScrollOrigin = CGPoint.zero
    private let clipPathMask = CAShapeLayer()
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
        defaultChildrenHost.isOpaque = false
        defaultChildrenHost.backgroundColor = .clear
        defaultChildrenHost.clipsToBounds = false
        paintView.layer.zPosition = 0
        defaultChildrenHost.layer.zPosition = 2
        addSubview(paintView)
        addSubview(defaultChildrenHost)
        if #available(iOS 13.4, *) {
            let delegate = HostNodePointerDelegate(node: self)
            let interaction = UIPointerInteraction(delegate: delegate)
            addInteraction(interaction)
            pointerDelegate = delegate
            pointerInteraction = interaction
        }
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

    override func didMoveToSuperview() {
        super.didMoveToSuperview()
        updateOverflowMask()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        if let mountedElement {
            mountedElement.view.frame = contentFrame
        }
        defaultChildrenHost.frame = bounds
        paintView.frame = bounds
        paintView.contentBox = contentFrame
        updateBackdropBlurGeometry()
        updateBoxShadowLayers()
        updateOverflowMask()
        updateClipPathMask()
        paintView.setNeedsDisplay()
    }

    func setLayoutFrame(_ frame: CGRect) {
        // `UIView.frame` is undefined while a non-identity transform is
        // installed. Keep layout geometry and presentation transform
        // independent by positioning the zero-anchor layer directly.
        bounds = CGRect(origin: .zero, size: frame.size)
        layer.position = frame.origin
        defaultChildrenHost.frame = bounds
        paintView.frame = bounds
        paintView.contentBox = contentFrame
        updateBackdropBlurGeometry()
        updateOverflowMask()
        updateClipPathMask()
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
    }

    func setOverflowClip(horizontal: Bool, vertical: Bool) {
        clipsOverflowHorizontally = horizontal
        clipsOverflowVertically = vertical
        clipsToBounds = false
        updateOverflowMask()
    }

    func sceneChildrenHost() -> UIView {
        mountedElement?.childrenHost() ?? defaultChildrenHost
    }

    func mountedContentDidInstall() {
        mountedElement?.view.layer.zPosition = 2
        bringSubviewToFront(defaultChildrenHost)
        scrollOffsetObservation = (mountedElement?.view as? UIScrollView)?.observe(
            \.contentOffset,
            options: [.new]
        ) { [weak self] scrollView, _ in
            self?.updateOverflowMaskPosition(scrollView)
        }
        updateOverflowMask()
    }

    func boxPaintDidChange() {
        paintView.contentBox = contentFrame
        paintView.setNeedsDisplay()
        updateBackdropBlurGeometry()
        updateBoxShadowLayers()
        updateOverflowMask()
        updateClipPathMask()
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
                paintView.layer.addSublayer(shadowLayer)
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
        }
        blurView.setBlurRadius(radius)
        updateBackdropBlurGeometry()
    }

    func setImageRendering(_ value: HostImageRendering) {
        imageRendering = value
        boxPainter.setImageRendering(value)
        paintView.setNeedsDisplay()
    }

    func setHitTestBehavior(_ value: Int32) {
        precondition((0...3).contains(value))
        hitTestBehavior = value
    }

    func setCursorKeyword(_ value: Int32) {
        precondition((0...34).contains(value))
        cursorKeyword = value
        if #available(iOS 13.4, *) {
            (pointerInteraction as? UIPointerInteraction)?.invalidate()
        }
    }

    /// Applies computed CSS visibility to this element's own presentation.
    /// Descendant node views stay mounted so an explicit `visibility: visible`
    /// can override a hidden ancestor.
    func setWhiskerVisibility(_ visible: Bool) {
        guard whiskerVisible != visible else { return }
        whiskerVisible = visible
        paintView.isHidden = !visible
        boxShadowLayers.forEach { $0.isHidden = !visible }
        backdropBlurView?.isHidden = !visible
        if let mountedElement, mountedElement.childrenHost() == nil {
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

    private func updateOverflowMask() {
        let host = overflowClipHost()
        guard clipsOverflowHorizontally || clipsOverflowVertically else {
            host.layer.mask = nil
            return
        }
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
        overflowMaskScrollOrigin = (host as? UIScrollView)?.bounds.origin ?? .zero
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

    /// Returns the stationary view that owns the element's clipping viewport.
    ///
    /// A children host is allowed to move inside its element root. In
    /// particular, `UIScrollView` translates its content view as the user
    /// scrolls. Attaching the overflow mask to that moving content view would
    /// pin the mask to the initial content coordinates and permanently hide
    /// every child that started outside the first viewport. The mounted root
    /// remains stationary and is therefore the correct clip coordinate space.
    private func overflowClipHost() -> UIView {
        if let mountedElement, mountedElement.childrenHost() != nil {
            return mountedElement.view
        }
        return defaultChildrenHost
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

private final class HostNodePaintView: UIView {
    private unowned let painter: HostBoxPainter
    var contentBox = CGRect.zero

    init(painter: HostBoxPainter) {
        self.painter = painter
        super.init(frame: .zero)
        isOpaque = false
        isUserInteractionEnabled = false
        backgroundColor = .clear
    }

    required init?(coder: NSCoder) { nil }

    override func draw(_ rect: CGRect) {
        painter.draw(in: bounds, contentBox: contentBox)
    }
}
