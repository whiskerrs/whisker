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
