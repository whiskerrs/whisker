//! `animation` shorthand — bundles the eight animation longhands
//! into one declaration. Multiple animations are comma-separated.

use core::fmt;

use crate::css::Css;
use crate::data_type::Time;
use crate::data_type_ext::EasingFunction;
use crate::keyword::{
    AnimationDirection, AnimationFillMode, AnimationIterationCount, AnimationPlayState,
};
use crate::shorthand::Keyframes;
use crate::style_value::to_animation_value;
use crate::to_css::ToCss;

/// One animation layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    /// `@keyframes` name.
    pub name: String,
    /// Typed keyframes used by the Rust-owned timeline.
    pub keyframes: Option<Keyframes>,
    /// Duration of one cycle.
    pub duration: Option<Time>,
    /// Timing function.
    pub timing: Option<EasingFunction>,
    /// Delay before the animation starts.
    pub delay: Option<Time>,
    /// How many cycles to run.
    pub iteration_count: Option<AnimationIterationCount>,
    /// Direction (forward, reverse, alternating).
    pub direction: Option<AnimationDirection>,
    /// Fill mode before/after the active period.
    pub fill_mode: Option<AnimationFillMode>,
    /// Play state.
    pub play_state: Option<AnimationPlayState>,
}

/// A checked keyframe definition or a migration-only string name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AnimationTarget {
    /// Builder-defined keyframes.
    Keyframes(Keyframes),
    /// Compatibility name without an attached Rust keyframe definition.
    Name(String),
}

impl From<Keyframes> for AnimationTarget {
    fn from(value: Keyframes) -> Self {
        Self::Keyframes(value)
    }
}

impl From<String> for AnimationTarget {
    fn from(value: String) -> Self {
        Self::Name(value)
    }
}

impl From<&str> for AnimationTarget {
    fn from(value: &str) -> Self {
        Self::Name(value.to_owned())
    }
}

impl Animation {
    /// Starts an animation from typed keyframes or a compatibility name.
    pub fn new(target: impl Into<AnimationTarget>) -> Self {
        let (name, keyframes) = match target.into() {
            AnimationTarget::Keyframes(keyframes) => (keyframes.name().to_owned(), Some(keyframes)),
            AnimationTarget::Name(name) => (name, None),
        };
        Self {
            name,
            keyframes,
            duration: None,
            timing: None,
            delay: None,
            iteration_count: None,
            direction: None,
            fill_mode: None,
            play_state: None,
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

    /// Set iteration count.
    pub fn iteration_count(mut self, c: AnimationIterationCount) -> Self {
        self.iteration_count = Some(c);
        self
    }

    /// Set direction.
    pub fn direction(mut self, d: AnimationDirection) -> Self {
        self.direction = Some(d);
        self
    }

    /// Set fill mode.
    pub fn fill_mode(mut self, f: AnimationFillMode) -> Self {
        self.fill_mode = Some(f);
        self
    }

    /// Set play state.
    pub fn play_state(mut self, p: AnimationPlayState) -> Self {
        self.play_state = Some(p);
        self
    }
}

impl ToCss for Animation {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(&self.name)?;
        // The shorthand grammar allows any order; this emits the Lynx
        // spec's order so output stays stable.
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
        if let Some(c) = &self.iteration_count {
            dest.write_char(' ')?;
            c.to_css(dest)?;
        }
        if let Some(d) = &self.direction {
            dest.write_char(' ')?;
            d.to_css(dest)?;
        }
        if let Some(f) = &self.fill_mode {
            dest.write_char(' ')?;
            f.to_css(dest)?;
        }
        if let Some(p) = &self.play_state {
            dest.write_char(' ')?;
            p.to_css(dest)?;
        }
        Ok(())
    }
}

impl Css {
    /// Sets the `animation` shorthand for a single animation.
    /// <https://lynxjs.org/api/css/properties/animation>
    pub fn animation(self, a: Animation) -> Self {
        let serialized = a.to_css_string();
        self.push_semantic(
            crate::StyleProperty::Animation,
            whisker_style::StyleValue::Animations(vec![to_animation_value(&a)]),
            serialized,
        )
    }

    /// Sets the `animation` shorthand for multiple comma-separated
    /// animations.
    pub fn animations(self, anims: impl IntoIterator<Item = Animation>) -> Self {
        let anims = anims.into_iter().collect::<Vec<_>>();
        let mut s = String::new();
        for (i, a) in anims.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            let _ = a.to_css(&mut s);
        }
        let semantic = anims.iter().map(to_animation_value).collect();
        self.push_semantic(
            crate::StyleProperty::Animation,
            whisker_style::StyleValue::Animations(semantic),
            s,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::data_type_ext::EasingFunction;
    use crate::ext::*;
    use crate::keyword::*;

    use super::*;

    #[test]
    fn animation_name_only() {
        let s = Css::new().animation(Animation::new("spin"));
        assert_eq!(s.to_string(), "animation: spin;");
    }

    #[test]
    fn animation_full_shorthand() {
        let s = Css::new().animation(
            Animation::new("spin")
                .duration(1.s())
                .timing(EasingFunction::Linear)
                .delay(100.ms())
                .iteration_count(AnimationIterationCount::Infinite)
                .direction(AnimationDirection::Alternate)
                .fill_mode(AnimationFillMode::Forwards)
                .play_state(AnimationPlayState::Running),
        );
        assert_eq!(
            s.to_string(),
            "animation: spin 1s linear 100ms infinite alternate forwards running;"
        );
    }

    #[test]
    fn animations_multiple() {
        let s = Css::new().animations([
            Animation::new("fade").duration(300.ms()),
            Animation::new("slide").duration(500.ms()).delay(100.ms()),
        ]);
        assert_eq!(s.to_string(), "animation: fade 300ms, slide 500ms 100ms;");
        let resolved = whisker_style::resolve_style(
            &s.to_specified_style(),
            None,
            whisker_style::StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(resolved.computed().motion().animations.len(), 2);
        assert_eq!(
            resolved.computed().motion().animations[1].name.as_deref(),
            Some("slide")
        );
        assert_eq!(
            resolved.computed().motion().animations[1].delay.get(),
            100.0
        );
    }

    #[test]
    fn builder_keyframes_reach_semantic_animation_value() {
        let keyframes = crate::Keyframes::builder()
            .named("fade")
            .from(Css::new().opacity(0.0))
            .to(Css::new().opacity(1.0))
            .build()
            .unwrap();
        let style = Css::new().animation(Animation::new(keyframes).duration(200.ms()));
        let resolved = whisker_style::resolve_style(
            &style.to_specified_style(),
            None,
            whisker_style::StyleEnvironment::default(),
        )
        .unwrap();
        let animation = &resolved.computed().motion().animations[0];
        assert_eq!(animation.name.as_deref(), Some("fade"));
        assert_eq!(animation.keyframes.as_ref().unwrap().frames.len(), 2);
    }
}
