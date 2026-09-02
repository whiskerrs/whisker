use super::*;

impl SurfaceRuntime {
    /// Samples active Rust-owned transitions at one Host frame timestamp.
    ///
    /// Returns `true` while at least one transition still needs another frame.
    pub fn step_motion(&self, timestamp_ms: f64) -> Result<bool, RuntimeBindingError> {
        if !timestamp_ms.is_finite() {
            return Err(RuntimeBindingError::InvalidMotionTimestamp);
        }
        let mut state = self.state.borrow_mut();
        state.take_binding_error()?;
        let mut motion_events = state
            .elements
            .values_mut()
            .flat_map(|entry| entry.pending_motion_events.drain(..))
            .collect::<Vec<_>>();
        let reference_boxes = state
            .elements
            .iter()
            .filter_map(|(element, entry)| {
                let layout = state.surface.node(entry.node?)?.layout()?;
                Some((
                    *element,
                    (layout.border_box.width, layout.border_box.height),
                ))
            })
            .collect::<HashMap<_, _>>();
        let mut completed_layout_transitions = Vec::new();
        for (element, entry) in &mut state.elements {
            for animation in &mut entry.animations {
                let name = animation.declaration.name.clone();
                motion_events.extend(
                    animation
                        .sample(timestamp_ms)
                        .into_iter()
                        .map(|kind| PendingMotionEvent::animation(*element, kind, name.as_deref())),
                );
            }
            for (property, transition) in entry
                .layout_transitions
                .as_deref_mut()
                .into_iter()
                .flat_map(|transitions| transitions.0.iter_mut())
            {
                let sample = transition.sample_progress(timestamp_ms);
                transition.current = interpolate_animated_property(
                    &transition.from,
                    &transition.to,
                    sample.progress,
                );
                if sample.started {
                    motion_events.push(PendingMotionEvent::transition(
                        *element,
                        "transitionstart",
                        *property,
                    ));
                }
                if sample.complete {
                    motion_events.push(PendingMotionEvent::transition(
                        *element,
                        "transitionend",
                        *property,
                    ));
                    completed_layout_transitions.push((*element, *property));
                }
            }
        }
        {
            let state = &mut *state;
            BindingState::apply_keyframe_animation_values(&state.elements, &mut state.surface)?;
        }
        let mut samples = Vec::new();
        for (element, entry) in &mut state.elements {
            let Some(node) = entry.node else {
                continue;
            };
            let opacity = entry.opacity_transition.as_deref_mut().map(|transition| {
                let sample = transition.sample_progress(timestamp_ms);
                transition.current = (transition.from
                    + (transition.to - transition.from) * sample.progress)
                    .clamp(0.0, 1.0);
                (transition.current, sample)
            });
            let colors = entry
                .color_transitions
                .as_deref_mut()
                .into_iter()
                .flat_map(|transitions| transitions.0.iter_mut())
                .map(|(property, transition)| {
                    let sample = transition.sample_progress(timestamp_ms);
                    transition.current =
                        transition.from.interpolate(transition.to, sample.progress);
                    (*property, transition.current, sample)
                })
                .collect::<Vec<_>>();
            let text_color = entry
                .text_color_transition
                .as_deref_mut()
                .map(|transition| {
                    let sample = transition.sample_progress(timestamp_ms);
                    transition.current =
                        transition.from.interpolate(transition.to, sample.progress);
                    sample
                });
            let transform = entry.transform_transition.as_deref_mut().map(|transition| {
                let sample = transition.sample_progress(timestamp_ms);
                let (reference_width, reference_height) =
                    reference_boxes.get(element).copied().unwrap_or_default();
                transition.current = interpolate_transform_style(
                    &transition.from,
                    &transition.to,
                    sample.progress,
                    reference_width,
                    reference_height,
                )
                .unwrap_or_else(|| {
                    if sample.progress < 0.5 {
                        transition.from.clone()
                    } else {
                        transition.to.clone()
                    }
                });
                (transition.current.clone(), sample)
            });
            if opacity.is_some()
                || !colors.is_empty()
                || text_color.is_some()
                || transform.is_some()
            {
                samples.push((*element, node, opacity, colors, text_color, transform));
            }
        }
        let mut completed_text_colors = Vec::new();
        for (element, node, opacity, colors, text_color, transform) in samples {
            if let Some((opacity, sample)) = opacity {
                if sample.started {
                    motion_events.push(PendingMotionEvent::transition(
                        element,
                        "transitionstart",
                        StyleProperty::Opacity,
                    ));
                }
                state.surface.set_opacity(node, opacity)?;
                if sample.complete {
                    motion_events.push(PendingMotionEvent::transition(
                        element,
                        "transitionend",
                        StyleProperty::Opacity,
                    ));
                    state.element_mut(element)?.opacity_transition = None;
                }
            }
            if !colors.is_empty() {
                let mut paint = state
                    .surface
                    .node(node)
                    .and_then(|node| node.box_paint())
                    .cloned()
                    .ok_or(RuntimeBindingError::UnknownElement { element })?;
                for (property, color, sample) in colors {
                    if sample.started {
                        motion_events.push(PendingMotionEvent::transition(
                            element,
                            "transitionstart",
                            property,
                        ));
                    }
                    set_box_color(&mut paint, property, color.into_paint());
                    if sample.complete {
                        motion_events.push(PendingMotionEvent::transition(
                            element,
                            "transitionend",
                            property,
                        ));
                        let entry = state.element_mut(element)?;
                        if let Some(transitions) = entry.color_transitions.as_deref_mut() {
                            transitions.0.remove(&property);
                            if transitions.0.is_empty() {
                                entry.color_transitions = None;
                            }
                        }
                    }
                }
                state.surface.set_box_paint(node, paint)?;
            }
            if text_color.is_some_and(|sample| sample.started) {
                motion_events.push(PendingMotionEvent::transition(
                    element,
                    "transitionstart",
                    StyleProperty::Color,
                ));
            }
            if text_color.is_some_and(|sample| sample.complete) {
                motion_events.push(PendingMotionEvent::transition(
                    element,
                    "transitionend",
                    StyleProperty::Color,
                ));
                completed_text_colors.push(element);
            }
            if let Some((mut transform, sample)) = transform {
                if sample.started {
                    motion_events.push(PendingMotionEvent::transition(
                        element,
                        "transitionstart",
                        StyleProperty::Transform,
                    ));
                }
                if let Some((x, y)) = active_transform_origin(state.element(element)?) {
                    transform.origin_x = x;
                    transform.origin_y = y;
                }
                BindingState::apply_transform_update(node, &transform, &mut state.surface)?;
                if sample.complete {
                    motion_events.push(PendingMotionEvent::transition(
                        element,
                        "transitionend",
                        StyleProperty::Transform,
                    ));
                    state.element_mut(element)?.transform_transition = None;
                }
            }
        }
        let text_updates = BindingState::active_text_color_updates(&state.elements);
        BindingState::apply_text_color_updates(text_updates, &mut state.surface)?;
        for element in completed_text_colors {
            state.element_mut(element)?.text_color_transition = None;
        }
        for (element, property) in completed_layout_transitions {
            let entry = state.element_mut(element)?;
            if let Some(transitions) = entry.layout_transitions.as_deref_mut() {
                transitions.0.remove(&property);
                if transitions.0.is_empty() {
                    entry.layout_transitions = None;
                }
            }
        }
        let active = state.elements.values().any(has_active_transition);
        let mut firings = Vec::new();
        motion_events.sort_by_key(|event| event.element.id());
        if let Some(root) = state.root {
            for event in motion_events {
                let Some(target) = state
                    .elements
                    .get(&event.element)
                    .and_then(|entry| entry.node)
                else {
                    continue;
                };
                let Ok(planned) = state.plan_event(root, target, event.kind) else {
                    continue;
                };
                let body = motion_event_body(&event, timestamp_ms, state.target_value(target));
                firings.extend(planned.into_iter().map(|(current_target, callback)| {
                    (
                        callback,
                        with_current_target(&body, state.target_value(current_target)),
                    )
                }));
            }
        }
        drop(state);
        for (callback, body) in firings {
            callback(body);
        }
        // A lifecycle callback may synchronously update style and start a new
        // timeline. Observe that re-entrant work before deciding whether the
        // Host should schedule another frame.
        Ok(active || self.has_active_motion())
    }

    /// Returns whether this surface has an active Rust-owned transition.
    pub fn has_active_motion(&self) -> bool {
        self.state
            .borrow()
            .elements
            .values()
            .any(has_active_transition)
    }
}

#[derive(Clone)]
pub(super) struct ActiveTransition<Value> {
    pub(super) from: Value,
    pub(super) to: Value,
    pub(super) current: Value,
    pub(super) duration_ms: f32,
    pub(super) delay_ms: f32,
    pub(super) easing: MotionEasing,
    pub(super) start_ms: Option<f64>,
    pub(super) current_progress: f32,
    pub(super) start_emitted: bool,
}

#[derive(Clone)]
pub(super) struct ActiveColorTransitions(
    pub(super) HashMap<StyleProperty, ActiveTransition<RgbaColor>>,
);

#[derive(Clone)]
pub(super) struct ActivePropertyTransitions(
    pub(super) HashMap<StyleProperty, ActiveTransition<AnimatedPropertyValue>>,
);

#[derive(Clone, Debug, PartialEq)]
pub(super) enum AnimatedPropertyValue {
    Number(f32),
    Color(RgbaColor),
    LengthPercentage(ComputedLengthPercentage),
    LengthPercentageAuto(ComputedLengthPercentageAuto),
    Size(ComputedSizeValue),
    FlexBasis(ComputedFlexBasis),
    Transform(ComputedTransformStyle),
    TransformOrigin {
        x: ComputedLengthPercentage,
        y: ComputedLengthPercentage,
    },
}

#[derive(Clone)]
pub(super) struct KeyframePoint {
    pub(super) offset: f32,
    pub(super) value: AnimatedPropertyValue,
    pub(super) easing: Option<MotionEasing>,
}

#[derive(Clone)]
pub(super) struct KeyframePropertyTrack {
    pub(super) property: StyleProperty,
    pub(super) points: Vec<KeyframePoint>,
}

#[derive(Clone)]
pub(super) struct ActiveKeyframeAnimation {
    pub(super) declaration: AnimationValue,
    pub(super) tracks: Vec<KeyframePropertyTrack>,
    pub(super) current_time_ms: f64,
    pub(super) last_timestamp_ms: Option<f64>,
    pub(super) current: HashMap<StyleProperty, AnimatedPropertyValue>,
    pub(super) finished: bool,
    pub(super) sampled_progress: Option<f32>,
    pub(super) start_emitted: bool,
    pub(super) completed_iterations: u64,
    pub(super) end_emitted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingMotionEvent {
    pub(super) element: Element,
    pub(super) kind: &'static str,
    pub(super) animation_type: &'static str,
    pub(super) name: String,
}

impl PendingMotionEvent {
    pub(super) fn transition(
        element: Element,
        kind: &'static str,
        property: StyleProperty,
    ) -> Self {
        Self {
            element,
            kind,
            animation_type: "transition-animation",
            name: property.css_name().to_owned(),
        }
    }

    pub(super) fn animation(element: Element, kind: &'static str, name: Option<&str>) -> Self {
        Self {
            element,
            kind,
            animation_type: "keyframe-animation",
            name: name.unwrap_or_default().to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct TransitionSample {
    pub(super) progress: f32,
    pub(super) started: bool,
    pub(super) complete: bool,
}

pub(super) fn has_active_transition(entry: &BoundElement) -> bool {
    entry.opacity_transition.is_some()
        || entry.text_color_transition.is_some()
        || entry.transform_transition.is_some()
        || entry
            .layout_transitions
            .as_deref()
            .is_some_and(|transitions| !transitions.0.is_empty())
        || entry
            .color_transitions
            .as_deref()
            .is_some_and(|transitions| !transitions.0.is_empty())
        || entry
            .animations
            .iter()
            .any(ActiveKeyframeAnimation::needs_frame)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RgbaColor {
    pub(super) red: f32,
    pub(super) green: f32,
    pub(super) blue: f32,
    pub(super) alpha: f32,
}

#[derive(Clone)]
pub(super) struct MotionSnapshot {
    pub(super) element: Element,
    pub(super) resolved: ResolvedNodeStyle,
    pub(super) initialized: bool,
    pub(super) layout_targets: HashMap<StyleProperty, AnimatedPropertyValue>,
    pub(super) layout_current: HashMap<StyleProperty, AnimatedPropertyValue>,
    pub(super) opacity_target: f32,
    pub(super) opacity_current: f32,
    pub(super) box_paint: BoxPaint,
    pub(super) current_colors: HashMap<StyleProperty, RgbaColor>,
    pub(super) transform_target: ComputedTransformStyle,
    pub(super) transform_current: ComputedTransformStyle,
    pub(super) text_color_target: Option<RgbaColor>,
    pub(super) text_color_current: Option<RgbaColor>,
}

impl<Value> ActiveTransition<Value> {
    pub(super) fn sample_progress(&mut self, timestamp_ms: f64) -> TransitionSample {
        let start_ms = *self.start_ms.get_or_insert(timestamp_ms);
        let elapsed_ms = timestamp_ms - start_ms - f64::from(self.delay_ms);
        let linear = (elapsed_ms / f64::from(self.duration_ms)).clamp(0.0, 1.0) as f32;
        self.current_progress = self.easing.sample(linear);
        let started = elapsed_ms >= 0.0 && !self.start_emitted;
        self.start_emitted |= started;
        TransitionSample {
            progress: self.current_progress,
            started,
            complete: elapsed_ms >= f64::from(self.duration_ms),
        }
    }
}

impl ActiveKeyframeAnimation {
    pub(super) fn needs_frame(&self) -> bool {
        !self.finished && self.declaration.play_state == MotionPlayState::Running
    }

    pub(super) fn sample(&mut self, timestamp_ms: f64) -> Vec<&'static str> {
        if self.declaration.play_state == MotionPlayState::Running {
            if let Some(previous_timestamp_ms) = self.last_timestamp_ms {
                self.current_time_ms += (timestamp_ms - previous_timestamp_ms).max(0.0);
            }
        }
        self.last_timestamp_ms = Some(timestamp_ms);
        self.sample_current_time();

        let mut events = Vec::new();
        let local_ms = self.current_time_ms - f64::from(self.declaration.delay.get());
        if local_ms >= 0.0 && !self.start_emitted {
            self.start_emitted = true;
            events.push("animationstart");
        }
        let duration_ms = f64::from(self.declaration.duration.get());
        if self.start_emitted && duration_ms > 0.0 {
            let iterations = match self.declaration.iteration_count {
                MotionIterationCount::Infinite => f64::INFINITY,
                MotionIterationCount::Count(value) => f64::from(value.get()),
            };
            let completed = (local_ms.max(0.0) / duration_ms).floor() as u64;
            let maximum = if iterations.is_finite() {
                (iterations.ceil() as u64).saturating_sub(1)
            } else {
                u64::MAX
            };
            let completed = completed.min(maximum);
            let event_count = completed
                .saturating_sub(self.completed_iterations)
                .min(4096);
            events.extend(std::iter::repeat_n(
                "animationiteration",
                event_count as usize,
            ));
            self.completed_iterations = completed;
        }
        if self.finished && self.start_emitted && !self.end_emitted {
            self.end_emitted = true;
            events.push("animationend");
        }
        events
    }

    pub(super) fn sample_current_time(&mut self) {
        let local_ms = self.current_time_ms - f64::from(self.declaration.delay.get());
        let duration_ms = f64::from(self.declaration.duration.get());
        let iterations = match self.declaration.iteration_count {
            MotionIterationCount::Infinite => f64::INFINITY,
            MotionIterationCount::Count(value) => f64::from(value.get()),
        };
        let active_duration = if duration_ms == 0.0 || iterations == 0.0 {
            0.0
        } else {
            duration_ms * iterations
        };

        let progress = if local_ms < 0.0 {
            self.finished = false;
            matches!(
                self.declaration.fill_mode,
                MotionFillMode::Backwards | MotionFillMode::Both
            )
            .then(|| directed_iteration_progress(0.0, self.declaration.direction, false))
        } else if local_ms >= active_duration && active_duration.is_finite() {
            self.finished = true;
            matches!(
                self.declaration.fill_mode,
                MotionFillMode::Forwards | MotionFillMode::Both
            )
            .then(|| directed_iteration_progress(iterations, self.declaration.direction, true))
        } else if duration_ms == 0.0 {
            self.finished = true;
            None
        } else {
            self.finished = false;
            Some(directed_iteration_progress(
                local_ms / duration_ms,
                self.declaration.direction,
                false,
            ))
        };

        self.sampled_progress = progress;

        self.current.clear();
        let Some(progress) = progress else {
            return;
        };
        for track in &self.tracks {
            self.current.insert(
                track.property,
                sample_keyframe_track(track, progress, self.declaration.easing, 0.0, 0.0),
            );
        }
    }
}

pub(super) fn directed_iteration_progress(
    overall: f64,
    direction: MotionDirection,
    at_end: bool,
) -> f32 {
    let (iteration, progress) = if at_end && overall > 0.0 {
        let ceiling = overall.ceil();
        let fractional = overall - overall.floor();
        if fractional == 0.0 {
            ((ceiling - 1.0) as u64, 1.0)
        } else {
            (overall.floor() as u64, fractional as f32)
        }
    } else {
        (overall.floor() as u64, (overall - overall.floor()) as f32)
    };
    let reverse = match direction {
        MotionDirection::Normal => false,
        MotionDirection::Reverse => true,
        MotionDirection::Alternate => iteration % 2 == 1,
        MotionDirection::AlternateReverse => iteration % 2 == 0,
    };
    if reverse { 1.0 - progress } else { progress }
}

pub(super) fn sample_keyframe_track(
    track: &KeyframePropertyTrack,
    progress: f32,
    default_easing: MotionEasing,
    reference_width: f32,
    reference_height: f32,
) -> AnimatedPropertyValue {
    let first = track
        .points
        .first()
        .expect("compiled keyframe tracks are non-empty");
    if progress <= first.offset {
        return first.value.clone();
    }
    let last = track
        .points
        .last()
        .expect("compiled keyframe tracks are non-empty");
    if progress >= last.offset {
        return last.value.clone();
    }
    for points in track.points.windows(2) {
        let from = &points[0];
        let to = &points[1];
        if progress <= to.offset {
            let interval = to.offset - from.offset;
            let local = if interval == 0.0 {
                1.0
            } else {
                (progress - from.offset) / interval
            };
            let eased = from.easing.unwrap_or(default_easing).sample(local);
            return interpolate_animated_property_with_reference(
                &from.value,
                &to.value,
                eased,
                reference_width,
                reference_height,
            );
        }
    }
    last.value.clone()
}

pub(super) fn interpolate_animated_property(
    from: &AnimatedPropertyValue,
    to: &AnimatedPropertyValue,
    progress: f32,
) -> AnimatedPropertyValue {
    interpolate_animated_property_with_reference(from, to, progress, 0.0, 0.0)
}

pub(super) fn interpolate_animated_property_with_reference(
    from: &AnimatedPropertyValue,
    to: &AnimatedPropertyValue,
    progress: f32,
    reference_width: f32,
    reference_height: f32,
) -> AnimatedPropertyValue {
    match (from, to) {
        (AnimatedPropertyValue::Number(from), AnimatedPropertyValue::Number(to)) => {
            AnimatedPropertyValue::Number(from + (to - from) * progress)
        }
        (AnimatedPropertyValue::Color(from), AnimatedPropertyValue::Color(to)) => {
            AnimatedPropertyValue::Color(from.interpolate(*to, progress))
        }
        (
            AnimatedPropertyValue::LengthPercentage(from),
            AnimatedPropertyValue::LengthPercentage(to),
        ) => AnimatedPropertyValue::LengthPercentage(interpolate_length_percentage(
            *from, *to, progress,
        )),
        (
            AnimatedPropertyValue::LengthPercentageAuto(from),
            AnimatedPropertyValue::LengthPercentageAuto(to),
        ) => AnimatedPropertyValue::LengthPercentageAuto(match (from, to) {
            (
                ComputedLengthPercentageAuto::Value(from),
                ComputedLengthPercentageAuto::Value(to),
            ) => ComputedLengthPercentageAuto::Value(interpolate_length_percentage(
                *from, *to, progress,
            )),
            _ if progress < 0.5 => *from,
            _ => *to,
        }),
        (AnimatedPropertyValue::Size(from), AnimatedPropertyValue::Size(to)) => {
            AnimatedPropertyValue::Size(match (from, to) {
                (ComputedSizeValue::Value(from), ComputedSizeValue::Value(to)) => {
                    ComputedSizeValue::Value(interpolate_length_percentage(*from, *to, progress))
                }
                _ if progress < 0.5 => *from,
                _ => *to,
            })
        }
        (AnimatedPropertyValue::FlexBasis(from), AnimatedPropertyValue::FlexBasis(to)) => {
            AnimatedPropertyValue::FlexBasis(match (from, to) {
                (ComputedFlexBasis::Value(from), ComputedFlexBasis::Value(to)) => {
                    ComputedFlexBasis::Value(interpolate_length_percentage(*from, *to, progress))
                }
                _ if progress < 0.5 => *from,
                _ => *to,
            })
        }
        (AnimatedPropertyValue::Transform(from), AnimatedPropertyValue::Transform(to)) => {
            interpolate_transform_style(from, to, progress, reference_width, reference_height)
                .map_or_else(
                    || {
                        AnimatedPropertyValue::Transform(if progress < 0.5 {
                            from.clone()
                        } else {
                            to.clone()
                        })
                    },
                    AnimatedPropertyValue::Transform,
                )
        }
        (
            AnimatedPropertyValue::TransformOrigin {
                x: from_x,
                y: from_y,
            },
            AnimatedPropertyValue::TransformOrigin { x: to_x, y: to_y },
        ) => AnimatedPropertyValue::TransformOrigin {
            x: interpolate_length_percentage(*from_x, *to_x, progress),
            y: interpolate_length_percentage(*from_y, *to_y, progress),
        },
        _ if progress < 0.5 => from.clone(),
        _ => to.clone(),
    }
}

pub(super) fn interpolate_length_percentage(
    from: ComputedLengthPercentage,
    to: ComputedLengthPercentage,
    progress: f32,
) -> ComputedLengthPercentage {
    ComputedLengthPercentage::new(
        from.length() + (to.length() - from.length()) * progress,
        from.fraction() + (to.fraction() - from.fraction()) * progress,
    )
}

impl RgbaColor {
    pub(super) fn from_paint(value: &PaintColor) -> Option<Self> {
        match value {
            PaintColor::Srgba {
                red,
                green,
                blue,
                alpha,
            } => Some(Self {
                red: f32::from(*red) / 255.0,
                green: f32::from(*green) / 255.0,
                blue: f32::from(*blue) / 255.0,
                alpha: *alpha,
            }),
            PaintColor::Hsla {
                hue_degrees,
                saturation,
                lightness,
                alpha,
            } => {
                let hue = hue_degrees.rem_euclid(360.0) / 360.0;
                let saturation = saturation / 100.0;
                let lightness = lightness / 100.0;
                let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
                let sector = hue * 6.0;
                let intermediate = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
                let (red, green, blue) = match sector.floor() as u8 {
                    0 => (chroma, intermediate, 0.0),
                    1 => (intermediate, chroma, 0.0),
                    2 => (0.0, chroma, intermediate),
                    3 => (0.0, intermediate, chroma),
                    4 => (intermediate, 0.0, chroma),
                    _ => (chroma, 0.0, intermediate),
                };
                let offset = lightness - chroma * 0.5;
                Some(Self {
                    red: red + offset,
                    green: green + offset,
                    blue: blue + offset,
                    alpha: *alpha,
                })
            }
            PaintColor::Named(name) if name.eq_ignore_ascii_case("transparent") => Some(Self {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                alpha: 0.0,
            }),
            PaintColor::Named(_) => None,
        }
    }

    pub(super) fn interpolate(self, target: Self, progress: f32) -> Self {
        let mix = |from: f32, to: f32| from + (to - from) * progress;
        let alpha = mix(self.alpha, target.alpha).clamp(0.0, 1.0);
        let channel = |from: f32, to: f32| {
            if alpha == 0.0 {
                0.0
            } else {
                (mix(from * self.alpha, to * target.alpha) / alpha).clamp(0.0, 1.0)
            }
        };
        Self {
            red: channel(self.red, target.red),
            green: channel(self.green, target.green),
            blue: channel(self.blue, target.blue),
            alpha,
        }
    }

    pub(super) fn into_paint(self) -> PaintColor {
        let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        PaintColor::Srgba {
            red: channel(self.red),
            green: channel(self.green),
            blue: channel(self.blue),
            alpha: self.alpha.clamp(0.0, 1.0),
        }
    }
}

const BOX_COLOR_PROPERTIES: [StyleProperty; 5] = [
    StyleProperty::BackgroundColor,
    StyleProperty::BorderTopColor,
    StyleProperty::BorderRightColor,
    StyleProperty::BorderBottomColor,
    StyleProperty::BorderLeftColor,
];

pub(super) fn box_color(paint: &BoxPaint, property: StyleProperty) -> &PaintColor {
    match property {
        StyleProperty::BackgroundColor => &paint.background_color,
        StyleProperty::BorderTopColor => &paint.border_colors.top,
        StyleProperty::BorderRightColor => &paint.border_colors.right,
        StyleProperty::BorderBottomColor => &paint.border_colors.bottom,
        StyleProperty::BorderLeftColor => &paint.border_colors.left,
        _ => unreachable!("only box color properties enter the transition table"),
    }
}

pub(super) fn set_box_color(paint: &mut BoxPaint, property: StyleProperty, color: PaintColor) {
    match property {
        StyleProperty::BackgroundColor => paint.background_color = color,
        StyleProperty::BorderTopColor => paint.border_colors.top = color,
        StyleProperty::BorderRightColor => paint.border_colors.right = color,
        StyleProperty::BorderBottomColor => paint.border_colors.bottom = color,
        StyleProperty::BorderLeftColor => paint.border_colors.left = color,
        _ => unreachable!("only box color properties enter the transition table"),
    }
}

const LAYOUT_ANIMATED_PROPERTIES: [StyleProperty; 23] = [
    StyleProperty::Left,
    StyleProperty::Right,
    StyleProperty::Top,
    StyleProperty::Bottom,
    StyleProperty::Width,
    StyleProperty::Height,
    StyleProperty::MinWidth,
    StyleProperty::MinHeight,
    StyleProperty::MaxWidth,
    StyleProperty::MaxHeight,
    StyleProperty::MarginTop,
    StyleProperty::MarginRight,
    StyleProperty::MarginBottom,
    StyleProperty::MarginLeft,
    StyleProperty::PaddingTop,
    StyleProperty::PaddingRight,
    StyleProperty::PaddingBottom,
    StyleProperty::PaddingLeft,
    StyleProperty::BorderTopWidth,
    StyleProperty::BorderRightWidth,
    StyleProperty::BorderBottomWidth,
    StyleProperty::BorderLeftWidth,
    StyleProperty::FlexBasis,
];

pub(super) fn keyframe_property(property: StyleProperty) -> bool {
    LAYOUT_ANIMATED_PROPERTIES.contains(&property)
        || matches!(
            property,
            StyleProperty::FlexGrow
                | StyleProperty::Opacity
                | StyleProperty::BackgroundColor
                | StyleProperty::BorderTopColor
                | StyleProperty::BorderRightColor
                | StyleProperty::BorderBottomColor
                | StyleProperty::BorderLeftColor
                | StyleProperty::Color
                | StyleProperty::Transform
                | StyleProperty::TransformOrigin
        )
}

pub(super) fn animated_property_value(
    resolved: &ResolvedNodeStyle,
    property: StyleProperty,
) -> Option<AnimatedPropertyValue> {
    let computed = resolved.computed();
    let layout = computed.layout();
    match property {
        StyleProperty::Left => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.inset.left,
        )),
        StyleProperty::Right => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.inset.right,
        )),
        StyleProperty::Top => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.inset.top,
        )),
        StyleProperty::Bottom => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.inset.bottom,
        )),
        StyleProperty::Width => Some(AnimatedPropertyValue::Size(layout.size.width)),
        StyleProperty::Height => Some(AnimatedPropertyValue::Size(layout.size.height)),
        StyleProperty::MinWidth => Some(AnimatedPropertyValue::Size(layout.min_size.width)),
        StyleProperty::MinHeight => Some(AnimatedPropertyValue::Size(layout.min_size.height)),
        StyleProperty::MaxWidth => Some(AnimatedPropertyValue::Size(layout.max_size.width)),
        StyleProperty::MaxHeight => Some(AnimatedPropertyValue::Size(layout.max_size.height)),
        StyleProperty::MarginTop => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.margin.top,
        )),
        StyleProperty::MarginRight => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.margin.right,
        )),
        StyleProperty::MarginBottom => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.margin.bottom,
        )),
        StyleProperty::MarginLeft => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.margin.left,
        )),
        StyleProperty::PaddingTop => {
            Some(AnimatedPropertyValue::LengthPercentage(layout.padding.top))
        }
        StyleProperty::PaddingRight => Some(AnimatedPropertyValue::LengthPercentage(
            layout.padding.right,
        )),
        StyleProperty::PaddingBottom => Some(AnimatedPropertyValue::LengthPercentage(
            layout.padding.bottom,
        )),
        StyleProperty::PaddingLeft => {
            Some(AnimatedPropertyValue::LengthPercentage(layout.padding.left))
        }
        StyleProperty::BorderTopWidth => {
            Some(AnimatedPropertyValue::LengthPercentage(layout.border.top))
        }
        StyleProperty::BorderRightWidth => {
            Some(AnimatedPropertyValue::LengthPercentage(layout.border.right))
        }
        StyleProperty::BorderBottomWidth => Some(AnimatedPropertyValue::LengthPercentage(
            layout.border.bottom,
        )),
        StyleProperty::BorderLeftWidth => {
            Some(AnimatedPropertyValue::LengthPercentage(layout.border.left))
        }
        StyleProperty::FlexBasis => Some(AnimatedPropertyValue::FlexBasis(layout.flex_basis)),
        StyleProperty::FlexGrow => Some(AnimatedPropertyValue::Number(layout.flex_grow.get())),
        StyleProperty::Opacity => Some(AnimatedPropertyValue::Number(
            computed.paint().opacity.get(),
        )),
        StyleProperty::Color => {
            RgbaColor::from_paint(&lower_color(computed.inherited_text().color()))
                .map(AnimatedPropertyValue::Color)
        }
        StyleProperty::Transform => Some(AnimatedPropertyValue::Transform(
            computed.paint().transform.clone(),
        )),
        StyleProperty::TransformOrigin => Some(AnimatedPropertyValue::TransformOrigin {
            x: computed.paint().transform.origin_x,
            y: computed.paint().transform.origin_y,
        }),
        property if BOX_COLOR_PROPERTIES.contains(&property) => {
            let paint = lower_paint(computed.paint(), computed.layout()).box_paint;
            RgbaColor::from_paint(box_color(&paint, property)).map(AnimatedPropertyValue::Color)
        }
        _ => None,
    }
}

pub(super) fn set_animated_layout_property(
    layout: &mut ComputedLayoutStyle,
    property: StyleProperty,
    value: &AnimatedPropertyValue,
) -> bool {
    match (property, value) {
        (StyleProperty::Left, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.inset.left = *value;
        }
        (StyleProperty::Right, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.inset.right = *value;
        }
        (StyleProperty::Top, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.inset.top = *value;
        }
        (StyleProperty::Bottom, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.inset.bottom = *value;
        }
        (StyleProperty::Width, AnimatedPropertyValue::Size(value)) => layout.size.width = *value,
        (StyleProperty::Height, AnimatedPropertyValue::Size(value)) => layout.size.height = *value,
        (StyleProperty::MinWidth, AnimatedPropertyValue::Size(value)) => {
            layout.min_size.width = *value;
        }
        (StyleProperty::MinHeight, AnimatedPropertyValue::Size(value)) => {
            layout.min_size.height = *value;
        }
        (StyleProperty::MaxWidth, AnimatedPropertyValue::Size(value)) => {
            layout.max_size.width = *value;
        }
        (StyleProperty::MaxHeight, AnimatedPropertyValue::Size(value)) => {
            layout.max_size.height = *value;
        }
        (StyleProperty::MarginTop, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.margin.top = *value;
        }
        (StyleProperty::MarginRight, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.margin.right = *value;
        }
        (StyleProperty::MarginBottom, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.margin.bottom = *value;
        }
        (StyleProperty::MarginLeft, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.margin.left = *value;
        }
        (StyleProperty::PaddingTop, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.padding.top = *value;
        }
        (StyleProperty::PaddingRight, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.padding.right = *value;
        }
        (StyleProperty::PaddingBottom, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.padding.bottom = *value;
        }
        (StyleProperty::PaddingLeft, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.padding.left = *value;
        }
        (StyleProperty::BorderTopWidth, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.border.top = *value;
        }
        (StyleProperty::BorderRightWidth, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.border.right = *value;
        }
        (StyleProperty::BorderBottomWidth, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.border.bottom = *value;
        }
        (StyleProperty::BorderLeftWidth, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.border.left = *value;
        }
        (StyleProperty::FlexBasis, AnimatedPropertyValue::FlexBasis(value)) => {
            layout.flex_basis = *value;
        }
        (StyleProperty::FlexGrow, AnimatedPropertyValue::Number(value)) => {
            layout.flex_grow = StyleNumber::new(*value);
        }
        _ => return false,
    }
    true
}

pub(super) fn layout_animation_values(
    resolved: &ResolvedNodeStyle,
) -> HashMap<StyleProperty, AnimatedPropertyValue> {
    LAYOUT_ANIMATED_PROPERTIES
        .into_iter()
        .chain([StyleProperty::FlexGrow, StyleProperty::TransformOrigin])
        .filter_map(|property| {
            animated_property_value(resolved, property).map(|value| (property, value))
        })
        .collect()
}

pub(super) fn smoothly_interpolable(
    from: &AnimatedPropertyValue,
    to: &AnimatedPropertyValue,
) -> bool {
    matches!(
        (from, to),
        (
            AnimatedPropertyValue::Number(_),
            AnimatedPropertyValue::Number(_)
        ) | (
            AnimatedPropertyValue::Color(_),
            AnimatedPropertyValue::Color(_)
        ) | (
            AnimatedPropertyValue::LengthPercentage(_),
            AnimatedPropertyValue::LengthPercentage(_)
        ) | (
            AnimatedPropertyValue::LengthPercentageAuto(ComputedLengthPercentageAuto::Value(_)),
            AnimatedPropertyValue::LengthPercentageAuto(ComputedLengthPercentageAuto::Value(_))
        ) | (
            AnimatedPropertyValue::Size(ComputedSizeValue::Value(_)),
            AnimatedPropertyValue::Size(ComputedSizeValue::Value(_))
        ) | (
            AnimatedPropertyValue::FlexBasis(ComputedFlexBasis::Value(_)),
            AnimatedPropertyValue::FlexBasis(ComputedFlexBasis::Value(_))
        ) | (
            AnimatedPropertyValue::TransformOrigin { .. },
            AnimatedPropertyValue::TransformOrigin { .. }
        )
    )
}

impl BindingState {
    pub(super) fn configure_style_motion(
        &mut self,
        snapshots: Vec<MotionSnapshot>,
    ) -> Result<(), RuntimeBindingError> {
        for snapshot in &snapshots {
            self.configure_layout_transitions(
                snapshot.element,
                &snapshot.layout_targets,
                &snapshot.layout_current,
                snapshot.initialized,
            )?;
            if self.element(snapshot.element)?.resolved.as_ref() != Some(&snapshot.resolved) {
                self.configure_keyframe_animations(snapshot.element)?;
            }
            self.configure_opacity_transition(
                snapshot.element,
                snapshot.opacity_target,
                snapshot.opacity_current,
                snapshot.initialized,
            )?;
            self.configure_color_transitions(
                snapshot.element,
                &snapshot.box_paint,
                &snapshot.current_colors,
                snapshot.initialized,
            )?;
            self.configure_transform_transition(
                snapshot.element,
                &snapshot.transform_target,
                &snapshot.transform_current,
                snapshot.initialized,
            )?;
        }
        {
            let state = &mut *self;
            Self::reapply_active_transitions(&state.elements, &mut state.surface)?;
        }
        for snapshot in snapshots {
            if let (Some(target), Some(current)) =
                (snapshot.text_color_target, snapshot.text_color_current)
            {
                self.configure_text_color_transition(
                    snapshot.element,
                    target,
                    current,
                    snapshot.initialized,
                )?;
            }
        }
        let text_updates = Self::active_text_color_updates(&self.elements);
        Self::apply_text_color_updates(text_updates, &mut self.surface)
    }

    pub(super) fn reapply_active_transitions(
        elements: &HashMap<Element, BoundElement>,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        Self::apply_keyframe_animation_values(elements, surface)?;
        for (element, entry) in elements {
            let Some(node) = entry.node else {
                continue;
            };
            if let Some(transition) = entry.opacity_transition.as_deref() {
                surface.set_opacity(node, transition.current)?;
            }
            if let Some(transitions) = entry.color_transitions.as_deref() {
                let mut paint = surface
                    .node(node)
                    .and_then(|node| node.box_paint())
                    .cloned()
                    .ok_or(RuntimeBindingError::UnknownElement { element: *element })?;
                for (property, transition) in &transitions.0 {
                    set_box_color(&mut paint, *property, transition.current.into_paint());
                }
                surface.set_box_paint(node, paint)?;
            }
            if let Some(transition) = entry.transform_transition.as_deref() {
                let mut transform = transition.current.clone();
                if let Some((x, y)) = active_transform_origin(entry) {
                    transform.origin_x = x;
                    transform.origin_y = y;
                }
                Self::apply_transform_update(node, &transform, surface)?;
            }
        }
        Self::reapply_active_text_colors(elements, surface)?;
        Ok(())
    }

    pub(super) fn reapply_active_text_colors(
        elements: &HashMap<Element, BoundElement>,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        Self::apply_text_color_updates(Self::active_text_color_updates(elements), surface)
    }

    pub(super) fn active_text_color_updates(
        elements: &HashMap<Element, BoundElement>,
    ) -> Vec<(Element, NodeId, RgbaColor)> {
        elements
            .iter()
            .filter_map(|(element, entry)| {
                let node = entry.node?;
                if entry.text.is_none() && !entry.kind.receives_text_style() {
                    return None;
                }
                Self::active_text_color(*element, elements).map(|color| (*element, node, color))
            })
            .collect()
    }

    pub(super) fn apply_text_color_updates(
        updates: Vec<(Element, NodeId, RgbaColor)>,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        for (element, node, color) in updates {
            if let Some(mut content) = surface.node(node).and_then(|node| node.text()).cloned() {
                content.paint.foreground = color.into_paint();
                surface.set_text_content(node, content)?;
            } else if let Some(mut style) = surface
                .node(node)
                .and_then(|node| node.text_style())
                .cloned()
            {
                style.paint.foreground = color.into_paint();
                surface.set_text_style_snapshot(node, style)?;
            } else {
                return Err(RuntimeBindingError::UnknownElement { element });
            }
        }
        Ok(())
    }

    pub(super) fn active_text_color(
        element: Element,
        elements: &HashMap<Element, BoundElement>,
    ) -> Option<RgbaColor> {
        let mut current = Some(element);
        while let Some(candidate) = current {
            let entry = elements.get(&candidate)?;
            if let Some(transition) = entry.text_color_transition.as_deref() {
                return Some(transition.current);
            }
            if let Some(color) =
                entry.animations.iter().rev().find_map(|animation| {
                    match animation.current.get(&StyleProperty::Color) {
                        Some(AnimatedPropertyValue::Color(color)) => Some(*color),
                        _ => None,
                    }
                })
            {
                return Some(color);
            }
            if entry
                .specified
                .declarations()
                .any(|declaration| declaration.property() == StyleProperty::Color)
            {
                return None;
            }
            current = entry.parent;
        }
        None
    }

    pub(super) fn active_transform_updates(
        elements: &HashMap<Element, BoundElement>,
        surface: &SurfaceEngine,
    ) -> Vec<(NodeId, ComputedTransformStyle)> {
        elements
            .values()
            .filter_map(|entry| {
                let node = entry.node?;
                let layout = surface.node(node)?.layout()?;
                let mut current = if let Some(transition) = entry.transform_transition.as_deref() {
                    interpolate_transform_style(
                        &transition.from,
                        &transition.to,
                        transition.current_progress,
                        layout.border_box.width,
                        layout.border_box.height,
                    )
                    .unwrap_or_else(|| transition.current.clone())
                } else {
                    entry.animations.iter().rev().find_map(|animation| {
                        let progress = animation.sampled_progress?;
                        let track = animation
                            .tracks
                            .iter()
                            .find(|track| track.property == StyleProperty::Transform)?;
                        match sample_keyframe_track(
                            track,
                            progress,
                            animation.declaration.easing,
                            layout.border_box.width,
                            layout.border_box.height,
                        ) {
                            AnimatedPropertyValue::Transform(transform) => Some(transform),
                            _ => None,
                        }
                    })?
                };
                if let Some((x, y)) = active_transform_origin(entry) {
                    current.origin_x = x;
                    current.origin_y = y;
                }
                Some((node, current))
            })
            .collect()
    }

    pub(super) fn apply_transform_updates(
        updates: Vec<(NodeId, ComputedTransformStyle)>,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        for (node, transform) in updates {
            Self::apply_transform_update(node, &transform, surface)?;
        }
        Ok(())
    }

    pub(super) fn apply_transform_update(
        node: NodeId,
        transform: &ComputedTransformStyle,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        let Some(layout) = surface.node(node).and_then(|node| node.layout()) else {
            return Ok(());
        };
        let transform =
            lower_transform(transform, layout.border_box.width, layout.border_box.height)
                .expect("resolved transform and layout geometry must produce a finite matrix");
        surface.set_transform(node, transform)?;
        Ok(())
    }

    pub(super) fn compile_keyframe_animations(
        &self,
        element: Element,
    ) -> Result<Vec<ActiveKeyframeAnimation>, RuntimeBindingError> {
        let entry = self.element(element)?;
        let base = entry.effective_specified();
        let resolved = entry
            .resolved
            .as_ref()
            .ok_or(RuntimeBindingError::UnknownElement { element })?;
        let parent_inherited = entry
            .parent
            .and_then(|parent| self.elements.get(&parent))
            .and_then(|parent| parent.resolved.as_ref())
            .map(|parent| parent.inherited_for_children().clone());
        let mut animations = Vec::new();
        for declaration in &resolved.computed().motion().animations {
            let Some(keyframes) = declaration.keyframes.as_ref() else {
                continue;
            };
            let properties = keyframes
                .frames
                .iter()
                .flat_map(|frame| frame.style.resolved())
                .map(|declaration| declaration.property())
                .filter(|property| keyframe_property(*property))
                .collect::<HashSet<_>>();
            let mut tracks = Vec::new();
            for property in properties {
                let Some(underlying) = animated_property_value(resolved, property) else {
                    continue;
                };
                let mut points = Vec::new();
                for frame in &keyframes.frames {
                    if !frame
                        .style
                        .resolved()
                        .iter()
                        .any(|declaration| declaration.property() == property)
                    {
                        continue;
                    }
                    let frame_style = base.clone().merge(frame.style.clone());
                    let frame_resolved =
                        resolve_style(&frame_style, parent_inherited.as_ref(), self.environment)?;
                    if let Some(value) = animated_property_value(&frame_resolved, property) {
                        points.push(KeyframePoint {
                            offset: frame.offset.get(),
                            value,
                            easing: frame.easing,
                        });
                    }
                }
                if points.first().is_none_or(|point| point.offset > 0.0) {
                    points.insert(
                        0,
                        KeyframePoint {
                            offset: 0.0,
                            value: underlying.clone(),
                            easing: None,
                        },
                    );
                }
                if points.last().is_none_or(|point| point.offset < 1.0) {
                    points.push(KeyframePoint {
                        offset: 1.0,
                        value: underlying,
                        easing: None,
                    });
                }
                if !points.is_empty() {
                    tracks.push(KeyframePropertyTrack { property, points });
                }
            }
            animations.push(ActiveKeyframeAnimation {
                declaration: declaration.clone(),
                tracks,
                current_time_ms: 0.0,
                last_timestamp_ms: None,
                current: HashMap::new(),
                finished: false,
                sampled_progress: None,
                start_emitted: false,
                completed_iterations: 0,
                end_emitted: false,
            });
        }
        for animation in &mut animations {
            animation.sample_current_time();
        }
        Ok(animations)
    }

    pub(super) fn configure_keyframe_animations(
        &mut self,
        element: Element,
    ) -> Result<(), RuntimeBindingError> {
        let mut animations = self.compile_keyframe_animations(element)?;
        let previous_animations = self.element(element)?.animations.clone();
        for (animation, previous_animation) in animations.iter_mut().zip(previous_animations.iter())
        {
            let same_definition = animation.declaration.name == previous_animation.declaration.name
                && animation.declaration.keyframes == previous_animation.declaration.keyframes;
            if !same_definition {
                continue;
            }
            animation.current_time_ms = previous_animation.current_time_ms;
            animation.start_emitted = previous_animation.start_emitted;
            animation.completed_iterations = previous_animation.completed_iterations;
            animation.end_emitted = previous_animation.end_emitted;
            animation.last_timestamp_ms = if previous_animation.declaration.play_state
                == MotionPlayState::Paused
                && animation.declaration.play_state == MotionPlayState::Running
            {
                None
            } else {
                previous_animation.last_timestamp_ms
            };
            animation.sample_current_time();
        }
        let canceled = previous_animations
            .iter()
            .enumerate()
            .filter(|(index, previous)| {
                if previous.end_emitted {
                    return false;
                }
                animations.get(*index).is_none_or(|animation| {
                    animation.declaration.name != previous.declaration.name
                        || animation.declaration.keyframes != previous.declaration.keyframes
                })
            })
            .map(|(_, previous)| {
                PendingMotionEvent::animation(
                    element,
                    "animationcancel",
                    previous.declaration.name.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        let needs_frame = animations.iter().any(ActiveKeyframeAnimation::needs_frame);
        let entry = self.element_mut(element)?;
        entry.pending_motion_events.extend(canceled);
        entry.animations = animations;
        if needs_frame {
            crate::runtime_wake::wake_runtime();
        }
        Ok(())
    }

    pub(super) fn apply_keyframe_animation_values(
        elements: &HashMap<Element, BoundElement>,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        for (element, entry) in elements {
            if entry.animations.is_empty() && entry.layout_transitions.is_none() {
                continue;
            }
            let Some(node) = entry.node else {
                continue;
            };
            let Some(resolved) = entry.resolved.as_ref() else {
                continue;
            };
            let mut values = HashMap::new();
            let mut tracked = HashSet::new();
            for animation in &entry.animations {
                tracked.extend(animation.tracks.iter().map(|track| track.property));
                for (property, value) in &animation.current {
                    values.insert(*property, value.clone());
                }
            }
            if let Some(transitions) = entry.layout_transitions.as_deref() {
                tracked.extend(transitions.0.keys().copied());
                for (property, transition) in &transitions.0 {
                    values.insert(*property, transition.current.clone());
                }
            }
            let computed = resolved.computed();
            let mut layout = computed.layout().clone();
            let mut layout_changed = false;
            for (property, value) in &values {
                layout_changed |= set_animated_layout_property(&mut layout, *property, value);
            }
            if layout_changed {
                surface.update_layout_style(node, layout.clone())?;
            }
            if tracked.contains(&StyleProperty::Opacity) {
                let opacity = match values.get(&StyleProperty::Opacity) {
                    Some(AnimatedPropertyValue::Number(value)) => *value,
                    _ => computed.paint().opacity.get(),
                };
                surface.set_opacity(node, opacity.clamp(0.0, 1.0))?;
            }
            if layout_changed
                || tracked
                    .iter()
                    .any(|property| BOX_COLOR_PROPERTIES.contains(property))
            {
                let mut paint = lower_paint(computed.paint(), &layout).box_paint;
                for property in BOX_COLOR_PROPERTIES {
                    if let Some(AnimatedPropertyValue::Color(value)) = values.get(&property) {
                        set_box_color(&mut paint, property, value.into_paint());
                    }
                }
                surface.set_box_paint(node, paint)?;
            }
            if tracked.contains(&StyleProperty::Transform)
                || tracked.contains(&StyleProperty::TransformOrigin)
            {
                let mut transform = match values.get(&StyleProperty::Transform) {
                    Some(AnimatedPropertyValue::Transform(value)) => value.clone(),
                    _ => computed.paint().transform.clone(),
                };
                if let Some(AnimatedPropertyValue::TransformOrigin { x, y }) =
                    values.get(&StyleProperty::TransformOrigin)
                {
                    transform.origin_x = *x;
                    transform.origin_y = *y;
                }
                Self::apply_transform_update(node, &transform, surface)?;
            }
            let _ = element;
        }
        let text_updates = Self::active_text_color_updates(elements);
        Self::apply_text_color_updates(text_updates, surface)
    }

    pub(super) fn configure_layout_transitions(
        &mut self,
        element: Element,
        previous_targets: &HashMap<StyleProperty, AnimatedPropertyValue>,
        previous_current: &HashMap<StyleProperty, AnimatedPropertyValue>,
        was_initialized: bool,
    ) -> Result<(), RuntimeBindingError> {
        let (targets, transitions) = {
            let entry = self.element(element)?;
            let resolved = entry
                .resolved
                .as_ref()
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            (
                layout_animation_values(resolved),
                resolved.computed().motion().transitions.clone(),
            )
        };
        let entry = self.element_mut(element)?;
        if !was_initialized {
            entry.layout_transitions = None;
            return Ok(());
        }

        let mut started = false;
        for (property, target) in targets {
            let Some(previous_target) = previous_targets.get(&property) else {
                continue;
            };
            if previous_target == &target {
                continue;
            }
            let canceled = entry
                .layout_transitions
                .as_deref_mut()
                .is_some_and(|active| active.0.remove(&property).is_some());
            if canceled {
                entry
                    .pending_motion_events
                    .push_back(PendingMotionEvent::transition(
                        element,
                        "transitioncancel",
                        property,
                    ));
            }
            let transition = transitions.iter().rev().find(|transition| {
                matches!(transition.property, ComputedTransitionProperty::All)
                    || transition.property == ComputedTransitionProperty::Property(property)
            });
            let Some(transition) = transition.filter(|value| value.duration.get() > 0.0) else {
                continue;
            };
            let from = previous_current
                .get(&property)
                .unwrap_or(previous_target)
                .clone();
            if !smoothly_interpolable(&from, &target) {
                continue;
            }
            entry
                .layout_transitions
                .get_or_insert_with(|| Box::new(ActivePropertyTransitions(HashMap::new())))
                .0
                .insert(
                    property,
                    ActiveTransition {
                        from: from.clone(),
                        to: target,
                        current: from,
                        duration_ms: transition.duration.get(),
                        delay_ms: transition.delay.get(),
                        easing: transition.easing,
                        start_ms: None,
                        current_progress: 0.0,
                        start_emitted: false,
                    },
                );
            started = true;
        }
        if entry
            .layout_transitions
            .as_deref()
            .is_some_and(|transitions| transitions.0.is_empty())
        {
            entry.layout_transitions = None;
        }
        if started {
            crate::runtime_wake::wake_runtime();
        }
        Ok(())
    }

    pub(super) fn configure_opacity_transition(
        &mut self,
        element: Element,
        previous_target: f32,
        previous_current: f32,
        was_initialized: bool,
    ) -> Result<(), RuntimeBindingError> {
        let (node, target, transition) = {
            let entry = self.element(element)?;
            let node = entry
                .node
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let resolved = entry
                .resolved
                .as_ref()
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let target = resolved.computed().paint().opacity.get();
            let transition = resolved
                .computed()
                .motion()
                .transitions
                .iter()
                .rev()
                .find(|transition| {
                    matches!(
                        transition.property,
                        ComputedTransitionProperty::All
                            | ComputedTransitionProperty::Property(StyleProperty::Opacity)
                    )
                })
                .copied();
            (node, target, transition)
        };

        let entry = self.element_mut(element)?;
        entry.style_initialized = true;
        if !was_initialized {
            entry.opacity_transition = None;
            self.surface.set_opacity(node, target)?;
            return Ok(());
        }
        if previous_target.to_bits() == target.to_bits() {
            return Ok(());
        }
        if entry.opacity_transition.take().is_some() {
            entry
                .pending_motion_events
                .push_back(PendingMotionEvent::transition(
                    element,
                    "transitioncancel",
                    StyleProperty::Opacity,
                ));
        }
        let Some(transition) = transition.filter(|value| value.duration.get() > 0.0) else {
            self.surface.set_opacity(node, target)?;
            return Ok(());
        };
        entry.opacity_transition = Some(Box::new(ActiveTransition {
            from: previous_current,
            to: target,
            current: previous_current,
            duration_ms: transition.duration.get(),
            delay_ms: transition.delay.get(),
            easing: transition.easing,
            start_ms: None,
            current_progress: 0.0,
            start_emitted: false,
        }));
        self.surface.set_opacity(node, previous_current)?;
        crate::runtime_wake::wake_runtime();
        Ok(())
    }

    pub(super) fn configure_color_transitions(
        &mut self,
        element: Element,
        previous: &BoxPaint,
        previous_current: &HashMap<StyleProperty, RgbaColor>,
        was_initialized: bool,
    ) -> Result<(), RuntimeBindingError> {
        let (node, target, transitions) = {
            let entry = self.element(element)?;
            let node = entry
                .node
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let resolved = entry
                .resolved
                .as_ref()
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let computed = resolved.computed();
            (
                node,
                lower_paint(computed.paint(), computed.layout()).box_paint,
                computed.motion().transitions.clone(),
            )
        };

        let entry = self.element_mut(element)?;
        if !was_initialized {
            entry.color_transitions = None;
            self.surface.set_box_paint(node, target)?;
            return Ok(());
        }
        let mut started = false;
        for property in BOX_COLOR_PROPERTIES {
            let previous_target = box_color(previous, property);
            let target_color = box_color(&target, property);
            if previous_target == target_color {
                continue;
            }
            let canceled = entry
                .color_transitions
                .as_deref_mut()
                .is_some_and(|active| active.0.remove(&property).is_some());
            if canceled {
                entry
                    .pending_motion_events
                    .push_back(PendingMotionEvent::transition(
                        element,
                        "transitioncancel",
                        property,
                    ));
            }
            let transition = transitions.iter().rev().find(|transition| {
                matches!(transition.property, ComputedTransitionProperty::All)
                    || transition.property == ComputedTransitionProperty::Property(property)
            });
            let Some(transition) = transition.filter(|value| value.duration.get() > 0.0) else {
                continue;
            };
            let from = previous_current
                .get(&property)
                .copied()
                .or_else(|| RgbaColor::from_paint(previous_target));
            let to = RgbaColor::from_paint(target_color);
            let (Some(from), Some(to)) = (from, to) else {
                continue;
            };
            entry
                .color_transitions
                .get_or_insert_with(|| Box::new(ActiveColorTransitions(HashMap::new())))
                .0
                .insert(
                    property,
                    ActiveTransition {
                        from,
                        to,
                        current: from,
                        duration_ms: transition.duration.get(),
                        delay_ms: transition.delay.get(),
                        easing: transition.easing,
                        start_ms: None,
                        current_progress: 0.0,
                        start_emitted: false,
                    },
                );
            started = true;
        }

        let mut current = target;
        if entry
            .color_transitions
            .as_deref()
            .is_some_and(|transitions| transitions.0.is_empty())
        {
            entry.color_transitions = None;
        }
        if let Some(transitions) = entry.color_transitions.as_deref() {
            for (property, transition) in &transitions.0 {
                set_box_color(&mut current, *property, transition.current.into_paint());
            }
        }
        self.surface.set_box_paint(node, current)?;
        if started {
            crate::runtime_wake::wake_runtime();
        }
        Ok(())
    }

    pub(super) fn configure_text_color_transition(
        &mut self,
        element: Element,
        previous_target: RgbaColor,
        previous_current: RgbaColor,
        was_initialized: bool,
    ) -> Result<(), RuntimeBindingError> {
        let (target, transition) = {
            let entry = self.element(element)?;
            let resolved = entry
                .resolved
                .as_ref()
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let target =
                RgbaColor::from_paint(&lower_color(resolved.computed().inherited_text().color()));
            let transition = resolved
                .computed()
                .motion()
                .transitions
                .iter()
                .rev()
                .find(|transition| {
                    matches!(transition.property, ComputedTransitionProperty::All)
                        || transition.property
                            == ComputedTransitionProperty::Property(StyleProperty::Color)
                })
                .copied();
            (target, transition)
        };

        let entry = self.element_mut(element)?;
        if !was_initialized || Some(previous_target) == target {
            return Ok(());
        }
        if entry.text_color_transition.take().is_some() {
            entry
                .pending_motion_events
                .push_back(PendingMotionEvent::transition(
                    element,
                    "transitioncancel",
                    StyleProperty::Color,
                ));
        }
        let Some((target, transition)) =
            target.zip(transition.filter(|transition| transition.duration.get() > 0.0))
        else {
            return Ok(());
        };
        entry.text_color_transition = Some(Box::new(ActiveTransition {
            from: previous_current,
            to: target,
            current: previous_current,
            duration_ms: transition.duration.get(),
            delay_ms: transition.delay.get(),
            easing: transition.easing,
            start_ms: None,
            current_progress: 0.0,
            start_emitted: false,
        }));
        crate::runtime_wake::wake_runtime();
        Ok(())
    }

    pub(super) fn configure_transform_transition(
        &mut self,
        element: Element,
        previous_target: &ComputedTransformStyle,
        previous_current: &ComputedTransformStyle,
        was_initialized: bool,
    ) -> Result<(), RuntimeBindingError> {
        let (node, target, transition) = {
            let entry = self.element(element)?;
            let node = entry
                .node
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let resolved = entry
                .resolved
                .as_ref()
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let target = resolved.computed().paint().transform.clone();
            let transition = resolved
                .computed()
                .motion()
                .transitions
                .iter()
                .rev()
                .find(|transition| {
                    matches!(transition.property, ComputedTransitionProperty::All)
                        || transition.property
                            == ComputedTransitionProperty::Property(StyleProperty::Transform)
                })
                .copied();
            (node, target, transition)
        };

        let entry = self.element_mut(element)?;
        if !was_initialized {
            entry.transform_transition = None;
            return Ok(());
        }
        if previous_target.functions == target.functions {
            let current = entry.transform_transition.as_deref_mut().map(|active| {
                let from_functions = active.from.functions.clone();
                let to_functions = active.to.functions.clone();
                let current_functions = active.current.functions.clone();
                active.from = target.clone();
                active.from.functions = from_functions;
                active.to = target.clone();
                active.to.functions = to_functions;
                active.current = target;
                active.current.functions = current_functions;
                active.current.clone()
            });
            if let Some(current) = current {
                Self::apply_transform_update(node, &current, &mut self.surface)?;
            }
            return Ok(());
        }
        if entry.transform_transition.take().is_some() {
            entry
                .pending_motion_events
                .push_back(PendingMotionEvent::transition(
                    element,
                    "transitioncancel",
                    StyleProperty::Transform,
                ));
        }
        let Some(transition) = transition.filter(|transition| transition.duration.get() > 0.0)
        else {
            return Ok(());
        };
        let mut from = target.clone();
        from.functions = previous_current.functions.clone();
        entry.transform_transition = Some(Box::new(ActiveTransition {
            from: from.clone(),
            to: target,
            current: from,
            duration_ms: transition.duration.get(),
            delay_ms: transition.delay.get(),
            easing: transition.easing,
            start_ms: None,
            current_progress: 0.0,
            start_emitted: false,
        }));
        let current = entry
            .transform_transition
            .as_deref()
            .expect("transition was installed above")
            .current
            .clone();
        Self::apply_transform_update(node, &current, &mut self.surface)?;
        crate::runtime_wake::wake_runtime();
        Ok(())
    }
}
