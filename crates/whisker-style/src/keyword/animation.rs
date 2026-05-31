//! Animation and transition keyword enums.
//!
//! References:
//! - <https://lynxjs.org/api/css/properties/animation-direction>
//! - <https://lynxjs.org/api/css/properties/animation-fill-mode>
//! - <https://lynxjs.org/api/css/properties/animation-iteration-count>
//! - <https://lynxjs.org/api/css/properties/animation-play-state>
//! - <https://lynxjs.org/api/css/properties/transition-property>

use core::fmt;

use crate::data_type::CssString;
use crate::to_css::{write_number, ToCss};

/// The `animation-direction` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AnimationDirection {
    /// `normal` — play forward each iteration. Default.
    Normal,
    /// `reverse` — play backward each iteration.
    Reverse,
    /// `alternate` — alternate forward, backward.
    Alternate,
    /// `alternate-reverse` — alternate backward, forward.
    AlternateReverse,
}

impl ToCss for AnimationDirection {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            AnimationDirection::Normal => "normal",
            AnimationDirection::Reverse => "reverse",
            AnimationDirection::Alternate => "alternate",
            AnimationDirection::AlternateReverse => "alternate-reverse",
        })
    }
}

/// The `animation-fill-mode` keyword. Controls how the animated
/// values apply before/after the active period.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AnimationFillMode {
    /// `none` — no fill outside the active period. Default.
    None,
    /// `forwards` — keep the final keyframe values after the
    /// animation completes.
    Forwards,
    /// `backwards` — apply the initial keyframe values during
    /// `animation-delay`.
    Backwards,
    /// `both` — `forwards` and `backwards` combined.
    Both,
}

impl ToCss for AnimationFillMode {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            AnimationFillMode::None => "none",
            AnimationFillMode::Forwards => "forwards",
            AnimationFillMode::Backwards => "backwards",
            AnimationFillMode::Both => "both",
        })
    }
}

/// The `animation-iteration-count` value: either `infinite` or a
/// non-negative number of iterations.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum AnimationIterationCount {
    /// `infinite` — repeat forever.
    Infinite,
    /// Explicit iteration count.
    Count(f32),
}

impl ToCss for AnimationIterationCount {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            AnimationIterationCount::Infinite => dest.write_str("infinite"),
            AnimationIterationCount::Count(n) => write_number(dest, *n),
        }
    }
}

/// The `animation-play-state` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AnimationPlayState {
    /// `running` — animation is playing. Default.
    Running,
    /// `paused` — animation is paused.
    Paused,
}

impl ToCss for AnimationPlayState {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            AnimationPlayState::Running => "running",
            AnimationPlayState::Paused => "paused",
        })
    }
}

/// The `transition-property` value: either the `all`/`none` keyword
/// or one specific property name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransitionPropertyKind {
    /// `all` — transition every animatable property.
    All,
    /// `none` — disable transitions on this element.
    None,
    /// A specific CSS property name (`opacity`, `transform`, …).
    Name(CssString),
}

impl TransitionPropertyKind {
    /// Build [`TransitionPropertyKind::Name`] from anything string-like.
    pub fn name(s: impl Into<String>) -> Self {
        Self::Name(CssString::new(s))
    }
}

impl ToCss for TransitionPropertyKind {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            TransitionPropertyKind::All => dest.write_str("all"),
            TransitionPropertyKind::None => dest.write_str("none"),
            // Property names are CSS identifiers, written bare (no
            // surrounding quotes).
            TransitionPropertyKind::Name(n) => dest.write_str(n.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_direction_all() {
        let cases = [
            (AnimationDirection::Normal, "normal"),
            (AnimationDirection::Reverse, "reverse"),
            (AnimationDirection::Alternate, "alternate"),
            (AnimationDirection::AlternateReverse, "alternate-reverse"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn animation_fill_mode_all() {
        let cases = [
            (AnimationFillMode::None, "none"),
            (AnimationFillMode::Forwards, "forwards"),
            (AnimationFillMode::Backwards, "backwards"),
            (AnimationFillMode::Both, "both"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn iteration_count_keyword() {
        assert_eq!(
            AnimationIterationCount::Infinite.to_css_string(),
            "infinite"
        );
    }

    #[test]
    fn iteration_count_number() {
        assert_eq!(AnimationIterationCount::Count(2.0).to_css_string(), "2");
        assert_eq!(AnimationIterationCount::Count(0.5).to_css_string(), "0.5");
    }

    #[test]
    fn play_state_all() {
        assert_eq!(AnimationPlayState::Running.to_css_string(), "running");
        assert_eq!(AnimationPlayState::Paused.to_css_string(), "paused");
    }

    #[test]
    fn transition_property_all() {
        assert_eq!(TransitionPropertyKind::All.to_css_string(), "all");
        assert_eq!(TransitionPropertyKind::None.to_css_string(), "none");
        assert_eq!(
            TransitionPropertyKind::name("opacity").to_css_string(),
            "opacity"
        );
    }
}
