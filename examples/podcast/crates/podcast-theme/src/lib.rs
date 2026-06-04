//! Design tokens for the podcast example.
//!
//! Typed `Color` / `Length` constants ready to drop directly into
//! `css!(...)` builder calls — `font_size: theme::T_HERO,
//! color: theme::TEXT_PRIMARY`. The crate depends on `whisker-css`
//! only (no `whisker` umbrella) so a redesign touches this crate
//! and nothing else, and the lower-level layers (domain, data)
//! keep building even when the UI is gutted.
//!
//! Tokens follow a flat-namespace, name-by-role convention (not
//! name-by-value): a future palette change updates the constant
//! without forcing a rename through every consumer. The exception
//! is the type scale, where the `T_*` names encode the size class
//! so a consumer reading `font_size: T_HERO` knows what they're
//! getting without jumping back to this file.

use whisker_css::{Color, Length};

// ----- Surfaces ------------------------------------------------------------

/// Root page background. Near-black for the dark-theme browse screen.
pub const BG: Color = Color::hex(0x000000);

/// Elevated surface (cards, the mini player). Slightly lifted from
/// `BG` so the card edge reads without a border.
pub const SURFACE: Color = Color::hex(0x1C1C1E);

/// Bottom mini-player backdrop — translucent so the content behind
/// stays visible while the player floats over it.
pub const MINI_PLAYER_BG: Color = Color::rgba(40, 40, 42, 0.92);

// ----- Text ----------------------------------------------------------------

/// Primary text — section titles, card titles, hero copy.
pub const TEXT_PRIMARY: Color = Color::hex(0xFFFFFF);

/// Muted text — meta labels, subtitles, secondary lines.
pub const TEXT_SECONDARY: Color = Color::rgba(235, 235, 245, 0.6);

/// Even-more-muted — tertiary captions, ranking numbers when faded.
pub const TEXT_TERTIARY: Color = Color::rgba(235, 235, 245, 0.3);

// ----- Accent -------------------------------------------------------------

/// Interactive / brand accent — top-bar buttons and chevrons.
pub const ACCENT: Color = Color::hex(0xA78BFA);

// ----- Type scale ---------------------------------------------------------

/// Page hero title ("New", "Top Shows" section labels).
pub const T_HERO: Length = Length::Px(28.0);
/// Section title chevron-row.
pub const T_SECTION: Length = Length::Px(22.0);
/// Featured card's two-line title.
pub const T_FEATURED_TITLE: Length = Length::Px(20.0);
/// Standard card title under artwork.
pub const T_CARD_TITLE: Length = Length::Px(13.0);
/// Card subtitle (artist / author).
pub const T_CARD_SUBTITLE: Length = Length::Px(13.0);
/// Category label above a featured card ("NEW SEASON" etc).
pub const T_CATEGORY: Length = Length::Px(11.0);
/// Top-nav title.
pub const T_NAV_TITLE: Length = Length::Px(15.0);

// ----- Spacing ------------------------------------------------------------

/// Horizontal page gutter — left/right edge insets.
pub const GUTTER: Length = Length::Px(16.0);

/// Vertical gap between major sections (between "New" and "Top Shows" etc).
pub const SECTION_GAP: Length = Length::Px(32.0);

/// Gap between a section header and its row of cards.
pub const HEADER_GAP: Length = Length::Px(12.0);

/// Gap between adjacent cards in a horizontal row.
pub const CARD_GAP: Length = Length::Px(12.0);

// ----- Card sizing --------------------------------------------------------

/// Featured card width (the large hero items in "New").
pub const FEATURED_CARD_WIDTH: Length = Length::Px(300.0);

/// Standard ranked card artwork side length.
pub const RANKED_CARD_SIDE: Length = Length::Px(144.0);

/// Corner radius for all artwork. Apple-style soft-rounded. The
/// `Length` form drops into `css!(border_radius: …)`; the `f64`
/// form is what `whisker-image:Image`'s `corner_radius` prop wants
/// (Lynx's CSS cascade doesn't reach custom UI elements' image
/// bitmap layer on Android, so the typed prop is the consistent
/// path — see `whisker_image::image`'s docs).
pub const ARTWORK_RADIUS: Length = Length::Px(8.0);
pub const ARTWORK_RADIUS_PX: f64 = 8.0;

// ----- Top nav ------------------------------------------------------------

/// Top-nav bar height (status-bar inset not included — host handles
/// safe-area separately).
pub const NAV_HEIGHT: Length = Length::Px(44.0);

// ----- Mini player --------------------------------------------------------

/// Mini-player floating bar height.
pub const MINI_PLAYER_HEIGHT: Length = Length::Px(56.0);

/// Mini player bottom inset from the bottom-of-page anchor (matches
/// the home-indicator clearance on iOS).
pub const MINI_PLAYER_BOTTOM: Length = Length::Px(16.0);
