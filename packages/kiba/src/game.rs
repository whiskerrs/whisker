use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;

use glam::{Mat4, Quat, Vec3};
use hecs::World;
use web_time::Instant;
use whisker::runtime::{RuntimeDispatcher, runtime_dispatcher};

use crate::{Batch, Mesh, Renderer};

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
    /// Places an entity at `eye` with its -Z axis facing `target`. `up` must not point along that axis.
    pub fn looking_at(eye: Vec3, target: Vec3, up: Vec3) -> Self {
        Self {
            translation: eye,
            rotation: Quat::from_mat4(&Mat4::look_at_rh(eye, target, up).inverse()),
            scale: Vec3::ONE,
        }
    }

    fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

/// Views the scene in perspective from its entity's [`Transform`], looking along -Z.
///
/// Nothing is drawn without a camera, and only one is used if the world has several.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Field of view across the viewport's shorter side, in radians.
    pub fov: f32,
    /// Distance to the near clipping plane, greater than zero.
    pub near: f32,
    /// Distance to the far clipping plane, greater than `near`.
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            fov: 60f32.to_radians(),
            near: 0.1,
            far: 1000.0,
        }
    }
}

impl Camera {
    fn projection(&self, aspect: f32) -> Mat4 {
        let half_tan = (self.fov / 2.0).tan() / aspect.min(1.0);
        // Swapped planes give the reversed depth the renderer expects.
        Mat4::perspective_rh(2.0 * half_tan.atan(), aspect, self.far, self.near)
    }
}

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
pub struct GameSetup<C> {
    /// The entities of this game.
    pub world: World,
    update: Option<UpdateCallback<C>>,
}

impl<C> GameSetup<C> {
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
    setup: RefCell<GameSetup<C>>,
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
        let mut setup = state.setup.borrow_mut();
        let GameSetup { world, update } = &mut *setup;
        if let Some(update) = update {
            update(world, &Time { delta_seconds }, commands);
        }

        let (width, height) = (target.size)();
        let batches = visible_batches(world, width, height);
        target.renderer.render(width, height, &batches);
        schedule_frame(&state.dispatcher, self.id);
    }

    fn as_any(self: Rc<Self>) -> Rc<dyn std::any::Any> {
        self
    }
}

fn visible_batches(world: &World, width: u32, height: u32) -> Vec<Batch> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let mut cameras = world.query::<(&Transform, &Camera)>();
    let Some((_, (eye, camera))) = cameras.iter().next() else {
        return Vec::new();
    };
    let view = Mat4::from_rotation_translation(eye.rotation, eye.translation).inverse();
    let view_projection = camera.projection(width as f32 / height as f32) * view;
    let mut batches = HashMap::<usize, Batch>::new();
    for (_, (transform, mesh)) in world.query::<(&Transform, &Mesh)>().iter() {
        batches
            .entry(mesh.key())
            .or_insert_with(|| Batch::new(mesh.clone(), Vec::new()))
            .transforms
            .push(view_projection * transform.matrix());
    }
    batches.into_values().collect()
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
    pub fn new(configure: impl FnOnce(&mut GameSetup<C>)) -> Self {
        let dispatcher = runtime_dispatcher().expect("Game::new requires a Whisker runtime");
        let mut setup = GameSetup {
            world: World::new(),
            update: None,
        };
        configure(&mut setup);
        let id = NEXT_ID.replace(NEXT_ID.get() + 1);
        let state = Rc::new(GameState {
            setup: RefCell::new(setup),
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

    fn world_with_camera_at(eye: Vec3) -> World {
        let mut world = World::new();
        world.spawn((
            Camera::default(),
            Transform::looking_at(eye, Vec3::ZERO, Vec3::Y),
        ));
        world
    }

    fn assert_near(actual: Vec3, expected: Vec3) {
        assert!(
            actual.abs_diff_eq(expected, 1e-5),
            "{actual} is not near {expected}"
        );
    }

    #[test]
    fn nothing_is_drawn_without_a_camera_or_a_size() {
        let mut world = World::new();
        world.spawn((Transform::default(), Mesh::triangle()));
        assert!(visible_batches(&world, 100, 100).is_empty());

        let mut world = world_with_camera_at(Vec3::Z);
        world.spawn((Transform::default(), Mesh::triangle()));
        assert!(visible_batches(&world, 0, 100).is_empty());
    }

    #[test]
    fn entities_with_the_same_mesh_share_a_batch() {
        let mut world = world_with_camera_at(Vec3::Z);
        let shared = Mesh::cube(1.0);
        world.spawn((Transform::default(), shared.clone()));
        world.spawn((Transform::default(), shared));
        world.spawn((Transform::default(), Mesh::triangle()));
        world.spawn((Transform::default(),));

        let mut counts: Vec<usize> = visible_batches(&world, 100, 100)
            .iter()
            .map(|batch| batch.transforms.len())
            .collect();
        counts.sort_unstable();

        assert_eq!(counts, [1, 2]);
    }

    #[test]
    fn nearer_points_have_greater_depth_inside_the_clip_range() {
        let project = |distance: f32| {
            Camera::default()
                .projection(1.0)
                .project_point3(Vec3::new(0.0, 0.0, -distance))
                .z
        };

        assert!((project(0.1) - 1.0).abs() < 1e-5);
        assert!(project(1.0) > project(10.0));
        assert!(project(1000.0).abs() < 1e-5);
    }

    #[test]
    fn the_camera_target_lands_at_the_center_from_any_direction() {
        for eye in [Vec3::Z, Vec3::X, Vec3::new(3.0, 2.0, -1.0)] {
            let mut world = world_with_camera_at(eye);
            world.spawn((Transform::default(), Mesh::triangle()));

            let clip =
                visible_batches(&world, 100, 100)[0].transforms[0].project_point3(Vec3::ZERO);

            assert_near(clip * Vec3::new(1.0, 1.0, 0.0), Vec3::ZERO);
        }
    }

    #[test]
    fn farther_entities_look_smaller() {
        let height_at = |distance: f32| {
            let mut world = world_with_camera_at(Vec3::Z * distance);
            world.spawn((Transform::default(), Mesh::triangle()));
            visible_batches(&world, 100, 100)[0].transforms[0]
                .project_point3(Vec3::Y)
                .y
        };

        assert_near(
            Vec3::splat(height_at(4.0)),
            Vec3::splat(height_at(2.0) / 2.0),
        );
    }

    #[test]
    fn the_field_of_view_spans_the_shorter_side_in_any_aspect_ratio() {
        // At distance 1, a half field of view of 45 degrees reaches exactly 1 unit off-center.
        let camera = Camera {
            fov: 90f32.to_radians(),
            ..Camera::default()
        };
        let point = Vec3::new(1.0, 1.0, -1.0);
        let project = |aspect| camera.projection(aspect).project_point3(point);

        assert_near(
            project(2.0) * Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.5, 1.0, 0.0),
        );
        assert_near(
            project(0.5) * Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(1.0, 0.5, 0.0),
        );
    }
}
