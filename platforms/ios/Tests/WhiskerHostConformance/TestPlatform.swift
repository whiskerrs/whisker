#if canImport(UIKit)
import UIKit

func renderLayer(of view: UIView, in context: CGContext) {
    view.layer.render(in: context)
}

func layerTransform(of view: UIView) -> CATransform3D {
    view.layer.transform
}
#else
import AppKit
@testable import WhiskerModule

typealias UIView = NSView
typealias UIColor = NSColor
typealias UIFont = NSFont
typealias UIFontDescriptor = NSFontDescriptor
typealias UIEdgeInsets = NSEdgeInsets
typealias UILabel = WhiskerTextLabel

extension NSView {
    func layoutIfNeeded() { layoutSubtreeIfNeeded() }

    var contentScaleFactor: CGFloat {
        get { layer?.contentsScale ?? 1 }
        set { layer?.contentsScale = newValue }
    }
}

extension NSFontDescriptor.SymbolicTraits {
    static let traitItalic = NSFontDescriptor.SymbolicTraits.italic
}

private var captureWindows = [NSWindow]()

/// AppKit only attaches sublayers and fills backing stores for views in a window.
func renderLayer(of view: NSView, in context: CGContext) {
    if view.window == nil {
        let window = NSWindow(
            contentRect: view.frame,
            styleMask: [.borderless],
            backing: .buffered,
            defer: true
        )
        window.contentView?.addSubview(view)
        captureWindows.append(window)
    }
    view.layoutSubtreeIfNeeded()
    view.displayIfNeeded()
    CATransaction.flush()
    view.layer?.render(in: context)
}

func layerTransform(of view: NSView) -> CATransform3D {
    view.layer?.transform ?? CATransform3DIdentity
}
#endif
