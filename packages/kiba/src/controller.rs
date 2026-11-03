use glam::{EulerRot, Quat, Vec3};
use hecs::World;

use crate::{Input, Key, Time, Transform};

const MAX_PITCH: f32 = 1.55;

/// Turns the camera around a point: drag to orbit, wheel or pinch to zoom.
///
/// Add it next to a [`Camera`](crate::Camera). It overwrites the entity's [`Transform`] each frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    /// The point looked at.
    pub target: Vec3,
    /// Distance from `target`.
    pub distance: f32,
    /// Turn around +Y, in radians.
    pub yaw: f32,
    /// Height angle above the target, in radians.
    pub pitch: f32,
    /// Radians per pixel dragged.
    pub sensitivity: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            distance: 4.0,
            yaw: 0.0,
            pitch: 0.4,
            sensitivity: 0.005,
        }
    }
}

/// Flies the camera: W, A, S, D, Q, and E to move, drag to look, Shift to go faster.
///
/// Add it next to a [`Camera`](crate::Camera). The entity's [`Transform`] gives the start position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlyCamera {
    /// Units per second.
    pub speed: f32,
    /// Radians per pixel dragged.
    pub sensitivity: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self {
            speed: 5.0,
            sensitivity: 0.004,
        }
    }
}

pub(crate) fn run_controllers(world: &mut World, time: &Time, input: &Input) {
    for (_, (transform, orbit)) in world.query_mut::<(&mut Transform, &mut OrbitCamera)>() {
        let drag = input.drag();
        orbit.yaw -= drag.x * orbit.sensitivity;
        orbit.pitch = (orbit.pitch + drag.y * orbit.sensitivity).clamp(-MAX_PITCH, MAX_PITCH);
        let zoom = input.wheel() * 0.001 - input.pinch() * 0.005;
        orbit.distance = (orbit.distance * zoom.exp()).max(0.01);

        let rotation = Quat::from_euler(EulerRot::YXZ, orbit.yaw, -orbit.pitch, 0.0);
        transform.rotation = rotation;
        transform.translation = orbit.target + rotation * Vec3::Z * orbit.distance;
    }
    for (_, (transform, fly)) in world.query_mut::<(&mut Transform, &FlyCamera)>() {
        let drag = input.drag();
        let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        let yaw = yaw - drag.x * fly.sensitivity;
        let pitch = (pitch - drag.y * fly.sensitivity).clamp(-MAX_PITCH, MAX_PITCH);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);

        let axis = |positive, negative| {
            f32::from(u8::from(input.key_down(positive)))
                - f32::from(u8::from(input.key_down(negative)))
        };
        let direction = Vec3::new(
            axis(Key::D, Key::A),
            axis(Key::E, Key::Q),
            axis(Key::S, Key::W),
        );
        let boost = if input.key_down(Key::ShiftLeft) || input.key_down(Key::ShiftRight) {
            3.0
        } else {
            1.0
        };
        transform.translation += transform.rotation
            * direction.normalize_or_zero()
            * fly.speed
            * boost
            * time.delta_seconds();
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::input::InputEvent;

    fn pressed_input(drag: Vec2) -> Input {
        let mut input = Input::default();
        input.apply(InputEvent::PointerDown {
            id: 1,
            position: Vec2::ZERO,
        });
        input.apply(InputEvent::PointerMove {
            id: 1,
            position: drag,
        });
        input
    }

    #[test]
    fn an_orbit_camera_stays_at_its_distance_and_looks_at_the_target() {
        let mut world = World::new();
        let target = Vec3::new(1.0, 2.0, 3.0);
        let entity = world.spawn((
            Transform::default(),
            OrbitCamera {
                target,
                distance: 5.0,
                ..OrbitCamera::default()
            },
        ));

        run_controllers(
            &mut world,
            &Time::for_test(0.1),
            &pressed_input(Vec2::new(40.0, 10.0)),
        );

        let transform = *world.get::<&Transform>(entity).unwrap();
        assert!((transform.translation.distance(target) - 5.0).abs() < 1e-4);
        let forward = transform.rotation * Vec3::NEG_Z;
        let to_target = (target - transform.translation).normalize();
        assert!(forward.abs_diff_eq(to_target, 1e-4));
        assert!(
            transform.translation.y > target.y,
            "the default pitch looks down"
        );
    }

    #[test]
    fn the_wheel_moves_an_orbit_camera_away_and_a_pinch_brings_it_closer() {
        let mut world = World::new();
        let entity = world.spawn((Transform::default(), OrbitCamera::default()));
        let distance = |world: &World| world.get::<&OrbitCamera>(entity).unwrap().distance;
        let start = distance(&world);

        let mut input = Input::default();
        input.apply(InputEvent::Wheel(100.0));
        run_controllers(&mut world, &Time::for_test(0.1), &input);
        assert!(distance(&world) > start);
    }

    #[test]
    fn a_fly_camera_moves_along_its_facing_at_its_speed() {
        let mut world = World::new();
        let entity = world.spawn((Transform::default(), FlyCamera::default()));
        let mut input = Input::default();
        input.apply(InputEvent::KeyDown(Key::W));

        run_controllers(&mut world, &Time::for_test(1.0), &input);

        let position = world.get::<&Transform>(entity).unwrap().translation;
        assert!(position.abs_diff_eq(Vec3::new(0.0, 0.0, -5.0), 1e-4));
    }

    #[test]
    fn looking_up_with_a_fly_camera_stops_before_flipping_over() {
        let mut world = World::new();
        let entity = world.spawn((Transform::default(), FlyCamera::default()));

        run_controllers(
            &mut world,
            &Time::for_test(0.0),
            &pressed_input(Vec2::new(0.0, -100000.0)),
        );

        let up = world.get::<&Transform>(entity).unwrap().rotation * Vec3::Y;
        assert!(up.y > 0.0);
    }
}
