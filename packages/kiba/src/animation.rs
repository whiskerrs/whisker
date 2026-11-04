use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};
use hecs::World;

use crate::model::ModelData;
use crate::{Model, Transform};

/// The most joints one skin can have, which fixes the size of its GPU uniform.
pub(crate) const MAX_JOINTS: usize = 128;

/// Where each node of a [`Model`]'s rig sits relative to its parent, for a game that builds poses itself.
///
/// Fill one from [`Model::rest_pose`] and [`Model::sample`], blend with [`Transform::lerp`], change any
/// node, and add it to the model's entity, which then draws in it instead of its [`Animator`]'s.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pose {
    /// One per node, indexed as [`Model::node`] names them.
    pub nodes: Vec<Transform>,
}

#[derive(Clone, Debug)]
pub(crate) struct RigNode {
    pub(crate) name: Option<String>,
    /// Always earlier in the node list than this node.
    pub(crate) parent: Option<usize>,
    pub(crate) rest: Transform,
}

#[derive(Clone, Debug)]
pub(crate) struct Skin {
    pub(crate) joints: Vec<usize>,
    pub(crate) inverse_binds: Vec<Mat4>,
}

#[derive(Clone, Debug)]
pub(crate) enum Keys {
    Translation(Vec<Vec3>),
    Rotation(Vec<Quat>),
    Scale(Vec<Vec3>),
}

#[derive(Clone, Debug)]
pub(crate) struct Channel {
    pub(crate) node: usize,
    pub(crate) times: Vec<f32>,
    pub(crate) keys: Keys,
    pub(crate) step: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Clip {
    pub(crate) duration: f32,
    pub(crate) channels: Vec<Channel>,
}

/// The moving hierarchy of a model.
#[derive(Clone, Debug, Default)]
pub(crate) struct Rig {
    pub(crate) nodes: Vec<RigNode>,
    pub(crate) skins: Vec<Skin>,
    pub(crate) clips: HashMap<String, Clip>,
}

/// Where a model's nodes are this frame, and the joint matrices of each of its skins.
pub(crate) struct PosedRig {
    pub(crate) nodes: Vec<Mat4>,
    pub(crate) palettes: Vec<Arc<[Mat4]>>,
}

impl Channel {
    /// The surrounding keys of `time` and how far it is from the first to the second.
    fn span(&self, time: f32) -> (usize, usize, f32) {
        let next = self.times.partition_point(|&t| t <= time);
        if next == 0 {
            return (0, 0, 0.0);
        }
        if next == self.times.len() {
            return (next - 1, next - 1, 0.0);
        }
        let (start, end) = (self.times[next - 1], self.times[next]);
        let amount = if self.step {
            0.0
        } else {
            (time - start) / (end - start)
        };
        (next - 1, next, amount)
    }

    fn apply(&self, time: f32, pose: &mut Transform) {
        let (a, b, amount) = self.span(time);
        match &self.keys {
            Keys::Translation(keys) => pose.translation = keys[a].lerp(keys[b], amount),
            Keys::Rotation(keys) => pose.rotation = keys[a].slerp(keys[b], amount).normalize(),
            Keys::Scale(keys) => pose.scale = keys[a].lerp(keys[b], amount),
        }
    }
}

impl Rig {
    pub(crate) fn rest(&self) -> Vec<Transform> {
        self.nodes.iter().map(|node| node.rest).collect()
    }

    /// Sets the nodes `clip` moves to where they are at `time`, leaving the others as they are.
    pub(crate) fn sample_into(&self, clip: &Clip, time: f32, poses: &mut [Transform]) {
        for channel in &clip.channels {
            channel.apply(time, &mut poses[channel.node]);
        }
    }

    fn sample(&self, clip: Option<&Clip>, time: f32) -> Vec<Transform> {
        let mut poses = self.rest();
        if let Some(clip) = clip {
            self.sample_into(clip, time, &mut poses);
        }
        poses
    }

    /// The pose `animator` shows now, mid-fade included.
    pub(crate) fn animated(&self, animator: Option<&Animator>) -> Vec<Transform> {
        let mut poses = self.sample(
            animator.and_then(|a| self.clips.get(&a.clip)),
            animator.map_or(0.0, |a| a.time),
        );
        if let Some((animator, previous)) = animator.and_then(|a| Some((a, a.previous.as_ref()?))) {
            let from = self.sample(self.clips.get(&previous.clip), previous.time);
            let amount = (animator.fade_elapsed / previous.fade_seconds).min(1.0);
            for (pose, from) in poses.iter_mut().zip(&from) {
                *pose = from.lerp(pose, amount);
            }
        }
        poses
    }

    /// Each node's matrix within the model.
    pub(crate) fn node_matrices(&self, poses: &[Transform]) -> Vec<Mat4> {
        let mut nodes: Vec<Mat4> = Vec::with_capacity(self.nodes.len());
        for (i, node) in self.nodes.iter().enumerate() {
            let pose = poses.get(i).unwrap_or(&node.rest);
            let parent = node.parent.map_or(Mat4::IDENTITY, |parent| nodes[parent]);
            nodes.push(parent * pose.matrix());
        }
        nodes
    }

    pub(crate) fn posed(&self, poses: &[Transform]) -> PosedRig {
        let nodes = self.node_matrices(poses);
        let palettes = self
            .skins
            .iter()
            .map(|skin| {
                skin.joints
                    .iter()
                    .zip(&skin.inverse_binds)
                    .map(|(&joint, inverse_bind)| nodes[joint] * *inverse_bind)
                    .collect()
            })
            .collect();
        PosedRig { nodes, palettes }
    }
}

#[derive(Clone, Debug)]
struct PreviousClip {
    clip: String,
    time: f32,
    fade_seconds: f32,
}

/// Plays a [`Model`]'s animation clips, used as a component next to it.
///
/// A model without one rests in its file's pose. [`Model::clip_names`] lists what can play.
#[derive(Clone, Debug)]
pub struct Animator {
    clip: String,
    time: f32,
    previous: Option<PreviousClip>,
    fade_elapsed: f32,
    finished: bool,
    /// How fast the clip plays, with 1 as authored.
    pub speed: f32,
    /// Whether the clip repeats, rather than holding its last pose.
    pub looping: bool,
}

impl Animator {
    /// Starts looping `clip` at normal speed. An unknown name rests in the file's pose.
    pub fn new(clip: impl Into<String>) -> Self {
        Self {
            clip: clip.into(),
            time: 0.0,
            previous: None,
            fade_elapsed: 0.0,
            finished: false,
            speed: 1.0,
            looping: true,
        }
    }

    /// Switches to `clip` from its start, fading from the current pose over `fade_seconds`.
    ///
    /// Naming the clip already playing restarts it, so compare with [`Animator::clip`] first when
    /// calling this every frame.
    pub fn play(&mut self, clip: impl Into<String>, fade_seconds: f32) {
        self.previous = (fade_seconds > 0.0).then(|| PreviousClip {
            clip: std::mem::take(&mut self.clip),
            time: self.time,
            fade_seconds,
        });
        self.clip = clip.into();
        self.time = 0.0;
        self.fade_elapsed = 0.0;
        self.finished = false;
    }

    /// The clip playing now.
    pub fn clip(&self) -> &str {
        &self.clip
    }

    /// Whether a clip that does not loop has reached its end.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Seconds into the clip playing now.
    pub fn time(&self) -> f32 {
        self.time
    }

    /// Moves to `seconds` into the clip playing now, as the next frame shows it.
    pub fn seek(&mut self, seconds: f32) {
        self.time = seconds.max(0.0);
        self.finished = false;
    }

    /// The pose this plays on `model` now, mid-fade included, or `None` before the model loads.
    pub fn pose(&self, model: &Model) -> Option<Pose> {
        let rig = model.data()?.rig.as_ref()?;
        Some(Pose {
            nodes: rig.animated(Some(self)),
        })
    }

    fn advance(&mut self, data: &ModelData, delta_seconds: f32) {
        let Some(rig) = &data.rig else {
            return;
        };
        let step = delta_seconds * self.speed;
        self.time = self.moved(rig, step);
        self.fade_elapsed += delta_seconds;
        let fade_seconds = self.previous.as_ref().map(|previous| previous.fade_seconds);
        if fade_seconds.is_some_and(|fade_seconds| self.fade_elapsed >= fade_seconds) {
            self.previous = None;
        }
    }

    fn moved(&mut self, rig: &Rig, step: f32) -> f32 {
        let Some(clip) = rig.clips.get(&self.clip) else {
            return 0.0;
        };
        let time = self.time + step;
        if self.looping {
            return time.rem_euclid(clip.duration.max(f32::EPSILON));
        }
        self.finished = time >= clip.duration;
        time.clamp(0.0, clip.duration)
    }
}

pub(crate) fn run_animators(world: &mut World, delta_seconds: f32) {
    for (_, (animator, model)) in world.query_mut::<(&mut Animator, &Model)>() {
        if let Some(data) = model.data() {
            animator.advance(data, delta_seconds);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest() -> Transform {
        Transform {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// A parent that moves +X by one per second for two seconds, and a child one up from it.
    fn rig() -> Rig {
        let slide = Clip {
            duration: 2.0,
            channels: vec![Channel {
                node: 0,
                times: vec![0.0, 2.0],
                keys: Keys::Translation(vec![Vec3::ZERO, Vec3::X * 2.0]),
                step: false,
            }],
        };
        let lift = Clip {
            duration: 1.0,
            channels: vec![Channel {
                node: 0,
                times: vec![0.0],
                keys: Keys::Translation(vec![Vec3::Z * 4.0]),
                step: false,
            }],
        };
        Rig {
            nodes: vec![
                RigNode {
                    name: None,
                    parent: None,
                    rest: rest(),
                },
                RigNode {
                    name: None,
                    parent: Some(0),
                    rest: Transform {
                        translation: Vec3::Y,
                        ..rest()
                    },
                },
            ],
            skins: vec![Skin {
                joints: vec![1],
                inverse_binds: vec![Mat4::from_translation(-Vec3::Y)],
            }],
            clips: HashMap::from([("slide".into(), slide), ("lift".into(), lift)]),
        }
    }

    fn data() -> ModelData {
        ModelData {
            parts: Vec::new(),
            rig: Some(rig()),
        }
    }

    fn child_origin(animator: &Animator) -> Vec3 {
        {
            let rig = rig();
            rig.posed(&rig.animated(Some(animator)))
        }
        .nodes[1]
            .transform_point3(Vec3::ZERO)
    }

    #[test]
    fn a_game_samples_clips_into_a_pose_and_reads_where_nodes_are() {
        let mut data = data();
        data.rig.as_mut().unwrap().nodes[1].name = Some("hand".into());
        let model = Model::from_data(data);
        let hand = model.node("hand").unwrap();
        assert_eq!(model.node_parent(hand), Some(0));
        assert_eq!(model.clip_duration("slide"), Some(2.0));

        let mut pose = model.rest_pose().unwrap();
        assert!(model.sample("slide", 1.0, &mut pose));
        assert!(!model.sample("missing", 1.0, &mut pose));
        let at = model
            .node_matrix(&pose, hand)
            .unwrap()
            .transform_point3(Vec3::ZERO);
        assert!(at.abs_diff_eq(Vec3::new(1.0, 1.0, 0.0), 1e-5));

        let mut halfway = model.rest_pose().unwrap();
        halfway.nodes[0] = pose.nodes[0].lerp(&halfway.nodes[0], 0.5);
        assert!(
            halfway.nodes[0]
                .translation
                .abs_diff_eq(Vec3::X * 0.5, 1e-5)
        );

        let mut animator = Animator::new("slide");
        animator.seek(1.0);
        assert_eq!(animator.time(), 1.0);
        assert_eq!(animator.pose(&model), Some(pose));
    }

    #[test]
    fn children_follow_interpolated_keys() {
        let mut animator = Animator::new("slide");
        animator.advance(&data(), 0.5);
        assert!(child_origin(&animator).abs_diff_eq(Vec3::new(0.5, 1.0, 0.0), 1e-5));
    }

    #[test]
    fn joint_matrices_are_identity_at_the_bind_pose_and_move_with_the_joint() {
        let posed = {
            let rig = rig();
            rig.posed(&rig.animated(None))
        };
        assert!(posed.palettes[0][0].abs_diff_eq(Mat4::IDENTITY, 1e-5));

        let mut animator = Animator::new("slide");
        animator.advance(&data(), 1.0);
        let moved = {
            let rig = rig();
            rig.posed(&rig.animated(Some(&animator)))
        }
        .palettes[0][0]
            .transform_point3(Vec3::ZERO);
        assert!(moved.abs_diff_eq(Vec3::X, 1e-5));
    }

    #[test]
    fn looping_clips_wrap_and_others_hold_and_finish() {
        let mut looping = Animator::new("slide");
        looping.advance(&data(), 2.5);
        assert!(child_origin(&looping).abs_diff_eq(Vec3::new(0.5, 1.0, 0.0), 1e-5));
        assert!(!looping.finished());

        let mut once = Animator::new("slide");
        once.looping = false;
        once.advance(&data(), 2.5);
        assert!(child_origin(&once).abs_diff_eq(Vec3::new(2.0, 1.0, 0.0), 1e-5));
        assert!(once.finished());
    }

    #[test]
    fn playing_fades_from_where_the_last_clip_stopped() {
        let mut animator = Animator::new("slide");
        animator.advance(&data(), 2.0 - 1e-4);
        let slid = child_origin(&animator).x;
        animator.play("lift", 1.0);
        assert_eq!(animator.clip(), "lift");
        assert!((child_origin(&animator).x - slid).abs() < 1e-5);

        animator.advance(&data(), 0.5);
        let halfway = child_origin(&animator);
        assert!((halfway.x - slid / 2.0).abs() < 1e-3 && (halfway.z - 2.0).abs() < 1e-3);

        animator.advance(&data(), 0.5);
        assert!(child_origin(&animator).abs_diff_eq(Vec3::new(0.0, 1.0, 4.0), 1e-5));
    }

    #[test]
    fn an_unknown_clip_rests_without_panicking() {
        let mut animator = Animator::new("missing");
        animator.advance(&data(), 1.0);
        assert!(child_origin(&animator).abs_diff_eq(Vec3::Y, 1e-5));
    }
}
