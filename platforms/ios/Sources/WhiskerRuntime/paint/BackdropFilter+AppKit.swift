#if !canImport(UIKit) && canImport(AppKit)
import AppKit

/// AppKit projection of Whisker's single `backdrop-filter: blur()` primitive.
///
/// AppKit exposes no blur intensity, so any positive radius shows the
/// within-window material.
final class HostBackdropBlurView: NSVisualEffectView {
    private let shapeMask = CAShapeLayer()

    init() {
        super.init(frame: .zero)
        wantsLayer = true
        blendingMode = .withinWindow
        material = .hudWindow
        state = .active
        isHidden = true
    }

    required init?(coder: NSCoder) { nil }

    override var isFlipped: Bool { true }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func setBlurRadius(_ radius: CGFloat) {
        isHidden = radius <= 0
    }

    func setShape(_ path: CGPath, in bounds: CGRect) {
        shapeMask.frame = bounds
        shapeMask.path = path
        shapeMask.fillColor = NSColor.white.cgColor
        layer?.mask = shapeMask
    }
}
#endif
