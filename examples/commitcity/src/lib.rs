//! Commit City: a GitHub user's year of contributions drawn as an Edo-period castle town.
//!
//! Each day is one lot on the main street and busier days raise grander houses. The busiest
//! day earns the castle, the busiest week a temple and the longest streak a shrine.

mod camera;
mod city;
mod data;
mod fetch;
mod land;
mod town;

use std::time::Duration;

use whisker::css::{
    AlignItems, ColorStop, FlexDirection, FontWeight, Gradient, JustifyContent, PointerEvents,
    PositionKind,
};
use whisker::event::{BindType, TouchEvent};
use whisker::prelude::*;
use whisker::runtime::event::bind_typed;
use whisker::runtime::view::Element;
use whisker::runtime::view::renderer::{release_pointer_capture, set_pointer_capture};
use whisker_input::{AutoCapitalize, Input};
use whisker_safe_area::safe_area_insets;
use whisker_scene::math::{Vec2, Vec3};
use whisker_scene::{
    DirectionalLight, Environment, Fog, HemisphereLight, PerspectiveCamera, PostProcessing,
    SceneHit, SceneView, Shadow, ToneMapping, use_frame,
};

use crate::camera::{Orbit, Pointers};
use crate::city::Landmark;
use crate::data::Year;
use crate::town::{Countryside, Landmarks, Rise, Selection, Street};

const DEFAULT_USER: &str = "itome";

mod ink {
    use whisker::prelude::Color;

    pub fn paper() -> Color {
        Color::hex(0xf4ede0)
    }
    pub fn faded() -> Color {
        Color::rgba(244, 237, 224, 0.62)
    }
    pub fn vermilion() -> Color {
        Color::hex(0xe0603f)
    }
    pub fn haze() -> Color {
        Color::hex(0xcdd3cf)
    }
    /// Calendar cells from no contributions to the busiest quartile.
    pub fn level(level: u8) -> Color {
        match level {
            0 => Color::rgba(244, 237, 224, 0.12),
            1 => Color::hex(0x6e5a3c),
            2 => Color::hex(0x9c7a43),
            3 => Color::hex(0xc9994b),
            _ => Color::hex(0xf0c15e),
        }
    }
}

const SERIF: &str = "Iowan Old Style, Palatino, Georgia, serif";

fn text(size: f32, color: Color) -> Css {
    Css::new().color(color).font_size(px(size)).text_shadow(
        px(0),
        px(1),
        px(6),
        Color::rgba(0, 0, 0, 0.45),
    )
}

fn serif(size: f32) -> Css {
    text(size, ink::paper()).font_family(SERIF)
}

fn caps(size: f32, color: Color) -> Css {
    text(size, color)
        .letter_spacing(px(1.6))
        .font_weight(FontWeight::Numeric(600))
}

fn row() -> Css {
    Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Row)
        .align_items(AlignItems::Center)
}

fn fill() -> Css {
    Css::new()
        .position(PositionKind::Absolute)
        .top(px(0))
        .left(px(0))
        .right(px(0))
        .bottom(px(0))
}

fn fade(top: bool, height: f32) -> Css {
    let (from, to) = (Color::rgba(14, 12, 10, 0.7), Color::rgba(14, 12, 10, 0.0));
    let stops = if top { [from, to] } else { [to, from] };
    let edge = Css::new()
        .position(PositionKind::Absolute)
        .left(px(0))
        .right(px(0))
        .height(px(height))
        .pointer_events(PointerEvents::None)
        .background_image(Gradient::linear_to_bottom(stops.map(ColorStop::new)));
    if top {
        edge.top(px(0))
    } else {
        edge.bottom(px(0))
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Down,
    Move,
    Up,
}

/// Binds `handler` to touch input and to the pointer events that mouse and pen input arrive as.
fn on_pointer(element: Element, handler: impl Fn(Phase, TouchEvent) + Clone + 'static) {
    for (name, phase) in [
        ("touchstart", Phase::Down),
        ("touchmove", Phase::Move),
        ("touchend", Phase::Up),
        ("touchcancel", Phase::Up),
        ("pointerdown", Phase::Down),
        ("pointermove", Phase::Move),
        ("pointerup", Phase::Up),
        ("pointercancel", Phase::Up),
    ] {
        let handler = handler.clone();
        bind_typed(element, name, BindType::Bind, move |event: TouchEvent| {
            handler(phase, event)
        });
    }
}

/// Advances the town's clocks and the camera once per frame.
#[component]
fn clock(rise: Rise, orbit: Orbit) -> Element {
    let last = StoredValue::new(None::<Duration>);
    use_frame(move |frame| {
        let delta = last
            .get()
            .map_or(0.0, |previous| {
                frame.timestamp.saturating_sub(previous).as_secs_f32()
            })
            .min(0.1);
        last.set(Some(frame.timestamp));
        rise.tick(frame.timestamp.as_secs_f32());
        camera::apply_keyboard(orbit, delta);
        orbit.step(delta);
        let idle = camera::KEYBOARD.with_borrow_mut(|k| {
            k.asleep = k.idle() && !orbit.moving();
            k.asleep
        });
        if idle {
            orbit.awake.set(false);
        }
    });
    render! { Fragment {} }
}

fn landmark_caption(landmark: Landmark, year: &Year) -> (String, String) {
    match landmark {
        Landmark::Castle => {
            let day = year.busiest_day.map(|i| year.days[i]);
            (
                "Castle".into(),
                day.map(|d| {
                    format!(
                        "Busiest day · {} · {} contributions",
                        d.date.label(),
                        d.contributions
                    )
                })
                .unwrap_or_default(),
            )
        }
        Landmark::Temple => (
            "Temple".into(),
            year.busiest_week
                .map(|w| {
                    format!(
                        "Busiest week · from {} · {} contributions",
                        year.days[w.start].date.short(),
                        w.contributions
                    )
                })
                .unwrap_or_default(),
        ),
        Landmark::Shrine => (
            "Shrine".into(),
            year.longest_streak
                .map(|s| {
                    format!(
                        "Longest streak · {} days from {}",
                        s.days,
                        year.days[s.start].date.short()
                    )
                })
                .unwrap_or_default(),
        ),
    }
}

/// The year's calendar, which also scrubs the selection along the street.
#[component]
fn calendar(
    year: RwSignal<Year>,
    selected: RwSignal<Option<Selection>>,
    width: Signal<f32>,
) -> Element {
    const GAP: f32 = 2.0;
    let pitch = computed(move || width.get() / year.with(Year::weeks).max(1) as f32);
    let pick = move |event: TouchEvent| {
        let Some(at) = event.offset else { return };
        let pitch = pitch.get_untracked();
        let (week, weekday) = (
            (at.x as f32 / pitch).max(0.0) as usize,
            (at.y as f32 / pitch).clamp(0.0, 6.0) as usize,
        );
        let index = year.with_untracked(|year| {
            (week * 7 + weekday)
                .checked_sub(year.offset)
                .filter(|&i| i < year.days.len())
        });
        if let Some(index) = index {
            selected.set(Some(Selection::Day(index)));
        }
    };
    let pressed = StoredValue::new(false);
    let strip = render! {
        View(style: computed(move || Css::new().position(PositionKind::Relative).width(px(width.get())).height(px(pitch.get() * 7.0)))) {
            ForEach(
                each: move || (0..year.with(|y| y.days.len())).collect::<Vec<_>>(),
                key: |i: &usize| *i,
                children: move |i: ReadSignal<usize>| {
                    let i = i.get_untracked();
                    let style = computed(move || {
                        let (cell, level) = year.with(|y| (y.cell(i), y.days.get(i).map_or(0, |d| d.level)));
                        let pitch = pitch.get();
                        let picked = selected.get() == Some(Selection::Day(i));
                        Css::new()
                            .position(PositionKind::Absolute)
                            .left(px(cell.0 as f32 * pitch))
                            .top(px(cell.1 as f32 * pitch))
                            .width(px(pitch - GAP))
                            .height(px(pitch - GAP))
                            .border_radius(px(1.5))
                            .background_color(if picked { ink::vermilion() } else { ink::level(level) })
                    });
                    render! { View(style: style) }
                },
            )
        }
    };
    on_pointer(strip, move |phase, event| match phase {
        Phase::Down => {
            set_pointer_capture(strip, event.pointer_id);
            pressed.set(true);
            pick(event);
        }
        Phase::Move if pressed.get() => pick(event),
        Phase::Move => {}
        Phase::Up => {
            release_pointer_capture(strip, event.pointer_id);
            pressed.set(false);
        }
    });
    strip
}

#[whisker::main]
pub fn app() -> Element {
    let year = signal(Year::empty(DEFAULT_USER.to_string()));
    let draft = RwSignal::new(DEFAULT_USER.to_string());
    let request = signal(DEFAULT_USER.to_string());
    let loading = signal(true);
    let error = signal(None::<String>);
    let selected = signal(None::<Selection>);
    let rise = Rise::new();
    let orbit = Orbit::new();
    orbit.set_town_length(city::length(year.with_untracked(Year::weeks)));
    camera::listen_to_page(orbit);

    let fetched = resource(move || {
        let user = request.get();
        async move { fetch::year(user).await }
    });
    effect(move || match fetched.state() {
        ResourceState::Loading => loading.set(true),
        ResourceState::Ready(fresh) => {
            loading.set(false);
            error.set(None);
            draft.set(fresh.user.clone());
            if year.get_untracked() != fresh {
                orbit.set_town_length(city::length(fresh.weeks()));
                year.set(fresh);
                selected.set(None);
                rise.restart();
            }
        }
        ResourceState::Error(message) => {
            loading.set(false);
            error.set(Some(message));
        }
    });
    let build = move |input: String| match fetch::login(&input) {
        Ok(user) => {
            error.set(None);
            request.set(user);
        }
        Err(message) => error.set(Some(message)),
    };

    effect(move || {
        let Some(selection) = selected.get() else {
            return;
        };
        let target = year.with_untracked(|y| match selection {
            Selection::Day(i) => city::placement(y, i).0,
            Selection::Landmark(landmark) => city::landmarks(y)
                .into_iter()
                .find(|(l, _)| *l == landmark)
                .map_or(Vec3::ZERO, |(_, at)| at),
        });
        let distance = match selection {
            Selection::Day(_) => 13.0,
            Selection::Landmark(_) => 24.0,
        };
        orbit.fly_to(target, distance, 0.42);
    });
    let walk = move || {
        selected.set(None);
        let weeks = year.with_untracked(Year::weeks);
        orbit.start_tour(city::week_x(0, weeks), city::week_x(weeks - 1, weeks));
    };

    let caption = computed(move || {
        year.with(|y| match selected.get() {
            Some(Selection::Day(i)) => y.days.get(i).map(|d| {
                (
                    d.building.name().to_string(),
                    format!(
                        "{} · {} contribution{}",
                        d.date.label(),
                        d.contributions,
                        if d.contributions == 1 { "" } else { "s" }
                    ),
                )
            }),
            Some(Selection::Landmark(landmark)) => Some(landmark_caption(landmark, y)),
            None => None,
        })
    });
    let status = computed(move || match (loading.get(), error.get()) {
        (_, Some(message)) => message,
        (true, None) => "Surveying…".to_string(),
        (false, None) => String::new(),
    });
    let summary = computed(move || {
        year.with(|y| {
            format!(
                "{} contributions in the last year",
                y.total
                    .to_string()
                    .as_bytes()
                    .rchunks(3)
                    .rev()
                    .map(|c| std::str::from_utf8(c).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
    });

    let viewport = whisker::viewport();
    let wide = computed(move || viewport.get().width >= 720.0);
    let fov_y = computed(move || {
        let size = viewport.get();
        let aspect = if size.height > 0.0 {
            size.width / size.height
        } else {
            1.0
        };
        (2.0 * (28f32.to_radians().tan() / aspect.min(1.6)).atan())
            .clamp(36f32.to_radians(), 72f32.to_radians())
    });
    let eye = computed(move || orbit.eye());
    let look_at = computed(move || orbit.target.get() + Vec3::Y * 0.8);
    let insets = safe_area_insets();
    let calendar_width = computed(move || (viewport.get().width - 32.0).clamp(0.0, 640.0));

    let pointers = StoredValue::new(Pointers::default());
    let point = |event: &TouchEvent| Vec2::new(event.detail.x as f32, event.detail.y as f32);
    let haze = ink::haze();

    let field = Css::new()
        .width(px(150))
        .height(px(30))
        .padding_left(px(0))
        .padding_right(px(0))
        .color(ink::paper())
        .font_size(px(17))
        .font_family(SERIF)
        .background_color(Color::rgba(0, 0, 0, 0.0))
        .border_bottom(
            whisker::css::Border::new()
                .width(px(1))
                .color(ink::faded())
                .style(whisker::css::BorderStyle::Solid),
        );
    let link = || {
        caps(11.0, ink::faded())
            .padding_top(px(6))
            .padding_bottom(px(6))
            .margin_right(px(14))
    };

    let stage = render! {
        View(style: fill()) {
                SceneView(
                    style: fill(),
                    background: haze,
                    tone_mapping: ToneMapping::AgX,
                    exposure: 1.35,
                    fog: Some(Fog::Exponential { color: haze, density: 0.0026 }),
                    environment: Some(Environment::gradient(Color::hex(0x9db4cf), haze, Color::hex(0x4d5a35))),
                    environment_intensity: 0.8,
                    post_processing: PostProcessing {
                        bloom: None,
                        ambient_occlusion: None,
                        fxaa: true,
                        ..PostProcessing::default()
                    },
                    on_tap: move |hit: Option<SceneHit>| {
                        if hit.is_none() {
                            selected.set(None);
                        }
                    },
                ) {
                    PerspectiveCamera(fov_y: fov_y, position: eye, look_at: look_at, near: 0.3, far: 900.0)
                    DirectionalLight(
                        color: Color::hex(0xffdcb0),
                        intensity: 2.1,
                        direction: Vec3::new(-0.5, -0.8, -0.45),
                        shadow: Some(Shadow { map_size: 2048, cascades: 2, distance: 110.0, ..Shadow::default() }),
                    )
                    HemisphereLight(sky_color: Color::hex(0xb7c6dc), ground_color: Color::hex(0x5d5236), intensity: 0.35)
                    Countryside()
                    Street(year: year, rise: rise, selected: selected)
                    Landmarks(year: year, selected: selected)
                    Show(when: move || orbit.awake.get() || rise.running.get()) { Clock(rise: rise, orbit: orbit) }
                }
            }
    };
    on_pointer(stage, move |phase, event| match phase {
        Phase::Down => {
            let turning = event.pointer_type == "mouse" && event.buttons & 2 != 0;
            pointers.update(|p| p.press(event.pointer_id, point(&event), turning));
        }
        Phase::Move => {
            pointers.update(|p| p.moved(orbit, event.pointer_id, point(&event), event.timestamp));
        }
        Phase::Up => {
            pointers.update(|p| p.release(orbit, event.pointer_id, event.timestamp));
        }
    });

    render! {
        View(style: Css::new().flex_grow(1.0).background_color(haze)) {
            { stage }

            View(style: fade(true, 150.0))
            View(style: fade(false, 280.0))

            View(style: computed(move || {
                Css::new()
                    .position(PositionKind::Absolute)
                    .top(px(insets.get().top as f32 + 18.0))
                    .left(px(20))
                    .right(px(20))
            })) {
                View(style: Css::new().display_flex().flex_direction(FlexDirection::Column)) {
                    Text(style: caps(11.0, ink::vermilion()), value: "COMMIT CITY")
                    View(style: row().margin_top(px(4))) {
                        Text(style: serif(17.0).color(ink::faded()), value: "github.com/")
                        Input(
                            text: draft,
                            placeholder: "username",
                            auto_capitalize: AutoCapitalize::None,
                            style: field,
                            on_submit: move |input: String| build(input),
                        )
                    }
                    Show(when: move || year.with(|y| !y.days.is_empty())) {
                        Text(style: serif(15.0).color(ink::faded()).margin_top(px(4)), value: summary)
                    }
                    Show(when: move || !status.get().is_empty()) {
                        Text(style: text(13.0, ink::vermilion()).margin_top(px(4)), value: status)
                    }
                }
            }

            View(style: computed(move || {
                Css::new()
                    .position(PositionKind::Absolute)
                    .bottom(px(insets.get().bottom as f32 + 18.0))
                    .left(px(16))
                    .right(px(16))
                    .display_flex()
                    .flex_direction(FlexDirection::Column)
                    .align_items(AlignItems::Center)
            })) {
                View(style: computed(move || Css::new().display_flex().flex_direction(FlexDirection::Column).width(px(calendar_width.get())))) {
                    Show(
                        when: move || caption.get().is_some(),
                        fallback: move || render! {
                            Text(
                                style: text(12.0, ink::faded()).margin_bottom(px(10)),
                                value: computed(move || if wide.get() {
                                    "Drag to move · right-drag or twist to turn · scroll or pinch to zoom · WASD / QE"
                                } else {
                                    "Drag to move · twist to turn · pinch to zoom"
                                }.to_string()),
                            )
                        },
                    ) {
                        View(style: Css::new().display_flex().flex_direction(FlexDirection::Column).margin_bottom(px(10))) {
                            Text(style: serif(24.0), value: computed(move || caption.get().map(|c| c.0).unwrap_or_default()))
                            Text(style: text(13.0, ink::faded()).margin_top(px(2)), value: computed(move || caption.get().map(|c| c.1).unwrap_or_default()))
                        }
                    }
                    Calendar(year: year, selected: selected, width: calendar_width)
                    View(style: row().margin_top(px(10)).justify_content(JustifyContent::SpaceBetween)) {
                        View(style: row()) {
                            View(on_tap: move |_| { selected.set(None); orbit.overview(); }) {
                                Text(style: link(), value: "OVERVIEW")
                            }
                            View(on_tap: move |_| walk()) {
                                Text(style: link(), value: "WALK THE YEAR")
                            }
                        }
                        Show(when: move || wide.get()) {
                            Text(style: caps(10.0, ink::faded()), value: "WHISKER · WHISKER-SCENE")
                        }
                    }
                }
            }
        }
    }
}
