//! Pure data types — `Podcast`, `ChartSection`, `Category`.
//!
//! No I/O, no UI primitives, no `whisker` dependency. Both the data
//! layer (deserialises iTunes Search responses into these) and the
//! UI layer (renders them) consume this crate — keeping it pure
//! lets either side evolve without dragging the other along.
//!
//! `serde::Deserialize` is derived here (and not in a DTO crate)
//! because the iTunes Search wire shape is close enough to the
//! domain that field renames + a `#[serde(rename = "...")]` carry
//! the mapping cleanly. If the shapes diverge in the future,
//! introduce a `dto::ItunesSearchResultRaw` in `podcast-data` and
//! map there — don't pollute this crate with API quirks.

use serde::Deserialize;

/// A single podcast (an iTunes "podcast" entity / a show, not an
/// episode). Carries enough fields to render every variant the UI
/// kit needs: hero featured cards, ranked grid cards, and the
/// future Show detail screen.
///
/// Field naming follows iTunes Search verbatim where harmless
/// (`collection_name`, `artist_name`, `artwork_url_600`) so the
/// data layer's `#[serde(rename)]` mapping stays mechanical.
#[derive(Debug, Clone, Deserialize)]
pub struct Podcast {
    /// iTunes collection id. Stable, used as the list key.
    #[serde(rename = "collectionId")]
    pub id: u64,

    /// Show title — what's shown under the artwork.
    #[serde(rename = "collectionName")]
    pub collection_name: String,

    /// Artist / publisher — the show creator credited line.
    #[serde(rename = "artistName")]
    pub artist_name: String,

    /// 600×600 cover. iTunes also exposes 30 / 60 / 100 — those are
    /// for thumbnails so small we don't need them here; the 600
    /// scales down acceptably at any of our UI sizes. If memory
    /// pressure shows up we can swap in `artwork_url_100` for the
    /// ranked grid, keeping 600 only for the featured hero.
    #[serde(rename = "artworkUrl600")]
    pub artwork_url_600: String,

    /// Primary genre — used as the category label above a featured
    /// card. `Option` because not every entry has one set.
    #[serde(rename = "primaryGenreName", default)]
    pub primary_genre_name: Option<String>,

    /// Number of episodes published.
    #[serde(rename = "trackCount", default)]
    pub track_count: u32,

    /// "Yes" if the show carries explicit content; iTunes also uses
    /// "cleaned" / "notExplicit". We expose the raw string and let
    /// the UI normalise — covers the case where the API adds a new
    /// value (we just don't show the E badge).
    #[serde(rename = "trackExplicitness", default)]
    pub track_explicitness: Option<String>,

    /// RSS feed URL — handed to the future episode-list / playback
    /// layer. iTunes returns this for almost every podcast result.
    #[serde(rename = "feedUrl", default)]
    pub feed_url: Option<String>,
}

impl Podcast {
    /// True if iTunes reports the show as `explicit`. The UI uses
    /// this to render the "E" badge on the artwork.
    pub fn is_explicit(&self) -> bool {
        self.track_explicitness
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("explicit"))
            .unwrap_or(false)
    }
}

/// A horizontally-scrolling row on the browse screen. Each section
/// has a title, a layout style (featured-hero vs ranked-grid), and
/// the podcasts that populate it.
///
/// The data layer assembles these from one or more iTunes Search
/// calls; the UI layer renders them through `ui_kit::section_row`
/// without needing to know how they were sourced.
#[derive(Debug, Clone)]
pub struct ChartSection {
    /// Section header copy ("New", "Top Shows", "New Shows", …).
    pub title: String,

    /// Layout variant.
    pub layout: SectionLayout,

    /// Podcasts shown in the row, in display order.
    pub items: Vec<Podcast>,
}

/// How a section renders its items.
///
/// Adding a new variant (e.g. a horizontal carousel of episode
/// rows, or a 2-row grid) is a UI-kit concern — the data layer
/// just picks the variant and hands over the items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionLayout {
    /// Large hero cards with a category label above and the artwork
    /// below the title (the "New" row in the design).
    Featured,
    /// Compact grid cards with a leading rank number under the
    /// artwork (the "Top Shows" / "New Shows" rows).
    Ranked,
}
