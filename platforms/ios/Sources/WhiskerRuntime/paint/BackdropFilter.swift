#if canImport(UIKit)
import UIKit

/// UIKit projection of Whisker's single `backdrop-filter: blur()` primitive.
///
/// UIKit does not expose a blur radius. A paused property animator gives us a
/// stable, public-API intensity control; 30 logical pixels maps to the full
/// system material and larger radii saturate there.
final class HostBackdropBlurView: UIVisualEffectView {
    private var animator: UIViewPropertyAnimator?
    private let shapeMask = HostActionlessShapeLayer()
    private var blurRadius: CGFloat = 0

    init() {
        super.init(effect: nil)
        isUserInteractionEnabled = false
        isHidden = true
        clipsToBounds = true
        shapeMask.fillColor = UIColor.white.cgColor
    }

    required init?(coder: NSCoder) { nil }

    deinit {
        animator?.stopAnimation(true)
    }

    func setBlurRadius(_ radius: CGFloat) {
        let radius = min(max(radius, 0), 30)
        guard radius != blurRadius else { return }
        blurRadius = radius
        animator?.stopAnimation(true)
        animator = nil
        effect = nil

        guard radius > 0 else {
            isHidden = true
            return
        }

        isHidden = false
        let next = UIViewPropertyAnimator(duration: 1, curve: .linear) { [weak self] in
            self?.effect = UIBlurEffect(style: .regular)
        }
        next.startAnimation()
        next.pauseAnimation()
        next.fractionComplete = radius / 30
        animator = next
    }

    func setShape(_ path: CGPath, in bounds: CGRect) {
        var rect = CGRect.zero
        if path.isRect(&rect), rect == bounds {
            // The view already clips to its bounds. A shape mask here adds a
            // compositing pass to a rectangular, often full-screen blur.
            if layer.mask != nil {
                layer.mask = nil
                shapeMask.path = nil
            }
            return
        }
        if shapeMask.frame != bounds { shapeMask.frame = bounds }
        if shapeMask.path != path { shapeMask.path = path }
        if layer.mask == nil { layer.mask = shapeMask }
    }
}
#endif
