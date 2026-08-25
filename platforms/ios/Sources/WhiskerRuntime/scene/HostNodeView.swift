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
    private let clipPathMask = CAShapeLayer()
    private var clipPath: HostClipPath?
    private var boxShadows: [HostBoxShadow] = []
    private var boxShadowLayers: [CAShapeLayer] = []
    private var boxShadowMaskLayers: [CAShapeLayer] = []
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
        updateClipPathMask()
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
        bringSubviewToFront(defaultChildrenHost)
        updateOverflowMask()
    }

    func boxPaintDidChange() {
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
            // New entries are farther back in CSS list order. Inserting each
            // at zero keeps the first shadow visually above all later ones.
            layer.insertSublayer(shadowLayer, at: 0)
            boxShadowLayers.append(shadowLayer)
            boxShadowMaskLayers.append(CAShapeLayer())
        }
        boxShadows = shadows
        updateBoxShadowLayers()
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
        clipPathMask.fillRule = .nonZero
        layer.mask = clipPathMask
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
