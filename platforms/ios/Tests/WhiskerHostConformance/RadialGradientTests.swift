import CoreGraphics
import WhiskerCBridge
@testable import WhiskerRuntime
import XCTest

final class RadialGradientTests: XCTestCase {
    private let center = WhiskerMobileLengthPercentage(length: 0, fraction: 0.5)
    private let zero = WhiskerMobileLengthPercentage()

    func testFarthestCornerResolvesCircleAndEllipseAgainstImageBox() {
        let painter = HostBackgroundPainter()
        let bounds = CGRect(x: 0, y: 0, width: 200, height: 100)
        let circle = painter.resolveRadialRadii(
            gradient(shape: .circle, extent: .farthestCorner),
            in: bounds
        )
        XCTAssertEqual(circle.width, circle.height)
        XCTAssertEqual(circle.width, 111.8034, accuracy: 0.001)

        let ellipse = painter.resolveRadialRadii(
            gradient(shape: .ellipse, extent: .farthestCorner),
            in: bounds
        )
        XCTAssertEqual(ellipse.width, 141.42136, accuracy: 0.001)
        XCTAssertEqual(ellipse.height, 70.71068, accuracy: 0.001)
    }

    func testExplicitCircleUsesOneRadiusOnBothAxes() {
        let radius = WhiskerMobileLengthPercentage(length: 40, fraction: 0)
        let gradient = HostRadialGradient(
            shape: .circle,
            extent: .explicit,
            centerX: center,
            centerY: center,
            radiusX: radius,
            radiusY: zero,
            stops: []
        )
        let radii = HostBackgroundPainter().resolveRadialRadii(
            gradient,
            in: CGRect(x: 0, y: 0, width: 200, height: 100)
        )
        XCTAssertEqual(radii, CGSize(width: 40, height: 40))
    }

    private func gradient(
        shape: HostRadialShape,
        extent: HostRadialExtent
    ) -> HostRadialGradient {
        HostRadialGradient(
            shape: shape,
            extent: extent,
            centerX: center,
            centerY: center,
            radiusX: zero,
            radiusY: zero,
            stops: []
        )
    }
}
