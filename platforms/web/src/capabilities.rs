use whisker_protocol::{
    CapabilityEntry, CapabilitySupport, ProtocolVersion, RenderCapabilities, RenderCapability,
};

pub(crate) fn detect_host_capabilities() -> RenderCapabilities {
    host_capabilities(
        web_sys::css::supports_with_value("backdrop-filter", "blur(1px)").unwrap_or(false),
    )
}

pub(crate) fn host_capabilities(backdrop_blur: bool) -> RenderCapabilities {
    let mut entries = vec![
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
    ];
    if backdrop_blur {
        entries.push(RenderCapability::BackdropBlur);
    }
    RenderCapabilities::new(
        ProtocolVersion::CURRENT,
        entries.into_iter().map(|capability| CapabilityEntry {
            capability,
            support: CapabilitySupport::Native,
        }),
    )
    .expect("Web capability profile is unique")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_profile_does_not_conflate_backdrop_blur_with_visual_effects() {
        let without_backdrop = host_capabilities(false);
        assert!(without_backdrop.supports(RenderCapability::VisualEffects));
        assert!(!without_backdrop.supports(RenderCapability::BackdropBlur));

        let with_backdrop = host_capabilities(true);
        assert!(with_backdrop.supports(RenderCapability::BackdropBlur));
    }
}
