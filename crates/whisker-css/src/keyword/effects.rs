//! Visual-effect keyword enums.

use core::fmt;

use crate::to_css::ToCss;

/// Lynx-compatible `image-rendering` value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageRendering {
    /// Use the Host's normal interpolation policy.
    #[default]
    Auto,
    /// Preserve hard source-pixel edges with nearest-neighbor sampling.
    Pixelated,
    /// Lynx-compatible keyword currently equivalent to `auto`.
    CrispEdges,
}

impl ToCss for ImageRendering {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            Self::Auto => "auto",
            Self::Pixelated => "pixelated",
            Self::CrispEdges => "crisp-edges",
        })
    }
}
