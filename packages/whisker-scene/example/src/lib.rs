//! `whisker-scene` example app.
//!
//! A lit scene written as components: a turning group of shapes over a floor, an orbiting point
//! light labelled with a [`SceneAnchor`], a toggle that mounts and unmounts one of the shapes, and
//! a perspective/orthographic camera switch.

use whisker::css::{FlexDirection, FontWeight, Overflow};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_scene::math::{Quat, Vec2, Vec3};
use whisker_scene::{
    AmbientLight, DirectionalLight, Geometry, Group, Material, Mesh, OrthographicCamera,
    PerspectiveCamera, PointLight, SceneAnchor, SceneView, Shadow, StandardMaterial, use_frame,
};

fn star(points: u32, outer: f32, inner: f32) -> Vec<Vec2> {
    (0..points * 2)
        .map(|i| {
            let radius = if i % 2 == 0 { outer } else { inner };
            Vec2::from_angle(i as f32 * std::f32::consts::PI / points as f32) * radius
        })
        .collect()
}

fn standard(color: Color, metallic: f32, roughness: f32) -> Material {
    Material::Standard(StandardMaterial {
        base_color: color,
        metallic,
        roughness,
        ..Default::default()
    })
}

/// Turns its children around +Y, one revolution every eight seconds.
#[component]
fn turntable(children: Children) -> Element {
    let angle = signal(0.0f32);
    use_frame(move |frame| {
        angle.set(frame.timestamp.as_secs_f32() * std::f32::consts::TAU / 8.0);
    });
    let projected = StoredValue::new(children.clone());
    render! {
        Group(rotation: computed(move || Quat::from_rotation_y(angle.get()))) {
            Fragment { { projected.get()() } }
        }
    }
}

/// A warm point light circling above the shapes.
#[component]
fn orbiting_light() -> Element {
    let position = signal(Vec3::new(2.0, 1.5, 0.0));
    use_frame(move |frame| {
        let t = frame.timestamp.as_secs_f32() * 1.3;
        position.set(Vec3::new(2.0 * t.cos(), 1.5, 2.0 * t.sin()));
    });
    let label = Css::new()
        .color(Color::hex(0xffc896))
        .font_size(px(12))
        .margin_left(px(6));
    render! {
        Fragment {
            PointLight(color: Color::rgb(255, 170, 90), intensity: 3.0, range: 6.0, position: position)
            Mesh(
                geometry: Geometry::sphere(0.06, 12),
                material: Material::Unlit { color: Color::rgb(255, 200, 150) },
                position: position,
            )
            SceneAnchor(position: position) {
                Text(style: label.clone(), value: "light")
            }
        }
    }
}

#[whisker::main]
pub fn app() -> Element {
    let show_sphere = signal(true);
    let orthographic = signal(false);
    // Signals are Copy, so the nested children closures can each take the shared geometry.
    let cube = Signal::from(Geometry::cuboid(Vec3::splat(0.8)));
    let sphere = Signal::from(Geometry::sphere(0.45, 48));
    let floor = Signal::from(Geometry::disc(3.0, 64));
    let torus = Signal::from(Geometry::torus(0.28, 0.1, 48, 24));
    let cone = Signal::from(Geometry::cone(0.3, 0.8, 32));
    let star = Signal::from(Geometry::extrude(&star(5, 0.4, 0.18), 0.25));
    let eye = Vec3::new(0.0, 2.2, 4.5);
    let target = Vec3::new(0.0, 0.3, 0.0);

    let page = Css::new()
        .background_color(Color::hex(0x101012))
        .flex_grow(1.0)
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .padding_top(px(48))
        .padding_left(px(16))
        .padding_right(px(16));
    let title = Css::new()
        .color(Color::hex(0xf0f0f3))
        .font_size(px(22))
        .font_weight(FontWeight::Numeric(700))
        .margin_bottom(px(16));
    let stage = Css::new()
        .height(px(360))
        .border_radius(px(16))
        .overflow(Overflow::Hidden);
    let button = Css::new()
        .margin_top(px(16))
        .padding(px(14))
        .border_radius(px(10))
        .background_color(Color::hex(0x5e8df0));
    let button_text = Css::new().color(Color::hex(0xffffff)).font_size(px(16));
    let camera_text = button_text.clone();

    render! {
        View(style: page) {
            Text(style: title, value: "whisker-scene")
            SceneView(style: stage, background: Color::hex(0x1b1d24)) {
                PerspectiveCamera(
                    position: eye,
                    look_at: target,
                    active: computed(move || !orthographic.get()),
                )
                OrthographicCamera(
                    height: 3.6,
                    position: eye,
                    look_at: target,
                    active: computed(move || orthographic.get()),
                )
                AmbientLight(intensity: 0.15)
                DirectionalLight(intensity: 0.9, direction: Vec3::new(-0.4, -1.0, -0.6), shadow: Some(Shadow::default()))
                OrbitingLight()
                Mesh(
                    geometry: floor,
                    material: standard(Color::hex(0x3a3f4b), 0.0, 0.9),
                    position: Vec3::new(0.0, -0.4, 0.0),
                )
                Turntable() {
                    Mesh(
                        geometry: cube,
                        material: standard(Color::hex(0xe8574f), 0.0, 0.45),
                        position: Vec3::new(-0.9, 0.0, 0.0),
                        rotation: Quat::from_rotation_y(0.6),
                    )
                    Show(when: move || show_sphere.get()) {
                        Mesh(
                            geometry: sphere,
                            material: standard(Color::hex(0xd9dce3), 1.0, 0.25),
                            position: Vec3::new(0.9, 0.05, 0.0),
                        )
                    }
                    Mesh(
                        geometry: cube,
                        material: Material::Standard(StandardMaterial {
                            base_color: Color::rgba(90, 160, 255, 0.5),
                            roughness: 0.2,
                            ..Default::default()
                        }),
                        position: Vec3::new(0.0, 0.1, -1.0),
                        scale: Vec3::splat(0.7),
                    )
                    Mesh(
                        geometry: torus,
                        material: standard(Color::hex(0xf2b84b), 1.0, 0.3),
                        position: Vec3::new(0.0, 0.0, 1.0),
                        rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    )
                    Mesh(
                        geometry: cone,
                        material: standard(Color::hex(0x5fc28a), 0.0, 0.6),
                        position: Vec3::new(-0.9, 0.0, -0.9),
                    )
                    Mesh(
                        geometry: star,
                        material: standard(Color::hex(0xb07fe0), 0.0, 0.4),
                        position: Vec3::new(0.9, -0.4, -0.9),
                    )
                }
            }
            View(
                style: button.clone(),
                on_tap: move |_| show_sphere.set(!show_sphere.get_untracked()),
            ) {
                Text(
                    style: button_text,
                    value: computed(move || if show_sphere.get() {
                        "Remove the sphere".to_string()
                    } else {
                        "Add the sphere".to_string()
                    }),
                )
            }
            View(
                style: button,
                on_tap: move |_| orthographic.set(!orthographic.get_untracked()),
            ) {
                Text(
                    style: camera_text,
                    value: computed(move || if orthographic.get() {
                        "Use the perspective camera".to_string()
                    } else {
                        "Use the orthographic camera".to_string()
                    }),
                )
            }
        }
    }
}
