import UIKit
import WhiskerModule

struct HostBoxPaint {
    let background: UIColor
    let widths: [WhiskerMobileLengthPercentage]
    let colors: [UIColor]
    let styles: [UInt32]
    let radii: [WhiskerMobileLengthPercentage]

    init(_ raw: WhiskerMobileBoxPaint) {
        background = parsePaintColor(raw.background)
        widths = tupleArray(raw.widths)
        colors = tupleArray(raw.colors).map(parsePaintColor)
        styles = tupleArray(raw.styles)
        radii = tupleArray(raw.radii)
    }
}

final class HostBoxPainter {
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
        cornerRadii = paint.radii.map { value in
            CGSize(
                width: resolve(value, axis: bounds.width),
                height: resolve(value, axis: bounds.height)
            )
        }
    }

    func draw(in bounds: CGRect) {
        let path = roundedPath(in: bounds, radii: cornerRadii)
        fillColor.setFill()
        path.fill()
        drawSolidBorders(in: bounds, clippedBy: path)
    }

    private func drawSolidBorders(in bounds: CGRect, clippedBy outerPath: UIBezierPath) {
        guard let context = UIGraphicsGetCurrentContext() else { return }
        let widths = Array((borderWidths + [0, 0, 0, 0]).prefix(4)).map { max(0, $0) }
        let colors = Array((borderColors + [.clear, .clear, .clear, .clear]).prefix(4))
        let styles = Array((borderStyles + [0, 0, 0, 0]).prefix(4))
        guard zip(widths, styles).contains(where: { $0.0 > 0 && $0.1 == borderStyleSolid }) else {
            return
        }

        let top = min(widths[0], bounds.height)
        let right = min(widths[1], bounds.width)
        let bottom = min(widths[2], bounds.height)
        let left = min(widths[3], bounds.width)
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
        for index in 0..<4 where widths[index] > 0 && styles[index] == borderStyleSolid {
            let region = UIBezierPath()
            region.move(to: regions[index][0])
            for point in regions[index].dropFirst() { region.addLine(to: point) }
            region.close()
            colors[index].setFill()
            region.fill()
        }
        context.restoreGState()
    }
}

private func tupleArray<T>(_ value: (T, T, T, T)) -> [T] {
    [value.0, value.1, value.2, value.3]
}

private func resolve(_ value: WhiskerMobileLengthPercentage, axis: CGFloat) -> CGFloat {
    CGFloat(value.length) + CGFloat(value.fraction) * axis
}

private func roundedPath(in rect: CGRect, radii: [CGSize]) -> UIBezierPath {
    var normalized = Array((radii + [.zero, .zero, .zero, .zero]).prefix(4)).map {
        CGSize(width: max($0.width, 0), height: max($0.height, 0))
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

private let borderStyleSolid: UInt32 = 2
