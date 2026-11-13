#if canImport(UIKit)
import UIKit
@testable import WhiskerRuntime
import XCTest

final class NativeScrollArbiterTests: XCTestCase {
    func testCaptureStopsTheEnabledScrollViewsAboveTheTouch() {
        let root = UIView()
        let outer = UIScrollView()
        let locked = UIScrollView()
        let leaf = UIView()
        let elsewhere = UIScrollView()
        root.addSubview(outer)
        outer.addSubview(locked)
        locked.addSubview(leaf)
        root.addSubview(elsewhere)
        locked.isScrollEnabled = false
        var arbiter = HostNativeScrollArbiter()
        arbiter.begin(pointer: 1, touched: leaf, root: root)

        XCTAssertEqual(arbiter.capture(pointer: 2), [])
        XCTAssertEqual(arbiter.capture(pointer: 1), [outer])
        XCTAssertTrue(outer.panGestureRecognizer.isEnabled)
        XCTAssertFalse(locked.panGestureRecognizer.isEnabled)

        arbiter.end(pointer: 1)
        XCTAssertEqual(arbiter.capture(pointer: 1), [])
    }
}
#endif
