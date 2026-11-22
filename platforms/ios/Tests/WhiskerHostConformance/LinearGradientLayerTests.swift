import CoreGraphics
import Foundation
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif
import WhiskerCBridge
@testable import WhiskerModule
@testable import WhiskerRuntime
import XCTest

final class LinearGradientLayerTests: XCTestCase {
    func testBorderBoxLinearGradientPaintsWithoutRasterizingTheBox() throws {
        let bounds = CGRect(x: 0, y: 0, width: 200, height: 100)
        let painter = makePainter(layers: [linearLayer(angle: 180)], bounds: bounds)

        let gradient = try XCTUnwrap(layerGradient(painter.paintPlan(in: bounds)))
        XCTAssertEqual(gradient.locations, [0, 1])
        assertPoint(gradient.start, CGPoint(x: 100, y: 0))
        assertPoint(gradient.end, CGPoint(x: 100, y: 100))
        let strip = try XCTUnwrap(gradient.makeStripImage(texels: 300))
        XCTAssertEqual(strip.width, 300)
        XCTAssertEqual(strip.height, 1)
    }

    func testGradientLayerFollowsTheCSSGradientLineOfTheBox() throws {
        let bounds = CGRect(x: 0, y: 0, width: 200, height: 100)
        let painter = makePainter(layers: [linearLayer(angle: 135)], bounds: bounds)

        let gradient = try XCTUnwrap(layerGradient(painter.paintPlan(in: bounds)))
        // The 135deg line spans |200 sin| + |100 cos| through the center.
        assertPoint(gradient.start, CGPoint(x: 25, y: -25))
        assertPoint(gradient.end, CGPoint(x: 175, y: 125))
    }

    func testGradientLayerKeepsAUniformCornerRadius() throws {
        let bounds = CGRect(x: 0, y: 0, width: 100, height: 100)
        let painter = makePainter(layers: [linearLayer(angle: 90)], radius: 8, bounds: bounds)

        guard case let .layer(paint) = painter.paintPlan(in: bounds) else {
            return XCTFail("a uniformly rounded gradient must not rasterize")
        }
        XCTAssertEqual(paint.cornerRadius, 8)
        XCTAssertNotNil(paint.gradient)
    }

    func testOtherBackgroundsStillRasterize() {
        let bounds = CGRect(x: 0, y: 0, width: 100, height: 100)
        let tile = HostBackgroundGeometry(
            positionX: WhiskerMobileLengthPercentage(),
            positionY: WhiskerMobileLengthPercentage(),
            sizeWidth: WhiskerMobileLengthPercentage(length: 10, fraction: 0),
            sizeHeight: WhiskerMobileLengthPercentage(length: 10, fraction: 0),
            repeatX: .repeat,
            repeatY: .repeat,
            origin: .padding,
            clip: .border
        )
        let contentClip = HostBackgroundGeometry(
            positionX: WhiskerMobileLengthPercentage(),
            positionY: WhiskerMobileLengthPercentage(),
            sizeWidth: nil,
            sizeHeight: nil,
            repeatX: .repeat,
            repeatY: .repeat,
            origin: .padding,
            clip: .content
        )
        let radial = HostBackgroundLayer(
            image: .radial(HostRadialGradient(
                centerX: WhiskerMobileLengthPercentage(length: 0, fraction: 0.5),
                centerY: WhiskerMobileLengthPercentage(length: 0, fraction: 0.5),
                radiusX: WhiskerMobileLengthPercentage(length: 0, fraction: 0.5),
                radiusY: WhiskerMobileLengthPercentage(length: 0, fraction: 0.5),
                stops: stops()
            )),
            geometry: .initial
        )
        let cases: [(String, HostBoxPainter)] = [
            ("two layers", makePainter(layers: [linearLayer(angle: 90), linearLayer(angle: 0)], bounds: bounds)),
            ("radial", makePainter(layers: [radial], bounds: bounds)),
            ("tiled", makePainter(layers: [linearLayer(angle: 90, geometry: tile)], bounds: bounds)),
            ("content clip", makePainter(layers: [linearLayer(angle: 90, geometry: contentClip)], bounds: bounds)),
            ("bordered", makePainter(layers: [linearLayer(angle: 90)], borderWidth: 2, bounds: bounds)),
            ("uneven corners", makePainter(layers: [linearLayer(angle: 90)], radius: 8, topLeft: 2, bounds: bounds)),
        ]
        for (name, painter) in cases {
            guard case .draw = painter.paintPlan(in: bounds) else {
                XCTFail("\(name) must keep the rasterized paint")
                continue
            }
        }
    }

    func testNodePresentsTheGradientTopToBottom() throws {
        let node = WhiskerNodeView(element: WhiskerBuiltInElements.viewName)
        let bounds = CGRect(x: 0, y: 0, width: 20, height: 20)
        node.setLayoutFrame(bounds)
        let paint = HostBoxPaint(WhiskerMobileBoxPaint())
        node.paint = paint
        node.boxPainter.update(paint, bounds: node.bounds)
        node.boxPainter.updateBackgroundLayers([linearLayer(angle: 180)])
        node.boxPaintDidChange()
        node.layoutIfNeeded()

        XCTAssertEqual(gradientLayers(in: rootLayer(node)).count, 1)
        let pixels = render(rootLayer(node), size: bounds.size)
        let top = pixel(pixels, x: 10, y: 1, width: 20)
        let bottom = pixel(pixels, x: 10, y: 18, width: 20)
        XCTAssertGreaterThan(top.red, top.blue, "a `to bottom` gradient starts at the top")
        XCTAssertGreaterThan(bottom.blue, bottom.red, "a `to bottom` gradient ends at the bottom")

        node.boxPainter.updateBackgroundLayers([linearLayer(angle: 90)])
        node.boxPaintDidChange()
        let horizontal = render(rootLayer(node), size: bounds.size)
        XCTAssertGreaterThan(pixel(horizontal, x: 2, y: 10, width: 20).red, 200, "`to right` starts at the left")
        XCTAssertGreaterThan(pixel(horizontal, x: 18, y: 10, width: 20).blue, 200, "`to right` ends at the right")

        node.boxPainter.updateBackgroundLayers([linearLayer(angle: 135)])
        node.boxPaintDidChange()
        let diagonal = render(rootLayer(node), size: bounds.size)
        let topRight = pixel(diagonal, x: 18, y: 1, width: 20)
        XCTAssertGreaterThan(pixel(diagonal, x: 1, y: 1, width: 20).red, 200, "`to bottom right` starts top left")
        XCTAssertGreaterThan(pixel(diagonal, x: 18, y: 18, width: 20).blue, 200, "`to bottom right` ends bottom right")
        XCTAssertEqual(Int(topRight.red), Int(topRight.blue), accuracy: 24, "the other diagonal is mid-way")

        node.boxPainter.updateBackgroundLayers([])
        node.boxPaintDidChange()
        XCTAssertTrue(gradientLayers(in: rootLayer(node)).isEmpty)
    }

    private func rootLayer(_ node: WhiskerNodeView) -> CALayer {
        #if canImport(UIKit)
        node.layer
        #else
        node.backingLayer
        #endif
    }

    private func gradientLayers(in layer: CALayer) -> [CALayer] {
        (layer.sublayers ?? []).flatMap { sublayer -> [CALayer] in
            let own = sublayer.contents != nil ? [sublayer] : []
            return own + gradientLayers(in: sublayer)
        }
    }

    /// RGBA rows from the top edge of the layer down.
    private func render(_ layer: CALayer, size: CGSize) -> [UInt8] {
        // A green strip along the top edge tells which bitmap row the layer's top lands on.
        let marker = CALayer()
        marker.frame = CGRect(x: 0, y: 0, width: 1, height: size.height / 2)
        marker.backgroundColor = CGColor(srgbRed: 0, green: 1, blue: 0, alpha: 1)
        marker.zPosition = 10
        layer.addSublayer(marker)
        defer { marker.removeFromSuperlayer() }
        let width = Int(size.width)
        let height = Int(size.height)
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        pixels.withUnsafeMutableBytes { buffer in
            guard let context = CGContext(
                data: buffer.baseAddress,
                width: width,
                height: height,
                bitsPerComponent: 8,
                bytesPerRow: width * 4,
                space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return }
            layer.render(in: context)
        }
        let markerAtFirstRow = pixels[1] > 200
        guard !markerAtFirstRow else { return pixels }
        let row = width * 4
        return (0..<height).reversed().flatMap { pixels[$0 * row..<($0 + 1) * row] }
    }

    private func pixel(_ pixels: [UInt8], x: Int, y: Int, width: Int) -> (red: UInt8, blue: UInt8) {
        let offset = (y * width + x) * 4
        return (pixels[offset], pixels[offset + 2])
    }

    private func stops() -> [HostLinearGradientStop] {
        [(255, 0, 0, 0), (0, 0, 255, 1)].map { red, green, blue, position in
            var raw = WhiskerMobileGradientStop()
            raw.color.kind = 1
            raw.color.red = UInt8(red)
            raw.color.green = UInt8(green)
            raw.color.blue = UInt8(blue)
            raw.color.alpha = 1
            raw.position.fraction = Float(position)
            return HostLinearGradientStop(raw)
        }
    }

    private func linearLayer(
        angle: CGFloat,
        geometry: HostBackgroundGeometry = .initial
    ) -> HostBackgroundLayer {
        HostBackgroundLayer(
            image: .linear(HostLinearGradient(angleDegrees: angle, stops: stops())),
            geometry: geometry
        )
    }

    private func makePainter(
        layers: [HostBackgroundLayer],
        borderWidth: Float = 0,
        radius: Float = 0,
        topLeft: Float? = nil,
        bounds: CGRect
    ) -> HostBoxPainter {
        var raw = WhiskerMobileBoxPaint()
        let width = WhiskerMobileLengthPercentage(length: borderWidth, fraction: 0)
        raw.widths = (width, width, width, width)
        var color = WhiskerMobileColor()
        color.kind = 1
        color.alpha = 1
        raw.colors = (color, color, color, color)
        // CSS `solid`.
        raw.styles = (2, 2, 2, 2)
        let corner = WhiskerMobileLengthPercentage(length: radius, fraction: 0)
        let first = WhiskerMobileLengthPercentage(length: topLeft ?? radius, fraction: 0)
        raw.radii_horizontal = (first, corner, corner, corner)
        raw.radii_vertical = (first, corner, corner, corner)
        let painter = HostBoxPainter()
        painter.update(HostBoxPaint(raw), bounds: bounds)
        painter.updateBackgroundLayers(layers)
        return painter
    }

    private func layerGradient(_ plan: HostBoxPaintPlan) -> HostLinearGradientAxis? {
        guard case let .layer(paint) = plan else { return nil }
        return paint.gradient
    }

    private func assertPoint(
        _ actual: CGPoint,
        _ expected: CGPoint,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        XCTAssertEqual(actual.x, expected.x, accuracy: 0.0001, file: file, line: line)
        XCTAssertEqual(actual.y, expected.y, accuracy: 0.0001, file: file, line: line)
    }
}
