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

struct HostRadialGradient {
    let centerX: WhiskerMobileLengthPercentage
    let centerY: WhiskerMobileLengthPercentage
    let radiusX: WhiskerMobileLengthPercentage
    let radiusY: WhiskerMobileLengthPercentage
    let stops: [HostLinearGradientStop]
}

struct HostConicGradient {
    let fromDegrees: CGFloat
    let centerX: WhiskerMobileLengthPercentage
    let centerY: WhiskerMobileLengthPercentage
    let stops: [HostLinearGradientStop]
}

enum HostBackgroundRepeat {
    case `repeat`
    case noRepeat
}

enum HostBackgroundBox {
    case border
    case padding
}

struct HostBackgroundGeometry {
    let positionX: WhiskerMobileLengthPercentage
    let positionY: WhiskerMobileLengthPercentage
    let sizeWidth: WhiskerMobileLengthPercentage?
    let sizeHeight: WhiskerMobileLengthPercentage?
    let repeatX: HostBackgroundRepeat
    let repeatY: HostBackgroundRepeat
    let origin: HostBackgroundBox
    let clip: HostBackgroundBox

    static let initial = HostBackgroundGeometry(
        positionX: WhiskerMobileLengthPercentage(),
        positionY: WhiskerMobileLengthPercentage(),
        sizeWidth: nil,
        sizeHeight: nil,
        repeatX: .repeat,
        repeatY: .repeat,
        origin: .padding,
        clip: .border
    )

    func imageBounds(in positioningBox: CGRect) -> CGRect {
        guard let sizeWidth, let sizeHeight else { return positioningBox }
        let width = resolve(sizeWidth, extent: positioningBox.width)
        let height = resolve(sizeHeight, extent: positioningBox.height)
        return CGRect(
            x: positioningBox.minX + CGFloat(positionX.length) +
                CGFloat(positionX.fraction) * (positioningBox.width - width),
            y: positioningBox.minY + CGFloat(positionY.length) +
                CGFloat(positionY.fraction) * (positioningBox.height - height),
            width: width,
            height: height
        )
    }

    func tileRects(in positioningBox: CGRect, covering paintBounds: CGRect) -> [CGRect] {
        let image = imageBounds(in: positioningBox)
        let xOrigins = backgroundTileOrigins(
            base: image.minX,
            tileSize: image.width,
            coverage: paintBounds.minX..<paintBounds.maxX,
            repeatMode: repeatX
        )
        let yOrigins = backgroundTileOrigins(
            base: image.minY,
            tileSize: image.height,
            coverage: paintBounds.minY..<paintBounds.maxY,
            repeatMode: repeatY
        )
        guard !yOrigins.isEmpty,
              xOrigins.count <= 65_536 / yOrigins.count else { return [] }
        return xOrigins.flatMap { x in
            yOrigins.map { y in
                CGRect(x: x, y: y, width: image.width, height: image.height)
            }
        }
    }
}

private enum HostBackgroundImage {
    case linear(HostLinearGradient)
    case radial(HostRadialGradient)
    case conic(HostConicGradient)
}

final class HostBackgroundPainter {
    private var image: HostBackgroundImage?
    private var geometry = HostBackgroundGeometry.initial
    private var conicCache: (size: CGSize, scale: CGFloat, image: CGImage)?

    func update(
        linearGradient: HostLinearGradient?,
        geometry: HostBackgroundGeometry = .initial
    ) {
        image = linearGradient.map(HostBackgroundImage.linear)
        self.geometry = geometry
        conicCache = nil
    }

    func update(radialGradient: HostRadialGradient, geometry: HostBackgroundGeometry) {
        image = .radial(radialGradient)
        self.geometry = geometry
        conicCache = nil
    }

    func update(conicGradient: HostConicGradient, geometry: HostBackgroundGeometry) {
        image = .conic(conicGradient)
        self.geometry = geometry
        conicCache = nil
    }

    func draw(
        borderBox: CGRect,
        paddingBox: CGRect,
        borderClip: CGPath,
        paddingClip: CGPath
    ) {
        let positioningBox = geometry.origin == .border ? borderBox : paddingBox
        let clipPath = geometry.clip == .border ? borderClip : paddingClip
        let imageBounds = geometry.imageBounds(in: positioningBox)
        guard let image, imageBounds.width > 0, imageBounds.height > 0,
              let context = UIGraphicsGetCurrentContext() else { return }
        context.saveGState()
        context.addPath(clipPath)
        context.clip()
        defer { context.restoreGState() }
        let deviceScale = max(
            hypot(context.ctm.a, context.ctm.c),
            hypot(context.ctm.b, context.ctm.d),
            1
        )
        let leadingEdgeInset = backgroundLeadingEdgeInset(deviceScale: deviceScale)
        for tile in geometry.tileRects(
            in: positioningBox,
            covering: clipPath.boundingBoxOfPath
        ) {
            var tileClip = tile
            if geometry.repeatX == .noRepeat && tileClip.minX > positioningBox.minX {
                tileClip.origin.x += leadingEdgeInset
                tileClip.size.width = max(0, tileClip.width - leadingEdgeInset)
            }
            if geometry.repeatY == .noRepeat && tileClip.minY > positioningBox.minY {
                tileClip.origin.y += leadingEdgeInset
                tileClip.size.height = max(0, tileClip.height - leadingEdgeInset)
            }
            context.saveGState()
            context.clip(to: tileClip)
            switch image {
            case let .linear(gradient):
                drawLinear(gradient, in: tile, context: context)
            case let .radial(gradient):
                drawRadial(gradient, in: tile, context: context)
            case let .conic(gradient):
                drawConic(gradient, in: tile, context: context)
            }
            context.restoreGState()
        }
    }

    private func drawLinear(
        _ linearGradient: HostLinearGradient,
        in bounds: CGRect,
        context: CGContext
    ) {
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
        context.drawLinearGradient(
            gradient,
            start: start,
            end: end,
            options: [.drawsBeforeStartLocation, .drawsAfterEndLocation]
        )
    }

    private func drawRadial(
        _ radialGradient: HostRadialGradient,
        in bounds: CGRect,
        context: CGContext
    ) {
        let center = CGPoint(
            x: resolve(radialGradient.centerX, extent: bounds.width) + bounds.minX,
            y: resolve(radialGradient.centerY, extent: bounds.height) + bounds.minY
        )
        let radiusX = resolve(radialGradient.radiusX, extent: bounds.width)
        let radiusY = resolve(radialGradient.radiusY, extent: bounds.height)
        guard radiusX > 0, radiusY > 0 else { return }
        let resolved = resolveStops(radialGradient.stops, lineLength: radiusX)
        guard let gradient = makeGradient(resolved) else { return }

        context.saveGState()
        context.translateBy(x: center.x, y: center.y)
        context.scaleBy(x: 1, y: radiusY / radiusX)
        context.translateBy(x: -center.x, y: -center.y)
        context.drawRadialGradient(
            gradient.value,
            startCenter: center,
            startRadius: radiusX * gradient.domainStart,
            endCenter: center,
            endRadius: radiusX * gradient.domainEnd,
            options: [.drawsBeforeStartLocation, .drawsAfterEndLocation]
        )
        context.restoreGState()
    }

    private func drawConic(
        _ conicGradient: HostConicGradient,
        in bounds: CGRect,
        context: CGContext
    ) {
        let stops = normalizedConicStops(conicGradient.stops)
        guard stops.count >= 2 else { return }
        let scale = max(hypot(context.ctm.a, context.ctm.c), 1)
        let image: CGImage
        if let cache = conicCache, cache.size == bounds.size, cache.scale == scale {
            image = cache.image
        } else {
            guard let rendered = rasterizeConic(
                conicGradient,
                stops: stops,
                size: bounds.size,
                scale: scale
            ) else { return }
            conicCache = (bounds.size, scale, rendered)
            image = rendered
        }

        UIImage(cgImage: image, scale: scale, orientation: .up).draw(in: bounds)
    }
}

func backgroundLeadingEdgeInset(deviceScale: CGFloat) -> CGFloat {
    0.5 / max(deviceScale, 1)
}

func backgroundTileOrigins(
    base: CGFloat,
    tileSize: CGFloat,
    coverage: Range<CGFloat>,
    repeatMode: HostBackgroundRepeat
) -> [CGFloat] {
    guard tileSize > 0, coverage.lowerBound < coverage.upperBound else { return [] }
    guard repeatMode == .repeat else { return [base] }
    let first = base + floor((coverage.lowerBound - base) / tileSize) * tileSize
    let rawCount = ceil((coverage.upperBound - first) / tileSize)
    guard rawCount.isFinite, rawCount > 0, rawCount <= 65_536 else { return [] }
    let count = Int(rawCount)
    return (0..<count).map { first + CGFloat($0) * tileSize }
}

private struct ResolvedGradient {
    let value: CGGradient
    let domainStart: CGFloat
    let domainEnd: CGFloat
}

private func resolve(
    _ value: WhiskerMobileLengthPercentage,
    extent: CGFloat
) -> CGFloat {
    CGFloat(value.length) + CGFloat(value.fraction) * extent
}

private func resolveStops(
    _ stops: [HostLinearGradientStop],
    lineLength: CGFloat
) -> [(color: UIColor, position: CGFloat)] {
    stops.map { stop in
        (
            color: stop.color,
            position: CGFloat(stop.position.length) / lineLength +
                CGFloat(stop.position.fraction)
        )
    }
}

private func makeGradient(
    _ input: [(color: UIColor, position: CGFloat)]
) -> ResolvedGradient? {
    guard input.count >= 2 else { return nil }
    var resolved = input
    let domainStart = min(0, resolved[0].position)
    let domainEnd = max(1, resolved[resolved.count - 1].position)
    let domainLength = domainEnd - domainStart
    guard domainLength > 0 else { return nil }
    if resolved[0].position > domainStart {
        resolved.insert((resolved[0].color, domainStart), at: 0)
    }
    if resolved[resolved.count - 1].position < domainEnd {
        resolved.append((resolved[resolved.count - 1].color, domainEnd))
    }
    let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
    let components = resolved.flatMap { rgbaComponents($0.color) }
    let locations = resolved.map { ($0.position - domainStart) / domainLength }
    guard let value = CGGradient(
        colorSpace: colorSpace,
        colorComponents: components,
        locations: locations,
        count: resolved.count
    ) else { return nil }
    return ResolvedGradient(value: value, domainStart: domainStart, domainEnd: domainEnd)
}

private func normalizedConicStops(
    _ input: [HostLinearGradientStop]
) -> [(color: UIColor, position: CGFloat)] {
    guard input.count >= 2 else { return [] }
    var previous = -CGFloat.infinity
    let ordered = input.map { stop -> (color: UIColor, position: CGFloat) in
        let position = max(previous, CGFloat(stop.position.fraction))
        previous = position
        return (stop.color, position)
    }
    var result = [(color: color(at: 0, in: ordered), position: CGFloat(0))]
    result.append(contentsOf: ordered.filter { (0...1).contains($0.position) })
    result.append((color: color(at: 1, in: ordered), position: 1))
    return result
}

/// Evaluates the resolved, non-repeating stop list at one turn fraction.
private func color(
    at position: CGFloat,
    in stops: [(color: UIColor, position: CGFloat)]
) -> UIColor {
    guard let first = stops.first, position > first.position else {
        return stops.first?.color ?? .clear
    }
    for (left, right) in zip(stops, stops.dropFirst()) where position <= right.position {
        let distance = right.position - left.position
        guard distance > 0 else { return right.color }
        return interpolate(left.color, right.color, amount: (position - left.position) / distance)
    }
    return stops.last?.color ?? .clear
}

private func rasterizeConic(
    _ gradient: HostConicGradient,
    stops: [(color: UIColor, position: CGFloat)],
    size: CGSize,
    scale: CGFloat
) -> CGImage? {
    let width = max(Int(ceil(size.width * scale)), 1)
    let height = max(Int(ceil(size.height * scale)), 1)
    let centerX = resolve(gradient.centerX, extent: size.width) * scale
    let centerY = resolve(gradient.centerY, extent: size.height) * scale
    let startTurn = gradient.fromDegrees / 360
    let rasterStops = stops.map { (color: rgba($0.color), position: $0.position) }
    var pixels = [UInt8](repeating: 0, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let dx = CGFloat(x) + 0.5 - centerX
            let dy = CGFloat(y) + 0.5 - centerY
            let turn = (atan2(dx, -dy) / (2 * .pi) - startTurn).truncatingRemainder(
                dividingBy: 1
            )
            let components = conicColor(
                at: turn < 0 ? turn + 1 : turn,
                in: rasterStops
            )
            let alpha = max(0, min(1, components.alpha))
            let offset = (y * width + x) * 4
            pixels[offset] = byte(components.red * alpha)
            pixels[offset + 1] = byte(components.green * alpha)
            pixels[offset + 2] = byte(components.blue * alpha)
            pixels[offset + 3] = byte(alpha)
        }
    }
    guard let bitmap = CGContext(
        data: &pixels,
        width: width,
        height: height,
        bitsPerComponent: 8,
        bytesPerRow: width * 4,
        space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else { return nil }
    return bitmap.makeImage()
}

private struct RGBA {
    let red: CGFloat
    let green: CGFloat
    let blue: CGFloat
    let alpha: CGFloat
}

private func rgba(_ color: UIColor) -> RGBA {
    let components = rgbaComponents(color)
    return RGBA(
        red: components[0],
        green: components[1],
        blue: components[2],
        alpha: components[3]
    )
}

private func conicColor(
    at position: CGFloat,
    in stops: [(color: RGBA, position: CGFloat)]
) -> RGBA {
    guard let first = stops.first, position > first.position else {
        return stops.first?.color ?? RGBA(red: 0, green: 0, blue: 0, alpha: 0)
    }
    for (left, right) in zip(stops, stops.dropFirst()) where position <= right.position {
        let distance = right.position - left.position
        guard distance > 0 else { return right.color }
        let amount = (position - left.position) / distance
        return RGBA(
            red: left.color.red + (right.color.red - left.color.red) * amount,
            green: left.color.green + (right.color.green - left.color.green) * amount,
            blue: left.color.blue + (right.color.blue - left.color.blue) * amount,
            alpha: left.color.alpha + (right.color.alpha - left.color.alpha) * amount
        )
    }
    return stops.last?.color ?? RGBA(red: 0, green: 0, blue: 0, alpha: 0)
}

private func byte(_ value: CGFloat) -> UInt8 {
    UInt8((max(0, min(1, value)) * 255).rounded())
}

private func interpolate(_ left: UIColor, _ right: UIColor, amount: CGFloat) -> UIColor {
    let lhs = rgbaComponents(left)
    let rhs = rgbaComponents(right)
    return UIColor(
        red: lhs[0] + (rhs[0] - lhs[0]) * amount,
        green: lhs[1] + (rhs[1] - lhs[1]) * amount,
        blue: lhs[2] + (rhs[2] - lhs[2]) * amount,
        alpha: lhs[3] + (rhs[3] - lhs[3]) * amount
    )
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
