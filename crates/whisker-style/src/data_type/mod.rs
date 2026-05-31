//! The 11 data types Lynx documents under
//! <https://lynxjs.org/api/css/data-type.html>.
//!
//! Each submodule maps one Lynx data type to a Rust type with a
//! [`ToCss`](crate::ToCss) implementation. The set is intentionally
//! narrow: Lynx does not expose `<integer>`, `<image>`, `<position>`,
//! or `<easing-function>` as standalone data types; the equivalents
//! live in [`crate::data_type_ext`].

mod angle;
mod color;
mod gradient;
mod length;
mod length_percentage;
mod number;
mod percentage;
mod sizing;
mod string;
mod time;

pub use angle::Angle;
pub use color::{Color, NamedColor};
pub use gradient::{ColorStop, Gradient, LinearDirection, RadialShape, StopPosition};
pub use length::Length;
pub use length_percentage::{CalcExpr, LengthPercentage};
pub use number::Number;
pub use percentage::Percentage;
pub use sizing::{FitContent, MaxContent};
pub use string::CssString;
pub use time::Time;
