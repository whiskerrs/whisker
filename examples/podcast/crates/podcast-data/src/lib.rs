//! Data layer for the podcast example.
//!
//! Two layers, in order of stability:
//!
//! 1. [`itunes`] — thin HTTP client wrapping the iTunes Search API.
//!    Knows nothing about the rest of the app; if iTunes' wire
//!    format changes, this module is the only thing that breaks.
//! 2. [`repository`] — domain-typed facade. Exposes
//!    [`fetch_browse_screen()`] which returns `Vec<ChartSection>`
//!    populated by combining multiple iTunes search queries. The
//!    UI layer calls this and ignores the API quirks.
//!
//! Errors are surfaced as `Result<_, FetchError>` so callers can
//! pattern-match the failure mode (network vs parse) and render
//! distinct UI states. The iTunes API only returns 200s with empty
//! result arrays for unknown terms, so there's no "not found"
//! variant — empty results aren't an error here.

mod itunes;
mod repository;

pub use itunes::{
    FetchError, SearchQuery, fetch_episodes_blocking as fetch_episodes, search_blocking as search,
};
pub use repository::fetch_browse_screen;
