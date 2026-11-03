use std::collections::{HashMap, HashSet};

use glam::{Quat, Vec3};
use hecs::{Entity, Without, World};
use rapier3d::na;
use rapier3d::prelude as rapier;
use rapier3d::prelude::{
    CCDSolver, ColliderBuilder, ColliderSet, DefaultBroadPhase, ImpulseJointSet,
    IntegrationParameters, IslandManager, MultibodyJointSet, NarrowPhase, PhysicsPipeline,
    QueryFilter, QueryPipeline, RigidBodyBuilder, RigidBodySet,
};

use crate::{Model, Transform};

/// Simulates every [`RigidBody`], spawned alone as `(Physics::default(),)`.
///
/// It steps after each `on_fixed_update`, so a body with `Interpolated` draws smoothly. Only one is
/// used if the world has several. Query it from callbacks for [`Physics::raycast`].
pub struct Physics {
    /// Acceleration on dynamic bodies. The default is downward at 9.81.
    pub gravity: Vec3,
    pipeline: PhysicsPipeline,
    parameters: IntegrationParameters,
    islands: IslandManager,
    broad_phase: DefaultBroadPhase,
    narrow_phase: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    ccd: CCDSolver,
    query: QueryPipeline,
    handles: HashMap<Entity, rapier::RigidBodyHandle>,
    owners: HashMap<rapier::RigidBodyHandle, Entity>,
}

impl Default for Physics {
    fn default() -> Self {
        Self {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            pipeline: PhysicsPipeline::new(),
            parameters: IntegrationParameters::default(),
            islands: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            ccd: CCDSolver::new(),
            query: QueryPipeline::new(),
            handles: HashMap::new(),
            owners: HashMap::new(),
        }
    }
}

/// How a [`RigidBody`] moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    /// Moved by forces and collisions. Its `Transform` is written by the simulation.
    Dynamic,
    /// Never moves, such as terrain.
    Static,
    /// Follows its `Transform`, which you write, and pushes what it meets, such as a door.
    Kinematic,
}

/// Makes an entity take part in the simulation, as a component next to a [`Transform`] and a [`Collider`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigidBody {
    /// How it moves.
    pub kind: BodyKind,
    /// Units per second. The simulation updates it on dynamic bodies, and writing it steers them.
    pub velocity: Vec3,
}

impl RigidBody {
    /// A body at rest of the given kind.
    pub fn new(kind: BodyKind) -> Self {
        Self {
            kind,
            velocity: Vec3::ZERO,
        }
    }
}

/// The solid shape of a [`RigidBody`], in the entity's space and without its scale.
#[derive(Clone, Debug)]
pub enum Collider {
    /// A box, by half its size along each axis.
    Cuboid(Vec3),
    /// A ball, by radius.
    Ball(f32),
    /// An upright capsule, by half the height of its middle and its radius.
    Capsule(f32, f32),
    /// Triangles, for static scenery. Build one with [`Collider::from_model`].
    Triangles(Vec<Vec3>, Vec<[u32; 3]>),
}

impl Collider {
    /// Triangles of a loaded model's simplest meshes, or `None` while it loads.
    ///
    /// Scale in the model's parts is kept, and collision meshes do not follow its animation.
    pub fn from_model(model: &Model) -> Option<Self> {
        model.data()?;
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        for part in model.parts() {
            let mesh = part.lods.last().map_or(&part.mesh, |lod| &lod.mesh);
            let data = mesh.data();
            let first = positions.len() as u32;
            positions.extend(
                data.vertices
                    .iter()
                    .map(|vertex| part.transform.transform_point3(vertex.position.into())),
            );
            triangles.extend(data.indices.chunks_exact(3).map(|triangle| {
                [
                    first + triangle[0],
                    first + triangle[1],
                    first + triangle[2],
                ]
            }));
        }
        Some(Self::Triangles(positions, triangles))
    }

    fn build(&self) -> Option<rapier::Collider> {
        let builder = match self {
            Self::Cuboid(half) => ColliderBuilder::cuboid(half.x, half.y, half.z),
            Self::Ball(radius) => ColliderBuilder::ball(*radius),
            Self::Capsule(half_height, radius) => ColliderBuilder::capsule_y(*half_height, *radius),
            Self::Triangles(positions, triangles) => {
                if triangles.is_empty() {
                    return None;
                }
                let points = positions
                    .iter()
                    .map(|&p| na::Point3::new(p.x, p.y, p.z))
                    .collect();
                ColliderBuilder::trimesh(points, triangles.clone())
            }
        };
        Some(builder.build())
    }
}

/// What a [`Physics::raycast`] struck.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    /// The entity holding the collider.
    pub entity: Entity,
    /// How far along the ray, in multiples of the direction's length.
    pub distance: f32,
    /// Where it struck.
    pub point: Vec3,
}

fn to_rapier(value: Vec3) -> rapier::Vector<f32> {
    na::Vector3::new(value.x, value.y, value.z)
}

fn pose(transform: &Transform) -> rapier::Isometry<f32> {
    let [x, y, z, w] = transform.rotation.to_array();
    rapier::Isometry::from_parts(
        rapier::Translation::new(
            transform.translation.x,
            transform.translation.y,
            transform.translation.z,
        ),
        na::UnitQuaternion::new_normalize(na::Quaternion::new(w, x, y, z)),
    )
}

impl Physics {
    /// Finds the nearest collider along a ray, as of the last step, within `longest` direction lengths.
    pub fn raycast(&self, origin: Vec3, direction: Vec3, longest: f32) -> Option<RayHit> {
        let ray = rapier::Ray::new(
            na::Point3::new(origin.x, origin.y, origin.z),
            to_rapier(direction),
        );
        let (collider, distance) = self.query.cast_ray(
            &self.bodies,
            &self.colliders,
            &ray,
            longest,
            true,
            QueryFilter::default(),
        )?;
        let body = self.colliders.get(collider)?.parent()?;
        Some(RayHit {
            entity: *self.owners.get(&body)?,
            distance,
            point: origin + direction * distance,
        })
    }

    fn step(&mut self, delta_seconds: f32) {
        self.parameters.dt = delta_seconds;
        self.pipeline.step(
            &to_rapier(self.gravity),
            &self.parameters,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.ccd,
            Some(&mut self.query),
            &(),
            &(),
        );
    }

    fn remove(&mut self, entity: Entity) {
        if let Some(handle) = self.handles.remove(&entity) {
            self.owners.remove(&handle);
            self.bodies.remove(
                handle,
                &mut self.islands,
                &mut self.colliders,
                &mut self.impulse_joints,
                &mut self.multibody_joints,
                true,
            );
        }
    }
}

/// Marks an entity already known to the simulation.
struct Simulated;

pub(crate) fn run_physics(world: &mut World, delta_seconds: f32) {
    let added: Vec<(Entity, Transform, RigidBody, Collider)> = world
        .query_mut::<Without<(&Transform, &RigidBody, &Collider), &Simulated>>()
        .into_iter()
        .map(|(entity, (transform, body, collider))| (entity, *transform, *body, collider.clone()))
        .collect();
    let bodies: Vec<(Entity, Transform, RigidBody)> = world
        .query_mut::<(&Transform, &RigidBody)>()
        .into_iter()
        .map(|(entity, (transform, body))| (entity, *transform, *body))
        .collect();
    let Some((_, physics)) = world.query_mut::<&mut Physics>().into_iter().next() else {
        return;
    };

    for (entity, transform, body, collider) in &added {
        let builder = match body.kind {
            BodyKind::Dynamic => RigidBodyBuilder::dynamic().linvel(to_rapier(body.velocity)),
            BodyKind::Static => RigidBodyBuilder::fixed(),
            BodyKind::Kinematic => RigidBodyBuilder::kinematic_position_based(),
        };
        let handle = physics
            .bodies
            .insert(builder.position(pose(transform)).build());
        if let Some(built) = collider.build() {
            physics
                .colliders
                .insert_with_parent(built, handle, &mut physics.bodies);
        }
        physics.handles.insert(*entity, handle);
        physics.owners.insert(handle, *entity);
    }

    let present: HashSet<Entity> = bodies.iter().map(|(entity, ..)| *entity).collect();
    let gone: Vec<Entity> = physics
        .handles
        .keys()
        .copied()
        .filter(|entity| !present.contains(entity))
        .collect();
    for entity in gone {
        physics.remove(entity);
    }

    for (entity, transform, body) in &bodies {
        let Some(simulated) = physics
            .handles
            .get(entity)
            .and_then(|&h| physics.bodies.get_mut(h))
        else {
            continue;
        };
        match body.kind {
            BodyKind::Dynamic => simulated.set_linvel(to_rapier(body.velocity), true),
            BodyKind::Kinematic => simulated.set_next_kinematic_position(pose(transform)),
            BodyKind::Static => {}
        }
    }
    physics.step(delta_seconds);

    let moved: Vec<(Entity, Vec3, Quat, Vec3)> = bodies
        .iter()
        .filter(|(_, _, body)| body.kind == BodyKind::Dynamic)
        .filter_map(|(entity, ..)| {
            let simulated = physics.bodies.get(*physics.handles.get(entity)?)?;
            let at = simulated.position();
            let turned = at.rotation.coords;
            let moving = simulated.linvel();
            Some((
                *entity,
                Vec3::new(at.translation.x, at.translation.y, at.translation.z),
                Quat::from_xyzw(turned.x, turned.y, turned.z, turned.w),
                Vec3::new(moving.x, moving.y, moving.z),
            ))
        })
        .collect();
    for (entity, translation, rotation, velocity) in moved {
        if let Ok((transform, body)) =
            world.query_one_mut::<(&mut Transform, &mut RigidBody)>(entity)
        {
            transform.translation = translation;
            transform.rotation = rotation;
            body.velocity = velocity;
        }
    }
    for (entity, ..) in added {
        let _ = world.insert_one(entity, Simulated);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Material, Mesh};

    const STEP: f32 = 1.0 / 60.0;

    fn run(world: &mut World, seconds: f32) {
        for _ in 0..(seconds / STEP) as usize {
            run_physics(world, STEP);
        }
    }

    fn placed(y: f32) -> Transform {
        Transform {
            translation: Vec3::Y * y,
            ..Transform::default()
        }
    }

    fn floor(world: &mut World) -> Entity {
        world.spawn((
            placed(-0.5),
            RigidBody::new(BodyKind::Static),
            Collider::Cuboid(Vec3::new(50.0, 0.5, 50.0)),
        ))
    }

    fn height(world: &World, entity: Entity) -> f32 {
        world.get::<&Transform>(entity).unwrap().translation.y
    }

    #[test]
    fn a_dynamic_ball_falls_and_rests_on_the_floor() {
        let mut world = World::new();
        world.spawn((Physics::default(),));
        floor(&mut world);
        let ball = world.spawn((
            placed(5.0),
            RigidBody::new(BodyKind::Dynamic),
            Collider::Ball(0.5),
        ));
        run(&mut world, 0.2);
        assert!(height(&world, ball) < 5.0);
        assert!(world.get::<&RigidBody>(ball).unwrap().velocity.y < 0.0);
        run(&mut world, 4.0);
        assert!(
            (height(&world, ball) - 0.5).abs() < 0.05,
            "{}",
            height(&world, ball)
        );
    }

    #[test]
    fn writing_a_velocity_steers_a_body() {
        let mut world = World::new();
        world.spawn((Physics {
            gravity: Vec3::ZERO,
            ..Physics::default()
        },));
        let ball = world.spawn((
            placed(0.0),
            RigidBody::new(BodyKind::Dynamic),
            Collider::Ball(0.5),
        ));
        run(&mut world, 0.1);
        world.get::<&mut RigidBody>(ball).unwrap().velocity = Vec3::X * 2.0;
        run(&mut world, 1.0);
        let x = world.get::<&Transform>(ball).unwrap().translation.x;
        assert!((x - 2.0).abs() < 0.1, "{x}");
    }

    #[test]
    fn a_kinematic_body_follows_its_transform_and_pushes() {
        let mut world = World::new();
        world.spawn((Physics {
            gravity: Vec3::ZERO,
            ..Physics::default()
        },));
        let door = world.spawn((
            placed(0.0),
            RigidBody::new(BodyKind::Kinematic),
            Collider::Cuboid(Vec3::splat(0.5)),
        ));
        let ball = world.spawn((
            placed(2.0),
            RigidBody::new(BodyKind::Dynamic),
            Collider::Ball(0.5),
        ));
        for frame in 0..120 {
            world.get::<&mut Transform>(door).unwrap().translation.y = frame as f32 * 0.05;
            run_physics(&mut world, STEP);
        }
        assert!(height(&world, ball) > 2.5, "{}", height(&world, ball));
    }

    #[test]
    fn rays_name_what_they_strike_and_forget_despawned_entities() {
        let mut world = World::new();
        world.spawn((Physics::default(),));
        let floor = floor(&mut world);
        run(&mut world, STEP);
        let ray = |world: &mut World| {
            let (_, physics) = world.query_mut::<&Physics>().into_iter().next().unwrap();
            physics.raycast(Vec3::Y * 3.0, Vec3::NEG_Y, 10.0)
        };
        let hit = ray(&mut world).expect("hits");
        assert_eq!(hit.entity, floor);
        assert!((hit.distance - 3.0).abs() < 1e-3);

        world.despawn(floor).unwrap();
        run(&mut world, STEP);
        assert_eq!(ray(&mut world), None);
    }

    #[test]
    fn a_model_collides_through_its_triangles() {
        let ground = Model::new([(Mesh::plane(20.0), Material::default(), glam::Mat4::IDENTITY)]);
        let mut world = World::new();
        world.spawn((Physics::default(),));
        world.spawn((
            placed(0.0),
            RigidBody::new(BodyKind::Static),
            Collider::from_model(&ground).unwrap(),
        ));
        let ball = world.spawn((
            placed(2.0),
            RigidBody::new(BodyKind::Dynamic),
            Collider::Ball(0.5),
        ));
        run(&mut world, 4.0);
        assert!(
            (height(&world, ball) - 0.5).abs() < 0.1,
            "{}",
            height(&world, ball)
        );
    }
}
