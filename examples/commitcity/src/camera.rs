//! A free orbit camera over the town: drag to pan, twist or right-drag to turn, pinch or scroll to zoom.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use whisker::prelude::*;
use whisker_scene::math::{Vec2, Vec3};

const MIN_DISTANCE: f32 = 3.0;
const MAX_DISTANCE: f32 = 260.0;
const MIN_PITCH: f32 = 0.08;
const MAX_PITCH: f32 = 1.45;

/// Where a fly-to is heading.
#[derive(Clone, Copy, PartialEq)]
struct Goal {
    target: Vec3,
    distance: f32,
    pitch: f32,
    yaw: Option<f32>,
}

#[derive(Clone, Copy)]
pub struct Orbit {
    pub target: RwSignal<Vec3>,
    pub yaw: RwSignal<f32>,
    pub pitch: RwSignal<f32>,
    pub distance: RwSignal<f32>,
    goal: RwSignal<Option<Goal>>,
    /// Pan speed left over from the last drag, in world units per second.
    drift: RwSignal<Vec2>,
    /// The x a tour has reached and the x it ends at.
    tour: RwSignal<Option<(f32, f32)>>,
    bounds: RwSignal<Vec2>,
    /// Whether the per-frame clock runs; the scene redraws only on change while it sleeps.
    pub awake: RwSignal<bool>,
}

impl Orbit {
    pub fn new() -> Orbit {
        Orbit {
            target: signal(Vec3::new(0.0, 0.0, -6.0)),
            yaw: signal(0.32),
            pitch: signal(0.5),
            distance: signal(64.0),
            goal: signal(None),
            drift: signal(Vec2::ZERO),
            tour: signal(None),
            bounds: signal(Vec2::new(80.0, 60.0)),
            awake: signal(true),
        }
    }

    pub fn eye(self) -> Vec3 {
        let (yaw, pitch, distance) = (self.yaw.get(), self.pitch.get(), self.distance.get());
        self.target.get()
            + Vec3::new(
                yaw.sin() * pitch.cos(),
                pitch.sin(),
                yaw.cos() * pitch.cos(),
            ) * distance
    }

    pub fn set_town_length(self, length: f32) {
        self.bounds.set(Vec2::new(length / 2.0 + 30.0, 60.0));
    }

    /// Whether a fly-to, tour or drift is still moving the camera.
    pub fn moving(self) -> bool {
        self.goal.get_untracked().is_some()
            || self.tour.get_untracked().is_some()
            || self.drift.get_untracked().length() > 0.01
    }

    fn stop(self) {
        self.goal.set(None);
        self.tour.set(None);
    }

    /// Moves the target by a screen-space drag of `delta` pixels.
    pub fn pan(self, delta: Vec2) {
        self.stop();
        let world = self.drag_to_world(delta);
        self.move_target(world);
        self.drift.set(Vec2::ZERO);
    }

    /// Releases a drag that was moving `velocity` pixels per second.
    pub fn fling(self, velocity: Vec2) {
        let world = self.drag_to_world(velocity);
        self.drift.set(Vec2::new(world.x, world.z));
        self.awake.set(true);
    }

    fn drag_to_world(self, delta: Vec2) -> Vec3 {
        let yaw = self.yaw.get_untracked();
        let scale = self.distance.get_untracked() * 0.0016;
        let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
        let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        (-right * delta.x + forward * delta.y) * scale
    }

    fn move_target(self, by: Vec3) {
        let bounds = self.bounds.get_untracked();
        let t = self.target.get_untracked() + by;
        self.target.set(Vec3::new(
            t.x.clamp(-bounds.x, bounds.x),
            0.0,
            t.z.clamp(-bounds.y, bounds.y * 0.6),
        ));
    }

    pub fn turn(self, yaw: f32, pitch: f32) {
        self.stop();
        self.yaw.set(self.yaw.get_untracked() + yaw);
        self.pitch
            .set((self.pitch.get_untracked() + pitch).clamp(MIN_PITCH, MAX_PITCH));
    }

    pub fn zoom(self, factor: f32) {
        self.stop();
        self.distance
            .set((self.distance.get_untracked() * factor).clamp(MIN_DISTANCE, MAX_DISTANCE));
    }

    pub fn fly_to(self, target: Vec3, distance: f32, pitch: f32, yaw: Option<f32>) {
        self.tour.set(None);
        self.drift.set(Vec2::ZERO);
        self.goal.set(Some(Goal {
            target,
            distance,
            pitch,
            yaw,
        }));
        self.awake.set(true);
    }

    pub fn overview(self) {
        self.tour.set(None);
        self.drift.set(Vec2::ZERO);
        self.goal.set(Some(Goal {
            target: Vec3::new(0.0, 0.0, -6.0),
            distance: 64.0,
            pitch: 0.5,
            yaw: Some(0.32),
        }));
        self.awake.set(true);
    }

    pub fn start_tour(self, from: f32, to: f32) {
        self.goal.set(None);
        self.drift.set(Vec2::ZERO);
        self.tour.set(Some((from, to)));
        self.awake.set(true);
    }

    /// Advances fly-tos, tours and drift by `delta` seconds.
    pub fn step(self, delta: f32) {
        if let Some((x, end)) = self.tour.get_untracked() {
            let x = x + delta * 2.6;
            let target = Vec3::new(x.min(end), 0.0, crate::city::STREET_Z);
            let ease = (delta * 2.0).min(1.0);
            let yaw = self.yaw.get_untracked();
            let street_view = turn_toward(yaw, -FRAC_PI_2);
            self.target
                .set(self.target.get_untracked().lerp(target, ease));
            self.distance
                .set(lerp(self.distance.get_untracked(), 8.0, ease));
            self.pitch.set(lerp(self.pitch.get_untracked(), 0.36, ease));
            self.yaw.set(lerp(yaw, yaw + street_view, ease));
            self.tour.set((x < end).then_some((x, end)));
            return;
        }
        if let Some(Goal {
            target,
            distance,
            pitch,
            yaw,
        }) = self.goal.get_untracked()
        {
            let ease = (delta * 3.0).min(1.0);
            if let Some(yaw) = yaw {
                let now = self.yaw.get_untracked();
                self.yaw.set(lerp(now, now + turn_toward(now, yaw), ease));
            }
            let t = self.target.get_untracked().lerp(target, ease);
            self.target.set(t);
            self.distance
                .set(lerp(self.distance.get_untracked(), distance, ease));
            self.pitch
                .set(lerp(self.pitch.get_untracked(), pitch, ease));
            if t.distance(target) < 0.01 && (self.distance.get_untracked() - distance).abs() < 0.01
            {
                self.goal.set(None);
            }
            return;
        }
        let drift = self.drift.get_untracked();
        if drift.length() > 0.01 {
            self.move_target(Vec3::new(drift.x, 0.0, drift.y) * delta);
            self.drift.set(drift * (1.0 - (delta * 4.0).min(1.0)));
        }
    }
}

/// The shortest signed turn from yaw `from` to yaw `to`.
fn turn_toward(from: f32, to: f32) -> f32 {
    (to - from + PI).rem_euclid(TAU) - PI
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Pointers currently down on the view, and the gesture they make.
#[derive(Default)]
pub struct Pointers {
    down: Vec<(i64, Vec2, bool)>,
    last_move: f64,
    velocity: Vec2,
}

impl Pointers {
    pub fn press(&mut self, id: i64, at: Vec2, turning: bool) {
        self.down.retain(|(p, ..)| *p != id);
        self.down.push((id, at, turning));
        self.velocity = Vec2::ZERO;
    }

    /// Applies one pointer's move to `orbit`.
    pub fn moved(&mut self, orbit: Orbit, id: i64, at: Vec2, timestamp: f64) {
        let Some(index) = self.down.iter().position(|(p, ..)| *p == id) else {
            return;
        };
        let before: Vec<Vec2> = self.down.iter().map(|(_, at, _)| *at).collect();
        self.down[index].1 = at;
        match self.down.as_slice() {
            [(_, now, turning)] => {
                let delta = *now - before[0];
                if *turning {
                    orbit.turn(-delta.x * 0.006, delta.y * 0.004);
                } else {
                    orbit.pan(delta);
                    let dt = ((timestamp - self.last_move) / 1000.0) as f32;
                    if dt > 0.0 && dt < 0.1 {
                        self.velocity = self.velocity.lerp(delta / dt, 0.5);
                    }
                }
            }
            [(_, a, _), (_, b, _), ..] => {
                let (a0, b0) = (before[0], before[1]);
                let (span0, span) = ((b0 - a0).length(), (*b - *a).length());
                if span0 > 1.0 && span > 1.0 {
                    orbit.zoom(span0 / span);
                }
                let angle0 = (b0 - a0).y.atan2((b0 - a0).x);
                let angle = (*b - *a).y.atan2((*b - *a).x);
                let twist = (angle - angle0 + PI).rem_euclid(TAU) - PI;
                let center_delta = (*a + *b) / 2.0 - (a0 + b0) / 2.0;
                orbit.turn(twist, center_delta.y * 0.003);
                self.velocity = Vec2::ZERO;
            }
            [] => {}
        }
        self.last_move = timestamp;
    }

    pub fn release(&mut self, orbit: Orbit, id: i64, timestamp: f64) {
        let was_single = self.down.len() == 1;
        self.down.retain(|(p, ..)| *p != id);
        let recent = timestamp - self.last_move < 80.0;
        if was_single && recent && self.velocity.length() > 50.0 {
            orbit.fling(self.velocity);
        }
        self.velocity = Vec2::ZERO;
    }
}

/// Keys held and scroll accumulated by the page, read once per frame.
#[derive(Default)]
pub struct Keyboard {
    pub held: Vec<String>,
    pub wheel: f32,
    /// Set when the clock stops, so the next key or scroll posts a wake-up.
    pub asleep: bool,
}

impl Keyboard {
    pub fn idle(&self) -> bool {
        self.held.is_empty() && self.wheel == 0.0
    }
}

thread_local! {
    pub static KEYBOARD: std::cell::RefCell<Keyboard> = std::cell::RefCell::default();
}

/// Applies held keys and accumulated wheel scroll for `delta` seconds.
pub fn apply_keyboard(orbit: Orbit, delta: f32) {
    KEYBOARD.with_borrow_mut(|keyboard| {
        if keyboard.wheel != 0.0 {
            orbit.zoom((keyboard.wheel * 0.0015).exp());
            keyboard.wheel = 0.0;
        }
        let held = |names: &[&str]| keyboard.held.iter().any(|k| names.contains(&k.as_str()));
        let mut pan = Vec2::ZERO;
        if held(&["w", "W", "ArrowUp"]) {
            pan.y += 1.0;
        }
        if held(&["s", "S", "ArrowDown"]) {
            pan.y -= 1.0;
        }
        if held(&["a", "A", "ArrowLeft"]) {
            pan.x += 1.0;
        }
        if held(&["d", "D", "ArrowRight"]) {
            pan.x -= 1.0;
        }
        if pan != Vec2::ZERO {
            orbit.pan(pan.normalize() * 520.0 * delta);
        }
        let turn = f32::from(u8::from(held(&["q", "Q"]))) - f32::from(u8::from(held(&["e", "E"])));
        if turn != 0.0 {
            orbit.turn(turn * 1.2 * delta, 0.0);
        }
        let zoom = f32::from(u8::from(held(&["-", "_", "f", "F"])))
            - f32::from(u8::from(held(&["=", "+", "r", "R"])));
        if zoom != 0.0 {
            orbit.zoom((zoom * 1.4 * delta).exp());
        }
    });
}

/// Feeds the page's wheel and key events into [`KEYBOARD`] and keeps right-drags from opening the context menu.
#[cfg(target_arch = "wasm32")]
pub fn listen_to_page(orbit: Orbit) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    let Some(window) = web_sys::window() else {
        return;
    };
    let dispatcher = whisker::runtime_dispatcher();
    // Page listeners run outside the runtime, so they post the wake-up instead of setting the signal.
    let wake = move || {
        if let Some(dispatcher) = &dispatcher
            && KEYBOARD.with_borrow_mut(|k| std::mem::take(&mut k.asleep))
        {
            dispatcher.post(move || orbit.awake.set(true));
        }
    };
    let woken = wake.clone();
    let wheel =
        Closure::<dyn FnMut(web_sys::WheelEvent)>::new(move |event: web_sys::WheelEvent| {
            event.prevent_default();
            let scale = if event.delta_mode() == 1 { 16.0 } else { 1.0 };
            KEYBOARD.with_borrow_mut(|k| k.wheel += event.delta_y() as f32 * scale);
            woken();
        });
    let options = web_sys::AddEventListenerOptions::new();
    options.set_passive(false);
    let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
        "wheel",
        wheel.as_ref().unchecked_ref(),
        &options,
    );
    wheel.forget();
    let typing = |event: &web_sys::KeyboardEvent| {
        event
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|e| matches!(e.tag_name().as_str(), "INPUT" | "TEXTAREA"))
    };
    let down =
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |event: web_sys::KeyboardEvent| {
            if typing(&event) || event.meta_key() || event.ctrl_key() {
                return;
            }
            let key = event.key();
            KEYBOARD.with_borrow_mut(|k| {
                if !k.held.contains(&key) {
                    k.held.push(key);
                }
            });
            wake();
        });
    let up = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|event: web_sys::KeyboardEvent| {
        let key = event.key();
        KEYBOARD.with_borrow_mut(|k| {
            k.held
                .retain(|held| !held.eq_ignore_ascii_case(&key) && held != &key)
        });
    });
    let menu =
        Closure::<dyn FnMut(web_sys::Event)>::new(|event: web_sys::Event| event.prevent_default());
    let _ = window.add_event_listener_with_callback("contextmenu", menu.as_ref().unchecked_ref());
    menu.forget();
    let blur = Closure::<dyn FnMut()>::new(|| KEYBOARD.with_borrow_mut(|k| k.held.clear()));
    let _ = window.add_event_listener_with_callback("keydown", down.as_ref().unchecked_ref());
    let _ = window.add_event_listener_with_callback("keyup", up.as_ref().unchecked_ref());
    let _ = window.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref());
    down.forget();
    up.forget();
    blur.forget();
}

#[cfg(not(target_arch = "wasm32"))]
pub fn listen_to_page(_orbit: Orbit) {}
