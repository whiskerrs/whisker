//! Animation longhand properties.

use crate::css::Css;
use crate::data_type::Time;
use crate::data_type_ext::EasingFunction;
use crate::keyword::{
    AnimationDirection, AnimationFillMode, AnimationIterationCount, AnimationPlayState,
};
use crate::shorthand::{Animation, AnimationTarget};
use crate::style_value::{to_animation_value, to_motion_easing, to_motion_time};
use crate::to_css::ToCss;

impl Css {
    /// Sets `animation-name` — references a `@keyframes` block.
    /// <https://lynxjs.org/api/css/properties/animation-name>
    pub fn animation_name(self, target: impl Into<AnimationTarget>) -> Self {
        match target.into() {
            AnimationTarget::Name(name) => {
                let semantic = (name != "none").then_some(name.clone());
                self.push_semantic(
                    crate::StyleProperty::AnimationName,
                    whisker_style::StyleValue::AnimationNames(vec![semantic]),
                    name,
                )
            }
            AnimationTarget::Keyframes(keyframes) => {
                let animation = Animation::new(keyframes);
                let name = animation.name.clone();
                self.push_semantic(
                    crate::StyleProperty::AnimationName,
                    whisker_style::StyleValue::Animations(vec![to_animation_value(&animation)]),
                    name,
                )
            }
        }
    }

    /// Sets `animation-duration`.
    /// <https://lynxjs.org/api/css/properties/animation-duration>
    pub fn animation_duration(self, v: Time) -> Self {
        self.push_semantic(
            crate::StyleProperty::AnimationDuration,
            whisker_style::StyleValue::AnimationDurations(vec![to_motion_time(v)]),
            v.to_css_string(),
        )
    }

    /// Sets `animation-timing-function`.
    /// <https://lynxjs.org/api/css/properties/animation-timing-function>
    pub fn animation_timing_function(self, v: EasingFunction) -> Self {
        self.push_semantic(
            crate::StyleProperty::AnimationTimingFunction,
            whisker_style::StyleValue::AnimationEasings(vec![to_motion_easing(v)]),
            v.to_css_string(),
        )
    }

    /// Sets `animation-delay`.
    /// <https://lynxjs.org/api/css/properties/animation-delay>
    pub fn animation_delay(self, v: Time) -> Self {
        self.push_semantic(
            crate::StyleProperty::AnimationDelay,
            whisker_style::StyleValue::AnimationDelays(vec![to_motion_time(v)]),
            v.to_css_string(),
        )
    }

    /// Sets `animation-iteration-count`.
    /// <https://lynxjs.org/api/css/properties/animation-iteration-count>
    pub fn animation_iteration_count(self, v: AnimationIterationCount) -> Self {
        self.push_typed(crate::StyleProperty::AnimationIterationCount, v)
    }

    /// Sets `animation-direction`.
    /// <https://lynxjs.org/api/css/properties/animation-direction>
    pub fn animation_direction(self, v: AnimationDirection) -> Self {
        self.push_typed(crate::StyleProperty::AnimationDirection, v)
    }

    /// Sets `animation-fill-mode`.
    /// <https://lynxjs.org/api/css/properties/animation-fill-mode>
    pub fn animation_fill_mode(self, v: AnimationFillMode) -> Self {
        self.push_typed(crate::StyleProperty::AnimationFillMode, v)
    }

    /// Sets `animation-play-state`.
    /// <https://lynxjs.org/api/css/properties/animation-play-state>
    pub fn animation_play_state(self, v: AnimationPlayState) -> Self {
        self.push_typed(crate::StyleProperty::AnimationPlayState, v)
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
