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

    let game = Game::<Command>::new(move |app| {
        let triangle = app.world.spawn((Transform::default(), Triangle));

        let mut speed = 1.0;
        let mut angle = 0.0;
        app.on_update(move |world, time, commands| {
            for command in commands {
                match command {
                    Command::Boost => speed *= 2.0,
                }
            }
            angle += speed * time.delta_seconds();
            world
                .get::<&mut Transform>(triangle)
                .expect("the triangle is never despawned")
                .rotation = Quat::from_rotation_z(angle);

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
