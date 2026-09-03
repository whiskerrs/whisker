//! Keyword enums used by property values.
//!
//! Each enum covers the subset implemented by Whisker's typed style engine.
//! Unsupported values are absent so they fail at compile time rather than
//! becoming runtime no-ops.

mod animation;
mod background;
mod border;
mod effects;
mod flex;
mod grid;
mod layout;
mod text;
mod transform;
mod typography;

pub use animation::{
    AnimationDirection, AnimationFillMode, AnimationIterationCount, AnimationPlayState,
    TransitionPropertyKind,
};
pub use background::{
    BackgroundAttachment, BackgroundClip, BackgroundOrigin, BackgroundRepeat, BackgroundSize,
    BackgroundSizeAxis,
};
pub use border::BorderStyle;
pub use effects::ImageRendering;
pub use flex::{AlignContent, AlignItems, AlignSelf, FlexDirection, FlexWrap, JustifyContent};
pub use grid::GridAutoFlow;
pub use layout::{
    BoxSizing, Clear, Display, Float, Overflow, PointerEvents, PositionKind, Visibility,
};
pub use text::{
    Direction, TextAlign, TextDecorationLine, TextDecorationStyle, TextOverflow, TextTransform,
    VerticalAlign, WhiteSpace, WordBreak, WordWrap,
};
pub use transform::{BackfaceVisibility, TransformBox, TransformStyle};
pub use typography::{Cursor, FontStyle, FontVariant, FontWeight};
