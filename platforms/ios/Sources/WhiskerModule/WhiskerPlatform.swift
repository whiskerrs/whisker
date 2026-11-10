#if canImport(UIKit)
import UIKit

public typealias WhiskerPlatformView = UIView
public typealias WhiskerPlatformColor = UIColor
public typealias WhiskerPlatformFont = UIFont
public typealias WhiskerPlatformEdgeInsets = UIEdgeInsets

/** Current CoreGraphics context inside a platform view's `draw(_:)`. */
public func whiskerCurrentGraphicsContext() -> CGContext? {
    UIGraphicsGetCurrentContext()
}

/** sRGB components of `color`, or `nil` when it has no RGB representation. */
public func whiskerRGBAComponents(_ color: UIColor) -> (red: CGFloat, green: CGFloat, blue: CGFloat, alpha: CGFloat)? {
    var red: CGFloat = 0
    var green: CGFloat = 0
    var blue: CGFloat = 0
    var alpha: CGFloat = 0
    guard color.getRed(&red, green: &green, blue: &blue, alpha: &alpha) else { return nil }
    return (red, green, blue, alpha)
}

#elseif canImport(AppKit)
import AppKit

public typealias WhiskerPlatformView = NSView
public typealias WhiskerPlatformColor = NSColor
public typealias WhiskerPlatformFont = NSFont
public typealias WhiskerPlatformEdgeInsets = NSEdgeInsets

/** Current CoreGraphics context inside a platform view's `draw(_:)`. */
public func whiskerCurrentGraphicsContext() -> CGContext? {
    NSGraphicsContext.current?.cgContext
}

/** sRGB components of `color`, or `nil` when it has no RGB representation. */
public func whiskerRGBAComponents(_ color: NSColor) -> (red: CGFloat, green: CGFloat, blue: CGFloat, alpha: CGFloat)? {
    // `NSColor.getRed` raises for pattern and catalog colors instead of failing.
    guard let rgb = color.usingColorSpace(.extendedSRGB) else { return nil }
    return (rgb.redComponent, rgb.greenComponent, rgb.blueComponent, rgb.alphaComponent)
}

extension NSEdgeInsets {
    public static let zero = NSEdgeInsets(top: 0, left: 0, bottom: 0, right: 0)
}

extension NSView.AutoresizingMask {
    public static let flexibleWidth = NSView.AutoresizingMask.width
    public static let flexibleHeight = NSView.AutoresizingMask.height
}

extension NSView {
    /** UIKit spelling shared by Host code compiled for both platforms. */
    public func setNeedsLayout() { needsLayout = true }
    /** UIKit spelling shared by Host code compiled for both platforms. */
    public func setNeedsDisplay() { needsDisplay = true }
}

extension NSFont {
    /** Line height matching `UIFont.lineHeight`. */
    public var lineHeight: CGFloat { ascender - descender + leading }
}
#endif
