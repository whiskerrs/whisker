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

    init(element: String) {
        self.element = element
        super.init(frame: .zero)
        isOpaque = false
    }

    required init?(coder: NSCoder) { nil }

    override func layoutSubviews() {
        super.layoutSubviews()
        if let mountedElement {
            mountedElement.view.frame = contentFrame
        }
        setNeedsDisplay()
    }

    override func draw(_ rect: CGRect) {
        boxPainter.draw(in: bounds)
    }
}
