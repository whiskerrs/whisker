//! Keyword enums used by property values.
//!
//! Each enum covers exactly the keywords Lynx accepts for a given
//! property family. Values explicitly rejected by Lynx (e.g.
//! `position: static`, `overflow: scroll`, `flex-direction:
//! vertical`) are absent from the enums so writing them is a
//! compile error rather than a runtime no-op.

mod animation;
mod background;
mod border;
mod flex;
mod grid;
mod layout;
mod linear;
mod text;
mod transform;
mod typography;

pub use animation::{
    AnimationDirection, AnimationFillMode, AnimationIterationCount, AnimationPlayState,
    TransitionPropertyKind,
};
pub use background::{
    BackgroundAttachment, BackgroundClip, BackgroundOrigin, BackgroundRepeat, BackgroundSize,
};
pub use border::BorderStyle;
pub use flex::{AlignContent, AlignItems, AlignSelf, FlexDirection, FlexWrap, JustifyContent};
pub use grid::GridAutoFlow;
pub use layout::{BoxSizing, Display, Overflow, PointerEvents, PositionKind, Visibility};
pub use linear::{LinearCrossGravity, LinearGravity, LinearLayoutGravity, LinearOrientation};
pub use text::{
    Direction, TextAlign, TextDecorationLine, TextDecorationStyle, TextOverflow, TextTransform,
    VerticalAlign, WhiteSpace, WordBreak, WordWrap,
};
pub use transform::{BackfaceVisibility, TransformBox, TransformStyle};
pub use typography::{Cursor, FontStyle, FontVariant, FontWeight};
