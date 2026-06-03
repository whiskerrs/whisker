//! Reusable atomic widgets for the podcast example.
//!
//! Each module is one component, named after what it renders. Cross-
//! component shared concerns (e.g. the safe-area inset, the page
//! gutter) live in [`podcast_theme`] tokens — `ui-kit` itself is
//! stateless and dep-light: it pulls `whisker` for the render
//! primitives, `podcast-theme` for tokens, `podcast-domain` for
//! the value types it renders. Nothing else.
//!
//! Style strings are inline `format!("…{TOKEN}…")` against
//! `podcast_theme` constants. We deliberately don't reach for the
//! typed `css::ext` builder yet — keeping styles textual lets a
//! reader skim the module and see the rendered shape without
//! cross-referencing a builder chain. When a sample component
//! ships with a real `Style` API (later), this is the boundary
//! where it'd swap in.
//!
//! `#[component]` hides the snake_case fn behind a private inner
//! module and `pub use`-aliases it to the PascalCase name (plus a
//! `XxxProps` typed builder). That's what we re-export here.

mod featured_card;
mod horizontal_row;
mod mini_player;
mod ranked_card;
mod section_header;
mod top_nav;

pub use featured_card::{FeaturedCard, FeaturedCardProps};
pub use horizontal_row::{HorizontalRow, HorizontalRowProps};
pub use mini_player::{MiniPlayer, MiniPlayerProps};
pub use ranked_card::{RankedCard, RankedCardProps};
pub use section_header::{SectionHeader, SectionHeaderProps};
pub use top_nav::{TopNav, TopNavProps};
