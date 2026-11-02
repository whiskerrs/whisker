use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;

use glam::{Mat4, Quat, Vec3};
use hecs::World;
use web_time::Instant;
use whisker::runtime::{RuntimeDispatcher, runtime_dispatcher};

use crate::Renderer;

const MAX_DELTA_SECONDS: f32 = 0.1;

/// Position, orientation, and size of an entity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Position.
    pub translation: Vec3,
    /// Orientation.
    pub rotation: Quat,
    /// Per-axis size.
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

/// Marks an entity to be drawn as a triangle.
#[derive(Clone, Copy, Debug, Default)]
pub struct Triangle;

/// Frame timing passed to update callbacks.
#[derive(Clone, Copy, Debug)]
pub struct Time {
    delta_seconds: f32,
}

impl Time {
    /// Seconds since the previous frame, capped so a long stall does not jump the simulation.
    pub fn delta_seconds(&self) -> f32 {
        self.delta_seconds
    }
}

type UpdateCallback<C> = Box<dyn FnMut(&mut World, &Time, Vec<C>)>;

/// Configures a [`Game`] during [`Game::new`].
pub struct App<C> {
    /// The entities of this game.
    pub world: World,
    update: Option<UpdateCallback<C>>,
}

impl<C> App<C> {
    /// Sets the callback that runs once per frame, before drawing.
    ///
    /// It runs inside the Whisker runtime, so it can write signals. Write only
    /// on change, because each write schedules UI work.
    pub fn on_update(&mut self, update: impl FnMut(&mut World, &Time, Vec<C>) + 'static) {
        self.update = Some(Box::new(update));
    }
}

struct RenderTarget {
    renderer: Renderer,
    size: Box<dyn FnMut() -> (u32, u32)>,
}

struct GameState<C> {
    app: RefCell<App<C>>,
    commands: RefCell<Vec<C>>,
    target: RefCell<Option<RenderTarget>>,
    last_frame: Cell<Option<Instant>>,
    running: Cell<bool>,
    dispatcher: RuntimeDispatcher,
}

trait RunningGame {
    fn attach(&self, target: RenderTarget);
    fn detach(&self);
    fn frame(&self);
    fn as_any(self: Rc<Self>) -> Rc<dyn std::any::Any>;
}

thread_local! {
    static GAMES: RefCell<HashMap<i64, Rc<dyn RunningGame>>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<i64> = const { Cell::new(1) };
}

fn find(id: i64) -> Option<Rc<dyn RunningGame>> {
    GAMES.with_borrow(|games| games.get(&id).cloned())
}

// Posting instead of a separate animation-frame loop runs the game inside the runtime and at the
// Host's frame rate, and the id keeps the posted closure `Send`.
fn schedule_frame(dispatcher: &RuntimeDispatcher, id: i64) {
    dispatcher.post(move || {
        if let Some(game) = find(id) {
            game.frame();
        }
    });
}

struct Registered<C> {
    id: i64,
    state: Rc<GameState<C>>,
}

impl<C: 'static> RunningGame for Registered<C> {
    fn attach(&self, target: RenderTarget) {
        *self.state.target.borrow_mut() = Some(target);
        if !self.state.running.replace(true) {
            self.state.last_frame.set(None);
            schedule_frame(&self.state.dispatcher, self.id);
        }
    }

    fn detach(&self) {
        *self.state.target.borrow_mut() = None;
    }

    fn frame(&self) {
        let state = &self.state;
        let mut target = state.target.borrow_mut();
        let Some(target) = target.as_mut() else {
            state.running.set(false);
            return;
        };

        let now = Instant::now();
        let delta_seconds = state.last_frame.replace(Some(now)).map_or(0.0, |last| {
            (now - last).as_secs_f32().min(MAX_DELTA_SECONDS)
        });
        let commands = state.commands.take();
        let mut app = state.app.borrow_mut();
        let App { world, update } = &mut *app;
        if let Some(update) = update {
            update(world, &Time { delta_seconds }, commands);
        }

        let (width, height) = (target.size)();
        let transforms = clip_transforms(world, width, height);
        target.renderer.render(width, height, &transforms);
        schedule_frame(&state.dispatcher, self.id);
    }

    fn as_any(self: Rc<Self>) -> Rc<dyn std::any::Any> {
        self
    }
}

fn clip_transforms(world: &World, width: u32, height: u32) -> Vec<Mat4> {
    let fit = fit_to_viewport(width, height);
    world
        .query::<(&Transform, &Triangle)>()
        .iter()
        .map(|(_, (transform, _))| fit * transform.matrix())
        .collect()
}

fn fit_to_viewport(width: u32, height: u32) -> Mat4 {
    if width == 0 || height == 0 {
        return Mat4::IDENTITY;
    }
    let shortest = width.min(height) as f32;
    Mat4::from_scale(Vec3::new(
        shortest / width as f32,
        shortest / height as f32,
        1.0,
    ))
}

/// A running game, shown by passing it to [`Viewport`](crate::Viewport).
///
/// This is a `Copy` key, so it moves freely into event handlers. `C` is the type of commands
/// the UI sends with [`Game::send`].
pub struct Game<C: 'static = ()> {
    id: i64,
    _commands: PhantomData<fn(C)>,
}

impl<C> Clone for Game<C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C> Copy for Game<C> {}

impl<C: 'static> Game<C> {
    /// Creates a game owned by the current Whisker owner, and removes it with that owner.
    ///
    /// # Panics
    ///
    /// Panics outside a running Whisker runtime.
    pub fn new(setup: impl FnOnce(&mut App<C>)) -> Self {
        let dispatcher = runtime_dispatcher().expect("Game::new requires a Whisker runtime");
        let mut app = App {
            world: World::new(),
            update: None,
        };
        setup(&mut app);
        let id = NEXT_ID.replace(NEXT_ID.get() + 1);
        let state = Rc::new(GameState {
            app: RefCell::new(app),
            commands: RefCell::new(Vec::new()),
            target: RefCell::new(None),
            last_frame: Cell::new(None),
            running: Cell::new(false),
            dispatcher,
        });
        GAMES.with_borrow_mut(|games| games.insert(id, Rc::new(Registered { id, state })));
        whisker::runtime::reactive::on_cleanup(move || {
            GAMES.with_borrow_mut(|games| games.remove(&id));
        });
        Self {
            id,
            _commands: PhantomData,
        }
    }

    /// Queues a command for the next update callback. Does nothing after the game is removed.
    pub fn send(self, command: C) {
        let Some(game) = find(self.id) else {
            return;
        };
        if let Ok(game) = game.as_any().downcast::<Registered<C>>() {
            game.state.commands.borrow_mut().push(command);
        }
    }
}

impl<C> From<Game<C>> for whisker::Signal<String> {
    fn from(game: Game<C>) -> Self {
        game.id.to_string().into()
    }
}

/// Connects a Host's render target to a game and starts its frames.
#[doc(hidden)]
pub fn attach_renderer(id: i64, renderer: Renderer, size: impl FnMut() -> (u32, u32) + 'static) {
    if let Some(game) = find(id) {
        game.attach(RenderTarget {
            renderer,
            size: Box::new(size),
        });
    }
}

/// Disconnects a Host's render target, which stops the game's frames.
#[doc(hidden)]
pub fn detach_renderer(id: i64) {
    if let Some(game) = find(id) {
        game.detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_triangles_are_drawn_and_they_keep_their_aspect_ratio() {
        let mut world = World::new();
        world.spawn((Transform::default(), Triangle));
        world.spawn((Transform::default(),));

        let transforms = clip_transforms(&world, 200, 100);

        assert_eq!(transforms.len(), 1);
        let corner = transforms[0].transform_point3(Vec3::new(1.0, 1.0, 0.0));
        assert_eq!(corner, Vec3::new(0.5, 1.0, 0.0));
    }
}
