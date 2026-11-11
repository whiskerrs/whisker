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
        radius: Float
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
        var raw = WhiskerMobileBoxPaint()
        raw.background.kind = 1
        let length = WhiskerMobileLengthPercentage(length: radius, fraction: 0)
        raw.radii_horizontal = (length, length, length, length)
        raw.radii_vertical = (length, length, length, length)
        let paint = HostBoxPaint(raw)
        node.paint = paint
        node.boxPainter.update(paint, bounds: node.bounds)
        node.boxPaintDidChange()
        node.layoutIfNeeded()
        return node
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
        let node = try roundedNode(childPolicy: .none, inset: 0, radius: 20)
        let path = try XCTUnwrap(contentMaskPath(node))
        XCTAssertFalse(path.contains(CGPoint(x: 1, y: 1)))
        XCTAssertFalse(path.contains(CGPoint(x: 39, y: 39)))
        XCTAssertTrue(path.contains(CGPoint(x: 20, y: 20)))
        XCTAssertTrue(path.contains(CGPoint(x: 20, y: 1)))
    }

    @objc func testLeafModuleContentUsesTheInnerContentRadius() throws {
        // A 4pt inset on a 12pt radius leaves an 8pt radius in content coordinates.
        let node = try roundedNode(childPolicy: .none, inset: 4, radius: 12)
        let path = try XCTUnwrap(contentMaskPath(node))
        XCTAssertFalse(path.contains(CGPoint(x: 1, y: 1)))
        XCTAssertTrue(path.contains(CGPoint(x: 4, y: 4)))
        XCTAssertTrue(path.contains(CGPoint(x: 16, y: 1)))
    }

    @objc func testElementWithChildrenKeepsVisibleOverflow() throws {
        let node = try roundedNode(childPolicy: .elements, inset: 0, radius: 20)
        XCTAssertNil(contentMaskPath(node))
    }

    @objc func testSquareContentCornersInstallNoMask() throws {
        let node = try roundedNode(childPolicy: .none, inset: 4, radius: 4)
        XCTAssertNil(contentMaskPath(node))
    }
}
