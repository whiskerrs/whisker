import UIKit
import WhiskerModule

enum HostClipReferenceBox {
    case border
    case padding
    case content
}

enum HostClipPath {
    case inset(HostInsetClipPath)
    case circle(HostCircleClipPath)
    case ellipse(HostEllipseClipPath)
    case path(HostPathClipPath)

    var fillRule: CAShapeLayerFillRule {
        if case let .path(shape) = self, shape.evenOdd { return .evenOdd }
        return .nonZero
    }

    func path(in bounds: CGRect, contentBox: CGRect, painter: HostBoxPainter) -> CGPath {
        switch self {
        case let .inset(shape): shape.path(in: bounds, contentBox: contentBox, painter: painter)
        case let .circle(shape): shape.path(in: bounds, contentBox: contentBox, painter: painter)
        case let .ellipse(shape): shape.path(in: bounds, contentBox: contentBox, painter: painter)
        case let .path(shape): shape.path(in: bounds, contentBox: contentBox, painter: painter)
        }
    }
}

struct HostPathCommand {
    let kind: UInt32
    let points: [WhiskerMobileLengthPercentage]
}

struct HostPathClipPath {
    let referenceBox: HostClipReferenceBox
    let evenOdd: Bool
    let commands: [HostPathCommand]

    func path(in bounds: CGRect, contentBox: CGRect, painter: HostBoxPainter) -> CGPath {
        let reference = clipReferenceBox(referenceBox, bounds, contentBox, painter)
        func point(_ command: HostPathCommand, _ offset: Int) -> CGPoint {
            CGPoint(
                x: reference.minX + resolve(command.points[offset], axis: reference.width),
                y: reference.minY + resolve(command.points[offset + 1], axis: reference.height)
            )
        }
        let path = CGMutablePath()
        for command in commands {
            switch command.kind {
            case UInt32(WHISKER_PATH_MOVE_TO): path.move(to: point(command, 0))
            case UInt32(WHISKER_PATH_LINE_TO): path.addLine(to: point(command, 0))
            case UInt32(WHISKER_PATH_QUADRATIC_TO):
                path.addQuadCurve(to: point(command, 2), control: point(command, 0))
            case UInt32(WHISKER_PATH_CUBIC_TO):
                path.addCurve(
                    to: point(command, 4),
                    control1: point(command, 0),
                    control2: point(command, 2)
                )
            case UInt32(WHISKER_PATH_CLOSE): path.closeSubpath()
            default: break
            }
        }
        return path
    }
}

/// Rounded inset basic shape copied out of the borrowed mobile ABI payload.
struct HostInsetClipPath {
    let referenceBox: HostClipReferenceBox
    let edges: [WhiskerMobileLengthPercentage]
    let radiiHorizontal: [WhiskerMobileLengthPercentage]
    let radiiVertical: [WhiskerMobileLengthPercentage]

    func path(in bounds: CGRect, contentBox: CGRect, painter: HostBoxPainter) -> CGPath {
        let reference = switch referenceBox {
        case .border: bounds
        case .padding: painter.paddingBoxRect(in: bounds)
        case .content: contentBox
        }
        let top = resolve(edges[0], axis: reference.height)
        let right = resolve(edges[1], axis: reference.width)
        let bottom = resolve(edges[2], axis: reference.height)
        let left = resolve(edges[3], axis: reference.width)
        let inset = CGRect(
            x: reference.minX + left,
            y: reference.minY + top,
            width: max(0, reference.width - left - right),
            height: max(0, reference.height - top - bottom)
        )
        let radii = radiiHorizontal.indices.map { index in
            CGSize(
                width: max(0, resolve(radiiHorizontal[index], axis: inset.width)),
                height: max(0, resolve(radiiVertical[index], axis: inset.height))
            )
        }
        return roundedPath(in: inset, radii: radii).cgPath
    }
}

struct HostCircleClipPath {
    let referenceBox: HostClipReferenceBox
    let radius: WhiskerMobileLengthPercentage
    let centerX: WhiskerMobileLengthPercentage
    let centerY: WhiskerMobileLengthPercentage

    func path(in bounds: CGRect, contentBox: CGRect, painter: HostBoxPainter) -> CGPath {
        let reference = clipReferenceBox(referenceBox, bounds, contentBox, painter)
        let center = CGPoint(
            x: reference.minX + resolve(centerX, axis: reference.width),
            y: reference.minY + resolve(centerY, axis: reference.height)
        )
        let diagonal = hypot(reference.width, reference.height) / sqrt(2)
        let radius = max(0, resolve(radius, axis: diagonal))
        return UIBezierPath(arcCenter: center, radius: radius, startAngle: 0, endAngle: .pi * 2, clockwise: true).cgPath
    }
}

struct HostEllipseClipPath {
    let referenceBox: HostClipReferenceBox
    let radiusX: WhiskerMobileLengthPercentage
    let radiusY: WhiskerMobileLengthPercentage
    let centerX: WhiskerMobileLengthPercentage
    let centerY: WhiskerMobileLengthPercentage

    func path(in bounds: CGRect, contentBox: CGRect, painter: HostBoxPainter) -> CGPath {
        let reference = clipReferenceBox(referenceBox, bounds, contentBox, painter)
        let centerX = reference.minX + resolve(centerX, axis: reference.width)
        let centerY = reference.minY + resolve(centerY, axis: reference.height)
        let radiusX = max(0, resolve(radiusX, axis: reference.width))
        let radiusY = max(0, resolve(radiusY, axis: reference.height))
        return UIBezierPath(ovalIn: CGRect(
            x: centerX - radiusX, y: centerY - radiusY,
            width: radiusX * 2, height: radiusY * 2
        )).cgPath
    }
}

private func clipReferenceBox(
    _ box: HostClipReferenceBox,
    _ bounds: CGRect,
    _ contentBox: CGRect,
    _ painter: HostBoxPainter
) -> CGRect {
    switch box {
    case .border: bounds
    case .padding: painter.paddingBoxRect(in: bounds)
    case .content: contentBox
    }
}

private func resolve(_ value: WhiskerMobileLengthPercentage, axis: CGFloat) -> CGFloat {
    CGFloat(value.length) + CGFloat(value.fraction) * axis
}
