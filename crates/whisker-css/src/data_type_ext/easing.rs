//! `<easing-function>` — describes how a value progresses between
//! two points in a transition or animation.
//!
//! Lynx does not document `<easing-function>` as a standalone data
//! type; the keywords and `cubic-bezier()` / `steps()` functions are
//! described inline on the `transition-timing-function` and
//! `animation-timing-function` property pages. The grammar mirrors
//! [CSS Easing Functions Level 1](https://www.w3.org/TR/css-easing-1/).

use core::fmt;

use crate::to_css::{write_number, ToCss};

/// A CSS `<easing-function>` value.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum EasingFunction {
    /// `linear` — constant rate.
    Linear,
    /// `ease` — slow start, then fast, then end slowly. Default for
    /// `transition-timing-function`.
    Ease,
    /// `ease-in` — slow at the start.
    EaseIn,
    /// `ease-out` — slow at the end.
    EaseOut,
    /// `ease-in-out` — slow at both ends.
    EaseInOut,
    /// `step-start` — equivalent to `steps(1, jump-start)`.
    StepStart,
    /// `step-end` — equivalent to `steps(1, jump-end)`.
    StepEnd,
    /// `cubic-bezier(x1, y1, x2, y2)` — custom Bézier curve. The
    /// `x` coordinates must be in `0.0..=1.0`; Lynx will clamp
    /// values outside that range.
    CubicBezier(f32, f32, f32, f32),
    /// `steps(<n>, <position>)` — discrete jumps.
    Steps(u32, StepPosition),
}

/// Position parameter of the `steps()` easing function.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum StepPosition {
    /// `jump-start` — jump at the start of each interval.
    JumpStart,
    /// `jump-end` — jump at the end of each interval. Default.
    JumpEnd,
    /// `jump-none` — no jump at either end.
    JumpNone,
    /// `jump-both` — jumps at both the start and the end.
    JumpBoth,
    /// Legacy alias for [`StepPosition::JumpStart`].
    Start,
    /// Legacy alias for [`StepPosition::JumpEnd`].
    End,
}

impl ToCss for StepPosition {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            StepPosition::JumpStart => "jump-start",
            StepPosition::JumpEnd => "jump-end",
            StepPosition::JumpNone => "jump-none",
            StepPosition::JumpBoth => "jump-both",
            StepPosition::Start => "start",
            StepPosition::End => "end",
        })
    }
}

impl ToCss for EasingFunction {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            EasingFunction::Linear => dest.write_str("linear"),
            EasingFunction::Ease => dest.write_str("ease"),
            EasingFunction::EaseIn => dest.write_str("ease-in"),
            EasingFunction::EaseOut => dest.write_str("ease-out"),
            EasingFunction::EaseInOut => dest.write_str("ease-in-out"),
            EasingFunction::StepStart => dest.write_str("step-start"),
            EasingFunction::StepEnd => dest.write_str("step-end"),
            EasingFunction::CubicBezier(a, b, c, d) => {
                dest.write_str("cubic-bezier(")?;
                write_number(dest, *a)?;
                dest.write_str(", ")?;
                write_number(dest, *b)?;
                dest.write_str(", ")?;
                write_number(dest, *c)?;
                dest.write_str(", ")?;
                write_number(dest, *d)?;
                dest.write_char(')')
            }
            EasingFunction::Steps(n, pos) => {
                write!(dest, "steps({n}, ")?;
                pos.to_css(dest)?;
                dest.write_char(')')
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_easings() {
        assert_eq!(EasingFunction::Linear.to_css_string(), "linear");
        assert_eq!(EasingFunction::Ease.to_css_string(), "ease");
        assert_eq!(EasingFunction::EaseIn.to_css_string(), "ease-in");
        assert_eq!(EasingFunction::EaseOut.to_css_string(), "ease-out");
        assert_eq!(EasingFunction::EaseInOut.to_css_string(), "ease-in-out");
        assert_eq!(EasingFunction::StepStart.to_css_string(), "step-start");
        assert_eq!(EasingFunction::StepEnd.to_css_string(), "step-end");
    }

    #[test]
    fn cubic_bezier() {
        assert_eq!(
            EasingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0).to_css_string(),
            "cubic-bezier(0.25, 0.1, 0.25, 1)"
        );
    }

    #[test]
    fn steps_all_positions() {
        let cases = [
            (StepPosition::JumpStart, "jump-start"),
            (StepPosition::JumpEnd, "jump-end"),
            (StepPosition::JumpNone, "jump-none"),
            (StepPosition::JumpBoth, "jump-both"),
            (StepPosition::Start, "start"),
            (StepPosition::End, "end"),
        ];
        for (pos, expected) in cases {
            let s = EasingFunction::Steps(4, pos).to_css_string();
            assert_eq!(s, format!("steps(4, {expected})"));
        }
    }
}
