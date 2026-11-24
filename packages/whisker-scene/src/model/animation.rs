use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use glam::{Quat, Vec3, Vec4};

use super::Instance;
use crate::core::{Node, Transform};

/// One named animation of a model: keyframe channels on its nodes.
pub(crate) struct Clip {
    pub(crate) name: String,
    pub(crate) duration: f32,
    channels: Vec<Channel>,
}

impl Clip {
    pub(crate) fn new(name: String, channels: Vec<Channel>) -> Clip {
        let duration = channels
            .iter()
            .filter_map(|channel| channel.times.last().copied())
            .fold(0.0, f32::max);
        Clip {
            name,
            duration,
            channels,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Property {
    Translation,
    Rotation,
    Scale,
    Weights,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Interpolation {
    Linear,
    Step,
    CubicSpline,
}

pub(crate) struct Channel {
    node: usize,
    property: Property,
    interpolation: Interpolation,
    times: Vec<f32>,
    /// Keyframe values back to back; cubic splines store in-tangent, value, out-tangent per key.
    values: Vec<f32>,
    /// Numbers per value.
    width: usize,
}

impl Channel {
    pub(crate) fn new(
        node: usize,
        property: Property,
        interpolation: Interpolation,
        times: Vec<f32>,
        values: Vec<f32>,
    ) -> Channel {
        let per_key = if interpolation == Interpolation::CubicSpline {
            3
        } else {
            1
        };
        let width = values.len() / (times.len() * per_key).max(1);
        Channel {
            node,
            property,
            interpolation,
            times,
            values,
            width,
        }
    }

    fn value(&self, key: usize, part: usize) -> &[f32] {
        let per_key = if self.interpolation == Interpolation::CubicSpline {
            3
        } else {
            1
        };
        let start = (key * per_key + part) * self.width;
        &self.values[start..start + self.width]
    }

    /// The value at `time`, held at the first and last keys outside their range.
    pub(crate) fn sample(&self, time: f32) -> Vec<f32> {
        let middle = usize::from(self.interpolation == Interpolation::CubicSpline);
        let last = self.times.len() - 1;
        if time <= self.times[0] || last == 0 {
            return self.value(0, middle).to_vec();
        }
        if time >= self.times[last] {
            return self.value(last, middle).to_vec();
        }
        let next = self.times.partition_point(|&t| t <= time);
        let previous = next - 1;
        let (t0, t1) = (self.times[previous], self.times[next]);
        let span = t1 - t0;
        let s = if span > 0.0 { (time - t0) / span } else { 0.0 };
        match self.interpolation {
            Interpolation::Step => self.value(previous, 0).to_vec(),
            Interpolation::Linear => {
                let (a, b) = (self.value(previous, 0), self.value(next, 0));
                if self.property == Property::Rotation {
                    let q = quat(a).slerp(quat(b), s);
                    q.to_array().to_vec()
                } else {
                    a.iter().zip(b).map(|(a, b)| a + (b - a) * s).collect()
                }
            }
            Interpolation::CubicSpline => {
                let (s2, s3) = (s * s, s * s * s);
                let (h00, h10, h01, h11) = (
                    2.0 * s3 - 3.0 * s2 + 1.0,
                    s3 - 2.0 * s2 + s,
                    -2.0 * s3 + 3.0 * s2,
                    s3 - s2,
                );
                let (v0, out0) = (self.value(previous, 1), self.value(previous, 2));
                let (in1, v1) = (self.value(next, 0), self.value(next, 1));
                let mut value: Vec<f32> = (0..self.width)
                    .map(|i| h00 * v0[i] + h10 * span * out0[i] + h01 * v1[i] + h11 * span * in1[i])
                    .collect();
                if self.property == Property::Rotation {
                    value = quat(&value).normalize().to_array().to_vec();
                }
                value
            }
        }
    }
}

fn quat(values: &[f32]) -> Quat {
    Quat::from_xyzw(values[0], values[1], values[2], values[3])
}

/// How [`AnimationPlayer::play`] starts a clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationOptions {
    /// Seconds over which the clip fades in while the others fade out.
    pub fade: f32,
    /// Starts over at the end instead of holding the last pose.
    pub looping: bool,
    /// Playback rate; 1 is the authored speed.
    pub speed: f32,
}

impl Default for AnimationOptions {
    fn default() -> Self {
        AnimationOptions {
            fade: 0.25,
            looping: true,
            speed: 1.0,
        }
    }
}

/// Plays a model instance's animation clips, blending them while they cross-fade.
///
/// Call [`update`](Self::update) every frame; `ModelView` does this itself.
#[derive(Clone)]
pub struct AnimationPlayer {
    state: Rc<RefCell<PlayerState>>,
}

struct PlayerState {
    instance: Rc<Instance>,
    actions: Vec<Action>,
    /// Targets an earlier update wrote, restored to their rest values once nothing animates them.
    touched: Vec<(usize, Property)>,
}

struct Action {
    clip: usize,
    time: f32,
    speed: f32,
    looping: bool,
    weight: f32,
    fade: Fade,
}

#[derive(Clone, Copy)]
struct Fade {
    from: f32,
    to: f32,
    duration: f32,
    elapsed: f32,
}

impl Fade {
    fn to(level: f32, duration: f32, current: f32) -> Fade {
        Fade {
            from: current,
            to: level,
            duration,
            elapsed: 0.0,
        }
    }

    fn level(&self) -> f32 {
        if self.duration <= 0.0 || self.elapsed >= self.duration {
            self.to
        } else {
            self.from + (self.to - self.from) * (self.elapsed / self.duration)
        }
    }

    fn faded_out(&self) -> bool {
        self.to == 0.0 && self.level() == 0.0
    }
}

impl AnimationPlayer {
    /// A player for a root returned by [`Model::instantiate`](super::Model::instantiate); `None`
    /// for any other node.
    pub fn new(root: &Node) -> Option<AnimationPlayer> {
        Some(AnimationPlayer {
            state: Rc::new(RefCell::new(PlayerState {
                instance: root.instance()?,
                actions: Vec::new(),
                touched: Vec::new(),
            })),
        })
    }

    /// Fades the clip named `name` in and every other clip out; false if there is no such clip.
    ///
    /// A clip that is already playing keeps its time.
    pub fn play(&self, name: &str, options: AnimationOptions) -> bool {
        let mut state = self.state.borrow_mut();
        let Some(clip) = state.clip(name) else {
            return false;
        };
        for action in &mut state.actions {
            let target = if action.clip == clip { 1.0 } else { 0.0 };
            if action.fade.to != target {
                action.fade = Fade::to(target, options.fade, action.fade.level());
            }
            if action.clip == clip {
                action.speed = options.speed;
                action.looping = options.looping;
            }
        }
        if !state.actions.iter().any(|action| action.clip == clip) {
            state.actions.push(Action {
                clip,
                time: 0.0,
                speed: options.speed,
                looping: options.looping,
                weight: 1.0,
                fade: Fade::to(1.0, options.fade, 0.0),
            });
        }
        true
    }

    /// Fades every clip out over `fade` seconds, returning the model to its rest pose.
    pub fn stop(&self, fade: f32) {
        for action in &mut self.state.borrow_mut().actions {
            action.fade = Fade::to(0.0, fade, action.fade.level());
        }
    }

    /// Moves a playing clip to `time` seconds.
    pub fn seek(&self, name: &str, time: f32) {
        let mut state = self.state.borrow_mut();
        let Some(clip) = state.clip(name) else {
            return;
        };
        for action in &mut state.actions {
            if action.clip == clip {
                action.time = time;
            }
        }
    }

    /// Scales a playing clip's influence, `0.0..=1.0`, on top of its fade.
    pub fn set_weight(&self, name: &str, weight: f32) {
        let mut state = self.state.borrow_mut();
        let Some(clip) = state.clip(name) else {
            return;
        };
        for action in &mut state.actions {
            if action.clip == clip {
                action.weight = weight.clamp(0.0, 1.0);
            }
        }
    }

    /// Whether any clip is playing or still fading out.
    pub fn is_playing(&self) -> bool {
        !self.state.borrow().actions.is_empty()
    }

    /// Advances every clip by `delta` seconds and poses the model.
    pub fn update(&self, delta: f32) {
        let mut state = self.state.borrow_mut();
        let instance = state.instance.clone();
        let clips = &instance.model.data().clips;
        for action in &mut state.actions {
            let duration = clips[action.clip].duration;
            action.time += delta * action.speed;
            if action.looping && duration > 0.0 {
                action.time = action.time.rem_euclid(duration);
            } else {
                action.time = action.time.clamp(0.0, duration);
            }
            action.fade.elapsed += delta;
        }
        state.actions.retain(|action| !action.fade.faded_out());

        let mut blends: HashMap<(usize, Property), Blend> = HashMap::new();
        for action in &state.actions {
            let weight = action.weight * action.fade.level();
            if weight <= 0.0 {
                continue;
            }
            for channel in &clips[action.clip].channels {
                blends
                    .entry((channel.node, channel.property))
                    .or_default()
                    .add(channel.property, &channel.sample(action.time), weight);
            }
        }
        let rest = &instance.model.data().nodes;
        let mut transforms: HashMap<usize, Transform> = HashMap::new();
        for key in std::mem::take(&mut state.touched) {
            blends.entry(key).or_default();
        }
        for (&(node, property), blend) in &blends {
            let Some(definition) = rest.get(node) else {
                continue;
            };
            if property == Property::Weights {
                let weights = blend.finish_weights(&definition.weights);
                for primitive in &instance.primitives[node] {
                    if let Some(deform) = primitive.deform() {
                        *deform.weights.borrow_mut() = weights.clone();
                        primitive.request_frame();
                    }
                }
                continue;
            }
            let transform = transforms
                .entry(node)
                .or_insert_with(|| instance.nodes[node].transform());
            match property {
                Property::Translation => {
                    transform.translation = blend.finish_vec3(definition.transform.translation);
                }
                Property::Scale => {
                    transform.scale = blend.finish_vec3(definition.transform.scale);
                }
                Property::Rotation => {
                    transform.rotation = blend.finish_quat(definition.transform.rotation);
                }
                Property::Weights => {}
            }
        }
        for (node, transform) in transforms {
            if instance.nodes[node].transform() != transform {
                instance.nodes[node].set_transform(transform);
            }
        }
        state.touched = blends
            .into_iter()
            .filter(|(_, blend)| blend.weight > 0.0)
            .map(|(key, _)| key)
            .collect();
    }
}

impl PlayerState {
    fn clip(&self, name: &str) -> Option<usize> {
        self.instance
            .model
            .data()
            .clips
            .iter()
            .position(|clip| clip.name == name)
    }
}

/// A weighted sum of sampled values, completed with the rest value up to a total weight of 1.
#[derive(Default)]
struct Blend {
    weight: f32,
    sum: Vec<f32>,
}

impl Blend {
    fn add(&mut self, property: Property, value: &[f32], weight: f32) {
        if self.sum.len() < value.len() {
            self.sum.resize(value.len(), 0.0);
        }
        let mut value = value.to_vec();
        if property == Property::Rotation
            && self.weight > 0.0
            && Vec4::from_slice(&self.sum).dot(Vec4::from_slice(&value)) < 0.0
        {
            value.iter_mut().for_each(|v| *v = -*v);
        }
        for (sum, v) in self.sum.iter_mut().zip(&value) {
            *sum += v * weight;
        }
        self.weight += weight;
    }

    fn rest_share(&self) -> f32 {
        (1.0 - self.weight).max(0.0)
    }

    fn total(&self) -> f32 {
        self.weight.max(1.0)
    }

    fn finish_vec3(&self, rest: Vec3) -> Vec3 {
        if self.sum.len() < 3 {
            return rest;
        }
        (Vec3::from_slice(&self.sum) + rest * self.rest_share()) / self.total()
    }

    fn finish_quat(&self, rest: Quat) -> Quat {
        if self.sum.len() < 4 {
            return rest;
        }
        let mut rest = Vec4::from(rest.to_array());
        let sum = Vec4::from_slice(&self.sum);
        if sum.dot(rest) < 0.0 {
            rest = -rest;
        }
        let q = sum + rest * self.rest_share();
        q.try_normalize().map_or(Quat::IDENTITY, Quat::from_vec4)
    }

    fn finish_weights(&self, rest: &[f32]) -> Vec<f32> {
        let len = rest.len().max(self.sum.len());
        (0..len)
            .map(|i| {
                let sampled = self.sum.get(i).copied().unwrap_or(0.0);
                let base = rest.get(i).copied().unwrap_or(0.0);
                (sampled + base * self.rest_share()) / self.total()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
