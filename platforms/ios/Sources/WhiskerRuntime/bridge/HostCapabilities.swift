import WhiskerCBridge

/// Immutable renderer profile advertised once for each mounted Whisker surface.
struct HostRenderProfile: Equatable {
    let abiMajor: UInt16
    let abiMinor: UInt16
    let protocolMajor: UInt16
    let protocolMinor: UInt16
    let native: UInt64
    let emulated: UInt64

    var rawValue: WhiskerMobileHostCapabilities {
        WhiskerMobileHostCapabilities(
            abi_major: abiMajor,
            abi_minor: abiMinor,
            protocol_major: protocolMajor,
            protocol_minor: protocolMinor,
            native: native,
            emulated: emulated
        )
    }
}

enum IOSHostCapabilities {
    static let current = HostRenderProfile(
        abiMajor: UInt16(WHISKER_MOBILE_ABI_MAJOR),
        abiMinor: UInt16(WHISKER_MOBILE_ABI_MINOR),
        protocolMajor: UInt16(WHISKER_FRAME_PROTOCOL_MAJOR),
        protocolMinor: UInt16(WHISKER_FRAME_PROTOCOL_MINOR),
        native: [
            WHISKER_CAPABILITY_ELLIPTICAL_BORDER_RADIUS,
            WHISKER_CAPABILITY_VISUAL_EFFECTS,
            WHISKER_CAPABILITY_TEXT_EFFECTS,
            WHISKER_CAPABILITY_TEXT_TYPOGRAPHY,
            WHISKER_CAPABILITY_CURSOR,
            WHISKER_CAPABILITY_RESOURCE_LIFECYCLE,
            WHISKER_CAPABILITY_LINEAR_GRADIENTS,
            WHISKER_CAPABILITY_RADIAL_GRADIENTS,
            WHISKER_CAPABILITY_CONIC_GRADIENTS,
            WHISKER_CAPABILITY_BACKGROUND_GEOMETRY,
            WHISKER_CAPABILITY_BACKGROUND_LAYER_STACKING,
            WHISKER_CAPABILITY_BACKGROUND_IMAGE_RESOURCES,
            WHISKER_CAPABILITY_RADIAL_GRADIENT_VARIANTS,
        ].reduce(0) { $0 | UInt64($1) },
        // UIKit exposes material intensity, not an exact CSS blur radius.
        emulated: UInt64(WHISKER_CAPABILITY_BACKDROP_BLUR)
    )
}
