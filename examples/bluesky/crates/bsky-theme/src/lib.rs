//! Design tokens for the Bluesky example — typed consts the UI drops straight
//! into `css!(...)`. Single source for color / type scale / spacing so the look
//! changes in one place. Dark theme, Bluesky-blue accent.

use whisker_css::{Color, Length};

// ── Surfaces ────────────────────────────────────────────────────────────
/// Root background.
pub const BG: Color = Color::hex(0x000000);
/// Card / row separators.
pub const BORDER: Color = Color::hex(0x222831);
/// Pressed / subtle fill.
pub const SURFACE: Color = Color::hex(0x16191F);

// ── Text ────────────────────────────────────────────────────────────────
pub const TEXT_PRIMARY: Color = Color::hex(0xFFFFFF);
pub const TEXT_SECONDARY: Color = Color::hex(0x8B98A5);

// ── Accent (Bluesky blue) ────────────────────────────────────────────────
pub const ACCENT: Color = Color::hex(0x1083FE);
pub const ON_ACCENT: Color = Color::hex(0xFFFFFF);

// ── Type scale ────────────────────────────────────────────────────────────
pub const T_TITLE: Length = Length::Px(28.0);
pub const T_NAME: Length = Length::Px(15.0);
pub const T_HANDLE: Length = Length::Px(14.0);
pub const T_BODY: Length = Length::Px(15.0);
pub const T_META: Length = Length::Px(13.0);

// ── Spacing ───────────────────────────────────────────────────────────────
pub const GUTTER: Length = Length::Px(16.0);
pub const ROW_GAP: Length = Length::Px(12.0);

// ── Avatar ────────────────────────────────────────────────────────────────
pub const AVATAR_SIDE: Length = Length::Px(44.0);
/// Avatar corner radius in px, for whisker-image's numeric prop.
pub const AVATAR_RADIUS_PX: f64 = 22.0;
