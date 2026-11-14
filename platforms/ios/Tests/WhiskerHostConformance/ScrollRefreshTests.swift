#if canImport(UIKit)
import UIKit
import XCTest
@testable import WhiskerModule

extension HostConformanceTests {
    private func refreshScrollView(
        onEvent: @escaping (String) -> Void = { _ in }
    ) throws -> (WhiskerMountedElement, WhiskerScrollContainerView, UIWindow) {
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
                WhiskerPropertyBinding(id: 5, name: "enable-refresh", value: .bool),
                WhiskerPropertyBinding(id: 6, name: "refreshing", value: .bool),
            ],
            events: [
                WhiskerEventBinding(id: 1, name: "scroll", detail: .map),
                WhiskerEventBinding(id: 2, name: "refresh", detail: .map),
            ],
            commands: [
                WhiskerCommandBinding(id: 1, name: "scrollTo", arguments: .map),
                WhiskerCommandBinding(id: 2, name: "scrollBy", arguments: .map),
            ]
        )
        XCTAssertTrue(WhiskerElementRegistry.bind([registration]))
        let mounted = try XCTUnwrap(WhiskerElementRegistry.mount(3) { event, _ in onEvent(event.name) })
        let scroll = try XCTUnwrap(mounted.view as? WhiskerScrollContainerView)
        scroll.frame = CGRect(x: 0, y: 0, width: 300, height: 200)
        // UIRefreshControl only starts its indicator inside a window.
        let window = UIWindow(frame: scroll.frame)
        window.addSubview(scroll)
        window.isHidden = false
        return (mounted, scroll, window)
    }

    @objc func testRefreshControlAttachesOnlyWhileEnabledAndVertical() throws {
        let (mounted, scroll, window) = try refreshScrollView()
        defer { window.isHidden = true }
        XCTAssertNil(scroll.refreshControl)
        XCTAssertFalse(scroll.alwaysBounceVertical)

        mounted.setProperty(5, value: .bool(true))
        XCTAssertNotNil(scroll.refreshControl)
        XCTAssertTrue(scroll.alwaysBounceVertical)

        mounted.setProperty(1, value: .string("horizontal"))
        XCTAssertNil(scroll.refreshControl)
        XCTAssertFalse(scroll.alwaysBounceVertical)

        mounted.setProperty(1, value: .string("vertical"))
        XCTAssertNotNil(scroll.refreshControl)

        mounted.clearProperty(5)
        XCTAssertNil(scroll.refreshControl)
        XCTAssertFalse(scroll.alwaysBounceVertical)
    }

    @objc func testPullEmitsRefreshAndRefreshingPropDrivesTheIndicator() throws {
        var events: [String] = []
        let (mounted, scroll, window) = try refreshScrollView { events.append($0) }
        defer { window.isHidden = true }
        mounted.setEventMask(0b11)
        mounted.setProperty(5, value: .bool(true))
        let control = try XCTUnwrap(scroll.refreshControl)

        control.beginRefreshing()
        // Without a host application UIControl cannot route sendActions.
        for target in control.allTargets {
            for action in control.actions(forTarget: target, forControlEvent: .valueChanged) ?? [] {
                _ = (target as NSObject).perform(NSSelectorFromString(action))
            }
        }
        XCTAssertEqual(events.filter { $0 == "refresh" }, ["refresh"])

        mounted.setProperty(6, value: .bool(false))
        XCTAssertFalse(control.isRefreshing)

        mounted.setProperty(6, value: .bool(true))
        XCTAssertTrue(control.isRefreshing)

        mounted.clearProperty(6)
        XCTAssertFalse(control.isRefreshing)
    }

    @objc func testRefreshingRequestedBeforeEnablingStartsTheIndicator() throws {
        let (mounted, scroll, window) = try refreshScrollView()
        defer { window.isHidden = true }
        mounted.setProperty(6, value: .bool(true))
        mounted.setProperty(5, value: .bool(true))
        XCTAssertEqual(scroll.refreshControl?.isRefreshing, true)
    }
}
#endif
