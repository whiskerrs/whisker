import UIKit

/// UIKit projection of Whisker's single `backdrop-filter: blur()` primitive.
///
/// UIKit does not expose a blur radius. A paused property animator gives us a
/// stable, public-API intensity control; 30 logical pixels maps to the full
/// system material and larger radii saturate there.
final class HostBackdropBlurView: UIVisualEffectView {
    private var animator: UIViewPropertyAnimator?
    private let shapeMask = CAShapeLayer()

    init() {
        super.init(effect: nil)
        isUserInteractionEnabled = false
        isHidden = true
    }

    required init?(coder: NSCoder) { nil }

    deinit {
        animator?.stopAnimation(true)
    }

    func setBlurRadius(_ radius: CGFloat) {
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
        next.fractionComplete = min(radius / 30, 1)
        animator = next
    }

    func setShape(_ path: CGPath, in bounds: CGRect) {
        shapeMask.frame = bounds
        shapeMask.path = path
        shapeMask.fillColor = UIColor.white.cgColor
        layer.mask = shapeMask
    }
}
