import XCTest
import WhiskerCBridge
@testable import WhiskerRuntime

final class HostCapabilitiesTests: XCTestCase {
    func testUIKitAdvertisesBackdropBlurAsEmulated() {
        let profile = IOSHostCapabilities.current

        XCTAssertNotEqual(
            profile.native & UInt64(WHISKER_CAPABILITY_VISUAL_EFFECTS),
            0
        )
        XCTAssertEqual(
            profile.native & UInt64(WHISKER_CAPABILITY_BACKDROP_BLUR),
            0
        )
        XCTAssertNotEqual(
            profile.emulated & UInt64(WHISKER_CAPABILITY_BACKDROP_BLUR),
            0
        )
        XCTAssertEqual(profile.rawValue.protocol_minor, UInt16(WHISKER_FRAME_PROTOCOL_MINOR))
    }
}
