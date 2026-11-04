//! One world with two places along a river: a factory at night and a forest by day.

use kiba::prelude::*;
use whisker::css::PositionKind;
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_asset::asset;

#[derive(Clone, Copy)]
enum Command {
    Factory,
    Forest,
}

/// Each model's bounds as Meshy made it, from `showcase/bounds.py`.
const BOUNDS: &[(&str, [f32; 3], [f32; 3])] = &include!("../showcase/bounds.rs");

const FOREST_Z: f32 = -150.0;
const RIVER_X: f32 = 14.0;
const WEST_BANK: f32 = RIVER_X - 3.0;
const EAST_BANK: f32 = RIVER_X + 11.0;

fn model(name: &str) -> Model {
    match name {
        "warehouse" => Model::load(asset!("showcase/warehouse.kiba")),
        "smokestack" => Model::load(asset!("showcase/smokestack.kiba")),
        "storage_tank" => Model::load(asset!("showcase/storage_tank.kiba")),
        "gas_sphere" => Model::load(asset!("showcase/gas_sphere.kiba")),
        "pipe_bridge" => Model::load(asset!("showcase/pipe_bridge.kiba")),
        "cable_spool" => Model::load(asset!("showcase/cable_spool.kiba")),
        "barrel" => Model::load(asset!("showcase/barrel.kiba")),
        "crate" => Model::load(asset!("showcase/crate.kiba")),
        "ammo_crate" => Model::load(asset!("showcase/ammo_crate.kiba")),
        "utility_pole" => Model::load(asset!("showcase/utility_pole.kiba")),
        "barrier" => Model::load(asset!("showcase/barrier.kiba")),
        "oak" => Model::load(asset!("showcase/oak.kiba")),
        "spruce" => Model::load(asset!("showcase/spruce.kiba")),
        "boulder" => Model::load(asset!("showcase/boulder.kiba")),
        "river_rocks" => Model::load(asset!("showcase/river_rocks.kiba")),
        "fern" => Model::load(asset!("showcase/fern.kiba")),
        "grass" => Model::load(asset!("showcase/grass.kiba")),
        "mountain" => Model::load(asset!("showcase/mountain.kiba")),
        _ => unreachable!("{name} is not a showcase model"),
    }
}

/// Places models standing on the ground, scaled to a height, sharing one load per name.
struct Placer {
    loaded: Vec<(&'static str, Model)>,
}

impl Placer {
    fn place(&mut self, world: &mut World, name: &'static str, at: Vec3, height: f32, yaw: f32) {
        let (_, low, high) = BOUNDS
            .iter()
            .find(|(n, _, _)| *n == name)
            .unwrap_or_else(|| panic!("no bounds for {name}"));
        let scale = height / (high[1] - low[1]);
        let center = Vec3::new(low[0] + high[0], 0.0, low[2] + high[2]) / 2.0 * scale;
        let rotation = Quat::from_rotation_y(yaw);
        let lift = Vec3::Y * -low[1] * scale;
        let model = match self.loaded.iter().find(|(n, _)| *n == name) {
            Some((_, model)) => model.clone(),
            None => {
                let model = model(name);
                self.loaded.push((name, model.clone()));
                model
            }
        };
        world.spawn((
            Transform {
                translation: at + lift - rotation * center,
                rotation,
                scale: Vec3::splat(scale),
            },
            model,
        ));
    }
}

/// Smooth noise in 0..1, repeating every `period` cells.
fn noise(x: f32, y: f32, period: i32) -> f32 {
    let hash = |x: i32, y: i32| {
        let (x, y) = (x.rem_euclid(period) as u32, y.rem_euclid(period) as u32);
        let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263);
        h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        (h ^ (h >> 16)) as f32 / u32::MAX as f32
    };
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let top = hash(ix, iy) + (hash(ix + 1, iy) - hash(ix, iy)) * sx;
    let bottom = hash(ix, iy + 1) + (hash(ix + 1, iy + 1) - hash(ix, iy + 1)) * sx;
    top + (bottom - top) * sy
}

fn fractal(u: f32, v: f32, cells: i32) -> f32 {
    (0..4)
        .map(|octave| {
            let f = (1 << octave) as f32;
            noise(u * cells as f32 * f, v * cells as f32 * f, cells << octave) / f
        })
        .sum::<f32>()
        / 1.875
}

/// A tiling ground: sRGB color, and roughness in green, from a function of position and noise.
fn ground_textures(
    side: u32,
    shade: impl Fn(f32, f32, f32) -> ([f32; 3], f32),
) -> (Texture, Texture) {
    let mut color = Vec::with_capacity((side * side * 4) as usize);
    let mut rough = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let (u, v) = (x as f32 / side as f32, y as f32 / side as f32);
            let (rgb, roughness) = shade(u, v, fractal(u, v, 8));
            color.extend(rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8));
            color.push(255);
            rough.extend([0, (roughness.clamp(0.0, 1.0) * 255.0) as u8, 0, 255]);
        }
    }
    (
        Texture::from_rgba8(side, side, color),
        Texture::from_rgba8(side, side, rough),
    )
}

/// A flat rectangle at height `y` between two corners, with its texture repeating every `tile` units.
fn ground(
    world: &mut World,
    low: (f32, f32),
    high: (f32, f32),
    y: f32,
    tile: f32,
    material: Material,
) {
    let corner = |x: f32, z: f32| Vertex {
        position: [x, y, z],
        normal: [0.0, 1.0, 0.0],
        color: [1.0; 3],
        uv: [(x - low.0) / tile, (z - low.1) / tile],
    };
    let mesh = Mesh::new(
        vec![
            corner(low.0, low.1),
            corner(high.0, low.1),
            corner(high.0, high.1),
            corner(low.0, high.1),
        ],
        vec![0, 2, 1, 0, 3, 2],
    );
    world.spawn((Transform::default(), mesh, material));
}

fn textured((color, rough): (Texture, Texture)) -> Material {
    Material {
        roughness: 1.0,
        base_color_texture: Some(color),
        metallic_roughness_texture: Some(rough),
        ..Material::default()
    }
}

fn block(world: &mut World, center: Vec3, size: Vec3, material: Material) {
    world.spawn((
        Transform {
            scale: size,
            ..Transform::at(center)
        },
        Mesh::cube(1.0),
        material,
    ));
}

fn factory(world: &mut World, placer: &mut Placer) {
    let (color, rough) = ground_textures(256, |u, v, n| {
        let crack = (fractal(u * 3.0, v * 3.0, 8) - 0.5).abs() < 0.015;
        let wet = fractal(v + 0.3, u + 0.7, 4) > 0.55;
        let tone = 0.16 + n * 0.12 - if crack { 0.08 } else { 0.0 };
        let rgb = [tone, tone * 0.97, tone * 0.92];
        (rgb, if wet { 0.04 } else { 0.55 + n * 0.3 })
    });
    ground(
        world,
        (-40.0, -60.0),
        (WEST_BANK, 20.0),
        0.0,
        5.0,
        textured((color, rough)),
    );
    let dark_steel = Material {
        base_color: Vec3::splat(0.35),
        metallic: 1.0,
        roughness: 0.35,
        ..Material::default()
    };
    for x in [-1.2, 0.2] {
        block(
            world,
            Vec3::new(x, 0.04, -20.0),
            Vec3::new(0.08, 0.08, 60.0),
            dark_steel.clone(),
        );
    }
    let yellow = Material {
        base_color: Vec3::new(0.8, 0.6, 0.05),
        roughness: 0.5,
        metallic: 0.6,
        ..Material::default()
    };
    block(
        world,
        Vec3::new(RIVER_X - 3.4, 1.0, -20.0),
        Vec3::new(0.08, 0.08, 60.0),
        yellow.clone(),
    );
    block(
        world,
        Vec3::new(RIVER_X - 3.4, 0.55, -20.0),
        Vec3::new(0.06, 0.06, 60.0),
        yellow.clone(),
    );
    for i in 0..30 {
        block(
            world,
            Vec3::new(RIVER_X - 3.4, 0.5, 9.0 - i as f32 * 2.0),
            Vec3::new(0.08, 1.0, 0.08),
            yellow.clone(),
        );
    }
    let concrete = Material {
        base_color: Vec3::splat(0.45),
        roughness: 0.9,
        ..Material::default()
    };
    block(
        world,
        Vec3::new(WEST_BANK + 0.2, -0.6, -20.0),
        Vec3::new(0.4, 1.3, 80.0),
        concrete,
    );

    for (i, z) in [-2.0, -27.0].into_iter().enumerate() {
        placer.place(
            world,
            "warehouse",
            Vec3::new(-11.0, 0.0, z),
            7.0 + i as f32,
            std::f32::consts::FRAC_PI_2,
        );
        world.spawn((
            Transform::at(Vec3::new(-5.2, 3.2, z + 2.5)),
            PointLight {
                color: Vec3::new(1.0, 0.7, 0.4),
                intensity: 6.0,
                range: 10.0,
            },
        ));
    }
    for z in [-4.0, -16.0, -28.0, -40.0, -52.0] {
        placer.place(
            world,
            "utility_pole",
            Vec3::new(RIVER_X - 4.4, 0.0, z),
            8.0,
            -std::f32::consts::FRAC_PI_2,
        );
        world.spawn((
            Transform::at(Vec3::new(RIVER_X - 5.4, 6.8, z)),
            PointLight {
                color: Vec3::new(1.0, 0.8, 0.5),
                intensity: 10.0,
                range: 14.0,
            },
        ));
        world.spawn((
            Transform {
                scale: Vec3::splat(0.25),
                ..Transform::at(Vec3::new(RIVER_X - 5.4, 6.8, z))
            },
            Mesh::sphere(0.5),
            Material {
                emissive: Vec3::new(4.0, 3.0, 1.8),
                ..Material::default()
            },
        ));
    }
    for (i, z) in [6.0, 2.0, -6.0, -12.0, -24.0].into_iter().enumerate() {
        placer.place(
            world,
            "barrier",
            Vec3::new(RIVER_X - 4.8, 0.0, z),
            0.9,
            0.05 * i as f32,
        );
    }
    placer.place(world, "cable_spool", Vec3::new(-4.2, 0.0, 9.0), 1.4, 0.4);
    for (i, (x, z)) in [(-4.6, 5.0), (-4.0, 4.0), (-4.8, 3.6), (-4.3, -10.5)]
        .into_iter()
        .enumerate()
    {
        placer.place(world, "barrel", Vec3::new(x, 0.0, z), 1.0, i as f32);
    }
    for (i, (x, z, h)) in [
        (-4.3, 1.0, 1.1),
        (-4.5, -0.4, 0.9),
        (-4.4, -12.5, 1.0),
        (-3.9, -14.0, 0.8),
    ]
    .into_iter()
    .enumerate()
    {
        placer.place(world, "crate", Vec3::new(x, 0.0, z), h, 0.3 * i as f32);
    }
    placer.place(world, "ammo_crate", Vec3::new(-4.6, 0.0, 7.0), 0.7, 1.2);
    placer.place(world, "ammo_crate", Vec3::new(-4.4, 0.0, -1.8), 0.6, 0.2);

    let far = RIVER_X + 14.0;
    placer.place(world, "storage_tank", Vec3::new(far, 0.0, -2.0), 14.0, 0.0);
    placer.place(
        world,
        "storage_tank",
        Vec3::new(far + 16.0, 0.0, -18.0),
        16.0,
        1.0,
    );
    placer.place(
        world,
        "gas_sphere",
        Vec3::new(far - 2.0, 0.0, -18.0),
        11.0,
        0.3,
    );
    placer.place(
        world,
        "pipe_bridge",
        Vec3::new(4.0, 0.0, -40.0),
        9.0,
        std::f32::consts::FRAC_PI_2,
    );
    for (x, z, h) in [
        (far + 6.0, -60.0, 45.0),
        (far + 45.0, -85.0, 55.0),
        (far + 30.0, -45.0, 40.0),
    ] {
        placer.place(world, "smokestack", Vec3::new(x, 0.0, z), h, 0.0);
        let mut smoke = ParticleEmitter::default();
        smoke.rate = 25.0;
        smoke.lifetime = 9.0;
        smoke.speed = 2.0;
        smoke.spread = 0.15;
        smoke.gravity = Vec3::new(0.6, 0.15, 0.0);
        smoke.size = (1.5, 7.0);
        smoke.color = (Vec3::splat(0.025), Vec3::splat(0.01));
        smoke.capacity = 400;
        world.spawn((Transform::at(Vec3::new(x, h, z)), smoke));
    }
}

fn forest(world: &mut World, placer: &mut Placer) {
    let (color, rough) = ground_textures(256, |u, v, n| {
        let path = (u - 0.5 - (v * std::f32::consts::TAU).sin() * 0.08).abs() < 0.06 + n * 0.04;
        let rgb = if path {
            [0.33 + n * 0.12, 0.24 + n * 0.08, 0.15]
        } else {
            [0.12 + n * 0.1, 0.22 + n * 0.16, 0.06]
        };
        (rgb, 0.85)
    });
    ground(
        world,
        (-33.0, FOREST_Z - 45.0),
        (WEST_BANK, FOREST_Z + 45.0),
        0.0,
        60.0,
        textured((color, rough)),
    );
    let mut seed = 7u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    for i in 0..9 {
        let z = FOREST_Z + 22.0 - i as f32 * 6.0;
        placer.place(
            world,
            "oak",
            Vec3::new(-9.0 - random() * 4.0, 0.0, z),
            11.0 + random() * 5.0,
            random() * 6.0,
        );
    }
    for i in 0..22 {
        let side = if i % 3 == 0 {
            RIVER_X + 8.0 + random() * 14.0
        } else {
            -14.0 - random() * 16.0
        };
        let z = FOREST_Z + 30.0 - random() * 70.0;
        placer.place(
            world,
            "spruce",
            Vec3::new(side, 0.0, z),
            14.0 + random() * 10.0,
            random() * 6.0,
        );
    }
    for i in 0..10 {
        let x = RIVER_X + (random() - 0.5) * 10.0;
        let z = FOREST_Z + 20.0 - i as f32 * 5.0;
        placer.place(
            world,
            "boulder",
            Vec3::new(x, -0.4, z),
            1.0 + random() * 1.8,
            random() * 6.0,
        );
        placer.place(
            world,
            "river_rocks",
            Vec3::new(x + 1.5, -0.3, z + 2.0),
            0.6,
            random() * 6.0,
        );
    }
    for _ in 0..40 {
        let x = -8.0 + random() * 14.0;
        let z = FOREST_Z + 25.0 - random() * 55.0;
        placer.place(
            world,
            "fern",
            Vec3::new(x, 0.0, z),
            0.6 + random() * 0.5,
            random() * 6.0,
        );
    }
    for _ in 0..140 {
        let x = -10.0 + random() * 17.0;
        let z = FOREST_Z + 25.0 - random() * 55.0;
        placer.place(
            world,
            "grass",
            Vec3::new(x, 0.0, z),
            0.4 + random() * 0.4,
            random() * 6.0,
        );
    }
}

fn river(world: &mut World) {
    let (_, bed_rough) = ground_textures(64, |_, _, _| ([0.0; 3], 0.05));
    let length = 900.0;
    world.spawn((
        Transform {
            scale: Vec3::new(14.0, 1.0, length),
            ..Transform::at(Vec3::new(RIVER_X + 4.0, -0.35, -250.0))
        },
        Mesh::plane(1.0),
        Material {
            base_color: Vec3::new(0.03, 0.07, 0.08),
            roughness: 0.05,
            opacity: 0.85,
            alpha_mode: AlphaMode::Blend,
            metallic_roughness_texture: Some(bed_rough),
            ..Material::default()
        },
    ));
    block(
        world,
        Vec3::new(RIVER_X + 4.0, -1.2, -250.0),
        Vec3::new(14.0, 0.2, length),
        Material {
            base_color: Vec3::new(0.12, 0.11, 0.08),
            roughness: 0.9,
            ..Material::default()
        },
    );
}

fn mountains(world: &mut World, placer: &mut Placer) {
    let land = Material {
        base_color: Vec3::new(0.16, 0.18, 0.09),
        roughness: 1.0,
        ..Material::default()
    };
    ground(
        world,
        (-700.0, -700.0),
        (WEST_BANK, 200.0),
        -0.02,
        1.0,
        land.clone(),
    );
    ground(world, (EAST_BANK, -700.0), (700.0, 200.0), -0.02, 1.0, land);
    for (x, z, h, yaw) in [
        (-160.0, -420.0, 80.0, 0.0),
        (60.0, -470.0, 110.0, 1.3),
        (260.0, -400.0, 90.0, 2.4),
        (330.0, -150.0, 70.0, 0.7),
        (-330.0, -220.0, 75.0, 3.5),
    ] {
        placer.place(world, "mountain", Vec3::new(x, -2.0, z), h, yaw);
    }
}

struct Place {
    eye: Vec3,
    look: Vec3,
    sun: Sun,
    sky: Sky,
    fog: Fog,
}

fn night() -> Place {
    let direction = Vec3::new(-0.3, -0.5, 0.8);
    Place {
        eye: Vec3::new(5.0, 2.0, 12.0),
        look: Vec3::new(11.0, 3.0, -30.0),
        sun: Sun {
            direction,
            color: Vec3::new(0.14, 0.17, 0.3),
            ..Sun::default()
        },
        sky: Sky {
            zenith: Vec3::new(0.012, 0.024, 0.06),
            horizon: Vec3::new(0.05, 0.07, 0.11),
            ground: Vec3::new(0.01, 0.01, 0.012),
        },
        fog: Fog {
            color: Vec3::new(0.02, 0.03, 0.05),
            density: 0.015,
        },
    }
}

fn day() -> Place {
    let direction = Vec3::new(0.35, -0.75, 0.55);
    let sky = Sky::from_sun(direction);
    Place {
        eye: Vec3::new(3.0, 1.8, FOREST_Z + 26.0),
        look: Vec3::new(14.0, 3.0, FOREST_Z - 20.0),
        sun: Sun {
            direction,
            color: Vec3::new(1.0, 0.95, 0.85) * 1.2,
            ..Sun::default()
        },
        sky,
        fog: Fog {
            color: sky.horizon,
            density: 0.004,
        },
    }
}

fn go(world: &mut World, place: Place) {
    for (_, (transform, _)) in world.query_mut::<(&mut Transform, &Camera)>() {
        *transform = Transform::looking_at(place.eye, place.look, Vec3::Y);
    }
    for (_, sun) in world.query_mut::<&mut Sun>() {
        *sun = place.sun;
    }
    for (_, sky) in world.query_mut::<&mut Sky>() {
        *sky = place.sky;
    }
    for (_, fog) in world.query_mut::<&mut Fog>() {
        *fog = place.fog;
    }
}

pub fn app() -> Element {
    let game = Game::<Command>::new(|setup| {
        let world = &mut setup.world;
        let start = night();
        world.spawn((
            Camera {
                far: 2000.0,
                ..Camera::default()
            },
            Transform::looking_at(start.eye, start.look, Vec3::Y),
            FlyCamera {
                speed: 6.0,
                ..FlyCamera::default()
            },
        ));
        world.spawn((start.sun,));
        world.spawn((start.sky,));
        world.spawn((start.fog,));
        let mut placer = Placer { loaded: Vec::new() };
        factory(world, &mut placer);
        forest(world, &mut placer);
        river(world);
        mountains(world, &mut placer);
        for z in [-15.0, FOREST_Z] {
            world.spawn((
                Transform::at(Vec3::new(4.0, 3.0, z)),
                ReflectionProbe::new(Vec3::new(44.0, 16.0, 70.0)),
            ));
        }
        let mut captured = false;
        setup.on_update(move |context| {
            if captured {
                return;
            }
            let ready = context
                .world
                .query_mut::<&Model>()
                .into_iter()
                .all(|(_, model)| !matches!(model.load_state(), LoadState::Loading));
            if ready {
                captured = true;
                for (_, probe) in context.world.query_mut::<&mut ReflectionProbe>() {
                    probe.recapture();
                }
            }
        });
        setup.on_command(|context, command| {
            go(
                context.world,
                match command {
                    Command::Factory => night(),
                    Command::Forest => day(),
                },
            );
        });
    });

    let button = |label: &'static str, command: Command| {
        render! {
            View(
                style: css!(padding: px(10), margin_right: px(8), background_color: Color::hex(0x1d2733)),
                on_tap: move |_| game.send(command),
            ) {
                Text(value: label, style: css!(color: Color::hex(0xffffff)))
            }
        }
    };
    render! {
        View(style: css!(width: percent(100), height: percent(100))) {
            Viewport(game: game, style: css!(width: percent(100), height: percent(100)))
            View(style: css!(position: PositionKind::Absolute, left: px(16), top: px(16), flex_direction: FlexDirection::Row)) {
                {button("夜の工場", Command::Factory)}
                {button("昼の森", Command::Forest)}
            }
            Text(
                value: "WASD / QE で移動、ドラッグで視点",
                style: css!(position: PositionKind::Absolute, left: px(16), bottom: px(16), color: Color::hex(0xffffff)),
            )
        }
    }
}
