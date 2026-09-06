import UIKit
import XCTest
@testable import WhiskerModule
@testable import WhiskerRuntime

extension HostConformanceTests {
    private func scrollNode() throws -> (WhiskerNodeView, WhiskerScrollContainerView) {
        let registration = WhiskerElementRegistration(
            elementType: 3,
            name: WhiskerBuiltInElements.scrollViewName,
            childPolicy: .elements,
            measurement: .none,
            properties: [
                WhiskerPropertyBinding(id: 1, name: "scroll-orientation", value: .string),
                WhiskerPropertyBinding(id: 2, name: "item-snap", value: .map),
                WhiskerPropertyBinding(id: 3, name: "scroll-snap-stop", value: .string),
                WhiskerPropertyBinding(id: 4, name: "enable-scroll", value: .bool),
            ],
            events: [WhiskerEventBinding(id: 1, name: "scroll", detail: .map)],
            commands: [
                WhiskerCommandBinding(id: 1, name: "scrollTo", arguments: .map),
                WhiskerCommandBinding(id: 2, name: "scrollBy", arguments: .map),
            ]
        )
        XCTAssertTrue(WhiskerElementRegistry.bind([registration]))
        let mounted = try XCTUnwrap(WhiskerElementRegistry.mount(3) { _, _ in })
        let scroll = try XCTUnwrap(mounted.view as? WhiskerScrollContainerView)
        let node = WhiskerNodeView(element: registration.name)
        node.mountedElement = mounted
        node.addSubview(scroll)
        node.contentFrame = CGRect(x: 0, y: 0, width: 300, height: 200)
        node.setLayoutFrame(node.contentFrame)
        node.mountedContentDidInstall()
        node.layoutIfNeeded()
        scroll.setScrollOrientation("horizontal")
        return (node, scroll)
    }

    private func row(in scroll: WhiskerScrollContainerView) -> (WhiskerNodeView, WhiskerNodeView) {
        let row = WhiskerNodeView(element: WhiskerBuiltInElements.viewName)
        row.setLayoutFrame(CGRect(x: 0, y: 0, width: 300, height: 200))
        scroll.contentView.addSubview(row)
        let card = WhiskerNodeView(element: WhiskerBuiltInElements.viewName)
        row.sceneChildrenHost().addSubview(card)
        card.setLayoutFrame(CGRect(x: 240, y: 0, width: 200, height: 200))
        return (row, card)
    }

    @objc func testHorizontalRangeIncludesCardsOverflowingViewportSizedRow() throws {
        let (node, scroll) = try scrollNode()
        let (_, card) = row(in: scroll)
        withExtendedLifetime(node) {
            scroll.layoutIfNeeded()
            XCTAssertEqual(scroll.contentSize.width, 440)
            card.setLayoutFrame(CGRect(x: 400, y: 0, width: 200, height: 200))
            scroll.layoutIfNeeded()
            XCTAssertEqual(scroll.contentSize.width, 600)
            card.removeFromSuperview()
            scroll.layoutIfNeeded()
            XCTAssertEqual(scroll.contentSize.width, 300)
        }
    }

    @objc func testOverflowClipLimitsOnlyItsOwnAxis() throws {
        let (node, scroll) = try scrollNode()
        let (row, _) = row(in: scroll)
        withExtendedLifetime(node) {
            row.setOverflowClip(horizontal: false, vertical: true)
            scroll.layoutIfNeeded()
            XCTAssertEqual(scroll.contentSize.width, 440)
            row.setOverflowClip(horizontal: true, vertical: false)
            scroll.layoutIfNeeded()
            XCTAssertEqual(scroll.contentSize.width, 300)
        }
    }

    @objc func testNativeScrollingDoesNotRemeasureDescendantGeometry() {
        let scroll = WhiskerScrollContainerView(frame: CGRect(x: 0, y: 0, width: 300, height: 200))
        var measurements = 0
        scroll.installWhiskerContentExtentSource {
            measurements += 1
            return CGSize(width: 1000, height: 200)
        }
        scroll.layoutIfNeeded()
        let initial = measurements
        XCTAssertGreaterThan(initial, 0)
        for x in stride(from: 10, through: 200, by: 10) {
            scroll.contentOffset.x = CGFloat(x)
            scroll.layoutIfNeeded()
        }
        XCTAssertEqual(measurements, initial)
    }

    @objc func testNestedScrollContentDoesNotExpandOuterViewport() throws {
        let (outerNode, outerScroll) = try scrollNode()
        let (innerNode, innerScroll) = try scrollNode()
        outerScroll.contentView.addSubview(innerNode)
        _ = row(in: innerScroll)
        withExtendedLifetime(outerNode) {
            innerScroll.layoutIfNeeded()
            outerScroll.layoutIfNeeded()
            XCTAssertEqual(innerScroll.contentSize.width, 440)
            XCTAssertEqual(outerScroll.contentSize.width, 300)
            innerScroll.contentOffset.x = 100
            outerScroll.layoutIfNeeded()
            XCTAssertEqual(outerScroll.contentSize.width, 300)
        }
    }
}
