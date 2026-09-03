import UIKit
import WhiskerModule

struct HostBoxPaint {
    let background: UIColor
    let widths: [WhiskerMobileLengthPercentage]
    let colors: [UIColor]
    let styles: [UInt32]
    let radiiHorizontal: [WhiskerMobileLengthPercentage]
    let radiiVertical: [WhiskerMobileLengthPercentage]

    init(_ raw: WhiskerMobileBoxPaint) {
        background = parsePaintColor(raw.background)
        widths = tupleArray(raw.widths)
        colors = tupleArray(raw.colors).map(parsePaintColor)
        styles = tupleArray(raw.styles)
        radiiHorizontal = tupleArray(raw.radii_horizontal)
        radiiVertical = tupleArray(raw.radii_vertical)
    }
}

struct HostBoxShadow {
    let offset: CGSize
    let blurRadius: CGFloat
    let spreadRadius: CGFloat
    let color: UIColor
    let inset: Bool
}

final class HostBoxPainter {
    private let backgroundPainter = HostBackgroundPainter()
    private var fillColor = UIColor.clear
    /// CSS order: top, right, bottom, left.
    private var borderWidths = [CGFloat](repeating: 0, count: 4)
    private var borderColors = [UIColor](repeating: .clear, count: 4)
    private var borderStyles = [UInt32](repeating: 0, count: 4)
    private var cornerRadii = [CGSize](repeating: .zero, count: 4)

    func update(_ paint: HostBoxPaint, bounds: CGRect) {
        fillColor = paint.background
        borderWidths = [
            resolve(paint.widths[0], axis: bounds.height),
            resolve(paint.widths[1], axis: bounds.width),
            resolve(paint.widths[2], axis: bounds.height),
            resolve(paint.widths[3], axis: bounds.width)
        ]
        borderColors = paint.colors
        borderStyles = paint.styles
        cornerRadii = paint.radiiHorizontal.indices.map { index in
            CGSize(
                width: resolve(paint.radiiHorizontal[index], axis: bounds.width),
                height: resolve(paint.radiiVertical[index], axis: bounds.height)
            )
        }
    }

    func draw(in bounds: CGRect, contentBox: CGRect) {
        let path = roundedPath(in: bounds, radii: cornerRadii)
        let widths = Array((borderWidths + [0, 0, 0, 0]).prefix(4)).map { max(0, $0) }
        let paddingBox = CGRect(
            x: bounds.minX + min(widths[3], bounds.width),
            y: bounds.minY + min(widths[0], bounds.height),
            width: max(0, bounds.width - min(widths[3], bounds.width) - min(widths[1], bounds.width)),
            height: max(0, bounds.height - min(widths[0], bounds.height) - min(widths[2], bounds.height))
        )
        let outerRadii = normalizedRadii(cornerRadii, in: bounds)
        let paddingRadii = insetCornerRadii(
            outerRadii,
            top: widths[0],
            right: widths[1],
            bottom: widths[2],
            left: widths[3]
        )
        let contentRadii = insetCornerRadii(
            outerRadii,
            top: max(0, contentBox.minY - bounds.minY),
            right: max(0, bounds.maxX - contentBox.maxX),
            bottom: max(0, bounds.maxY - contentBox.maxY),
            left: max(0, contentBox.minX - bounds.minX)
        )
        let paddingPath = roundedPath(in: paddingBox, radii: paddingRadii)
        let contentPath = roundedPath(in: contentBox, radii: contentRadii)
        fillColor.setFill()
        path.fill()
        backgroundPainter.draw(
            borderBox: bounds,
            paddingBox: paddingBox,
            contentBox: contentBox,
            borderClip: path.cgPath,
            paddingClip: paddingPath.cgPath,
            contentClip: contentPath.cgPath
        )
        drawBorders(in: bounds, clippedBy: path)
    }

    func updateBackgroundLayers(_ layers: [HostBackgroundLayer]) {
        backgroundPainter.update(layers)
    }

    func setImageRendering(_ value: HostImageRendering) {
        backgroundPainter.setImageRendering(value)
    }

    func hardBoxShadowPath(in bounds: CGRect, shadow: HostBoxShadow) -> CGPath? {
        guard !shadow.inset else { return nil }
        let spread = shadow.spreadRadius
        let rect = bounds
            .insetBy(dx: -spread, dy: -spread)
            .offsetBy(dx: shadow.offset.width, dy: shadow.offset.height)
        guard !rect.isEmpty else { return nil }
        let radii = cornerRadii.map {
            CGSize(
                width: max(0, $0.width + spread),
                height: max(0, $0.height + spread)
            )
        }
        return roundedPath(in: rect, radii: radii).cgPath
    }

    func insetBoxShadowPath(in bounds: CGRect, shadow: HostBoxShadow) -> CGPath? {
        guard shadow.inset else { return nil }
        let widths = Array((borderWidths + [0, 0, 0, 0]).prefix(4)).map { max(0, $0) }
        let top = min(widths[0], bounds.height)
        let right = min(widths[1], bounds.width)
        let bottom = min(widths[2], bounds.height)
        let left = min(widths[3], bounds.width)
        let paddingBox = CGRect(
            x: bounds.minX + left,
            y: bounds.minY + top,
            width: max(0, bounds.width - left - right),
            height: max(0, bounds.height - top - bottom)
        )
        guard !paddingBox.isEmpty else { return nil }
        let paddingRadii = insetCornerRadii(
            normalizedRadii(cornerRadii, in: bounds),
            top: top,
            right: right,
            bottom: bottom,
            left: left
        )
        let spread = shadow.spreadRadius
        let hole = paddingBox
            .insetBy(dx: spread, dy: spread)
            .offsetBy(dx: shadow.offset.width, dy: shadow.offset.height)
        let extent = max(bounds.width, bounds.height) + abs(shadow.offset.width) +
            abs(shadow.offset.height) + abs(spread) + shadow.blurRadius * 2
        let exterior = paddingBox.insetBy(dx: -extent, dy: -extent)
        let path = CGMutablePath()
        path.addRect(exterior)
        if !hole.isEmpty {
            let holeRadii = paddingRadii.map {
                CGSize(
                    width: max(0, $0.width - spread),
                    height: max(0, $0.height - spread)
                )
            }
            path.addPath(roundedPath(in: hole, radii: holeRadii).cgPath)
        }
        return path
    }

    func paddingBoxPath(in bounds: CGRect) -> CGPath {
        let paddingBox = paddingBoxRect(in: bounds)
        let widths = Array((borderWidths + [0, 0, 0, 0]).prefix(4)).map { max(0, $0) }
        let top = min(widths[0], bounds.height)
        let right = min(widths[1], bounds.width)
        let bottom = min(widths[2], bounds.height)
        let left = min(widths[3], bounds.width)
        let radii = insetCornerRadii(
            normalizedRadii(cornerRadii, in: bounds),
            top: top,
            right: right,
            bottom: bottom,
            left: left
        )
        return roundedPath(in: paddingBox, radii: radii).cgPath
    }

    func paddingBoxRect(in bounds: CGRect) -> CGRect {
        let widths = Array((borderWidths + [0, 0, 0, 0]).prefix(4)).map { max(0, $0) }
        let top = min(widths[0], bounds.height)
        let right = min(widths[1], bounds.width)
        let bottom = min(widths[2], bounds.height)
        let left = min(widths[3], bounds.width)
        return CGRect(
            x: bounds.minX + left,
            y: bounds.minY + top,
            width: max(0, bounds.width - left - right),
            height: max(0, bounds.height - top - bottom)
        )
    }

    func borderBoxPath(in bounds: CGRect) -> CGPath {
        roundedPath(in: bounds, radii: cornerRadii).cgPath
    }

    func overflowClipPath(
        in bounds: CGRect,
        visibleBounds: CGRect,
        horizontal: Bool,
        vertical: Bool
    ) -> CGPath {
        let widths = Array((borderWidths + [0, 0, 0, 0]).prefix(4)).map { max(0, $0) }
        let top = min(widths[0], bounds.height)
        let right = min(widths[1], bounds.width)
        let bottom = min(widths[2], bounds.height)
        let left = min(widths[3], bounds.width)
        let innerRect = CGRect(
            x: bounds.minX + left,
            y: bounds.minY + top,
            width: max(0, bounds.width - left - right),
            height: max(0, bounds.height - top - bottom)
        )
        guard horizontal || vertical else { return UIBezierPath(rect: visibleBounds).cgPath }
        let outer = normalizedRadii(cornerRadii, in: bounds)
        let inner = [
            CGSize(
                width: max(0, outer[0].width - left),
                height: max(0, outer[0].height - top)
            ),
            CGSize(
                width: max(0, outer[1].width - right),
                height: max(0, outer[1].height - top)
            ),
            CGSize(
                width: max(0, outer[2].width - right),
                height: max(0, outer[2].height - bottom)
            ),
            CGSize(
                width: max(0, outer[3].width - left),
                height: max(0, outer[3].height - bottom)
            )
        ]
        let path = CGMutablePath()
        path.addPath(roundedPath(in: innerRect, radii: inner).cgPath)
        if !vertical {
            path.addRect(CGRect(
                x: innerRect.minX,
                y: visibleBounds.minY,
                width: innerRect.width,
                height: max(0, innerRect.minY - visibleBounds.minY)
            ))
            path.addRect(CGRect(
                x: innerRect.minX,
                y: innerRect.maxY,
                width: innerRect.width,
                height: max(0, visibleBounds.maxY - innerRect.maxY)
            ))
        }
        if !horizontal {
            path.addRect(CGRect(
                x: visibleBounds.minX,
                y: innerRect.minY,
                width: max(0, innerRect.minX - visibleBounds.minX),
                height: innerRect.height
            ))
            path.addRect(CGRect(
                x: innerRect.maxX,
                y: innerRect.minY,
                width: max(0, visibleBounds.maxX - innerRect.maxX),
                height: innerRect.height
            ))
        }
        return path
    }

    private func drawBorders(in bounds: CGRect, clippedBy outerPath: UIBezierPath) {
        guard let context = UIGraphicsGetCurrentContext() else { return }
        let widths = Array((borderWidths + [0, 0, 0, 0]).prefix(4)).map { max(0, $0) }
        let colors = Array((borderColors + [.clear, .clear, .clear, .clear]).prefix(4))
        let styles = Array((borderStyles + [0, 0, 0, 0]).prefix(4))
        guard zip(widths, styles).contains(where: { $0.0 > 0 && paintsBorderStyle($0.1) }) else {
            return
        }

        let top = min(widths[0], bounds.height)
        let right = min(widths[1], bounds.width)
        let bottom = min(widths[2], bounds.height)
        let left = min(widths[3], bounds.width)
        if widths.allSatisfy({ $0 > 0 }) &&
            styles.allSatisfy({ $0 == borderStyleSolid }) &&
            colors.dropFirst().allSatisfy({ $0.isEqual(colors[0]) }) {
            let innerRect = CGRect(
                x: bounds.minX + left,
                y: bounds.minY + top,
                width: max(0, bounds.width - left - right),
                height: max(0, bounds.height - top - bottom)
            )
            let innerRadii = insetCornerRadii(
                normalizedRadii(cornerRadii, in: bounds),
                top: top,
                right: right,
                bottom: bottom,
                left: left
            )
            let ring = CGMutablePath()
            ring.addPath(outerPath.cgPath)
            if !innerRect.isEmpty {
                ring.addPath(roundedPath(in: innerRect, radii: innerRadii).cgPath)
            }
            context.saveGState()
            colors[0].setFill()
            context.addPath(ring)
            context.drawPath(using: .eoFill)
            context.restoreGState()
            return
        }
        let innerTopLeft = CGPoint(x: bounds.minX + left, y: bounds.minY + top)
        let innerTopRight = CGPoint(x: bounds.maxX - right, y: bounds.minY + top)
        let innerBottomRight = CGPoint(x: bounds.maxX - right, y: bounds.maxY - bottom)
        let innerBottomLeft = CGPoint(x: bounds.minX + left, y: bounds.maxY - bottom)
        let regions = [
            [CGPoint(x: bounds.minX, y: bounds.minY), CGPoint(x: bounds.maxX, y: bounds.minY),
             innerTopRight, innerTopLeft],
            [CGPoint(x: bounds.maxX, y: bounds.minY), CGPoint(x: bounds.maxX, y: bounds.maxY),
             innerBottomRight, innerTopRight],
            [CGPoint(x: bounds.maxX, y: bounds.maxY), CGPoint(x: bounds.minX, y: bounds.maxY),
             innerBottomLeft, innerBottomRight],
            [CGPoint(x: bounds.minX, y: bounds.maxY), CGPoint(x: bounds.minX, y: bounds.minY),
             innerTopLeft, innerBottomLeft]
        ]

        context.saveGState()
        context.addPath(outerPath.cgPath)
        context.clip()
        for index in 0..<4 where widths[index] > 0 && paintsBorderStyle(styles[index]) {
            let region = UIBezierPath()
            region.move(to: regions[index][0])
            for point in regions[index].dropFirst() { region.addLine(to: point) }
            region.close()
            context.saveGState()
            context.addPath(region.cgPath)
            context.clip()
            colors[index].setFill()
            if styles[index] == borderStyleSolid {
                region.fill()
            } else if isReliefBorderStyle(styles[index]) {
                drawReliefBorder(
                    in: edgeRect(bounds: bounds, side: index, width: widths[index]),
                    side: index,
                    width: widths[index],
                    style: styles[index],
                    color: colors[index],
                    context: context
                )
            } else {
                drawPatternedBorder(
                    in: edgeRect(bounds: bounds, side: index, width: widths[index]),
                    side: index,
                    width: widths[index],
                    style: styles[index],
                    context: context
                )
            }
            context.restoreGState()
        }
        context.restoreGState()
    }

    private func edgeRect(bounds: CGRect, side: Int, width: CGFloat) -> CGRect {
        switch side {
        case 0: return CGRect(x: bounds.minX, y: bounds.minY, width: bounds.width, height: width)
        case 1: return CGRect(x: bounds.maxX - width, y: bounds.minY, width: width, height: bounds.height)
        case 2: return CGRect(x: bounds.minX, y: bounds.maxY - width, width: bounds.width, height: width)
        default: return CGRect(x: bounds.minX, y: bounds.minY, width: width, height: bounds.height)
        }
    }

    private func drawPatternedBorder(
        in edge: CGRect,
        side: Int,
        width: CGFloat,
        style: UInt32,
        context: CGContext
    ) {
        guard width > 0, !edge.isEmpty else { return }
        if style == borderStyleDouble {
            drawDoubleBorder(in: edge, side: side, width: width, context: context)
            return
        }
        let horizontal = side == 0 || side == 2
        let start = horizontal ? edge.minX : edge.minY
        let end = horizontal ? edge.maxX : edge.maxY
        let center = horizontal ? edge.midY : edge.midX
        if style == borderStyleDashed {
            let dash = width * 3
            let period = width * 4
            var position = start
            while position < end {
                let dashEnd = min(position + dash, end)
                let rect = horizontal
                    ? CGRect(x: position, y: edge.minY, width: dashEnd - position, height: edge.height)
                    : CGRect(x: edge.minX, y: position, width: edge.width, height: dashEnd - position)
                context.fill(rect)
                position += period
            }
        } else {
            let radius = width / 2
            var position = start + width
            while position - radius < end {
                let rect = horizontal
                    ? CGRect(x: position - radius, y: center - radius, width: width, height: width)
                    : CGRect(x: center - radius, y: position - radius, width: width, height: width)
                context.fillEllipse(in: rect)
                position += width * 2
            }
        }
    }

    private func drawDoubleBorder(
        in edge: CGRect,
        side: Int,
        width: CGFloat,
        context: CGContext
    ) {
        let band = width / 3
        let outer: CGRect
        let inner: CGRect
        switch side {
        case 0:
            outer = CGRect(x: edge.minX, y: edge.minY, width: edge.width, height: band)
            inner = CGRect(x: edge.minX, y: edge.maxY - band, width: edge.width, height: band)
        case 1:
            outer = CGRect(x: edge.maxX - band, y: edge.minY, width: band, height: edge.height)
            inner = CGRect(x: edge.minX, y: edge.minY, width: band, height: edge.height)
        case 2:
            outer = CGRect(x: edge.minX, y: edge.maxY - band, width: edge.width, height: band)
            inner = CGRect(x: edge.minX, y: edge.minY, width: edge.width, height: band)
        default:
            outer = CGRect(x: edge.minX, y: edge.minY, width: band, height: edge.height)
            inner = CGRect(x: edge.maxX - band, y: edge.minY, width: band, height: edge.height)
        }
        context.fill(outer)
        context.fill(inner)
    }

    private func drawReliefBorder(
        in edge: CGRect,
        side: Int,
        width: CGFloat,
        style: UInt32,
        color: UIColor,
        context: CGContext
    ) {
        let topOrLeft = side == 0 || side == 3
        if style == borderStyleInset || style == borderStyleOutset {
            let lighter = style == borderStyleInset ? !topOrLeft : topOrLeft
            shadedBorderColor(color, lighter: lighter).setFill()
            context.fill(edge)
            return
        }

        let outerLighter: Bool
        if style == borderStyleGroove {
            outerLighter = !topOrLeft
        } else {
            outerLighter = topOrLeft
        }
        let (outer, inner) = reliefBands(in: edge, side: side, width: width)
        shadedBorderColor(color, lighter: outerLighter).setFill()
        context.fill(outer)
        shadedBorderColor(color, lighter: !outerLighter).setFill()
        context.fill(inner)
    }

    private func reliefBands(in edge: CGRect, side: Int, width: CGFloat) -> (CGRect, CGRect) {
        let band = width / 2
        switch side {
        case 0:
            return (
                CGRect(x: edge.minX, y: edge.minY, width: edge.width, height: band),
                CGRect(x: edge.minX, y: edge.maxY - band, width: edge.width, height: band)
            )
        case 1:
            return (
                CGRect(x: edge.maxX - band, y: edge.minY, width: band, height: edge.height),
                CGRect(x: edge.minX, y: edge.minY, width: band, height: edge.height)
            )
        case 2:
            return (
                CGRect(x: edge.minX, y: edge.maxY - band, width: edge.width, height: band),
                CGRect(x: edge.minX, y: edge.minY, width: edge.width, height: band)
            )
        default:
            return (
                CGRect(x: edge.minX, y: edge.minY, width: band, height: edge.height),
                CGRect(x: edge.maxX - band, y: edge.minY, width: band, height: edge.height)
            )
        }
    }
}

func tupleArray<T>(_ value: (T, T, T, T)) -> [T] {
    [value.0, value.1, value.2, value.3]
}

func tupleArray<T>(_ value: (T, T, T, T, T, T)) -> [T] {
    [value.0, value.1, value.2, value.3, value.4, value.5]
}

private func resolve(_ value: WhiskerMobileLengthPercentage, axis: CGFloat) -> CGFloat {
    CGFloat(value.length) + CGFloat(value.fraction) * axis
}

func insetCornerRadii(
    _ radii: [CGSize],
    top: CGFloat,
    right: CGFloat,
    bottom: CGFloat,
    left: CGFloat
) -> [CGSize] {
    let resolved = Array((radii + [.zero, .zero, .zero, .zero]).prefix(4))
    return [
        CGSize(
            width: max(0, resolved[0].width - left),
            height: max(0, resolved[0].height - top)
        ),
        CGSize(
            width: max(0, resolved[1].width - right),
            height: max(0, resolved[1].height - top)
        ),
        CGSize(
            width: max(0, resolved[2].width - right),
            height: max(0, resolved[2].height - bottom)
        ),
        CGSize(
            width: max(0, resolved[3].width - left),
            height: max(0, resolved[3].height - bottom)
        )
    ]
}

func roundedPath(in rect: CGRect, radii: [CGSize]) -> UIBezierPath {
    let normalized = normalizedRadii(radii, in: rect)

    let k: CGFloat = 0.552_284_749_830_793_6
    let path = UIBezierPath()
    path.move(to: CGPoint(x: rect.minX + normalized[0].width, y: rect.minY))
    path.addLine(to: CGPoint(x: rect.maxX - normalized[1].width, y: rect.minY))
    path.addCurve(
        to: CGPoint(x: rect.maxX, y: rect.minY + normalized[1].height),
        controlPoint1: CGPoint(
            x: rect.maxX - normalized[1].width + k * normalized[1].width,
            y: rect.minY
        ),
        controlPoint2: CGPoint(
            x: rect.maxX,
            y: rect.minY + normalized[1].height - k * normalized[1].height
        )
    )
    path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY - normalized[2].height))
    path.addCurve(
        to: CGPoint(x: rect.maxX - normalized[2].width, y: rect.maxY),
        controlPoint1: CGPoint(
            x: rect.maxX,
            y: rect.maxY - normalized[2].height + k * normalized[2].height
        ),
        controlPoint2: CGPoint(
            x: rect.maxX - normalized[2].width + k * normalized[2].width,
            y: rect.maxY
        )
    )
    path.addLine(to: CGPoint(x: rect.minX + normalized[3].width, y: rect.maxY))
    path.addCurve(
        to: CGPoint(x: rect.minX, y: rect.maxY - normalized[3].height),
        controlPoint1: CGPoint(
            x: rect.minX + normalized[3].width - k * normalized[3].width,
            y: rect.maxY
        ),
        controlPoint2: CGPoint(
            x: rect.minX,
            y: rect.maxY - normalized[3].height + k * normalized[3].height
        )
    )
    path.addLine(to: CGPoint(x: rect.minX, y: rect.minY + normalized[0].height))
    path.addCurve(
        to: CGPoint(x: rect.minX + normalized[0].width, y: rect.minY),
        controlPoint1: CGPoint(
            x: rect.minX,
            y: rect.minY + normalized[0].height - k * normalized[0].height
        ),
        controlPoint2: CGPoint(
            x: rect.minX + normalized[0].width - k * normalized[0].width,
            y: rect.minY
        )
    )
    path.close()
    return path
}

func normalizedRadii(_ radii: [CGSize], in rect: CGRect) -> [CGSize] {
    var normalized = Array((radii + [.zero, .zero, .zero, .zero]).prefix(4)).map {
        let width = max($0.width, 0)
        let height = max($0.height, 0)
        // CSS Backgrounds requires the entire corner radius to become zero when
        // either axis is zero. Keeping a degenerate ellipse here makes
        // CoreGraphics draw a diagonal curve instead of a square corner.
        return width == 0 || height == 0
            ? .zero
            : CGSize(width: width, height: height)
    }
    let horizontalTop = normalized[0].width + normalized[1].width
    let horizontalBottom = normalized[3].width + normalized[2].width
    let verticalLeft = normalized[0].height + normalized[3].height
    let verticalRight = normalized[1].height + normalized[2].height
    let scale = [
        CGFloat(1),
        horizontalTop > 0 ? rect.width / horizontalTop : 1,
        horizontalBottom > 0 ? rect.width / horizontalBottom : 1,
        verticalLeft > 0 ? rect.height / verticalLeft : 1,
        verticalRight > 0 ? rect.height / verticalRight : 1
    ].min() ?? 1
    if scale < 1 {
        normalized = normalized.map {
            CGSize(width: $0.width * scale, height: $0.height * scale)
        }
    }
    return normalized
}

private let borderStyleSolid: UInt32 = 2
private let borderStyleDashed: UInt32 = 3
private let borderStyleDotted: UInt32 = 4
private let borderStyleDouble: UInt32 = 5
private let borderStyleGroove: UInt32 = 6
private let borderStyleRidge: UInt32 = 7
private let borderStyleInset: UInt32 = 8
private let borderStyleOutset: UInt32 = 9

private func paintsBorderStyle(_ style: UInt32) -> Bool {
    style == borderStyleSolid || style == borderStyleDashed || style == borderStyleDotted ||
        style == borderStyleDouble || isReliefBorderStyle(style)
}

private func isReliefBorderStyle(_ style: UInt32) -> Bool {
    style == borderStyleGroove || style == borderStyleRidge || style == borderStyleInset ||
        style == borderStyleOutset
}

private func shadedBorderColor(_ color: UIColor, lighter: Bool) -> UIColor {
    var red: CGFloat = 0
    var green: CGFloat = 0
    var blue: CGFloat = 0
    var alpha: CGFloat = 0
    guard color.getRed(&red, green: &green, blue: &blue, alpha: &alpha) else { return color }
    let amount: CGFloat = 0.45
    if lighter {
        red += (1 - red) * amount
        green += (1 - green) * amount
        blue += (1 - blue) * amount
    } else {
        red *= 1 - amount
        green *= 1 - amount
        blue *= 1 - amount
    }
    return UIColor(red: red, green: green, blue: blue, alpha: alpha)
}
