//! Transition longhand properties.

use crate::css::Css;
use crate::data_type::Time;
use crate::data_type_ext::EasingFunction;
use crate::keyword::TransitionPropertyKind;

impl Css {
    /// Sets `transition-property` — the property to transition.
    /// <https://lynxjs.org/api/css/properties/transition-property>
    pub fn transition_property(self, v: TransitionPropertyKind) -> Self {
        self.push("transition-property", v)
    }

    /// Sets `transition-duration`.
    /// <https://lynxjs.org/api/css/properties/transition-duration>
    pub fn transition_duration(self, v: Time) -> Self {
        self.push("transition-duration", v)
    }

    /// Sets `transition-timing-function`.
    /// <https://lynxjs.org/api/css/properties/transition-timing-function>
    pub fn transition_timing_function(self, v: EasingFunction) -> Self {
        self.push("transition-timing-function", v)
    }

    /// Sets `transition-delay`. Negative delays cause the transition
    /// to begin partway through its progression.
    /// <https://lynxjs.org/api/css/properties/transition-delay>
    pub fn transition_delay(self, v: Time) -> Self {
        self.push("transition-delay", v)
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::data_type_ext::EasingFunction;
    use crate::ext::*;
    use crate::keyword::TransitionPropertyKind;

    #[test]
    fn transition_set() {
        let s = Css::new()
            .transition_property(TransitionPropertyKind::name("opacity"))
            .transition_duration(300.ms())
            .transition_timing_function(EasingFunction::EaseInOut)
            .transition_delay(100.ms());
        assert_eq!(
            s.to_string(),
            "transition-property: opacity; transition-duration: 300ms; transition-timing-function: ease-in-out; transition-delay: 100ms;"
        );
    }

    #[test]
    fn transition_all_keyword() {
        let s = Css::new().transition_property(TransitionPropertyKind::All);
        assert_eq!(s.to_string(), "transition-property: all;");
    }
}
