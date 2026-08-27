//! Host-independent transition and keyframe-animation declarations.

use std::sync::Arc;

use crate::{SpecifiedStyle, StyleNumber, StyleProperty, StyleResolutionError, StyleValue};

/// A CSS time normalized to milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MotionTime(pub StyleNumber);

impl Default for MotionTime {
    fn default() -> Self {
        Self::milliseconds(0.0)
    }
}

impl MotionTime {
    /// Creates a millisecond value.
    pub const fn milliseconds(value: f32) -> Self {
        Self(StyleNumber::new(value))
    }

    /// Returns the normalized millisecond value.
    pub const fn get(self) -> f32 {
        self.0.get()
    }
}

/// Jump placement for a discrete timing function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotionStepPosition {
    /// Jump at the beginning of each interval.
    JumpStart,
    /// Jump at the end of each interval.
    JumpEnd,
    /// Omit jumps at both endpoints.
    JumpNone,
    /// Include jumps at both endpoints.
    JumpBoth,
}

/// One normalized timing function.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MotionEasing {
    /// Constant-rate interpolation.
    Linear,
    /// CSS `ease` cubic curve.
    #[default]
    Ease,
    /// CSS `ease-in` cubic curve.
    EaseIn,
    /// CSS `ease-out` cubic curve.
    EaseOut,
    /// CSS `ease-in-out` cubic curve.
    EaseInOut,
    /// A custom cubic Bézier curve `(x1, y1, x2, y2)`.
    CubicBezier([StyleNumber; 4]),
    /// A discrete step timing function.
    Steps {
        /// Number of equal intervals.
        count: u32,
        /// Endpoint jump placement.
        position: MotionStepPosition,
    },
}

impl MotionEasing {
    /// Samples this timing function at normalized input progress.
    pub fn sample(self, progress: f32) -> f32 {
        let progress = progress.clamp(0.0, 1.0);
        match self {
            Self::Linear => progress,
            Self::Ease => cubic_bezier(progress, 0.25, 0.1, 0.25, 1.0),
            Self::EaseIn => cubic_bezier(progress, 0.42, 0.0, 1.0, 1.0),
            Self::EaseOut => cubic_bezier(progress, 0.0, 0.0, 0.58, 1.0),
            Self::EaseInOut => cubic_bezier(progress, 0.42, 0.0, 0.58, 1.0),
            Self::CubicBezier([x1, y1, x2, y2]) => {
                cubic_bezier(progress, x1.get(), y1.get(), x2.get(), y2.get())
            }
            Self::Steps { count, position } => {
                let count = count as f32;
                match position {
                    MotionStepPosition::JumpStart => (progress * count).ceil() / count,
                    MotionStepPosition::JumpEnd => (progress * count).floor() / count,
                    MotionStepPosition::JumpNone => {
                        ((progress * count).floor() / (count - 1.0)).clamp(0.0, 1.0)
                    }
                    MotionStepPosition::JumpBoth => {
                        ((progress * count).floor() + 1.0) / (count + 1.0)
                    }
                }
            }
        }
    }
}

fn cubic_bezier(progress: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if progress == 0.0 || progress == 1.0 {
        return progress;
    }
    let coordinate = |time: f32, first: f32, second: f32| {
        let inverse = 1.0 - time;
        3.0 * inverse * inverse * time * first
            + 3.0 * inverse * time * time * second
            + time * time * time
    };
    let mut lower = 0.0;
    let mut upper = 1.0;
    for _ in 0..16 {
        let time = (lower + upper) * 0.5;
        if coordinate(time, x1, x2) < progress {
            lower = time;
        } else {
            upper = time;
        }
    }
    coordinate((lower + upper) * 0.5, y1, y2)
}

/// The property selection in one specified transition layer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransitionPropertyValue {
    /// Animate every supported property that changes.
    All,
    /// Disable the layer.
    None,
    /// A CSS property name resolved through Whisker's stable registry.
    Named(String),
}

/// One fully expanded specified transition layer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TransitionValue {
    /// Selected property.
    pub property: TransitionPropertyValue,
    /// Active duration.
    pub duration: MotionTime,
    /// Sampling curve.
    pub easing: MotionEasing,
    /// Delay before the active interval; negative values seek into it.
    pub delay: MotionTime,
}

/// Resolved property selection for one transition layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedTransitionProperty {
    /// Animate every supported property that changes.
    All,
    /// Disabled transition layer.
    None,
    /// One known stable property.
    Property(StyleProperty),
}

/// One validated and registry-resolved transition layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputedTransition {
    /// Selected property.
    pub property: ComputedTransitionProperty,
    /// Non-negative active duration.
    pub duration: MotionTime,
    /// Validated sampling curve.
    pub easing: MotionEasing,
    /// Delay before the active interval.
    pub delay: MotionTime,
}

/// Direction in which keyframe iterations run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MotionDirection {
    /// Every iteration runs from start to end.
    #[default]
    Normal,
    /// Every iteration runs from end to start.
    Reverse,
    /// Odd iterations run forward and even iterations backward.
    Alternate,
    /// Odd iterations run backward and even iterations forward.
    AlternateReverse,
}

/// Keyframe values retained outside the active interval.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MotionFillMode {
    /// Do not retain keyframe values outside the active interval.
    #[default]
    None,
    /// Retain the final sampled value after completion.
    Forwards,
    /// Apply the initial sampled value during a positive delay.
    Backwards,
    /// Apply both backwards and forwards fill behavior.
    Both,
}

/// Whether a keyframe animation timeline advances.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MotionPlayState {
    /// Advance with the runtime clock.
    #[default]
    Running,
    /// Retain the current sample without advancing.
    Paused,
}

/// Number of keyframe iterations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotionIterationCount {
    /// Repeat without a finite end.
    Infinite,
    /// Run a non-negative, possibly fractional number of iterations.
    Count(StyleNumber),
}

/// One normalized keyframe in a Rust-owned animation definition.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyframeDefinition {
    /// Offset within one iteration, normalized to `0..=1`.
    pub offset: StyleNumber,
    /// Typed declarations contributed by this keyframe.
    pub style: SpecifiedStyle,
    /// Timing function for the interval beginning at this keyframe.
    pub easing: Option<MotionEasing>,
}

/// An immutable, shareable keyframe sequence.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyframesDefinition {
    /// Stable diagnostic and compatibility name.
    pub name: String,
    /// Frames sorted by ascending offset.
    pub frames: Vec<KeyframeDefinition>,
}

impl Default for MotionIterationCount {
    fn default() -> Self {
        Self::Count(StyleNumber::new(1.0))
    }
}

/// One fully expanded specified keyframe-animation layer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AnimationValue {
    /// Diagnostic keyframe name; `None` disables the layer.
    pub name: Option<String>,
    /// Typed keyframes for the Rust-owned timeline, when available.
    pub keyframes: Option<Arc<KeyframesDefinition>>,
    /// Active duration for one iteration.
    pub duration: MotionTime,
    /// Sampling curve applied within keyframe intervals.
    pub easing: MotionEasing,
    /// Delay before the active interval.
    pub delay: MotionTime,
    /// Number of iterations.
    pub iteration_count: MotionIterationCount,
    /// Iteration direction.
    pub direction: MotionDirection,
    /// Values retained outside the active interval.
    pub fill_mode: MotionFillMode,
    /// Whether the timeline advances.
    pub play_state: MotionPlayState,
}

/// Motion declarations consumed entirely by the Rust timeline layer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ComputedMotionStyle {
    /// Fully resolved transition layers.
    pub transitions: Vec<ComputedTransition>,
    /// Validated keyframe-animation layers. Typed definitions are retained
    /// inline; compatibility-only names have no Rust timeline to sample.
    pub animations: Vec<AnimationValue>,
}

fn invalid(property: StyleProperty) -> StyleResolutionError {
    StyleResolutionError::InvalidPropertyValue(property)
}

fn valid_time(value: MotionTime) -> bool {
    value.get().is_finite()
}

fn valid_easing(value: MotionEasing) -> bool {
    match value {
        MotionEasing::CubicBezier([x1, y1, x2, y2]) => {
            [x1, y1, x2, y2]
                .into_iter()
                .all(|value| value.get().is_finite())
                && (0.0..=1.0).contains(&x1.get())
                && (0.0..=1.0).contains(&x2.get())
        }
        MotionEasing::Steps { count, position } => {
            count > 0 && (position != MotionStepPosition::JumpNone || count > 1)
        }
        _ => true,
    }
}

fn transitionable(property: StyleProperty) -> bool {
    matches!(
        property,
        StyleProperty::Left
            | StyleProperty::Right
            | StyleProperty::Top
            | StyleProperty::Bottom
            | StyleProperty::Width
            | StyleProperty::Height
            | StyleProperty::Opacity
            | StyleProperty::BackgroundColor
            | StyleProperty::Color
            | StyleProperty::Transform
            | StyleProperty::TransformOrigin
            | StyleProperty::MaxWidth
            | StyleProperty::MinWidth
            | StyleProperty::MaxHeight
            | StyleProperty::MinHeight
            | StyleProperty::PaddingLeft
            | StyleProperty::PaddingRight
            | StyleProperty::PaddingTop
            | StyleProperty::PaddingBottom
            | StyleProperty::MarginLeft
            | StyleProperty::MarginRight
            | StyleProperty::MarginTop
            | StyleProperty::MarginBottom
            | StyleProperty::BorderLeftWidth
            | StyleProperty::BorderRightWidth
            | StyleProperty::BorderTopWidth
            | StyleProperty::BorderBottomWidth
            | StyleProperty::BorderLeftColor
            | StyleProperty::BorderRightColor
            | StyleProperty::BorderTopColor
            | StyleProperty::BorderBottomColor
            | StyleProperty::FlexBasis
            | StyleProperty::FlexGrow
    )
}

fn resolve_transition_property(
    value: &TransitionPropertyValue,
) -> Result<ComputedTransitionProperty, StyleResolutionError> {
    match value {
        TransitionPropertyValue::All => Ok(ComputedTransitionProperty::All),
        TransitionPropertyValue::None => Ok(ComputedTransitionProperty::None),
        TransitionPropertyValue::Named(name) => StyleProperty::from_css_name(name)
            .filter(|property| transitionable(*property))
            .map(ComputedTransitionProperty::Property)
            .ok_or_else(|| invalid(StyleProperty::TransitionProperty)),
    }
}

fn cyclic<T: Clone>(values: &[T], index: usize) -> T {
    values[index % values.len()].clone()
}

/// Resolves non-inherited motion declarations without involving a Host.
pub(crate) fn resolve_motion_style(
    specified: &SpecifiedStyle,
) -> Result<ComputedMotionStyle, StyleResolutionError> {
    let mut transition_properties = vec![TransitionPropertyValue::None];
    let mut transition_durations = vec![MotionTime::default()];
    let mut transition_easings = vec![MotionEasing::default()];
    let mut transition_delays = vec![MotionTime::default()];

    let mut animation_names = vec![None];
    let mut animation_keyframes = vec![None];
    let mut animation_durations = vec![MotionTime::default()];
    let mut animation_easings = vec![MotionEasing::default()];
    let mut animation_delays = vec![MotionTime::default()];
    let mut animation_iterations = vec![MotionIterationCount::default()];
    let mut animation_directions = vec![MotionDirection::default()];
    let mut animation_fills = vec![MotionFillMode::default()];
    let mut animation_play_states = vec![MotionPlayState::default()];

    for declaration in specified.resolved() {
        match (declaration.property(), declaration.value()) {
            (StyleProperty::Transition, StyleValue::Transitions(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::Transition));
                }
                transition_properties = values.iter().map(|value| value.property.clone()).collect();
                transition_durations = values.iter().map(|value| value.duration).collect();
                transition_easings = values.iter().map(|value| value.easing).collect();
                transition_delays = values.iter().map(|value| value.delay).collect();
            }
            (StyleProperty::TransitionProperty, StyleValue::TransitionProperties(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::TransitionProperty));
                }
                transition_properties = values.clone();
            }
            (StyleProperty::TransitionDuration, StyleValue::TransitionDurations(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::TransitionDuration));
                }
                transition_durations = values.clone();
            }
            (StyleProperty::TransitionTimingFunction, StyleValue::TransitionEasings(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::TransitionTimingFunction));
                }
                transition_easings = values.clone();
            }
            (StyleProperty::TransitionDelay, StyleValue::TransitionDelays(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::TransitionDelay));
                }
                transition_delays = values.clone();
            }
            (StyleProperty::Animation, StyleValue::Animations(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::Animation));
                }
                animation_names = values.iter().map(|value| value.name.clone()).collect();
                animation_keyframes = values.iter().map(|value| value.keyframes.clone()).collect();
                animation_durations = values.iter().map(|value| value.duration).collect();
                animation_easings = values.iter().map(|value| value.easing).collect();
                animation_delays = values.iter().map(|value| value.delay).collect();
                animation_iterations = values.iter().map(|value| value.iteration_count).collect();
                animation_directions = values.iter().map(|value| value.direction).collect();
                animation_fills = values.iter().map(|value| value.fill_mode).collect();
                animation_play_states = values.iter().map(|value| value.play_state).collect();
            }
            (StyleProperty::AnimationName, StyleValue::AnimationNames(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::AnimationName));
                }
                animation_names = values.clone();
                animation_keyframes = vec![None; animation_names.len()];
            }
            (StyleProperty::AnimationName, StyleValue::Animations(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::AnimationName));
                }
                animation_names = values.iter().map(|value| value.name.clone()).collect();
                animation_keyframes = values.iter().map(|value| value.keyframes.clone()).collect();
            }
            (StyleProperty::AnimationDuration, StyleValue::AnimationDurations(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::AnimationDuration));
                }
                animation_durations = values.clone();
            }
            (StyleProperty::AnimationTimingFunction, StyleValue::AnimationEasings(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::AnimationTimingFunction));
                }
                animation_easings = values.clone();
            }
            (StyleProperty::AnimationDelay, StyleValue::AnimationDelays(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::AnimationDelay));
                }
                animation_delays = values.clone();
            }
            (
                StyleProperty::AnimationIterationCount,
                StyleValue::AnimationIterationCounts(values),
            ) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::AnimationIterationCount));
                }
                animation_iterations = values.clone();
            }
            (StyleProperty::AnimationDirection, StyleValue::AnimationDirections(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::AnimationDirection));
                }
                animation_directions = values.clone();
            }
            (StyleProperty::AnimationFillMode, StyleValue::AnimationFillModes(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::AnimationFillMode));
                }
                animation_fills = values.clone();
            }
            (StyleProperty::AnimationPlayState, StyleValue::AnimationPlayStates(values)) => {
                if values.is_empty() {
                    return Err(invalid(StyleProperty::AnimationPlayState));
                }
                animation_play_states = values.clone();
            }
            (property, _) if matches!(property.domain(), crate::StylePropertyDomain::Motion) => {
                return Err(invalid(property));
            }
            _ => {}
        }
    }

    if transition_durations
        .iter()
        .any(|value| !valid_time(*value) || value.get() < 0.0)
    {
        return Err(invalid(StyleProperty::TransitionDuration));
    }
    if transition_delays.iter().any(|value| !valid_time(*value)) {
        return Err(invalid(StyleProperty::TransitionDelay));
    }
    if transition_easings.iter().any(|value| !valid_easing(*value)) {
        return Err(invalid(StyleProperty::TransitionTimingFunction));
    }
    let transition_count = transition_properties.len();
    let transitions = (0..transition_count)
        .map(|index| {
            Ok(ComputedTransition {
                property: resolve_transition_property(&transition_properties[index])?,
                duration: cyclic(&transition_durations, index),
                easing: cyclic(&transition_easings, index),
                delay: cyclic(&transition_delays, index),
            })
        })
        .collect::<Result<Vec<_>, StyleResolutionError>>()?;
    let transitions = transitions
        .into_iter()
        .filter(|value| value.property != ComputedTransitionProperty::None)
        .collect();

    if animation_durations
        .iter()
        .any(|value| !valid_time(*value) || value.get() < 0.0)
    {
        return Err(invalid(StyleProperty::AnimationDuration));
    }
    if animation_delays.iter().any(|value| !valid_time(*value)) {
        return Err(invalid(StyleProperty::AnimationDelay));
    }
    if animation_easings.iter().any(|value| !valid_easing(*value)) {
        return Err(invalid(StyleProperty::AnimationTimingFunction));
    }
    if animation_iterations.iter().any(|value| match value {
        MotionIterationCount::Infinite => false,
        MotionIterationCount::Count(value) => !value.get().is_finite() || value.get() < 0.0,
    }) {
        return Err(invalid(StyleProperty::AnimationIterationCount));
    }
    let animations = (0..animation_names.len())
        .map(|index| AnimationValue {
            name: animation_names[index].clone(),
            keyframes: cyclic(&animation_keyframes, index),
            duration: cyclic(&animation_durations, index),
            easing: cyclic(&animation_easings, index),
            delay: cyclic(&animation_delays, index),
            iteration_count: cyclic(&animation_iterations, index),
            direction: cyclic(&animation_directions, index),
            fill_mode: cyclic(&animation_fills, index),
            play_state: cyclic(&animation_play_states, index),
        })
        .filter(|value| value.name.is_some())
        .collect();

    Ok(ComputedMotionStyle {
        transitions,
        animations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transition(
        property: TransitionPropertyValue,
        duration: f32,
        easing: MotionEasing,
        delay: f32,
    ) -> TransitionValue {
        TransitionValue {
            property,
            duration: MotionTime::milliseconds(duration),
            easing,
            delay: MotionTime::milliseconds(delay),
        }
    }

    fn animation(name: Option<&str>) -> AnimationValue {
        AnimationValue {
            name: name.map(str::to_owned),
            keyframes: None,
            duration: MotionTime::milliseconds(200.0),
            easing: MotionEasing::Linear,
            delay: MotionTime::milliseconds(10.0),
            iteration_count: MotionIterationCount::Infinite,
            direction: MotionDirection::AlternateReverse,
            fill_mode: MotionFillMode::Both,
            play_state: MotionPlayState::Paused,
        }
    }

    #[test]
    fn defaults_do_not_allocate_inactive_timeline_layers() {
        assert_eq!(
            resolve_motion_style(&SpecifiedStyle::new()).unwrap(),
            ComputedMotionStyle::default()
        );
        let resolved = crate::resolve_style(
            &SpecifiedStyle::new(),
            None,
            crate::StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            resolved.computed().motion(),
            &ComputedMotionStyle::default()
        );
    }

    #[test]
    fn easing_and_transition_property_validation_cover_every_branch() {
        let number = StyleNumber::new;
        assert!(valid_easing(MotionEasing::Linear));
        assert!(valid_easing(MotionEasing::CubicBezier([
            number(0.0),
            number(-2.0),
            number(1.0),
            number(3.0),
        ])));
        assert!(!valid_easing(MotionEasing::CubicBezier([
            number(f32::NAN),
            number(0.0),
            number(1.0),
            number(1.0),
        ])));
        assert!(!valid_easing(MotionEasing::CubicBezier([
            number(-0.1),
            number(0.0),
            number(1.0),
            number(1.0),
        ])));
        assert!(!valid_easing(MotionEasing::CubicBezier([
            number(0.0),
            number(0.0),
            number(1.1),
            number(1.0),
        ])));
        assert!(valid_easing(MotionEasing::Steps {
            count: 1,
            position: MotionStepPosition::JumpEnd,
        }));
        assert!(!valid_easing(MotionEasing::Steps {
            count: 0,
            position: MotionStepPosition::JumpEnd,
        }));
        assert!(!valid_easing(MotionEasing::Steps {
            count: 1,
            position: MotionStepPosition::JumpNone,
        }));
        assert!(valid_easing(MotionEasing::Steps {
            count: 2,
            position: MotionStepPosition::JumpNone,
        }));

        assert_eq!(
            resolve_transition_property(&TransitionPropertyValue::All),
            Ok(ComputedTransitionProperty::All)
        );
        assert_eq!(
            resolve_transition_property(&TransitionPropertyValue::None),
            Ok(ComputedTransitionProperty::None)
        );
        for property in [
            StyleProperty::Left,
            StyleProperty::Right,
            StyleProperty::Top,
            StyleProperty::Bottom,
            StyleProperty::Width,
            StyleProperty::Height,
            StyleProperty::Opacity,
            StyleProperty::BackgroundColor,
            StyleProperty::Color,
            StyleProperty::Transform,
            StyleProperty::TransformOrigin,
            StyleProperty::MaxWidth,
            StyleProperty::MinWidth,
            StyleProperty::MaxHeight,
            StyleProperty::MinHeight,
            StyleProperty::PaddingLeft,
            StyleProperty::PaddingRight,
            StyleProperty::PaddingTop,
            StyleProperty::PaddingBottom,
            StyleProperty::MarginLeft,
            StyleProperty::MarginRight,
            StyleProperty::MarginTop,
            StyleProperty::MarginBottom,
            StyleProperty::BorderLeftWidth,
            StyleProperty::BorderRightWidth,
            StyleProperty::BorderTopWidth,
            StyleProperty::BorderBottomWidth,
            StyleProperty::BorderLeftColor,
            StyleProperty::BorderRightColor,
            StyleProperty::BorderTopColor,
            StyleProperty::BorderBottomColor,
            StyleProperty::FlexBasis,
            StyleProperty::FlexGrow,
        ] {
            assert!(transitionable(property));
        }
        assert!(!transitionable(StyleProperty::Display));
    }

    #[test]
    fn empty_and_mismatched_motion_declarations_are_rejected() {
        for (property, value) in [
            (StyleProperty::Transition, StyleValue::Transitions(vec![])),
            (
                StyleProperty::TransitionProperty,
                StyleValue::TransitionProperties(vec![]),
            ),
            (
                StyleProperty::TransitionDuration,
                StyleValue::TransitionDurations(vec![]),
            ),
            (
                StyleProperty::TransitionTimingFunction,
                StyleValue::TransitionEasings(vec![]),
            ),
            (
                StyleProperty::TransitionDelay,
                StyleValue::TransitionDelays(vec![]),
            ),
            (StyleProperty::Animation, StyleValue::Animations(vec![])),
            (
                StyleProperty::AnimationName,
                StyleValue::AnimationNames(vec![]),
            ),
            (StyleProperty::AnimationName, StyleValue::Animations(vec![])),
            (
                StyleProperty::AnimationDuration,
                StyleValue::AnimationDurations(vec![]),
            ),
            (
                StyleProperty::AnimationTimingFunction,
                StyleValue::AnimationEasings(vec![]),
            ),
            (
                StyleProperty::AnimationDelay,
                StyleValue::AnimationDelays(vec![]),
            ),
            (
                StyleProperty::AnimationIterationCount,
                StyleValue::AnimationIterationCounts(vec![]),
            ),
            (
                StyleProperty::AnimationDirection,
                StyleValue::AnimationDirections(vec![]),
            ),
            (
                StyleProperty::AnimationFillMode,
                StyleValue::AnimationFillModes(vec![]),
            ),
            (
                StyleProperty::AnimationPlayState,
                StyleValue::AnimationPlayStates(vec![]),
            ),
        ] {
            assert_eq!(
                resolve_motion_style(&SpecifiedStyle::new().push(property, value)),
                Err(invalid(property))
            );
        }
        assert_eq!(
            resolve_motion_style(&SpecifiedStyle::new().push(
                StyleProperty::AnimationName,
                StyleValue::AnimationDurations(vec![MotionTime::milliseconds(1.0)]),
            )),
            Err(invalid(StyleProperty::AnimationName))
        );
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(
                    StyleProperty::AnimationName,
                    StyleValue::AnimationDurations(vec![MotionTime::milliseconds(1.0)]),
                ),
                None,
                crate::StyleEnvironment::default(),
            ),
            Err(invalid(StyleProperty::AnimationName))
        );
    }

    #[test]
    fn animation_shorthand_and_every_longhand_resolve() {
        let shorthand = resolve_motion_style(&SpecifiedStyle::new().push(
            StyleProperty::Animation,
            StyleValue::Animations(vec![animation(Some("pulse")), animation(None)]),
        ))
        .unwrap();
        assert_eq!(shorthand.animations, vec![animation(Some("pulse"))]);

        let keyframes = Arc::new(KeyframesDefinition {
            name: "typed".into(),
            frames: vec![],
        });
        let mut typed = animation(Some("typed"));
        typed.keyframes = Some(Arc::clone(&keyframes));
        let typed_name = resolve_motion_style(&SpecifiedStyle::new().push(
            StyleProperty::AnimationName,
            StyleValue::Animations(vec![typed]),
        ))
        .unwrap();
        assert_eq!(
            typed_name.animations[0].keyframes.as_ref(),
            Some(&keyframes)
        );

        let longhands = resolve_motion_style(
            &SpecifiedStyle::new()
                .push(
                    StyleProperty::AnimationName,
                    StyleValue::AnimationNames(vec![Some("spin".into())]),
                )
                .push(
                    StyleProperty::AnimationDuration,
                    StyleValue::AnimationDurations(vec![MotionTime::milliseconds(300.0)]),
                )
                .push(
                    StyleProperty::AnimationTimingFunction,
                    StyleValue::AnimationEasings(vec![MotionEasing::EaseOut]),
                )
                .push(
                    StyleProperty::AnimationDelay,
                    StyleValue::AnimationDelays(vec![MotionTime::milliseconds(-10.0)]),
                )
                .push(
                    StyleProperty::AnimationIterationCount,
                    StyleValue::AnimationIterationCounts(vec![MotionIterationCount::Infinite]),
                )
                .push(
                    StyleProperty::AnimationDirection,
                    StyleValue::AnimationDirections(vec![MotionDirection::Reverse]),
                )
                .push(
                    StyleProperty::AnimationFillMode,
                    StyleValue::AnimationFillModes(vec![MotionFillMode::Forwards]),
                )
                .push(
                    StyleProperty::AnimationPlayState,
                    StyleValue::AnimationPlayStates(vec![MotionPlayState::Running]),
                ),
        )
        .unwrap();
        assert_eq!(longhands.animations.len(), 1);
        assert_eq!(longhands.animations[0].name.as_deref(), Some("spin"));
        assert_eq!(longhands.animations[0].duration.get(), 300.0);
    }

    #[test]
    fn invalid_motion_numbers_report_their_own_longhand() {
        for (property, value) in [
            (
                StyleProperty::TransitionDuration,
                StyleValue::TransitionDurations(vec![MotionTime::milliseconds(f32::NAN)]),
            ),
            (
                StyleProperty::TransitionDelay,
                StyleValue::TransitionDelays(vec![MotionTime::milliseconds(f32::NAN)]),
            ),
            (
                StyleProperty::TransitionTimingFunction,
                StyleValue::TransitionEasings(vec![MotionEasing::Steps {
                    count: 0,
                    position: MotionStepPosition::JumpStart,
                }]),
            ),
            (
                StyleProperty::AnimationDuration,
                StyleValue::AnimationDurations(vec![MotionTime::milliseconds(-1.0)]),
            ),
            (
                StyleProperty::AnimationDuration,
                StyleValue::AnimationDurations(vec![MotionTime::milliseconds(f32::NAN)]),
            ),
            (
                StyleProperty::AnimationDelay,
                StyleValue::AnimationDelays(vec![MotionTime::milliseconds(f32::NAN)]),
            ),
            (
                StyleProperty::AnimationTimingFunction,
                StyleValue::AnimationEasings(vec![MotionEasing::Steps {
                    count: 0,
                    position: MotionStepPosition::JumpBoth,
                }]),
            ),
            (
                StyleProperty::AnimationIterationCount,
                StyleValue::AnimationIterationCounts(vec![MotionIterationCount::Count(
                    StyleNumber::new(-1.0),
                )]),
            ),
            (
                StyleProperty::AnimationIterationCount,
                StyleValue::AnimationIterationCounts(vec![MotionIterationCount::Count(
                    StyleNumber::new(f32::NAN),
                )]),
            ),
        ] {
            assert_eq!(
                resolve_motion_style(&SpecifiedStyle::new().push(property, value)),
                Err(invalid(property))
            );
        }
    }

    #[test]
    fn easing_samples_normalized_progress() {
        assert_eq!(MotionEasing::Linear.sample(0.5), 0.5);
        assert_eq!(MotionEasing::Ease.sample(0.0), 0.0);
        assert_eq!(MotionEasing::Ease.sample(1.0), 1.0);
        assert!((0.0..=1.0).contains(&MotionEasing::Ease.sample(0.5)));
        assert!((0.0..=1.0).contains(&MotionEasing::EaseIn.sample(0.5)));
        assert!((0.0..=1.0).contains(&MotionEasing::EaseOut.sample(0.5)));
        assert!((0.0..=1.0).contains(&MotionEasing::EaseInOut.sample(0.5)));
        assert!(
            (0.0..=1.0).contains(
                &MotionEasing::CubicBezier([
                    StyleNumber::new(0.1),
                    StyleNumber::new(0.2),
                    StyleNumber::new(0.8),
                    StyleNumber::new(0.9),
                ])
                .sample(0.5)
            )
        );
        assert_eq!(MotionEasing::Linear.sample(-1.0), 0.0);
        assert_eq!(MotionEasing::Linear.sample(2.0), 1.0);
        assert_eq!(
            MotionEasing::Steps {
                count: 4,
                position: MotionStepPosition::JumpStart,
            }
            .sample(0.49),
            0.5
        );
        assert_eq!(
            MotionEasing::Steps {
                count: 4,
                position: MotionStepPosition::JumpEnd,
            }
            .sample(0.49),
            0.25
        );
        assert_eq!(
            MotionEasing::Steps {
                count: 2,
                position: MotionStepPosition::JumpNone,
            }
            .sample(0.49),
            0.0
        );
        assert_eq!(
            MotionEasing::Steps {
                count: 2,
                position: MotionStepPosition::JumpBoth,
            }
            .sample(0.49),
            1.0 / 3.0
        );
    }

    #[test]
    fn transition_shorthand_and_later_longhand_resolve_with_css_list_cycling() {
        let specified = SpecifiedStyle::new()
            .push(
                StyleProperty::Transition,
                StyleValue::Transitions(vec![
                    transition(
                        TransitionPropertyValue::Named("opacity".into()),
                        300.0,
                        MotionEasing::Linear,
                        10.0,
                    ),
                    transition(
                        TransitionPropertyValue::Named("background-color".into()),
                        500.0,
                        MotionEasing::EaseIn,
                        20.0,
                    ),
                ]),
            )
            .push(
                StyleProperty::TransitionDuration,
                StyleValue::TransitionDurations(vec![MotionTime::milliseconds(125.0)]),
            );
        let motion = resolve_motion_style(&specified).unwrap();
        assert_eq!(motion.transitions.len(), 2);
        assert_eq!(
            motion.transitions[0].property,
            ComputedTransitionProperty::Property(StyleProperty::Opacity)
        );
        assert_eq!(
            motion.transitions[1].property,
            ComputedTransitionProperty::Property(StyleProperty::BackgroundColor)
        );
        assert_eq!(motion.transitions[0].duration.get(), 125.0);
        assert_eq!(motion.transitions[1].duration.get(), 125.0);
        assert_eq!(motion.transitions[1].easing, MotionEasing::EaseIn);
        assert_eq!(motion.transitions[1].delay.get(), 20.0);
    }

    #[test]
    fn animation_longhand_lists_cycle_against_names() {
        let specified = SpecifiedStyle::new()
            .push(
                StyleProperty::AnimationName,
                StyleValue::AnimationNames(vec![Some("fade".into()), Some("slide".into())]),
            )
            .push(
                StyleProperty::AnimationDuration,
                StyleValue::AnimationDurations(vec![MotionTime::milliseconds(400.0)]),
            )
            .push(
                StyleProperty::AnimationDirection,
                StyleValue::AnimationDirections(vec![
                    MotionDirection::Normal,
                    MotionDirection::Alternate,
                ]),
            );
        let motion = resolve_motion_style(&specified).unwrap();
        assert_eq!(motion.animations.len(), 2);
        assert_eq!(motion.animations[0].name.as_deref(), Some("fade"));
        assert_eq!(motion.animations[1].name.as_deref(), Some("slide"));
        assert_eq!(motion.animations[1].duration.get(), 400.0);
        assert_eq!(motion.animations[1].direction, MotionDirection::Alternate);
    }

    #[test]
    fn invalid_timing_and_non_transitionable_properties_are_rejected() {
        for (property, value) in [
            (
                StyleProperty::TransitionDuration,
                StyleValue::TransitionDurations(vec![MotionTime::milliseconds(-1.0)]),
            ),
            (
                StyleProperty::TransitionTimingFunction,
                StyleValue::TransitionEasings(vec![MotionEasing::CubicBezier([
                    StyleNumber::new(2.0),
                    StyleNumber::new(0.0),
                    StyleNumber::new(1.0),
                    StyleNumber::new(1.0),
                ])]),
            ),
        ] {
            assert_eq!(
                resolve_motion_style(&SpecifiedStyle::new().push(property, value)),
                Err(invalid(property))
            );
        }

        assert_eq!(
            resolve_motion_style(&SpecifiedStyle::new().push(
                StyleProperty::TransitionProperty,
                StyleValue::TransitionProperties(vec![TransitionPropertyValue::Named(
                    "filter".into()
                )]),
            )),
            Err(invalid(StyleProperty::TransitionProperty))
        );
    }
}
