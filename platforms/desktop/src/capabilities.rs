use whisker_protocol::{
    CapabilityEntry, CapabilitySupport, ProtocolVersion, RenderCapabilities, RenderCapability,
};

pub(crate) fn host_capabilities() -> RenderCapabilities {
    RenderCapabilities::new(
        ProtocolVersion::CURRENT,
        [
            RenderCapability::EllipticalBorderRadius,
            RenderCapability::VisualEffects,
            RenderCapability::TextEffects,
            RenderCapability::TextTypography,
            RenderCapability::Cursor,
            RenderCapability::ResourceLifecycle,
            RenderCapability::LinearGradients,
            RenderCapability::RadialGradients,
            RenderCapability::ConicGradients,
            RenderCapability::BackgroundGeometry,
            RenderCapability::BackgroundLayerStacking,
            RenderCapability::BackgroundImageResources,
            RenderCapability::BackdropBlur,
            RenderCapability::RadialGradientVariants,
        ]
        .map(|capability| CapabilityEntry {
            capability,
            support: CapabilitySupport::Native,
        }),
    )
    .expect("Desktop capability profile is unique")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_advertises_native_backdrop_blur_and_resource_lifecycle() {
        let profile = host_capabilities();

        assert_eq!(
            profile.support(RenderCapability::BackdropBlur),
            CapabilitySupport::Native
        );
        assert_eq!(
            profile.support(RenderCapability::ResourceLifecycle),
            CapabilitySupport::Native
        );
    }
}
