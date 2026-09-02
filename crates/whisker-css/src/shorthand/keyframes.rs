//! Builder-based typed keyframe definitions.

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use whisker_style::{
    KeyframeDefinition, KeyframesDefinition, MotionEasing, StyleNumber, StylePropertyDomain,
};

use crate::Css;
use crate::data_type::Percentage;
use crate::data_type_ext::EasingFunction;
use crate::style_value::to_motion_easing;

/// A typed keyframe sequence shared by every animation that references it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Keyframes {
    definition: Arc<KeyframesDefinition>,
}

impl Keyframes {
    /// Starts a keyframe definition.
    pub fn builder() -> KeyframesBuilder {
        KeyframesBuilder::default()
    }

    /// Returns the diagnostic name used by events and compatibility output.
    pub fn name(&self) -> &str {
        &self.definition.name
    }

    pub(crate) fn definition(&self) -> Arc<KeyframesDefinition> {
        Arc::clone(&self.definition)
    }
}

/// One frame supplied to [`KeyframesBuilder`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Keyframe {
    offset: PercentageBits,
    style: Css,
    easing: Option<MotionEasing>,
}

impl Keyframe {
    /// Creates the `0%` frame.
    pub fn from(style: Css) -> Self {
        Self::at(Percentage::new(0.0), style)
    }

    /// Creates a frame at a percentage offset.
    pub fn at(offset: Percentage, style: Css) -> Self {
        Self {
            offset: PercentageBits(offset.value().to_bits()),
            style,
            easing: None,
        }
    }

    /// Creates the `100%` frame.
    pub fn to(style: Css) -> Self {
        Self::at(Percentage::new(100.0), style)
    }

    /// Sets the timing function for the interval beginning at this frame.
    pub fn easing(mut self, easing: EasingFunction) -> Self {
        self.easing = Some(to_motion_easing(easing));
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PercentageBits(u32);

impl PercentageBits {
    fn get(self) -> f32 {
        f32::from_bits(self.0)
    }
}

/// Fluent builder for an immutable [`Keyframes`] value.
#[derive(Clone, Debug, Default)]
pub struct KeyframesBuilder {
    name: Option<String>,
    frames: Vec<Keyframe>,
}

impl KeyframesBuilder {
    /// Sets the name reported by animation lifecycle events.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Adds one fully configured frame.
    pub fn frame(mut self, frame: Keyframe) -> Self {
        self.frames.push(frame);
        self
    }

    /// Adds the `0%` frame.
    pub fn from(self, style: Css) -> Self {
        self.frame(Keyframe::from(style))
    }

    /// Adds a frame at a percentage offset.
    pub fn at(self, offset: Percentage, style: Css) -> Self {
        self.frame(Keyframe::at(offset, style))
    }

    /// Adds the `100%` frame.
    pub fn to(self, style: Css) -> Self {
        self.frame(Keyframe::to(style))
    }

    /// Validates and freezes the definition.
    pub fn build(self) -> Result<Keyframes, KeyframesBuildError> {
        if self.frames.is_empty() {
            return Err(KeyframesBuildError::Empty);
        }
        let mut frames = Vec::<KeyframeDefinition>::with_capacity(self.frames.len());
        for frame in self.frames {
            let percentage = frame.offset.get();
            if !percentage.is_finite() || !(0.0..=100.0).contains(&percentage) {
                return Err(KeyframesBuildError::InvalidOffset(percentage));
            }
            let percentage = if percentage == 0.0 { 0.0 } else { percentage };
            let style = frame.style.to_specified_style();
            if let Some(property) = style
                .resolved()
                .into_iter()
                .map(|declaration| declaration.property())
                .find(|property| property.domain() == StylePropertyDomain::Motion)
            {
                return Err(KeyframesBuildError::MotionProperty(property));
            }
            frames.push(KeyframeDefinition {
                offset: StyleNumber::new(percentage / 100.0),
                style,
                easing: frame.easing,
            });
        }
        frames.sort_by(|left, right| left.offset.get().total_cmp(&right.offset.get()));
        let mut merged = Vec::<KeyframeDefinition>::with_capacity(frames.len());
        for frame in frames {
            if let Some(previous) = merged.last_mut() {
                if previous.offset == frame.offset {
                    previous.style = previous.style.clone().merge(frame.style);
                    previous.easing = frame.easing;
                    continue;
                }
            }
            merged.push(frame);
        }
        let name = self.name.unwrap_or_else(|| {
            let mut hasher = DefaultHasher::new();
            merged.hash(&mut hasher);
            format!("whisker-keyframes-{:016x}", hasher.finish())
        });
        Ok(Keyframes {
            definition: Arc::new(KeyframesDefinition {
                name,
                frames: merged,
            }),
        })
    }
}

/// Invalid data supplied to [`KeyframesBuilder::build`].
#[derive(Clone, Debug, PartialEq)]
pub enum KeyframesBuildError {
    /// No frames were supplied.
    Empty,
    /// An offset was non-finite or outside `0%..=100%`.
    InvalidOffset(f32),
    /// Timeline declarations cannot recursively appear inside a frame.
    MotionProperty(whisker_style::StyleProperty),
}

impl fmt::Display for KeyframesBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid Whisker keyframes: {self:?}")
    }
}

impl std::error::Error for KeyframesBuildError {}

#[cfg(test)]
mod tests {
    use crate::ext::*;

    use super::*;

    #[test]
    fn builder_sorts_and_merges_duplicate_offsets() {
        let frames = Keyframes::builder()
            .to(Css::new().opacity(1.0))
            .from(Css::new().opacity(0.0))
            .at(50.percent(), Css::new().opacity(0.4))
            .at(50.percent(), Css::new().width(20.px()))
            .build()
            .unwrap();
        assert_eq!(frames.definition.frames.len(), 3);
        assert_eq!(frames.definition.frames[1].style.len(), 2);
        assert!(frames.name().starts_with("whisker-keyframes-"));
    }

    #[test]
    fn builder_rejects_invalid_offsets_and_motion_properties() {
        assert!(matches!(
            Keyframes::builder()
                .at(101.percent(), Css::new().opacity(1.0))
                .build(),
            Err(KeyframesBuildError::InvalidOffset(101.0))
        ));
        assert!(matches!(
            Keyframes::builder()
                .from(Css::new().animation_duration(100.ms()))
                .build(),
            Err(KeyframesBuildError::MotionProperty(_))
        ));
    }
}
