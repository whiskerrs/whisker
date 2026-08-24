import CoreGraphics
import UIKit
import WhiskerModule

struct HostLinearGradientStop {
    let color: UIColor
    let position: WhiskerMobileLengthPercentage

    init(_ raw: WhiskerMobileGradientStop) {
        color = parsePaintColor(raw.color)
        position = raw.position
    }
}

struct HostLinearGradient {
    let angleDegrees: CGFloat
    let stops: [HostLinearGradientStop]
}

final class HostBackgroundPainter {
    private var linearGradient: HostLinearGradient?

    func update(linearGradient: HostLinearGradient?) {
        self.linearGradient = linearGradient
    }

    func draw(in bounds: CGRect, clippedBy clipPath: CGPath) {
        guard let linearGradient, bounds.width > 0, bounds.height > 0,
              let context = UIGraphicsGetCurrentContext() else { return }
        let radians = linearGradient.angleDegrees * .pi / 180
        let direction = CGVector(dx: sin(radians), dy: -cos(radians))
        let lineLength = abs(bounds.width * direction.dx) + abs(bounds.height * direction.dy)
        guard lineLength > 0 else { return }

        var resolved = linearGradient.stops.map { stop in
            (
                color: stop.color,
                position: CGFloat(stop.position.length) / lineLength +
                    CGFloat(stop.position.fraction)
            )
        }
        guard resolved.count >= 2 else { return }
        let domainStart = min(0, resolved[0].position)
        let domainEnd = max(1, resolved[resolved.count - 1].position)
        let domainLength = domainEnd - domainStart
        guard domainLength > 0 else { return }
        if resolved[0].position > domainStart {
            resolved.insert((resolved[0].color, domainStart), at: 0)
        }
        if resolved[resolved.count - 1].position < domainEnd {
            resolved.append((resolved[resolved.count - 1].color, domainEnd))
        }

        let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
        let components = resolved.flatMap { rgbaComponents($0.color) }
        let locations = resolved.map { ($0.position - domainStart) / domainLength }
        guard let gradient = CGGradient(
            colorSpace: colorSpace,
            colorComponents: components,
            locations: locations,
            count: resolved.count
        ) else { return }
        let center = CGPoint(x: bounds.midX, y: bounds.midY)
        let start = CGPoint(
            x: center.x + direction.dx * lineLength * (domainStart - 0.5),
            y: center.y + direction.dy * lineLength * (domainStart - 0.5)
        )
        let end = CGPoint(
            x: center.x + direction.dx * lineLength * (domainEnd - 0.5),
            y: center.y + direction.dy * lineLength * (domainEnd - 0.5)
        )
        context.saveGState()
        context.addPath(clipPath)
        context.clip()
        context.drawLinearGradient(
            gradient,
            start: start,
            end: end,
            options: [.drawsBeforeStartLocation, .drawsAfterEndLocation]
        )
        context.restoreGState()
    }
}

private func rgbaComponents(_ color: UIColor) -> [CGFloat] {
    var red: CGFloat = 0
    var green: CGFloat = 0
    var blue: CGFloat = 0
    var alpha: CGFloat = 0
    guard color.getRed(&red, green: &green, blue: &blue, alpha: &alpha) else {
        return [0, 0, 0, 0]
    }
    return [red, green, blue, alpha]
}
