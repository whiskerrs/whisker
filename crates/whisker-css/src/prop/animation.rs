//! Animation longhand properties.

use crate::css::Css;
use crate::data_type::Time;
use crate::data_type_ext::EasingFunction;
use crate::keyword::{
    AnimationDirection, AnimationFillMode, AnimationIterationCount, AnimationPlayState,
};

impl Css {
    /// Sets `animation-name` — references a `@keyframes` block.
    /// <https://lynxjs.org/api/css/properties/animation-name>
    pub fn animation_name(self, name: impl Into<String>) -> Self {
        // Names are CSS identifiers, written bare.
        self.push_raw("animation-name", name)
    }

    /// Sets `animation-duration`.
    /// <https://lynxjs.org/api/css/properties/animation-duration>
    pub fn animation_duration(self, v: Time) -> Self {
        self.push("animation-duration", v)
    }

    /// Sets `animation-timing-function`.
    /// <https://lynxjs.org/api/css/properties/animation-timing-function>
    pub fn animation_timing_function(self, v: EasingFunction) -> Self {
        self.push("animation-timing-function", v)
    }

    /// Sets `animation-delay`.
    /// <https://lynxjs.org/api/css/properties/animation-delay>
    pub fn animation_delay(self, v: Time) -> Self {
        self.push("animation-delay", v)
    }

    /// Sets `animation-iteration-count`.
    /// <https://lynxjs.org/api/css/properties/animation-iteration-count>
    pub fn animation_iteration_count(self, v: AnimationIterationCount) -> Self {
        self.push("animation-iteration-count", v)
    }

    /// Sets `animation-direction`.
    /// <https://lynxjs.org/api/css/properties/animation-direction>
    pub fn animation_direction(self, v: AnimationDirection) -> Self {
        self.push("animation-direction", v)
    }

    /// Sets `animation-fill-mode`.
    /// <https://lynxjs.org/api/css/properties/animation-fill-mode>
    pub fn animation_fill_mode(self, v: AnimationFillMode) -> Self {
        self.push("animation-fill-mode", v)
    }

    /// Sets `animation-play-state`.
    /// <https://lynxjs.org/api/css/properties/animation-play-state>
    pub fn animation_play_state(self, v: AnimationPlayState) -> Self {
        self.push("animation-play-state", v)
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::data_type_ext::EasingFunction;
    use crate::ext::*;
    use crate::keyword::*;

    #[test]
    fn animation_full_set() {
        let s = Css::new()
            .animation_name("spin")
            .animation_duration(2.s())
            .animation_timing_function(EasingFunction::Linear)
            .animation_delay(100.ms())
            .animation_iteration_count(AnimationIterationCount::Infinite)
            .animation_direction(AnimationDirection::Alternate)
            .animation_fill_mode(AnimationFillMode::Forwards)
            .animation_play_state(AnimationPlayState::Running);
        assert_eq!(
            s.to_string(),
            "animation-name: spin; animation-duration: 2s; animation-timing-function: linear; animation-delay: 100ms; animation-iteration-count: infinite; animation-direction: alternate; animation-fill-mode: forwards; animation-play-state: running;"
        );
    }

    #[test]
    fn iteration_count_explicit() {
        let s = Css::new().animation_iteration_count(AnimationIterationCount::Count(3.0));
        assert_eq!(s.to_string(), "animation-iteration-count: 3;");
    }
}
