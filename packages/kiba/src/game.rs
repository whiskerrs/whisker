use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use glam::{Mat4, Quat, Vec3, Vec4};
use hecs::World;
use web_time::Instant;
use whisker::runtime::{RuntimeDispatcher, runtime_dispatcher};

use crate::animation::{Animator, Pose, run_animators};
use crate::controller::run_controllers;
use crate::input::{InputEvent, Key};
use crate::lights::{LightUniform, MAX_LIGHTS, PointLight, SpotLight};
use crate::particles::{extract_particles, run_particles};
use crate::probes::ProbeVolume;
use crate::quality::{Governor, Quality, Tier};
use crate::reflection::{Reflection, ReflectionId, ReflectionProbe, face_view_projections};
use crate::renderer::{Batch, Cascade, Effects, Frame, Instance, Shadows};
use crate::sky::{Fog, Sky};
use crate::stream::run_streams;
use crate::{AlphaMode, Input, Material, Mesh, Model, Renderer, Restyle, Shader};

const MAX_DELTA_SECONDS: f32 = 0.1;
const DEFAULT_FIXED_HZ: f32 = 60.0;

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
    /// Places an entity at `translation`, unturned and unscaled.
    pub fn at(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::default()
        }
    }

    /// Places an entity at `eye` with its -Z axis facing `target`. `up` must not point along that axis.
    pub fn looking_at(eye: Vec3, target: Vec3, up: Vec3) -> Self {
        Self {
            translation: eye,
            rotation: Quat::from_mat4(&Mat4::look_at_rh(eye, target, up).inverse()),
            scale: Vec3::ONE,
        }
    }

    /// The matrix that places a point of the entity's space in the world.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    /// The way from `self` at 0 to `other` at 1, turning along the shorter arc.
    pub fn lerp(&self, other: &Self, amount: f32) -> Self {
        Self {
            translation: self.translation.lerp(other.translation, amount),
            rotation: self.rotation.slerp(other.rotation, amount),
            scale: self.scale.lerp(other.scale, amount),
        }
    }
}

/// Views the scene in perspective from its entity's [`Transform`], looking along -Z.
///
/// Spawn it with a [`Transform`]. Nothing is drawn without a camera, and only one is used if the
/// world has several. The default has a 60 degree field of view, planes at 0.1 and 1000, and
/// exposure 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Field of view across the viewport's shorter side, in radians.
    pub fov: f32,
    /// Distance to the near clipping plane, greater than zero.
    pub near: f32,
    /// Distance to the far clipping plane, greater than `near`.
    pub far: f32,
    /// Multiplies the light reaching the camera before tone mapping.
    pub exposure: f32,
    /// Blurs what is nearer or farther than a distance, as a real lens does. `None` keeps everything sharp.
    pub depth_of_field: Option<DepthOfField>,
}

/// How a [`Camera`] blurs what is out of focus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthOfField {
    /// Distance from the camera that stays sharp.
    pub focus_distance: f32,
    /// How wide the blur of something far behind the focus grows, as a share of the view's height.
    /// Something at half the focus distance blurs as wide.
    pub blur: f32,
}

impl Default for DepthOfField {
    /// Focused 5 units away, with a blur of 1% of the view's height.
    fn default() -> Self {
        Self {
            focus_distance: 5.0,
            blur: 0.01,
        }
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            fov: 60f32.to_radians(),
            near: 0.1,
            far: 1000.0,
            exposure: 1.0,
            depth_of_field: None,
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

/// Lights the scene from one direction, as an entity of its own.
///
/// Spawn it alone, as `(Sun::default(),)`. Surfaces are black without a sun, and only one is used
/// if the world has several. The default is warm white from the upper right behind the camera's
/// default position, with a dim blue ambient, and shadows out to 80 units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    /// Direction the light travels, of any length.
    pub direction: Vec3,
    /// Linear RGB light on a surface facing the sun. About 1 is full daylight and the scale for every
    /// other light: an emissive of 1 glows as bright as white paper in the sun.
    pub color: Vec3,
    /// Linear RGB light on every surface, whichever way it faces. A [`Sky`] replaces this.
    pub ambient: Vec3,
    /// How far from the camera surfaces receive shadows, fading out over the last stretch. Zero turns
    /// shadows off.
    pub shadow_distance: f32,
}

impl Default for Sun {
    fn default() -> Self {
        Self {
            direction: Vec3::new(-0.5, -1.0, -0.7),
            color: Vec3::new(1.0, 0.95, 0.85),
            ambient: Vec3::new(0.12, 0.14, 0.18),
            shadow_distance: 80.0,
        }
    }
}

impl Sun {
    const DARK: Self = Self {
        direction: Vec3::NEG_Y,
        color: Vec3::ZERO,
        ambient: Vec3::ZERO,
        shadow_distance: 0.0,
    };

    /// Shows surface colors nearly as they are.
    pub(crate) const UNLIT: Self = Self {
        direction: Vec3::NEG_Y,
        color: Vec3::ZERO,
        ambient: Vec3::ONE,
        shadow_distance: 0.0,
    };
}

/// Smooths an entity moved in `on_fixed_update` between fixed steps.
///
/// Add it next to the entity's [`Transform`]. The `Transform` keeps the latest fixed-step value,
/// and only drawing blends it with the previous one. To follow such an entity from `on_update`,
/// read [`Context::drawn_transform`], because its `Transform` would make the follower jitter.
#[derive(Clone, Copy, Debug, Default)]
pub struct Interpolated {
    previous: Option<Transform>,
}

/// Timing passed to callbacks.
#[derive(Clone, Copy, Debug)]
pub struct Time {
    delta_seconds: f32,
    elapsed_seconds: f64,
}

impl Time {
    /// Seconds this callback covers: the fixed step in `on_fixed_update`, otherwise the time since
    /// the previous frame, capped so a long stall does not jump the simulation.
    pub fn delta_seconds(&self) -> f32 {
        self.delta_seconds
    }

    /// Seconds of game time since the first frame.
    pub fn elapsed_seconds(&self) -> f64 {
        self.elapsed_seconds
    }

    #[cfg(test)]
    pub(crate) fn for_test(delta_seconds: f32) -> Self {
        Self {
            delta_seconds,
            elapsed_seconds: 0.0,
        }
    }
}

/// Cost of the previous frame on the CPU. GPU time is not included.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStats {
    /// Milliseconds spent in commands, fixed updates, and updates.
    pub update_ms: f32,
    /// Milliseconds spent collecting what to draw, including culling.
    pub extract_ms: f32,
    /// Milliseconds spent encoding and submitting GPU work.
    pub render_ms: f32,
    /// Separate draw calls, one per mesh and texture set.
    pub draw_calls: u32,
    /// Instances drawn after culling.
    pub instances: u32,
    /// Triangles drawn after culling.
    pub triangles: u64,
    /// Bytes of meshes and textures held on the GPU.
    pub gpu_bytes: u64,
    /// The rendering features in use.
    pub tier: Tier,
    /// Internal resolution as a fraction of the viewport.
    pub render_scale: f32,
}

/// What a callback can see and change.
pub struct Context<'a> {
    /// The entities of this game.
    pub world: &'a mut World,
    /// Timing of this callback.
    pub time: Time,
    /// Keyboard and pointer state.
    pub input: &'a Input,
    /// Viewport size in physical pixels, zero before the first frame.
    pub viewport: (u32, u32),
    /// Physical pixels per layout pixel, for sizing UI drawn over the viewport.
    pub pixel_ratio: f32,
    /// Cost of the previous frame.
    pub stats: FrameStats,
    alpha: f32,
    governor: &'a mut Governor,
}

impl Context<'_> {
    /// Where `entity` will be drawn this frame, or `None` if it has no [`Transform`].
    pub fn drawn_transform(&self, entity: hecs::Entity) -> Option<Transform> {
        let transform = *self.world.get::<&Transform>(entity).ok()?;
        let motion = self.world.get::<&Interpolated>(entity).ok();
        Some(shown(&transform, motion.as_deref(), self.alpha))
    }

    /// Keeps the game at one [`Tier`] from the next frame on, as [`GameSetup::set_tier`] does.
    pub fn set_tier(&mut self, tier: Tier) {
        self.governor.lock(tier);
    }
}

type Callback = Box<dyn FnMut(&mut Context<'_>)>;
type CommandCallback<C> = Box<dyn FnMut(&mut Context<'_>, C)>;

/// Configures a [`Game`] during [`Game::new`].
///
/// Each frame runs the callbacks in this order: commands, fixed updates, updates, drawing. Updates
/// can be added several times, and run in the order they were added.
pub struct GameSetup<C> {
    /// The entities of this game.
    pub world: World,
    on_command: Option<CommandCallback<C>>,
    on_fixed_update: Vec<Callback>,
    on_update: Vec<Callback>,
    fixed_delta: f32,
    accumulator: f32,
    elapsed: f64,
    fixed_elapsed: f64,
    frame_input: Input,
    fixed_input: Input,
    viewport: (u32, u32),
    pixel_ratio: f32,
    stats: FrameStats,
    governor: Governor,
    pointer_lock: bool,
}

impl<C> GameSetup<C> {
    fn new() -> Self {
        Self {
            world: World::new(),
            on_command: None,
            on_fixed_update: Vec::new(),
            on_update: Vec::new(),
            fixed_delta: 1.0 / DEFAULT_FIXED_HZ,
            accumulator: 0.0,
            elapsed: 0.0,
            fixed_elapsed: 0.0,
            frame_input: Input::default(),
            fixed_input: Input::default(),
            viewport: (0, 0),
            pixel_ratio: 1.0,
            stats: FrameStats::default(),
            governor: Governor::default(),
            pointer_lock: false,
        }
    }

    /// Hides the mouse and keeps it on the viewport from each click until Escape, for mouse look. The
    /// mouse's [`Pointer::delta`](crate::Pointer::delta) keeps reporting movement while its position stays put.
    ///
    /// The click that locks the pointer also arrives as [`Key::MouseLeft`], before
    /// [`Input::pointer_locked`](crate::Input::pointer_locked) turns true.
    pub fn lock_pointer(&mut self) {
        self.pointer_lock = true;
    }

    /// Keeps the game at one [`Tier`] from the start, for a known device. [`Context::set_tier`] does the
    /// same from a callback, such as a settings menu's command.
    ///
    /// Without this, the game starts at the top tier and steps down when frames take too long. It
    /// never climbs back, because a brief quiet moment says little about the next busy one.
    /// Internal resolution keeps adapting either way.
    pub fn set_tier(&mut self, tier: Tier) {
        self.governor.lock(tier);
    }

    /// Sets the callback for commands sent with [`Game::send`], which runs before the frame's
    /// updates. A later call replaces it, because a command has only one owner.
    pub fn on_command(&mut self, callback: impl FnMut(&mut Context<'_>, C) + 'static) {
        self.on_command = Some(Box::new(callback));
    }

    /// Adds a callback that runs at a steady rate, for physics and gameplay that must not depend on
    /// the display. It can run zero or several times in a frame.
    pub fn on_fixed_update(&mut self, callback: impl FnMut(&mut Context<'_>) + 'static) {
        self.on_fixed_update.push(Box::new(callback));
    }

    /// Adds a callback that runs once per frame, before drawing.
    ///
    /// It runs inside the Whisker runtime, so it can write signals. Write only
    /// on change, because each write schedules UI work.
    pub fn on_update(&mut self, callback: impl FnMut(&mut Context<'_>) + 'static) {
        self.on_update.push(Box::new(callback));
    }

    /// Names something the player does, and the keys that do it, for
    /// [`Input::action_down`] and its relatives. Binding a name again replaces its keys.
    ///
    /// Reading an action that was never bound panics in debug builds and reads as released in
    /// release builds.
    pub fn bind_action(&mut self, action: &'static str, keys: &[Key]) {
        self.frame_input.bind(action, keys);
        self.fixed_input.bind(action, keys);
    }

    /// Sets how many times per second `on_fixed_update` runs. The default is 60.
    ///
    /// Each step's [`Time::delta_seconds`] is exactly `1 / hz`. A frame advances game time by at most
    /// 0.1 seconds, so after a longer stall the game falls behind the clock rather than running a burst
    /// of steps.
    ///
    /// # Panics
    ///
    /// Panics if `hz` is not greater than zero.
    pub fn set_fixed_rate(&mut self, hz: f32) {
        assert!(hz > 0.0, "the fixed rate must be greater than zero");
        self.fixed_delta = 1.0 / hz;
    }

    fn apply_input(&mut self, event: InputEvent) {
        self.frame_input.apply(event);
        self.fixed_input.apply(event);
    }

    /// Runs one frame of simulation and returns how far drawing is between the last two fixed steps.
    fn advance(&mut self, delta_seconds: f32, commands: Vec<C>) -> f32 {
        let viewport = self.viewport;
        let pixel_ratio = self.pixel_ratio;
        let stats = self.stats;
        self.elapsed += f64::from(delta_seconds);
        let frame_time = Time {
            delta_seconds,
            elapsed_seconds: self.elapsed,
        };
        for command in commands {
            let mut context = Context {
                world: &mut self.world,
                time: frame_time,
                input: &self.frame_input,
                viewport,
                pixel_ratio,
                stats,
                alpha: 1.0,
                governor: &mut self.governor,
            };
            if let Some(callback) = &mut self.on_command {
                callback(&mut context, command);
            }
        }

        self.accumulator += delta_seconds;
        while self.accumulator >= self.fixed_delta {
            self.accumulator -= self.fixed_delta;
            self.fixed_elapsed += f64::from(self.fixed_delta);
            for (_, (transform, interpolated)) in
                self.world.query_mut::<(&Transform, &mut Interpolated)>()
            {
                interpolated.previous = Some(*transform);
            }
            let time = Time {
                delta_seconds: self.fixed_delta,
                elapsed_seconds: self.fixed_elapsed,
            };
            for callback in &mut self.on_fixed_update {
                callback(&mut Context {
                    world: &mut self.world,
                    time,
                    input: &self.fixed_input,
                    viewport,
                    pixel_ratio,
                    stats,
                    alpha: 1.0,
                    governor: &mut self.governor,
                });
            }
            #[cfg(feature = "physics")]
            crate::physics::run_physics(&mut self.world, self.fixed_delta);
            self.fixed_input.end_pass();
        }

        let alpha = self.accumulator / self.fixed_delta;
        for callback in &mut self.on_update {
            callback(&mut Context {
                world: &mut self.world,
                time: frame_time,
                input: &self.frame_input,
                viewport,
                pixel_ratio,
                stats,
                alpha,
                governor: &mut self.governor,
            });
        }
        run_animators(&mut self.world, frame_time.delta_seconds());
        run_particles(&mut self.world, frame_time.delta_seconds());
        run_streams(&mut self.world);
        if let Some((_, (eye, _))) = self
            .world
            .query_mut::<(&Transform, &Camera)>()
            .into_iter()
            .next()
        {
            whisker_mixer::set_listener(
                eye.translation.to_array(),
                (eye.rotation * Vec3::NEG_Z).to_array(),
                (eye.rotation * Vec3::Y).to_array(),
            );
        }
        run_controllers(&mut self.world, &frame_time, &self.frame_input);
        self.frame_input.end_pass();
        alpha
    }
}

struct RenderTarget {
    renderer: Renderer,
    size: Box<dyn FnMut() -> (u32, u32, f32)>,
}

struct GameState<C> {
    setup: RefCell<GameSetup<C>>,
    commands: RefCell<Vec<C>>,
    input: RefCell<Vec<InputEvent>>,
    target: RefCell<Option<RenderTarget>>,
    last_frame: Cell<Option<Instant>>,
    running: Cell<bool>,
    dispatcher: RuntimeDispatcher,
}

trait RunningGame {
    fn attach(&self, target: RenderTarget);
    fn detach(&self);
    fn frame(&self);
    fn input(&self, event: InputEvent);
    fn wants_pointer_lock(&self) -> bool;
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
        let real_delta = state
            .last_frame
            .replace(Some(now))
            .map_or(0.0, |last| (now - last).as_secs_f32());
        let delta_seconds = real_delta.min(MAX_DELTA_SECONDS);
        let commands = state.commands.take();
        let mut setup = state.setup.borrow_mut();
        for event in state.input.take() {
            setup.apply_input(event);
        }
        let (width, height, pixel_ratio) = (target.size)();
        setup.viewport = (width, height);
        setup.pixel_ratio = pixel_ratio;
        let alpha = setup.advance(delta_seconds, commands);

        let updated = Instant::now();
        setup.governor.resize(width, height);
        setup.governor.update(real_delta);
        let quality = setup.governor.quality(width, height);
        let mut frame = extract_frame(&setup.world, width, height, alpha, &quality);
        frame.effects.time = setup.elapsed as f32;
        if frame
            .effects
            .reflection
            .as_ref()
            .is_some_and(|r| target.renderer.needs_capture(&r.id))
        {
            add_capture(&mut frame, &setup.world, alpha);
        }
        let extracted = Instant::now();
        target.renderer.render(width, height, &frame);
        let rendered = Instant::now();
        let drawn = || {
            let casters = frame
                .shadows
                .iter()
                .flat_map(|s| s.casters.iter().flatten());
            frame.batches.iter().chain(casters)
        };
        setup.stats = FrameStats {
            update_ms: (updated - now).as_secs_f32() * 1000.0,
            extract_ms: (extracted - updated).as_secs_f32() * 1000.0,
            render_ms: (rendered - extracted).as_secs_f32() * 1000.0,
            draw_calls: drawn().count() as u32,
            instances: drawn().map(|b| b.instances.len() as u32).sum(),
            gpu_bytes: 0,
            tier: setup.governor.tier(),
            render_scale: frame.effects.quality.render_scale,
            triangles: drawn()
                .map(|b| b.instances.len() as u64 * b.mesh.data().indices.len() as u64 / 3)
                .sum(),
        };
        setup.stats.gpu_bytes = target.renderer.gpu_bytes();
        schedule_frame(&state.dispatcher, self.id);
    }

    fn input(&self, event: InputEvent) {
        self.state.input.borrow_mut().push(event);
    }

    fn wants_pointer_lock(&self) -> bool {
        self.state
            .setup
            .try_borrow()
            .is_ok_and(|setup| setup.pointer_lock)
    }

    fn as_any(self: Rc<Self>) -> Rc<dyn std::any::Any> {
        self
    }
}

// Weighs cascade ends toward even ratios over even steps, so nearby surfaces get most of the resolution.
const CASCADE_SPLIT_LAMBDA: f32 = 0.75;
// Reaches past the view so that tall casters outside it still reach the map.
const CASTER_MARGIN: f32 = 100.0;

fn cascades(
    eye: &Transform,
    camera: &Camera,
    aspect: f32,
    sun: &Sun,
    quality: &Quality,
) -> Option<Shadows> {
    let direction = sun.direction.try_normalize()?;
    if sun.shadow_distance <= 0.0 {
        return None;
    }
    let projection = camera.projection(aspect);
    let (tan_x, tan_y) = (1.0 / projection.x_axis.x, 1.0 / projection.y_axis.y);
    let corner_reach = Vec3::new(tan_x, tan_y, 1.0).length();
    let forward = eye.rotation * Vec3::NEG_Z;
    let far = sun.shadow_distance.min(camera.far);
    let up = if direction.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let map_size = quality.shadow_map_size;

    let slice = |start: f32, end: f32| {
        let middle = (start + end) / 2.0;
        // The farther corner bounds the slice, so the sphere never changes size as the camera turns.
        let radius = (end * corner_reach - middle)
            .max(middle - start)
            .max(end - middle);
        let center = eye.translation + forward * middle;

        let rotation = Mat4::look_to_rh(Vec3::ZERO, direction, up);
        // Snapping to whole shadow texels keeps edges from crawling as the camera moves.
        let texel = 2.0 * radius / map_size as f32;
        let light_space = rotation.transform_point3(center);
        let snapped = (light_space / texel).round() * texel;
        let light_view = Mat4::from_translation(-snapped) * rotation;
        let projection = Mat4::orthographic_rh(
            -radius,
            radius,
            -radius,
            radius,
            -radius - CASTER_MARGIN,
            radius,
        );
        Cascade {
            view_projection: projection * light_view,
            end,
            texel,
            depth_range: 2.0 * radius + CASTER_MARGIN,
        }
    };
    let count = quality
        .shadow_cascades
        .clamp(1, crate::renderer::MAX_CASCADES);
    let near = camera.near;
    let end = |i: usize| {
        let fraction = i as f32 / count as f32;
        let even_ratio = near * (far / near).powf(fraction);
        let even_step = near + (far - near) * fraction;
        CASCADE_SPLIT_LAMBDA * even_ratio + (1.0 - CASCADE_SPLIT_LAMBDA) * even_step
    };
    Some(Shadows {
        cascades: (0..count)
            .map(|i| slice(end(i).max(near), end(i + 1)))
            .collect(),
        map_size,
        soft: quality.soft_shadows,
        casters: vec![Vec::new(); count],
    })
}

/// Planes facing inward, each as a normal and a distance.
struct Frustum([Vec4; 6]);

impl Frustum {
    fn new(view_projection: &Mat4) -> Self {
        let matrix = view_projection.transpose();
        let [x, y, z, w] = [matrix.x_axis, matrix.y_axis, matrix.z_axis, matrix.w_axis];
        Self([w + x, w - x, w + y, w - y, z, w - z].map(|plane| plane / plane.truncate().length()))
    }

    fn contains_sphere(&self, center: Vec3, radius: f32) -> bool {
        let center = center.extend(1.0);
        self.0.iter().all(|plane| plane.dot(center) >= -radius)
    }

    fn contains(&self, mesh: &Mesh, transform: &Mat4, material: &Material) -> bool {
        let data = mesh.data();
        let reach = material.shader.as_ref().map_or(1.0, Shader::culling_scale);
        let center = transform.transform_point3(data.bounds_center);
        let scale = transform
            .x_axis
            .truncate()
            .length_squared()
            .max(transform.y_axis.truncate().length_squared())
            .max(transform.z_axis.truncate().length_squared())
            .sqrt();
        self.contains_sphere(center, data.bounds_radius * scale * reach)
    }
}

fn shown(transform: &Transform, motion: Option<&Interpolated>, alpha: f32) -> Transform {
    match motion.and_then(|motion| motion.previous) {
        Some(previous) => previous.lerp(transform, alpha),
        None => *transform,
    }
}

/// A mesh, textures, pipeline, joint palette, and shader with its parameters, which entities must
/// share to draw as one batch.
type BatchKey = (usize, [usize; 4], (u8, bool), usize, u64);
// Every drawn instance hashes its key once per pass, where SipHash's resistance to attack buys nothing.
type Batches = rustc_hash::FxHashMap<BatchKey, Batch>;

/// Adds one instance to the batch that draws `mesh` with `batch_material`'s textures and pipeline.
fn add_instance(
    batches: &mut Batches,
    mesh: &Mesh,
    batch_material: &Material,
    material: &Material,
    transform: Mat4,
    palette: Option<&Arc<[Mat4]>>,
) {
    // A posed mesh draws alone, because instances in a batch share their joint matrices.
    let owner = palette.map_or(0, |palette| palette.as_ptr() as usize);
    batches
        .entry((
            mesh.key(),
            batch_material.texture_keys(),
            batch_material.pipeline_key(),
            owner,
            batch_material.shader_key(),
        ))
        .or_insert_with(|| Batch {
            mesh: mesh.clone(),
            material: batch_material.clone(),
            palette: palette.cloned(),
            instances: Vec::new(),
        })
        .instances
        .push(Instance::new(transform, material));
}

/// Calls `visit` for every mesh the world draws, with models choosing their detail by distance from `eye`.
fn visit_drawn(
    world: &World,
    alpha: f32,
    eye: Vec3,
    mut visit: impl FnMut(&Mesh, Mat4, &Material, Option<&Arc<[Mat4]>>),
) {
    for (_, (transform, motion, mesh, material)) in world
        .query::<(&Transform, Option<&Interpolated>, &Mesh, Option<&Material>)>()
        .iter()
    {
        visit(
            mesh,
            shown(transform, motion, alpha).matrix(),
            material.unwrap_or(&Material::default()),
            None,
        );
    }
    for (_, (transform, motion, model, animator, pose, restyle)) in world
        .query::<(
            &Transform,
            Option<&Interpolated>,
            &Model,
            Option<&Animator>,
            Option<&Pose>,
            Option<&Restyle>,
        )>()
        .iter()
    {
        let matrix = shown(transform, motion, alpha).matrix();
        let parts = model.parts();
        let rig = model.data().and_then(|data| data.rig.as_ref());
        let posed = rig
            .filter(|_| {
                animator.is_some() || pose.is_some() || parts.iter().any(|part| part.skin.is_some())
            })
            .map(|rig| match pose {
                Some(pose) => rig.posed(&pose.nodes),
                None => rig.posed(&rig.animated(animator)),
            });
        for part in parts {
            let center = matrix * part.transform * part.mesh.data().bounds_center.extend(1.0);
            let mesh = part.mesh_at(center.truncate().distance(eye));
            let palette = posed
                .as_ref()
                .zip(part.skin)
                .map(|(posed, skin)| &posed.palettes[skin]);
            // Joint matrices already place a skinned mesh within the model.
            let within = match (palette, &posed, part.node) {
                (Some(_), _, _) => Mat4::IDENTITY,
                (None, Some(posed), Some(node)) => posed.nodes[node],
                _ => part.transform,
            };
            match restyle {
                Some(restyle) => visit(
                    mesh,
                    matrix * within,
                    &restyle.apply(&part.material),
                    palette,
                ),
                None => visit(mesh, matrix * within, &part.material, palette),
            }
        }
    }
}

pub(crate) fn extract_frame(
    world: &World,
    width: u32,
    height: u32,
    alpha: f32,
    quality: &Quality,
) -> Frame {
    let sun = world
        .query::<&Sun>()
        .iter()
        .next()
        .map_or(Sun::DARK, |(_, sun)| *sun);
    let mut cameras = world.query::<(&Transform, &Camera, Option<&Interpolated>)>();
    let camera = cameras.iter().next().filter(|_| width > 0 && height > 0);
    let Some((_, (eye, camera, eye_motion))) = camera else {
        return Frame {
            view_projection: Mat4::IDENTITY,
            camera_position: Vec3::ZERO,
            exposure: 1.0,
            sun,
            sky: None,
            probes: None,
            shadows: None,
            batches: Vec::new(),
            effects: Effects::default(),
        };
    };
    let eye = shown(eye, eye_motion, alpha);
    let view = Mat4::from_rotation_translation(eye.rotation, eye.translation).inverse();
    let projection = camera.projection(width as f32 / height as f32);
    let view_projection = projection * view;
    let mut batches = Batches::default();
    let frustum = Frustum::new(&view_projection);
    let shadows = cascades(&eye, camera, width as f32 / height as f32, &sun, quality);
    let caster_frustums = shadows.as_ref().map(|shadows| {
        shadows
            .cascades
            .iter()
            .map(|cascade| Frustum::new(&cascade.view_projection))
            .collect::<Vec<_>>()
    });
    let mut casters: Vec<Batches> =
        vec![Batches::default(); caster_frustums.as_ref().map_or(0, Vec::len)];
    visit_drawn(
        world,
        alpha,
        eye.translation,
        |mesh, transform, material, palette| {
            // Only a masked caster needs its textures, so the others share batches across materials.
            let caster = match material.alpha_mode {
                _ if !material.casts_shadow => None,
                AlphaMode::Blend => None,
                AlphaMode::Mask { .. } => Some(material.clone()),
                AlphaMode::Opaque => Some(Material {
                    double_sided: material.double_sided,
                    shader: material.shader.clone(),
                    parameters: material.parameters,
                    ..Material::default()
                }),
            };
            // Off-screen entities still throw shadows into the view.
            for (cascade, frustum) in caster_frustums.iter().flatten().enumerate() {
                let Some(caster) = caster.as_ref() else {
                    break;
                };
                if frustum.contains(mesh, &transform, material) {
                    add_instance(
                        &mut casters[cascade],
                        mesh,
                        caster,
                        material,
                        transform,
                        palette,
                    );
                }
            }
            if frustum.contains(mesh, &transform, material) {
                add_instance(&mut batches, mesh, material, material, transform, palette);
            }
        },
    );
    Frame {
        view_projection,
        camera_position: eye.translation,
        exposure: camera.exposure,
        sun,
        sky: world.query::<&Sky>().iter().next().map(|(_, sky)| *sky),
        probes: world
            .query::<&ProbeVolume>()
            .iter()
            .map(|(_, probes)| probes)
            .find(|probes| probes.contains(eye.translation))
            .cloned(),
        shadows: shadows.map(|shadows| Shadows {
            casters: casters
                .into_iter()
                .map(|batches| batches.into_values().collect())
                .collect(),
            ..shadows
        }),
        batches: batches.into_values().collect(),
        effects: Effects {
            quality: *quality,
            projection,
            depth_of_field: camera.depth_of_field,
            camera_right: eye.rotation * Vec3::X,
            camera_up: eye.rotation * Vec3::Y,
            fog: world.query::<&Fog>().iter().next().map(|(_, fog)| *fog),
            particles: extract_particles(world),
            lights: extract_lights(world, &frustum, eye.translation, alpha),
            reflection: world
                .query::<(&Transform, &ReflectionProbe)>()
                .iter()
                .map(|(entity, (transform, probe))| ReflectionId {
                    entity: entity.to_bits().get(),
                    revision: probe.revision(),
                    center: transform.translation,
                    size: probe.size,
                })
                .find(|id| id.contains(eye.translation))
                .map(|id| Reflection { id, faces: None }),
            time: 0.0,
        },
    }
}

/// Gives `frame`'s reflection probe what to draw into each of its capture layers.
pub(crate) fn add_capture(frame: &mut Frame, world: &World, alpha: f32) {
    let Some(reflection) = frame.effects.reflection.as_mut() else {
        return;
    };
    let center = reflection.id.center;
    let frustums = face_view_projections(center).map(|vp| Frustum::new(&vp));
    let mut faces: [Batches; 6] = Default::default();
    visit_drawn(
        world,
        alpha,
        center,
        |mesh, transform, material, palette| {
            // Blending needs sorting the capture does not do, and glass seen in a reflection is faint anyway.
            if material.alpha_mode == AlphaMode::Blend {
                return;
            }
            for (face, frustum) in frustums.iter().enumerate() {
                if frustum.contains(mesh, &transform, material) {
                    add_instance(
                        &mut faces[face],
                        mesh,
                        material,
                        material,
                        transform,
                        palette,
                    );
                }
            }
        },
    );
    reflection.faces = Some(Box::new(
        faces.map(|batches| batches.into_values().collect()),
    ));
}

fn extract_lights(world: &World, frustum: &Frustum, eye: Vec3, alpha: f32) -> Vec<LightUniform> {
    let points = world
        .query::<(&Transform, Option<&Interpolated>, &PointLight)>()
        .iter()
        .map(|(_, (transform, motion, light))| {
            LightUniform::point(&shown(transform, motion, alpha), light)
        })
        .collect::<Vec<_>>();
    let spots = world
        .query::<(&Transform, Option<&Interpolated>, &SpotLight)>()
        .iter()
        .map(|(_, (transform, motion, light))| {
            LightUniform::spot(&shown(transform, motion, alpha), light)
        })
        .collect::<Vec<_>>();
    let mut lights: Vec<_> = points
        .into_iter()
        .chain(spots)
        .filter(|light| frustum.contains_sphere(light.center(), light.range()))
        .collect();
    lights.sort_by(|a, b| {
        a.center()
            .distance_squared(eye)
            .total_cmp(&b.center().distance_squared(eye))
    });
    lights.truncate(MAX_LIGHTS);
    lights
}

/// A running game, shown by passing it to [`Viewport`](crate::Viewport).
///
/// This is a `Copy` key, so it moves freely into event handlers. `C` is the type of commands
/// the UI sends with [`Game::send`]; write `Game::<()>::new` for a game without commands.
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
        let mut setup = GameSetup::new();
        configure(&mut setup);
        let id = NEXT_ID.replace(NEXT_ID.get() + 1);
        let state = Rc::new(GameState {
            setup: RefCell::new(setup),
            commands: RefCell::new(Vec::new()),
            input: RefCell::new(Vec::new()),
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

    /// Holds or releases an action from outside the keyboard, such as an on-screen button.
    ///
    /// Bind the action first with [`GameSetup::bind_action`], with no keys if a button is its only
    /// source. Release it on touch cancel too, or it stays held. Does nothing after the game is
    /// removed.
    pub fn set_action(self, action: &'static str, down: bool) {
        let Some(game) = find(self.id) else {
            return;
        };
        game.input(InputEvent::Action { name: action, down });
    }

    /// Queues a command for [`GameSetup::on_command`] at the start of the next frame, before its fixed
    /// updates. Does nothing after the game is removed.
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
pub fn attach_renderer(
    id: i64,
    renderer: Renderer,
    size: impl FnMut() -> (u32, u32, f32) + 'static,
) {
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

/// Passes a Host's input to a game.
#[doc(hidden)]
pub fn send_input(id: i64, event: InputEvent) {
    if let Some(game) = find(id) {
        game.input(event);
    }
}

/// Whether a Host should lock the pointer to its viewport when it is clicked.
#[doc(hidden)]
pub fn wants_pointer_lock(id: i64) -> bool {
    find(id).is_some_and(|game| game.wants_pointer_lock())
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
    fn the_frame_carries_the_sun_of_the_world_or_darkness() {
        let mut world = world_with_camera_at(Vec3::Z);
        assert_eq!(
            extract_frame(&world, 100, 100, 1.0, &Quality::default()).sun,
            Sun::DARK
        );

        world.spawn((Sun::default(),));
        assert_eq!(
            extract_frame(&world, 100, 100, 1.0, &Quality::default()).sun,
            Sun::default()
        );
    }

    #[test]
    fn the_frame_takes_the_nearest_lights_in_view() {
        let mut world = world_with_camera_at(Vec3::Z * 10.0);
        let at = Transform::at;
        world.spawn((at(Vec3::Z * 100.0), PointLight::default()));
        for i in 0..MAX_LIGHTS + 4 {
            world.spawn((at(Vec3::X * i as f32), PointLight::default()));
        }
        world.spawn((at(Vec3::Z * 5.0), SpotLight::default()));

        let lights = extract_frame(&world, 100, 100, 1.0, &Quality::default())
            .effects
            .lights;
        assert_eq!(lights.len(), MAX_LIGHTS);
        assert_eq!(lights[0].center(), Vec3::Z * 5.0);
        assert!(lights.iter().all(|light| light.center().z < 50.0));
    }

    #[test]
    fn the_frame_uses_the_reflection_probe_around_the_camera_and_captures_on_request() {
        let mut world = world_with_camera_at(Vec3::Z * 10.0);
        world.spawn((
            Transform::at(Vec3::X * 50.0),
            ReflectionProbe::new(Vec3::splat(4.0)),
        ));
        assert!(
            extract_frame(&world, 100, 100, 1.0, &Quality::default())
                .effects
                .reflection
                .is_none()
        );

        world.spawn((
            Transform::at(Vec3::Z * 9.0),
            ReflectionProbe::new(Vec3::splat(4.0)),
        ));
        world.spawn((Transform::at(Vec3::Z * 9.0 + Vec3::X), Mesh::cube(0.5)));
        let mut frame = extract_frame(&world, 100, 100, 1.0, &Quality::default());
        let reflection = frame
            .effects
            .reflection
            .as_ref()
            .expect("the camera is inside");
        assert_eq!(reflection.id.center, Vec3::Z * 9.0);
        assert!(reflection.faces.is_none());

        add_capture(&mut frame, &world, 1.0);
        let faces = frame.effects.reflection.unwrap().faces.unwrap();
        let counts = faces.map(|batches| batches.len());
        assert_eq!(counts, [1, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn cascades_follow_the_tier_and_end_at_the_shadow_distance() {
        let mut world = world_with_camera_at(Vec3::Z * 10.0);
        world.spawn((Sun::default(),));
        for (cascades, size) in [(2, 1024), (4, 2048)] {
            let quality = Quality {
                shadow_cascades: cascades,
                shadow_map_size: size,
                ..Quality::default()
            };
            let frame = extract_frame(&world, 100, 100, 1.0, &quality);
            let shadows = frame.shadows.expect("the sun casts");
            let ends: Vec<f32> = shadows.cascades.iter().map(|c| c.end).collect();
            assert_eq!(ends.len(), cascades);
            assert_eq!(shadows.casters.len(), cascades);
            assert!(ends.windows(2).all(|pair| pair[0] < pair[1]), "{ends:?}");
            assert!((ends[cascades - 1] - Sun::default().shadow_distance).abs() < 1e-3);
            // Nearby surfaces get much finer texels than distant ones.
            assert!(shadows.cascades[0].texel * 4.0 < shadows.cascades[cascades - 1].texel);
        }
    }

    #[test]
    fn a_surface_that_casts_no_shadow_is_drawn_but_not_into_shadow_maps() {
        let mut world = world_with_camera_at(Vec3::Z * 10.0);
        world.spawn((Sun::default(),));
        world.spawn((
            Transform::default(),
            Mesh::cube(1.0),
            Material {
                casts_shadow: false,
                ..Material::default()
            },
        ));

        let frame = extract_frame(&world, 100, 100, 1.0, &Quality::default());

        assert_eq!(frame.batches.len(), 1);
        let shadows = frame.shadows.expect("the sun casts");
        assert!(shadows.casters.iter().all(Vec::is_empty));
    }

    #[test]
    fn model_parts_are_placed_relative_to_their_entity() {
        let mut world = world_with_camera_at(Vec3::Z * 10.0);
        let mesh = Mesh::triangle();
        let part = Mat4::from_translation(Vec3::X);
        let model = Model::new(vec![
            (mesh.clone(), Material::default(), part),
            (mesh, Material::default(), part * part),
        ]);
        let entity = Transform {
            translation: Vec3::Y,
            ..Transform::default()
        };
        world.spawn((entity, model));

        let frame = extract_frame(&world, 100, 100, 1.0, &Quality::default());

        assert_eq!(frame.batches.len(), 1);
        let origins: Vec<Vec3> = frame.batches[0]
            .instances
            .iter()
            .map(|instance| Vec3::from_array(instance.origin()))
            .collect();
        assert_eq!(
            origins,
            [Vec3::new(1.0, 1.0, 0.0), Vec3::new(2.0, 1.0, 0.0)]
        );
    }

    #[test]
    fn a_restyle_redraws_every_part_with_its_shader_and_keeps_their_textures() {
        let mut world = world_with_camera_at(Vec3::Z * 10.0);
        let texture = crate::Texture::from_rgba8(1, 1, vec![255; 4]);
        let model = Model::new(vec![(
            Mesh::triangle(),
            Material {
                base_color_texture: Some(texture.clone()),
                ..Material::default()
            },
            Mat4::IDENTITY,
        )]);
        let shader = Shader::new("fn surface(s: Surface, p: Pixel) -> Surface { return s; }");
        world.spawn((Transform::default(), model.clone()));
        world.spawn((
            Transform::default(),
            model,
            Restyle {
                alpha_mode: Some(AlphaMode::Mask { cutoff: 0.5 }),
                ..Restyle::new(shader.clone())
            },
        ));

        let frame = extract_frame(&world, 100, 100, 1.0, &Quality::default());

        assert_eq!(frame.batches.len(), 2);
        let restyled = frame
            .batches
            .iter()
            .find(|batch| batch.material.shader.is_some())
            .unwrap();
        assert_eq!(
            restyled.material.shader_key(),
            Restyle::new(shader)
                .apply(&Material::default())
                .shader_key()
        );
        assert_eq!(
            restyled.material.alpha_mode,
            AlphaMode::Mask { cutoff: 0.5 }
        );
        assert_eq!(restyled.material.texture_keys()[0], texture.key());
    }

    #[test]
    fn different_textures_split_a_batch_and_other_factors_do_not() {
        let mut world = world_with_camera_at(Vec3::Z);
        let mesh = Mesh::cube(1.0);
        let textured = Material {
            base_color_texture: Some(crate::Texture::from_rgba8(1, 1, vec![255; 4])),
            ..Material::default()
        };
        world.spawn((Transform::default(), mesh.clone(), textured));
        world.spawn((Transform::default(), mesh.clone()));
        let rough = Material {
            roughness: 1.0,
            ..Material::default()
        };
        world.spawn((Transform::default(), mesh, rough));

        let mut counts: Vec<usize> = extract_frame(&world, 100, 100, 1.0, &Quality::default())
            .batches
            .iter()
            .map(|batch| batch.instances.len())
            .collect();
        counts.sort_unstable();

        assert_eq!(counts, [1, 2]);
    }

    fn setup_counting_steps() -> (GameSetup<i32>, Rc<Cell<u32>>, Rc<Cell<u32>>) {
        let mut setup = GameSetup::<i32>::new();
        let (fixed, frames) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let counter = fixed.clone();
        setup.on_fixed_update(move |_| counter.set(counter.get() + 1));
        let counter = frames.clone();
        setup.on_update(move |_| counter.set(counter.get() + 1));
        (setup, fixed, frames)
    }

    #[test]
    fn fixed_steps_follow_game_time_whatever_the_frame_rate() {
        let (mut setup, fixed, frames) = setup_counting_steps();
        setup.set_fixed_rate(10.0);

        for _ in 0..30 {
            setup.advance(1.0 / 60.0, Vec::new());
        }
        assert!((4..=5).contains(&fixed.get()), "{}", fixed.get());
        assert_eq!(frames.get(), 30);

        let before = fixed.get();
        setup.advance(0.5, Vec::new());
        assert_eq!(fixed.get() - before, 5);
    }

    #[test]
    fn commands_reach_callbacks_before_the_frames_updates() {
        let mut setup = GameSetup::<i32>::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        let writer = log.clone();
        setup.on_command(move |_, command| writer.borrow_mut().push(command));
        let writer = log.clone();
        setup.on_update(move |_| writer.borrow_mut().push(0));

        setup.advance(0.0, vec![1, 2]);

        assert_eq!(*log.borrow(), [1, 2, 0]);
    }

    #[test]
    fn a_key_press_in_a_frame_without_a_fixed_step_reaches_the_next_step_once() {
        let mut setup = GameSetup::<i32>::new();
        setup.set_fixed_rate(10.0);
        let seen = Rc::new(Cell::new(0));
        let counter = seen.clone();
        setup.on_fixed_update(move |context| {
            if context.input.key_just_pressed(crate::Key::Space) {
                counter.set(counter.get() + 1);
            }
        });

        setup.apply_input(InputEvent::KeyDown(crate::Key::Space));
        setup.advance(0.01, Vec::new());
        assert_eq!(seen.get(), 0);
        setup.advance(0.3, Vec::new());
        assert_eq!(seen.get(), 1);
    }

    #[test]
    fn interpolated_entities_draw_between_their_last_two_fixed_positions() {
        let mut world = world_with_camera_at(Vec3::Z);
        let at = |x: f32| Transform {
            translation: Vec3::X * x,
            ..Transform::default()
        };
        world.spawn((
            at(2.0),
            Interpolated {
                previous: Some(at(0.0)),
            },
            Mesh::triangle(),
        ));

        let frame = extract_frame(&world, 100, 100, 0.25, &Quality::default());

        assert_eq!(frame.batches[0].instances[0].origin(), [0.5, 0.0, 0.0]);
    }

    #[test]
    fn entities_outside_the_view_are_not_drawn() {
        let count_at = |translation| {
            let mut world = world_with_camera_at(Vec3::Z * 5.0);
            world.spawn((
                Transform {
                    translation,
                    ..Transform::default()
                },
                Mesh::cube(1.0),
            ));
            extract_frame(&world, 100, 100, 1.0, &Quality::default())
                .batches
                .len()
        };

        assert_eq!(count_at(Vec3::ZERO), 1);
        for away in [
            Vec3::X * 100.0,
            Vec3::NEG_X * 100.0,
            Vec3::Y * 100.0,
            Vec3::NEG_Y * 100.0,
            Vec3::Z * 100.0,
            Vec3::NEG_Z * 2000.0,
        ] {
            assert_eq!(count_at(away), 0, "{away}");
        }
    }

    #[test]
    fn a_large_entity_centered_outside_the_view_still_draws_if_it_reaches_in() {
        let mut world = world_with_camera_at(Vec3::Z * 5.0);
        world.spawn((
            Transform {
                translation: Vec3::X * 20.0,
                scale: Vec3::splat(40.0),
                ..Transform::default()
            },
            Mesh::cube(1.0),
        ));

        assert_eq!(
            extract_frame(&world, 100, 100, 1.0, &Quality::default())
                .batches
                .len(),
            1
        );
    }

    #[test]
    fn nothing_is_drawn_without_a_camera_or_a_size() {
        let mut world = World::new();
        world.spawn((Transform::default(), Mesh::triangle()));
        assert!(
            extract_frame(&world, 100, 100, 1.0, &Quality::default())
                .batches
                .is_empty()
        );

        let mut world = world_with_camera_at(Vec3::Z);
        world.spawn((Transform::default(), Mesh::triangle()));
        assert!(
            extract_frame(&world, 0, 100, 1.0, &Quality::default())
                .batches
                .is_empty()
        );
    }

    #[test]
    fn entities_with_the_same_mesh_share_a_batch() {
        let mut world = world_with_camera_at(Vec3::Z);
        let shared = Mesh::cube(1.0);
        world.spawn((Transform::default(), shared.clone()));
        world.spawn((Transform::default(), shared));
        world.spawn((Transform::default(), Mesh::triangle()));
        world.spawn((Transform::default(),));

        let mut counts: Vec<usize> = extract_frame(&world, 100, 100, 1.0, &Quality::default())
            .batches
            .iter()
            .map(|batch| batch.instances.len())
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

            let clip = extract_frame(&world, 100, 100, 1.0, &Quality::default())
                .view_projection
                .project_point3(Vec3::ZERO);

            assert_near(clip * Vec3::new(1.0, 1.0, 0.0), Vec3::ZERO);
        }
    }

    #[test]
    fn farther_entities_look_smaller() {
        let height_at = |distance: f32| {
            let mut world = world_with_camera_at(Vec3::Z * distance);
            world.spawn((Transform::default(), Mesh::triangle()));
            extract_frame(&world, 100, 100, 1.0, &Quality::default())
                .view_projection
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
