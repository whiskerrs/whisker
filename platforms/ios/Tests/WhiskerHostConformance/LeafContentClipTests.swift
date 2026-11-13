#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif
import XCTest
@testable import WhiskerModule
@testable import WhiskerRuntime

private let solidContentElement = "whisker.test/SolidContent"

private final class SolidContentModule: Module {
    override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("SolidContent")
            View(WhiskerElementFactory(name: solidContentElement) {
                UIView(frame: .zero)
            }) {}
        }
    }
}

private var solidContentInstalled = false

extension HostConformanceTests {
    private func roundedNode(
        childPolicy: WhiskerChildPolicy,
        inset: CGFloat,
        radius: Float,
        topLeftRadius: Float? = nil
    ) throws -> WhiskerNodeView {
        if !solidContentInstalled {
            WhiskerModuleKernel.install(SolidContentModule())
            solidContentInstalled = true
        }
        let registration = WhiskerElementRegistration(
            elementType: 30,
            name: solidContentElement,
            childPolicy: childPolicy,
            measurement: .none
        )
        XCTAssertTrue(WhiskerElementRegistry.bind([registration]))
        let mounted = try XCTUnwrap(WhiskerElementRegistry.mount(30) { _, _ in })
        let node = WhiskerNodeView(element: registration.name)
        node.mountedElement = mounted
        node.addSubview(mounted.view)
        node.mountedContentDidInstall()
        let bounds = CGRect(x: 0, y: 0, width: 40, height: 40)
        node.contentFrame = bounds.insetBy(dx: inset, dy: inset)
        node.setLayoutFrame(bounds)
        setRadius(radius, topLeft: topLeftRadius, on: node)
        return node
    }

    private func setRadius(_ radius: Float, topLeft: Float? = nil, on node: WhiskerNodeView) {
        var raw = WhiskerMobileBoxPaint()
        raw.background.kind = 1
        let length = WhiskerMobileLengthPercentage(length: radius, fraction: 0)
        let first = WhiskerMobileLengthPercentage(length: topLeft ?? radius, fraction: 0)
        raw.radii_horizontal = (first, length, length, length)
        raw.radii_vertical = (first, length, length, length)
        let paint = HostBoxPaint(raw)
        node.paint = paint
        node.boxPainter.update(paint, bounds: node.bounds)
        node.boxPaintDidChange()
        node.layoutIfNeeded()
    }

    private func contentLayer(_ node: WhiskerNodeView) -> CALayer? {
        node.mountedElement?.view.layer
    }

    private func contentMaskPath(_ node: WhiskerNodeView) -> CGPath? {
        #if canImport(UIKit)
        let layer: CALayer? = node.mountedElement?.view.layer
        #else
        let layer = node.mountedElement?.view.layer
        #endif
        return (layer?.mask as? CAShapeLayer)?.path
    }

    @objc func testLeafModuleContentIsClippedToRoundedBorderBox() throws {
        let node = try roundedNode(childPolicy: .none, inset: 0, radius: 20, topLeftRadius: 10)
        let path = try XCTUnwrap(contentMaskPath(node))
        XCTAssertFalse(path.contains(CGPoint(x: 1, y: 1)))
        XCTAssertFalse(path.contains(CGPoint(x: 39, y: 39)))
        XCTAssertTrue(path.contains(CGPoint(x: 20, y: 20)))
        XCTAssertTrue(path.contains(CGPoint(x: 20, y: 1)))
    }

    @objc func testLeafModuleContentUsesTheInnerContentRadius() throws {
        // A 4pt inset on a 12pt radius leaves an 8pt radius in content coordinates.
        let node = try roundedNode(childPolicy: .none, inset: 4, radius: 12, topLeftRadius: 16)
        let path = try XCTUnwrap(contentMaskPath(node))
        XCTAssertFalse(path.contains(CGPoint(x: 1, y: 1)))
        XCTAssertTrue(path.contains(CGPoint(x: 4, y: 4)))
        XCTAssertTrue(path.contains(CGPoint(x: 16, y: 1)))
    }

    @objc func testUniformlyRoundedLeafContentClipsWithoutAMaskLayer() throws {
        let node = try roundedNode(childPolicy: .none, inset: 4, radius: 12)
        let layer = try XCTUnwrap(contentLayer(node))
        XCTAssertNil(layer.mask)
        XCTAssertEqual(layer.cornerRadius, 8)
        XCTAssertTrue(layer.masksToBounds)

        setRadius(0, on: node)
        XCTAssertNil(layer.mask)
        XCTAssertEqual(layer.cornerRadius, 0)
        #if canImport(UIKit)
        XCTAssertFalse(layer.masksToBounds)
        #endif
    }

    @objc func testElementWithChildrenKeepsVisibleOverflow() throws {
        let node = try roundedNode(childPolicy: .elements, inset: 0, radius: 20)
        XCTAssertNil(contentMaskPath(node))
        #if canImport(UIKit)
        XCTAssertEqual(contentLayer(node)?.masksToBounds, false)
        #endif
    }

    @objc func testSquareContentCornersInstallNoMask() throws {
        let node = try roundedNode(childPolicy: .none, inset: 4, radius: 4)
        XCTAssertNil(contentMaskPath(node))
        #if canImport(UIKit)
        XCTAssertEqual(contentLayer(node)?.masksToBounds, false)
        #endif
    }
}
