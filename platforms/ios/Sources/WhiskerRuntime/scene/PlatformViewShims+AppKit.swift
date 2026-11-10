#if !canImport(UIKit) && canImport(AppKit)
import AppKit

extension NSView {
    var alpha: CGFloat {
        get { alphaValue }
        set { alphaValue = newValue }
    }

    func insertSubview(_ view: NSView, at index: Int) {
        if index < subviews.count {
            addSubview(view, positioned: .below, relativeTo: subviews[index])
        } else {
            addSubview(view)
        }
    }

    func bringSubviewToFront(_ view: NSView) {
        guard view.superview === self, subviews.last !== view else { return }
        addSubview(view, positioned: .above, relativeTo: nil)
    }
}
#endif
