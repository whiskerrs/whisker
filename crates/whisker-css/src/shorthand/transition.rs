//! `transition` shorthand — bundles the four transition longhands
//! into one declaration. Multiple transitions are comma-separated.

use core::fmt;

use crate::css::Css;
use crate::data_type::Time;
use crate::data_type_ext::EasingFunction;
use crate::keyword::TransitionPropertyKind;
use crate::style_value::to_transition_value;
use crate::to_css::ToCss;

/// One transition layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    /// Which property to transition.
    pub property: TransitionPropertyKind,
    /// Duration of the transition.
    pub duration: Option<Time>,
    /// Timing function.
    pub timing: Option<EasingFunction>,
    /// Delay before the transition starts.
    pub delay: Option<Time>,
}

impl Transition {
    /// Start with a property to transition.
    pub fn new(property: TransitionPropertyKind) -> Self {
        Self {
            property,
            duration: None,
            timing: None,
            delay: None,
        }
    }

    /// Set duration.
    pub fn duration(mut self, d: Time) -> Self {
        self.duration = Some(d);
        self
    }

    /// Set timing function.
    pub fn timing(mut self, t: EasingFunction) -> Self {
        self.timing = Some(t);
        self
    }

    /// Set delay.
    pub fn delay(mut self, d: Time) -> Self {
        self.delay = Some(d);
        self
    }
}

impl ToCss for Transition {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        self.property.to_css(dest)?;
        if let Some(d) = &self.duration {
            dest.write_char(' ')?;
            d.to_css(dest)?;
        }
        if let Some(t) = &self.timing {
            dest.write_char(' ')?;
            t.to_css(dest)?;
        }
        if let Some(d) = &self.delay {
            dest.write_char(' ')?;
            d.to_css(dest)?;
        }
        Ok(())
    }
}

impl Css {
    /// Sets the `transition` shorthand for a single transition.
    /// <https://lynxjs.org/api/css/properties/transition>
    pub fn transition(self, t: Transition) -> Self {
        let serialized = t.to_css_string();
        self.push_semantic(
            crate::StyleProperty::Transition,
            whisker_style::StyleValue::Transitions(vec![to_transition_value(&t)]),
            serialized,
        )
    }

    /// Sets the `transition` shorthand for multiple comma-separated
    /// transitions.
    pub fn transitions(self, ts: impl IntoIterator<Item = Transition>) -> Self {
        let ts = ts.into_iter().collect::<Vec<_>>();
        let mut s = String::new();
        for (i, t) in ts.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            let _ = t.to_css(&mut s);
        }
        let semantic = ts.iter().map(to_transition_value).collect();
        self.push_semantic(
            crate::StyleProperty::Transition,
            whisker_style::StyleValue::Transitions(semantic),
            s,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::data_type_ext::EasingFunction;
    use crate::ext::*;
    use crate::keyword::TransitionPropertyKind;

    use super::*;

    #[test]
    fn transition_property_only() {
        let s = Css::new().transition(Transition::new(TransitionPropertyKind::All));
        assert_eq!(s.to_string(), "transition: all;");
    }

    #[test]
    fn transition_property_duration_timing_delay() {
        let s = Css::new().transition(
            Transition::new(TransitionPropertyKind::name("opacity"))
                .duration(300.ms())
                .timing(EasingFunction::EaseInOut)
                .delay(100.ms()),
        );
        assert_eq!(
            s.to_string(),
            "transition: opacity 300ms ease-in-out 100ms;"
        );
    }

    #[test]
    fn transitions_multiple_layers() {
        let s = Css::new().transitions([
            Transition::new(TransitionPropertyKind::name("opacity")).duration(300.ms()),
            Transition::new(TransitionPropertyKind::name("transform"))
                .duration(500.ms())
                .delay(100.ms()),
        ]);
        assert_eq!(
            s.to_string(),
            "transition: opacity 300ms, transform 500ms 100ms;"
        );
        let resolved = whisker_style::resolve_style(
            &s.to_specified_style(),
            None,
            whisker_style::StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(resolved.computed().motion().transitions.len(), 2);
        assert_eq!(
            resolved.computed().motion().transitions[1].property,
            whisker_style::ComputedTransitionProperty::Property(
                whisker_style::StyleProperty::Transform
            )
        );
        assert_eq!(
            resolved.computed().motion().transitions[1].duration.get(),
            500.0
        );
    }
}
