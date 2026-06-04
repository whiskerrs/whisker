//! Podcast browser — production-layered Whisker example.
//!
//! ## Architecture
//!
//! Composition root only. Real work lives in the sub-crates:
//!
//! ```text
//! podcast-theme               ← design tokens (colors, type scale, spacing)
//!                                no deps; pure consts
//! podcast-domain              ← Podcast / ChartSection value types
//!                                no deps; pure types + serde
//! podcast-data                ← iTunes Search API client + repositories
//!                                depends on: domain, ureq, serde
//! podcast-ui-kit              ← reusable atomic widgets
//!                                depends on: whisker, theme, domain
//! podcast-feature-browse      ← Browse screen, the only screen so far
//!                                depends on: whisker, theme, domain,
//!                                            data, ui-kit
//! ```
//!
//! The top-level crate (this one) only does the `#[whisker::main]`
//! ceremony and renders `BrowseScreen`. Adding a new screen later
//! (Now Playing, Library) means a new `podcast-feature-*` crate
//! and a route entry here — no churn in the lower layers.
//!
//! ### Why this many sub-crates for a sample?
//!
//! The example doubles as a layering reference for real Whisker
//! apps. The split lines mirror what a production team would draw:
//! design tokens stay independent so a redesign doesn't touch
//! business logic; domain types stay independent so they can be
//! consumed by both data layer (deserialising API responses into
//! them) and UI layer (rendering them) without cyclic deps; data
//! layer hides the iTunes API quirks behind a domain-typed
//! repository facade so UI doesn't see DTOs; UI kit owns the look
//! while features compose the kit into screens.

use podcast_feature_browse::{BrowseScreen, BrowseScreenProps};
use whisker::css::{Display, FlexDirection};
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[whisker::main]
fn app() -> Element {
    render! {
        // `page` needs explicit dimensions + background — without
        // `width: 100vw; height: 100vh;` it collapses to 0 px on
        // iOS and the host's clear color shows through. The bg
        // sits on the page so the system-level safe-area paint
        // matches the dark theme even before the first frame
        // draws.
        page(style: css!(
            width: vw(100),
            height: vh(100),
            background_color: podcast_theme::BG,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
            browse_screen()
        }
    }
}
