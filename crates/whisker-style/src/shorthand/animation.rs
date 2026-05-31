//! `animation` shorthand — bundles the eight animation longhands
//! into one declaration. Multiple animations are comma-separated.

use core::fmt;

use crate::data_type::Time;
use crate::data_type_ext::EasingFunction;
use crate::keyword::{
    AnimationDirection, AnimationFillMode, AnimationIterationCount, AnimationPlayState,
};
use crate::style::Style;
use crate::to_css::ToCss;

/// One animation layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    /// `@keyframes` name.
    pub name: String,
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

impl Animation {
    /// Start with the `@keyframes` name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
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
        // The CSS animation shorthand grammar allows any order; we
        // emit a stable order matching the Lynx spec for clarity.
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

impl Style {
    /// Sets the `animation` shorthand for a single animation.
    /// <https://lynxjs.org/api/css/properties/animation>
    pub fn animation(self, a: Animation) -> Self {
        self.push("animation", a)
    }

    /// Sets the `animation` shorthand for multiple comma-separated
    /// animations.
    pub fn animations(self, anims: impl IntoIterator<Item = Animation>) -> Self {
        let mut s = String::new();
        for (i, a) in anims.into_iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            let _ = a.to_css(&mut s);
        }
        self.push_raw("animation", s)
    }
}

#[cfg(test)]
mod tests {
    use crate::data_type_ext::EasingFunction;
    use crate::ext::*;
    use crate::keyword::*;
    use crate::Style;

    use super::*;

    #[test]
    fn animation_name_only() {
        let s = Style::new().animation(Animation::new("spin"));
        assert_eq!(s.to_string(), "animation: spin;");
    }

    #[test]
    fn animation_full_shorthand() {
        let s = Style::new().animation(
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
        let s = Style::new().animations([
            Animation::new("fade").duration(300.ms()),
            Animation::new("slide").duration(500.ms()).delay(100.ms()),
        ]);
        assert_eq!(
            s.to_string(),
            "animation: fade 300ms, slide 500ms 100ms;"
        );
    }
}
