//! Design tokens for the podcast example.
//!
//! Pure `const &str` so they can be dropped directly into Whisker
//! `style:` attribute strings via `format!("color: {TEXT_PRIMARY};")`.
//! No `whisker` dependency on purpose — a redesign touches this
//! crate and nothing else, and the lower-level layers (domain,
//! data) keep building even when the UI is gutted.
//!
//! Tokens follow a flat-namespace, name-by-role convention (not
//! name-by-value): a future palette change updates the constant
//! without forcing a rename through every consumer. The exception
//! is the type scale, where the `T_*` names encode the size class
//! so a consumer reading `font-size: {T_HERO}` knows what they're
//! getting without jumping back to this file.

// ----- Surfaces ------------------------------------------------------------

/// Root page background. Near-black for the dark-theme browse screen.
pub const BG: &str = "#000000";

/// Elevated surface (cards, the mini player). Slightly lifted from
/// `BG` so the card edge reads without a border.
pub const SURFACE: &str = "#1c1c1e";

/// Bottom mini-player backdrop — translucent so the content behind
/// stays visible while the player floats over it.
pub const MINI_PLAYER_BG: &str = "rgba(40, 40, 42, 0.92)";

// ----- Text ----------------------------------------------------------------

/// Primary text — section titles, card titles, hero copy.
pub const TEXT_PRIMARY: &str = "#ffffff";

/// Muted text — meta labels, subtitles, secondary lines.
pub const TEXT_SECONDARY: &str = "rgba(235, 235, 245, 0.6)";

/// Even-more-muted — tertiary captions, ranking numbers when faded.
pub const TEXT_TERTIARY: &str = "rgba(235, 235, 245, 0.3)";

// ----- Accent -------------------------------------------------------------

/// Interactive / brand accent — top-bar buttons and chevrons.
pub const ACCENT: &str = "#a78bfa";

// ----- Type scale ---------------------------------------------------------

/// Page hero title ("New", "Top Shows" section labels).
pub const T_HERO: &str = "28px";
/// Section title chevron-row.
pub const T_SECTION: &str = "22px";
/// Featured card's two-line title.
pub const T_FEATURED_TITLE: &str = "20px";
/// Standard card title under artwork.
pub const T_CARD_TITLE: &str = "13px";
/// Card subtitle (artist / author).
pub const T_CARD_SUBTITLE: &str = "13px";
/// Category label above a featured card ("NEW SEASON" etc).
pub const T_CATEGORY: &str = "11px";
/// Top-nav title.
pub const T_NAV_TITLE: &str = "15px";

// ----- Spacing ------------------------------------------------------------

/// Horizontal page gutter — left/right edge insets.
pub const GUTTER: &str = "16px";

/// Vertical gap between major sections (between "New" and "Top Shows" etc).
pub const SECTION_GAP: &str = "32px";

/// Gap between a section header and its row of cards.
pub const HEADER_GAP: &str = "12px";

/// Gap between adjacent cards in a horizontal row.
pub const CARD_GAP: &str = "12px";

// ----- Card sizing --------------------------------------------------------

/// Featured card width (the large hero items in "New").
pub const FEATURED_CARD_WIDTH: &str = "300px";

/// Standard ranked card artwork side length.
pub const RANKED_CARD_SIDE: &str = "144px";

/// Corner radius for all artwork. Apple-style soft-rounded. Two
/// representations: the CSS `border-radius` string for plain `view`
/// rounding, and the numeric value for the `whisker-image:Image`
/// element's `corner_radius` prop (Lynx's CSS cascade doesn't reach
/// custom UI elements' image bitmap layer on Android, so the typed
/// prop is the consistent path — see `whisker_image::image`'s docs).
pub const ARTWORK_RADIUS: &str = "8px";
pub const ARTWORK_RADIUS_PX: f64 = 8.0;

// ----- Top nav ------------------------------------------------------------

/// Top-nav bar height (status-bar inset not included — host handles
/// safe-area separately).
pub const NAV_HEIGHT: &str = "44px";

// ----- Mini player --------------------------------------------------------

/// Mini-player floating bar height.
pub const MINI_PLAYER_HEIGHT: &str = "56px";

/// Mini player bottom inset from the bottom-of-page anchor (matches
/// the home-indicator clearance on iOS).
pub const MINI_PLAYER_BOTTOM: &str = "16px";
