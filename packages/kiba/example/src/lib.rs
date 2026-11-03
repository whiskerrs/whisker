use kiba::prelude::*;
use whisker::css::PositionKind;
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[derive(Clone, Copy)]
enum Command {
    Boost,
}

#[whisker::main]
pub fn app() -> Element {
    let turns = signal(0);

    let game = Game::<Command>::new(move |setup| {
        setup.world.spawn((
            Camera::default(),
            Transform::looking_at(Vec3::new(0.0, 1.4, 3.2), Vec3::new(0.0, 0.0, -0.4), Vec3::Y),
        ));
        setup.world.spawn((Sun::default(),));

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
                    },
                ));
            }
        }

        let cube = setup.world.spawn((
            Transform::default(),
            Mesh::cube(0.8),
            Material {
                base_color: Vec3::new(0.1, 0.25, 0.6),
                metallic: 0.0,
                roughness: 0.3,
            },
        ));
        let moon = setup.world.spawn((
            Transform::default(),
            sphere,
            Material {
                base_color: Vec3::new(0.95, 0.95, 0.95),
                metallic: 1.0,
                roughness: 0.2,
            },
        ));

        let mut speed = 1.0;
        let mut angle = 0.0;
        setup.on_update(move |world, time, commands| {
            for command in commands {
                match command {
                    Command::Boost => speed *= 2.0,
                }
            }
            angle += speed * time.delta_seconds();
            world
                .get::<&mut Transform>(cube)
                .expect("the cube is never despawned")
                .rotation = Quat::from_rotation_y(angle);
            world
                .get::<&mut Transform>(moon)
                .expect("the moon is never despawned")
                .translation = Vec3::new(angle.cos(), 0.0, angle.sin()) * 0.8;

            let completed = (angle / std::f32::consts::TAU) as i32;
            if completed != turns.get_untracked() {
                turns.set(completed);
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
