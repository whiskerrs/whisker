use kiba::prelude::*;
use whisker::css::PositionKind;
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[cfg(feature = "showcase")]
mod showcase;
#[cfg(feature = "slime")]
mod slime;
#[cfg(feature = "slime-game")]
mod slime_game;

#[derive(Clone, Copy)]
enum Command {
    Boost,
}

struct Spin {
    speed: f32,
    angle: f32,
}

struct Orbit {
    angle: f32,
}

/// 10,000 instances that all move each frame, reporting cost to the console.
fn stress(setup: &mut GameSetup<Command>) {
    setup.world.spawn((
        Camera::default(),
        Transform::looking_at(Vec3::new(0.0, 60.0, 90.0), Vec3::ZERO, Vec3::Y),
    ));
    setup.world.spawn((Sun {
        shadow_distance: 200.0,
        ..Sun::default()
    },));
    setup.world.spawn((Sky::default(),));
    let cube = Mesh::cube(0.6);
    for x in 0..100 {
        for z in 0..100 {
            setup.world.spawn((
                Transform {
                    translation: Vec3::new(x as f32 - 50.0, 0.0, z as f32 - 50.0),
                    ..Transform::default()
                },
                cube.clone(),
                Material {
                    base_color: Vec3::new(x as f32 / 100.0, 0.4, z as f32 / 100.0),
                    roughness: 0.4,
                    ..Material::default()
                },
            ));
        }
    }
    let mut frames = 0;
    setup.on_update(move |context| {
        let rotation = Quat::from_rotation_y(context.time.elapsed_seconds() as f32);
        for (_, (transform, _)) in context.world.query_mut::<(&mut Transform, &Mesh)>() {
            transform.rotation = rotation;
        }
        frames += 1;
        if frames % 120 == 0 {
            kiba::log(&format!("{:?}", context.stats));
        }
    });
}

/// Flies back and forth across a 4 km square of cooked cells, reporting cost to the console.
fn soak(setup: &mut GameSetup<Command>) {
    let camera = setup
        .world
        .spawn((Camera::default(), Transform::default()));
    setup.world.spawn((Sun::default(),));
    setup.world.spawn((Sky::default(),));
    setup.world.spawn((Fog::default(),));
    let stream = setup.world.spawn((WorldStream::load(
        "/world/index.kiba",
        StreamSettings::default(),
    ),));
    let mut frames = 0u32;
    setup.on_update(move |context| {
        const SPEED: f32 = 20.0;
        const SIDE: f32 = 4096.0;
        const ROW: f32 = 256.0;
        let travelled = context.time.elapsed_seconds() as f32 * SPEED;
        let row = (travelled / SIDE).floor();
        let along = travelled.rem_euclid(SIDE);
        let x = if row as i32 % 2 == 0 { along } else { SIDE - along };
        let z = (row * ROW).rem_euclid(SIDE);
        let eye = Vec3::new(x, 30.0, z);
        let view = Transform::looking_at(eye, eye + Vec3::new(1.0, -0.2, 0.3), Vec3::Y);
        if let Ok(mut transform) = context.world.get::<&mut Transform>(camera) {
            *transform = view;
        }
        frames += 1;
        if frames % 300 == 0 {
            let loaded = context
                .world
                .get::<&WorldStream>(stream)
                .map_or(0, |stream| stream.loaded_bytes());
            kiba::log(&format!("{:?} loaded {loaded}", context.stats));
        }
    });
}

#[whisker::main]
pub fn app() -> Element {
    #[cfg(feature = "showcase")]
    return showcase::app();
    #[cfg(feature = "slime")]
    return slime::app();
    #[cfg(feature = "slime-game")]
    return slime_game::app();
    #[cfg(not(any(feature = "showcase", feature = "slime", feature = "slime-game")))]
    scene()
}

#[cfg_attr(any(feature = "showcase", feature = "slime", feature = "slime-game"), allow(dead_code))]
fn scene() -> Element {
    let turns = signal(0);

    let game = Game::<Command>::new(move |setup| {
        if cfg!(feature = "stress") {
            stress(setup);
            return;
        }
        if cfg!(feature = "soak") {
            soak(setup);
            return;
        }
        setup.world.spawn((
            Camera {
                depth_of_field: Some(DepthOfField {
                    focus_distance: 3.8,
                    blur: 0.006,
                }),
                ..Camera::default()
            },
            Transform::default(),
            OrbitCamera {
                target: Vec3::new(0.0, 0.0, -0.4),
                distance: 3.8,
                ..OrbitCamera::default()
            },
        ));
        setup.world.spawn((Sun::default(),));
        setup.world.spawn((Sky::default(),));
        setup.world.spawn((ProbeVolume::load(whisker_asset::asset!("robot.kiba")),));
        setup.world.spawn((
            Transform::at(Vec3::new(0.0, 0.0, 0.0)),
            ReflectionProbe::new(Vec3::new(8.0, 6.0, 8.0)),
        ));

        let sphere = Mesh::sphere(0.22);
        for column in 0..5 {
            let roughness = 0.1 + 0.2 * column as f32;
            let x = (column as f32 - 2.0) * 0.55;
            for (z, base_color, metallic) in [
                (-1.0, Vec3::new(1.0, 0.77, 0.34), 1.0),
                (-1.6, Vec3::new(0.7, 0.08, 0.08), 0.0),
            ] {
                setup.world.spawn((
                    Transform {
                        translation: Vec3::new(x, 0.0, z),
                        ..Transform::default()
                    },
                    sphere.clone(),
                    Material {
                        base_color,
                        metallic,
                        roughness,
                        ..Material::default()
                    },
                ));
            }
        }

        let side = 256;
        let checker = (0..side * side)
            .flat_map(|i: u32| {
                let shade = if (i % side / 32 + i / side / 32) % 2 == 0 {
                    200
                } else {
                    70
                };
                [shade, shade, shade, 255]
            })
            .collect();
        setup.world.spawn((
            Transform {
                translation: Vec3::new(0.0, -0.85, 0.0),
                ..Transform::default()
            },
            Mesh::plane(8.0),
            Material {
                base_color_texture: Some(Texture::from_rgba8(side, side, checker)),
                roughness: 0.8,
                ..Material::default()
            },
        ));

        setup.world.spawn((
            Spin {
                speed: 1.0,
                angle: 0.0,
            },
            Transform::default(),
            Model::load(whisker_asset::asset!("robot.glb")),
        ));
        setup.world.spawn((
            Transform {
                translation: Vec3::new(1.4, -0.85, 0.4),
                ..Transform::default()
            },
            ParticleEmitter::default(),
        ));

        let lamp = Vec3::new(-1.6, -0.3, 0.6);
        setup.world.spawn((
            Transform {
                scale: Vec3::splat(0.4),
                ..Transform::at(lamp)
            },
            Mesh::sphere(0.22),
            Material {
                emissive: Vec3::new(6.0, 3.0, 1.2),
                ..Material::default()
            },
        ));
        setup.world.spawn((
            Transform::at(lamp),
            PointLight {
                color: Vec3::new(1.0, 0.5, 0.2),
                intensity: 1.5,
                range: 4.0,
            },
        ));
        setup.world.spawn((
            Transform {
                translation: Vec3::new(0.0, -0.35, 0.9),
                rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                ..Transform::default()
            },
            Mesh::plane(1.0),
            Material {
                base_color: Vec3::new(0.6, 0.85, 1.0),
                roughness: 0.05,
                opacity: 0.2,
                alpha_mode: AlphaMode::Blend,
                double_sided: true,
                ..Material::default()
            },
        ));
        let leaf_side = 64;
        let leaf = (0..leaf_side * leaf_side)
            .flat_map(|i: u32| {
                let at = Vec2::new((i % leaf_side) as f32, (i / leaf_side) as f32) / leaf_side as f32;
                let inside = (at - Vec2::splat(0.5)).length() < 0.45
                    && ((at.x * 40.0).sin() + (at.y * 40.0).sin()) > -1.2;
                [70, 140, 40, if inside { 255 } else { 0 }]
            })
            .collect();
        setup.world.spawn((
            Transform {
                translation: Vec3::new(1.5, 0.6, -0.8),
                ..Transform::default()
            },
            Mesh::plane(1.2),
            Material {
                roughness: 0.7,
                alpha_mode: AlphaMode::Mask { cutoff: 0.5 },
                double_sided: true,
                transmission: 0.6,
                base_color_texture: Some(Texture::from_rgba8(leaf_side, leaf_side, leaf)),
                ..Material::default()
            },
        ));
        setup.world.spawn((
            Orbit { angle: 0.0 },
            Transform::default(),
            sphere,
            Material {
                base_color: Vec3::new(0.95, 0.95, 0.95),
                metallic: 1.0,
                roughness: 0.2,
                ..Material::default()
            },
        ));

        setup.bind_action("boost", &[Key::B, Key::Space]);
        setup.on_command(|context, command| match command {
            Command::Boost => {
                for (_, spin) in context.world.query_mut::<&mut Spin>() {
                    spin.speed *= 2.0;
                }
            }
        });
        setup.on_update(move |context| {
            let delta = context.time.delta_seconds();
            if context.input.action_just_pressed("boost") {
                for (_, spin) in context.world.query_mut::<&mut Spin>() {
                    spin.speed *= 2.0;
                }
            }
            for (_, (transform, spin)) in context.world.query_mut::<(&mut Transform, &mut Spin)>() {
                spin.angle += spin.speed * delta;
                transform.rotation = Quat::from_rotation_y(spin.angle);
                let completed = (spin.angle / std::f32::consts::TAU) as i32;
                if completed != turns.get_untracked() {
                    turns.set(completed);
                }
            }
            for (_, (transform, orbit)) in context.world.query_mut::<(&mut Transform, &mut Orbit)>()
            {
                orbit.angle += delta;
                transform.translation = Vec3::new(orbit.angle.cos(), 0.0, orbit.angle.sin()) * 0.8;
            }
        });
    });

    render! {
        View(style: css!(width: percent(100), height: percent(100))) {
            Viewport(game: game, style: css!(width: percent(100), height: percent(100)))
            View(
                style: css!(
                    position: PositionKind::Absolute,
                    left: px(16),
                    top: px(16),
                    padding: px(12),
                    background_color: Color::hex(0xffffff),
                ),
                on_tap: move |_| game.send(Command::Boost),
            ) {
                Text(value: computed(move || format!("Boost: {} turns", turns.get())))
            }
        }
    }
}
