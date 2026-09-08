//! Semantic tokens for Whisker Chat's warm, editorial interface.

pub mod color {
    pub const CANVAS: u32 = 0xf7f5f0;
    pub const PAPER: u32 = 0xffffff;
    pub const INK: u32 = 0x272722;
    pub const MUTED: u32 = 0x75746b;
    pub const BORDER: u32 = 0xe4e1d8;
    pub const TINT: u32 = 0xeeece5;
    pub const ACCENT: u32 = 0xb5442c;
    pub const ACCENT_SOFT: u32 = 0xf8e8df;
    pub const ON_ACCENT: u32 = 0xffffff;
    pub const CODE: u32 = 0x292c2b;
    pub const ON_CODE: u32 = 0xeaece6;
}

pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
    pub const HERO: f32 = 48.0;
}

pub mod radius {
    pub const CONTROL: f32 = 12.0;
    pub const CARD: f32 = 20.0;
}

pub mod size {
    pub const TOUCH: f32 = 44.0;
    pub const SIDEBAR: f32 = 272.0;
    pub const READING: f32 = 760.0;
    pub const FORM: f32 = 520.0;
    pub const CAPTION: f32 = 12.0;
    pub const LABEL: f32 = 14.0;
    pub const BODY: f32 = 16.0;
    pub const TITLE: f32 = 24.0;
    pub const DISPLAY: f32 = 40.0;
}
