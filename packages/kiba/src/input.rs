use std::collections::HashSet;

use glam::Vec2;

macro_rules! keys {
    ($($key:ident => $code:literal,)*) => {
        /// A physical key, named for its position on a US layout.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[allow(missing_docs)]
        pub enum Key {
            $($key,)*
        }

        impl Key {
            /// Maps a DOM `KeyboardEvent.code` value.
            #[doc(hidden)]
            pub fn from_code(code: &str) -> Option<Self> {
                match code {
                    $($code => Some(Self::$key),)*
                    _ => None,
                }
            }
        }
    };
}

keys! {
    A => "KeyA", B => "KeyB", C => "KeyC", D => "KeyD", E => "KeyE", F => "KeyF", G => "KeyG",
    H => "KeyH", I => "KeyI", J => "KeyJ", K => "KeyK", L => "KeyL", M => "KeyM", N => "KeyN",
    O => "KeyO", P => "KeyP", Q => "KeyQ", R => "KeyR", S => "KeyS", T => "KeyT", U => "KeyU",
    V => "KeyV", W => "KeyW", X => "KeyX", Y => "KeyY", Z => "KeyZ",
    Digit0 => "Digit0", Digit1 => "Digit1", Digit2 => "Digit2", Digit3 => "Digit3",
    Digit4 => "Digit4", Digit5 => "Digit5", Digit6 => "Digit6", Digit7 => "Digit7",
    Digit8 => "Digit8", Digit9 => "Digit9",
    Up => "ArrowUp", Down => "ArrowDown", Left => "ArrowLeft", Right => "ArrowRight",
    Space => "Space", Enter => "Enter", Escape => "Escape", Tab => "Tab",
    ShiftLeft => "ShiftLeft", ShiftRight => "ShiftRight",
    ControlLeft => "ControlLeft", ControlRight => "ControlRight",
    AltLeft => "AltLeft", AltRight => "AltRight",
}

/// What a Host reports. Positions are physical pixels from the viewport's top left.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    KeyDown(Key),
    KeyUp(Key),
    PointerDown { id: i32, position: Vec2 },
    PointerMove { id: i32, position: Vec2 },
    PointerUp { id: i32, position: Vec2 },
    PointerGone { id: i32 },
    Wheel(f32),
    FocusLost,
}

/// A mouse or a touch on the viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    /// Stays the same for as long as the pointer exists.
    pub id: i32,
    /// Physical pixels from the viewport's top left.
    pub position: Vec2,
    /// Movement since the previous callback.
    pub delta: Vec2,
    /// Whether the button or finger is down.
    pub pressed: bool,
    /// Whether it went down since the previous callback.
    pub just_pressed: bool,
    /// Whether it came up since the previous callback.
    pub just_released: bool,
    gone: bool,
}

/// Keyboard and pointer state seen by a callback.
///
/// "Just" states last until one callback pass has seen them, so a key press in a frame without
/// a fixed step still reaches the next `on_fixed_update`.
#[derive(Clone, Debug, Default)]
pub struct Input {
    down: HashSet<Key>,
    just_pressed: HashSet<Key>,
    just_released: HashSet<Key>,
    pointers: Vec<Pointer>,
    wheel: f32,
}

impl Input {
    /// Whether `key` is held.
    pub fn key_down(&self, key: Key) -> bool {
        self.down.contains(&key)
    }

    /// Whether `key` went down since the previous callback pass.
    pub fn key_just_pressed(&self, key: Key) -> bool {
        self.just_pressed.contains(&key)
    }

    /// Whether `key` came up since the previous callback pass.
    pub fn key_just_released(&self, key: Key) -> bool {
        self.just_released.contains(&key)
    }

    /// Every mouse and touch currently on the viewport.
    pub fn pointers(&self) -> &[Pointer] {
        &self.pointers
    }

    /// Movement of the first pressed pointer, or zero if none is pressed.
    pub fn drag(&self) -> Vec2 {
        self.pointers
            .iter()
            .find(|pointer| pointer.pressed)
            .map_or(Vec2::ZERO, |pointer| pointer.delta)
    }

    /// Growth in pixels of the distance between the first two pressed pointers.
    pub fn pinch(&self) -> f32 {
        let mut pressed = self.pointers.iter().filter(|pointer| pointer.pressed);
        let (Some(a), Some(b)) = (pressed.next(), pressed.next()) else {
            return 0.0;
        };
        let now = a.position.distance(b.position);
        let before = (a.position - a.delta).distance(b.position - b.delta);
        now - before
    }

    /// Wheel movement, positive when scrolling away from the user.
    pub fn wheel(&self) -> f32 {
        self.wheel
    }

    pub(crate) fn apply(&mut self, event: InputEvent) {
        match event {
            InputEvent::KeyDown(key) => {
                if self.down.insert(key) {
                    self.just_pressed.insert(key);
                }
            }
            InputEvent::KeyUp(key) => {
                if self.down.remove(&key) {
                    self.just_released.insert(key);
                }
            }
            InputEvent::PointerDown { id, position } => {
                let pointer = self.pointer(id, position);
                pointer.pressed = true;
                pointer.just_pressed = true;
                pointer.gone = false;
            }
            InputEvent::PointerMove { id, position } => {
                let pointer = self.pointer(id, position);
                pointer.delta += position - pointer.position;
                pointer.position = position;
            }
            InputEvent::PointerUp { id, position } => {
                let pointer = self.pointer(id, position);
                pointer.position = position;
                pointer.pressed = false;
                pointer.just_released = true;
            }
            InputEvent::PointerGone { id } => {
                if let Some(pointer) = self.pointers.iter_mut().find(|p| p.id == id) {
                    pointer.just_released |= pointer.pressed;
                    pointer.pressed = false;
                    pointer.gone = true;
                }
            }
            InputEvent::Wheel(delta) => self.wheel += delta,
            InputEvent::FocusLost => {
                let released: Vec<Key> = self.down.drain().collect();
                self.just_released.extend(released);
                for pointer in &mut self.pointers {
                    pointer.just_released |= pointer.pressed;
                    pointer.pressed = false;
                }
            }
        }
    }

    pub(crate) fn end_pass(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
        self.wheel = 0.0;
        self.pointers.retain(|pointer| !pointer.gone);
        for pointer in &mut self.pointers {
            pointer.delta = Vec2::ZERO;
            pointer.just_pressed = false;
            pointer.just_released = false;
        }
    }

    fn pointer(&mut self, id: i32, position: Vec2) -> &mut Pointer {
        if let Some(at) = self.pointers.iter().position(|p| p.id == id) {
            return &mut self.pointers[at];
        }
        self.pointers.push(Pointer {
            id,
            position,
            delta: Vec2::ZERO,
            pressed: false,
            just_pressed: false,
            just_released: false,
            gone: false,
        });
        self.pointers.last_mut().expect("just pushed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_press_lasts_one_pass_and_repeats_do_not_retrigger() {
        let mut input = Input::default();
        input.apply(InputEvent::KeyDown(Key::W));
        assert!(input.key_just_pressed(Key::W));

        input.end_pass();
        input.apply(InputEvent::KeyDown(Key::W));
        assert!(input.key_down(Key::W));
        assert!(!input.key_just_pressed(Key::W));

        input.apply(InputEvent::KeyUp(Key::W));
        assert!(!input.key_down(Key::W));
        assert!(input.key_just_released(Key::W));
    }

    #[test]
    fn losing_focus_releases_everything_so_keys_do_not_stick() {
        let mut input = Input::default();
        input.apply(InputEvent::KeyDown(Key::W));
        input.apply(InputEvent::PointerDown {
            id: 1,
            position: Vec2::ZERO,
        });
        input.apply(InputEvent::FocusLost);
        assert!(!input.key_down(Key::W));
        assert_eq!(input.drag(), Vec2::ZERO);
    }

    #[test]
    fn drag_counts_only_pressed_pointers_and_resets_each_pass() {
        let mut input = Input::default();
        input.apply(InputEvent::PointerMove {
            id: 1,
            position: Vec2::ZERO,
        });
        input.apply(InputEvent::PointerMove {
            id: 1,
            position: Vec2::X,
        });
        assert_eq!(input.drag(), Vec2::ZERO);

        input.apply(InputEvent::PointerDown {
            id: 1,
            position: Vec2::X,
        });
        input.apply(InputEvent::PointerMove {
            id: 1,
            position: Vec2::new(3.0, 0.0),
        });
        assert_eq!(input.drag(), Vec2::new(3.0, 0.0));

        input.end_pass();
        assert_eq!(input.drag(), Vec2::ZERO);
    }

    #[test]
    fn pinch_is_the_change_in_distance_between_two_touches() {
        let mut input = Input::default();
        for (id, x) in [(1, 0.0), (2, 10.0)] {
            input.apply(InputEvent::PointerDown {
                id,
                position: Vec2::new(x, 0.0),
            });
        }
        input.end_pass();
        input.apply(InputEvent::PointerMove {
            id: 2,
            position: Vec2::new(14.0, 0.0),
        });
        assert_eq!(input.pinch(), 4.0);
    }

    #[test]
    fn a_gone_pointer_is_removed_after_its_release_was_seen() {
        let mut input = Input::default();
        input.apply(InputEvent::PointerDown {
            id: 1,
            position: Vec2::ZERO,
        });
        input.end_pass();
        input.apply(InputEvent::PointerGone { id: 1 });
        assert!(input.pointers()[0].just_released);
        input.end_pass();
        assert!(input.pointers().is_empty());
    }
}
