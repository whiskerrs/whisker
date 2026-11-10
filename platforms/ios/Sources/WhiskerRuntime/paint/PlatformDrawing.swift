#if canImport(UIKit)
import UIKit

typealias WhiskerBezierPath = UIBezierPath

/** Draws a raster into the current context with UIKit's top-left orientation. */
func drawPlatformImage(_ image: CGImage, scale: CGFloat, in rect: CGRect) {
    UIImage(cgImage: image, scale: scale, orientation: .up).draw(in: rect)
}
#else
import AppKit

typealias WhiskerBezierPath = NSBezierPath

extension NSBezierPath {
    func addLine(to point: CGPoint) { line(to: point) }

    func addCurve(to point: CGPoint, controlPoint1: CGPoint, controlPoint2: CGPoint) {
        curve(to: point, controlPoint1: controlPoint1, controlPoint2: controlPoint2)
    }

    var usesEvenOddFillRule: Bool {
        get { windingRule == .evenOdd }
        set { windingRule = newValue ? .evenOdd : .nonZero }
    }

    convenience init(
        arcCenter center: CGPoint,
        radius: CGFloat,
        startAngle: CGFloat,
        endAngle: CGFloat,
        clockwise: Bool
    ) {
        self.init()
        // AppKit measures arcs in degrees with the opposite sense in a flipped space.
        appendArc(
            withCenter: center,
            radius: radius,
            startAngle: startAngle * 180 / .pi,
            endAngle: endAngle * 180 / .pi,
            clockwise: !clockwise
        )
    }
}

/** Draws a raster into the current flipped context with UIKit's top-left orientation. */
func drawPlatformImage(_ image: CGImage, scale: CGFloat, in rect: CGRect) {
    let size = CGSize(width: CGFloat(image.width) / scale, height: CGFloat(image.height) / scale)
    NSImage(cgImage: image, size: size).draw(
        in: rect,
        from: .zero,
        operation: .sourceOver,
        fraction: 1,
        respectFlipped: true,
        hints: nil
    )
}
#endif
