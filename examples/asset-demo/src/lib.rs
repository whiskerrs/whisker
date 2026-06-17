//! `whisker-asset` example app.
//!
//! Displays a single PNG that is **bundled** with the app (under this
//! crate's `assets/` dir) rather than fetched over the network. The
//! image is referenced by its logical path:
//!
//! ```ignore
//! Image(src: asset!("images/logo.png"), …)
//! ```
//!
//! `asset!` lowers to `whisker_asset::resolve("images/logo.png")`,
//! which composes the platform path/URL from the base that
//! whisker-asset's native module installs at launch:
//!
//! - iOS:     `file://<bundle>/whisker_assets/images/logo.png`
//! - Android: `file:///android_asset/whisker/images/logo.png`
//!
//! `whisker-image` then loads that resolved string (Kingfisher /
//! Coil). If the image paints, the full Phase 3 chain — build-plugin
//! bundling → native base install → resolve → native image load —
//! works end to end.

use whisker::css::{AlignItems, FlexDirection, FontWeight, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_asset::asset;
use whisker_image::{Image, ImageMode};

#[whisker::main]
pub fn app() -> Element {
    // Resolved once at render. On device this is the platform
    // `file://` path/URL; in pure-Rust tests / tooling (no native
    // base installed) it falls back to the logical path unchanged.
    let logo_src = asset!("images/logo.png");

    render! {
        view(style: css!(
            flex_grow: 1.0,
            background_color: Color::hex(0x101012),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: px(24),
        )) {
            text(
                style: css!(
                    color: Color::hex(0xF5F5F7),
                    font_size: px(20),
                    font_weight: FontWeight::Bold,
                    margin_bottom: px(24),
                ),
                value: "Bundled asset",
            )
            Image(
                style: css!(
                    width: px(160),
                    height: px(160),
                    border_radius: px(16),
                    background_color: Color::hex(0x222228),
                ),
                src: logo_src,
                mode: ImageMode::AspectFill,
            )
        }
    }
}
