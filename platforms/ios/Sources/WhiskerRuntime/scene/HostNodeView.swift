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
    private let overflowMask = CALayer()
    private var clipsOverflowHorizontally = false
    private var clipsOverflowVertically = false

    init(element: String) {
        self.element = element
        super.init(frame: .zero)
        isOpaque = false
        layer.anchorPoint = .zero
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
        updateOverflowMask()
        setNeedsDisplay()
    }

    override func draw(_ rect: CGRect) {
        boxPainter.draw(in: bounds)
    }

    func setLayoutFrame(_ frame: CGRect) {
        // `UIView.frame` is undefined while a non-identity transform is
        // installed. Keep layout geometry and presentation transform
        // independent by positioning the zero-anchor layer directly.
        bounds = CGRect(origin: .zero, size: frame.size)
        layer.position = frame.origin
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

    private func updateOverflowMask() {
        guard clipsOverflowHorizontally || clipsOverflowVertically else {
            layer.mask = nil
            return
        }
        // A layer mask participates in composition after this view's own box
        // has been painted, so the box remains intact while overflowing
        // descendants are constrained only on the requested axes. Visible
        // axes extend through the complete Host surface; the surface itself is
        // the final composition boundary.
        let compositionBounds = hostCompositionBounds()
        overflowMask.frame = CGRect(
            x: clipsOverflowHorizontally ? bounds.minX : compositionBounds.minX,
            y: clipsOverflowVertically ? bounds.minY : compositionBounds.minY,
            width: clipsOverflowHorizontally ? bounds.width : compositionBounds.width,
            height: clipsOverflowVertically ? bounds.height : compositionBounds.height
        )
        overflowMask.backgroundColor = UIColor.white.cgColor
        layer.mask = overflowMask
    }

    private func hostCompositionBounds() -> CGRect {
        var host: UIView = self
        while let parent = host.superview { host = parent }
        return convert(host.bounds, from: host)
    }
}
