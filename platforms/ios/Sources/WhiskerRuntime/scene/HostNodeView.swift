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
    private let overflowMask = CAShapeLayer()
    private var boxShadow: HostBoxShadow?
    private let boxShadowLayer = CAShapeLayer()
    private let boxShadowMaskLayer = CAShapeLayer()
    private var clipsOverflowHorizontally = false
    private var clipsOverflowVertically = false

    init(element: String) {
        self.element = element
        super.init(frame: .zero)
        isOpaque = false
        layer.anchorPoint = .zero
        defaultChildrenHost.isOpaque = false
        defaultChildrenHost.backgroundColor = .clear
        defaultChildrenHost.clipsToBounds = false
        addSubview(defaultChildrenHost)
        layer.insertSublayer(boxShadowLayer, at: 0)
    }

    required init?(coder: NSCoder) { nil }

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
        updateBoxShadowLayers()
        updateOverflowMask()
        setNeedsDisplay()
    }

    override func draw(_ rect: CGRect) {
        boxPainter.draw(in: bounds, contentBox: contentFrame)
    }

    func setLayoutFrame(_ frame: CGRect) {
        // `UIView.frame` is undefined while a non-identity transform is
        // installed. Keep layout geometry and presentation transform
        // independent by positioning the zero-anchor layer directly.
        bounds = CGRect(origin: .zero, size: frame.size)
        layer.position = frame.origin
        defaultChildrenHost.frame = bounds
        updateOverflowMask()
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
        bringSubviewToFront(defaultChildrenHost)
        updateOverflowMask()
    }

    func boxPaintDidChange() {
        updateBoxShadowLayers()
        updateOverflowMask()
    }

    func setBoxShadows(_ shadows: [HostBoxShadow]) {
        precondition(shadows.count <= 1)
        boxShadow = shadows.first
        updateBoxShadowLayers()
    }

    private func updateBoxShadowLayers() {
        guard let shadow = boxShadow else {
            boxShadowLayer.path = nil
            boxShadowLayer.mask = nil
            boxShadowLayer.shadowPath = nil
            return
        }
        if shadow.inset {
            guard let shadowPath = boxPainter.insetBoxShadowPath(in: bounds, shadow: shadow) else {
                boxShadowLayer.path = nil
                boxShadowLayer.mask = nil
                boxShadowLayer.shadowPath = nil
                return
            }
            boxShadowLayer.frame = bounds
            boxShadowLayer.path = shadowPath
            boxShadowLayer.fillColor = shadow.color.cgColor
            boxShadowLayer.fillRule = .evenOdd
            boxShadowLayer.strokeColor = nil
            // Let Core Animation derive the shadow from the even-odd ring;
            // `shadowPath` itself does not carry the layer's fill rule.
            boxShadowLayer.shadowPath = nil
            boxShadowLayer.shadowColor = shadow.color.withAlphaComponent(1).cgColor
            boxShadowLayer.shadowOpacity = Float(shadow.color.cgColor.alpha)
            boxShadowLayer.shadowOffset = .zero
            boxShadowLayer.shadowRadius = shadow.blurRadius / 2
            boxShadowMaskLayer.frame = bounds
            boxShadowMaskLayer.path = boxPainter.paddingBoxPath(in: bounds)
            boxShadowMaskLayer.fillColor = UIColor.white.cgColor
            boxShadowMaskLayer.fillRule = .nonZero
            boxShadowLayer.mask = boxShadowMaskLayer
            return
        }
        guard let shadowPath = boxPainter.hardBoxShadowPath(in: bounds, shadow: shadow) else {
            boxShadowLayer.path = nil
            boxShadowLayer.mask = nil
            boxShadowLayer.shadowPath = nil
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
        boxShadowLayer.frame = layerFrame
        boxShadowLayer.path = shadowPath.copy(using: &translation)
        boxShadowLayer.fillColor = shadow.color.cgColor
        boxShadowLayer.fillRule = .nonZero
        boxShadowLayer.strokeColor = nil
        boxShadowLayer.shadowPath = shadowPath.copy(using: &translation)
        boxShadowLayer.shadowColor = shadow.color.withAlphaComponent(1).cgColor
        boxShadowLayer.shadowOpacity = Float(shadow.color.cgColor.alpha)
        boxShadowLayer.shadowOffset = .zero
        boxShadowLayer.shadowRadius = shadow.blurRadius / 2

        let maskPath = CGMutablePath()
        maskPath.addRect(CGRect(origin: .zero, size: layerFrame.size))
        if let borderPath = boxPainter.borderBoxPath(in: bounds).copy(using: &translation) {
            maskPath.addPath(borderPath)
        }
        boxShadowMaskLayer.frame = CGRect(origin: .zero, size: layerFrame.size)
        boxShadowMaskLayer.path = maskPath
        boxShadowMaskLayer.fillColor = UIColor.white.cgColor
        boxShadowMaskLayer.fillRule = .evenOdd
        boxShadowLayer.mask = boxShadowMaskLayer
    }

    private func updateOverflowMask() {
        guard clipsOverflowHorizontally || clipsOverflowVertically else {
            sceneChildrenHost().layer.mask = nil
            return
        }
        let compositionBounds = hostCompositionBounds()
        let nodePath = boxPainter.overflowClipPath(
            in: bounds,
            visibleBounds: compositionBounds,
            horizontal: clipsOverflowHorizontally,
            vertical: clipsOverflowVertically
        )
        let host = sceneChildrenHost()
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
    }

    private func hostCompositionBounds() -> CGRect {
        var host: UIView = self
        while let parent = host.superview { host = parent }
        return convert(host.bounds, from: host)
    }
}

private final class WhiskerChildrenHostView: UIView {
    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        let result = super.hitTest(point, with: event)
        return result === self ? nil : result
    }
}
