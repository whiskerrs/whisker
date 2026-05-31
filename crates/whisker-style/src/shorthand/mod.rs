//! Compound shorthand builders for properties whose CSS shorthand
//! combines multiple longhands.
//!
//! Each submodule provides one Rust type (`Border`, `Background`,
//! …) plus a builder method on [`Style`](crate::Style) that expands
//! the compound into its longhand entries.

pub mod animation;
pub mod background;
pub mod border;
pub mod flex;
pub mod padding_margin;
pub mod transform;
pub mod transition;

pub use animation::Animation;
pub use background::{Background, BackgroundLayer};
pub use border::Border;
pub use flex::Flex;
pub use padding_margin::{Margin, MarginValue, Padding};
pub use transform::{Transform, TransformFn};
pub use transition::Transition;
