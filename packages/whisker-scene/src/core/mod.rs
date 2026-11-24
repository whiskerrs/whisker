//! The imperative scene graph that the components in the crate root are built on.

mod color;
mod deform;
mod environment;
mod geometry;
mod instances;
mod post;
mod texture;

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::{Rc, Weak};

use glam::{Mat3, Quat, Vec2, Vec3, Vec4};
use whisker::css::Color;
use whisker_surface::{FrameInfo, Surface, WeakSurface};

pub use environment::Environment;
pub use geometry::Geometry;
pub use instances::Instances;
pub use post::{AmbientOcclusion, Bloom, DepthOfField, PostProcessing};
pub use texture::{Texture, TextureError};

pub(crate) use color::{linear_rgba, srgb_rgba};
pub(crate) use deform::Deform;
pub(crate) use environment::{CUBE_LEVELS, CUBE_SIZE};
pub(crate) use geometry::{MorphTarget, Vertex, generate_tangents};

/// A tree of nodes that one or more surfaces draw.
#[derive(Clone)]
pub struct Scene {
    inner: Rc<SceneInner>,
}

pub(crate) struct SceneInner {
    root: Node,
    background: Cell<Color>,
    ambient: Cell<(Color, f32)>,
    hemisphere: Cell<(Color, Color, f32)>,
    tone_mapping: Cell<ToneMapping>,
    exposure: Cell<f32>,
    fog: Cell<Option<Fog>>,
    environment: RefCell<Option<(Environment, f32)>>,
    environment_background: Cell<bool>,
    post_processing: RefCell<PostProcessing>,
    active_camera: RefCell<Option<Node>>,
    surfaces: RefCell<Vec<WeakSurface>>,
    frame_callbacks: RefCell<Vec<(u64, FrameCallback)>>,
    next_callback: Cell<u64>,
}

pub(crate) type FrameCallback = Rc<RefCell<dyn FnMut(&FrameInfo)>>;

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene {
    pub fn new() -> Scene {
        let root = Node::new();
        let inner = Rc::new(SceneInner {
            root: root.clone(),
            background: Cell::new(Color::rgb(0, 0, 0)),
            ambient: Cell::new((Color::rgb(255, 255, 255), 0.0)),
            hemisphere: Cell::new((Color::rgb(255, 255, 255), Color::rgb(0, 0, 0), 0.0)),
            tone_mapping: Cell::new(ToneMapping::None),
            exposure: Cell::new(1.0),
            fog: Cell::new(None),
            environment: RefCell::new(None),
            environment_background: Cell::new(false),
            post_processing: RefCell::new(PostProcessing::default()),
            active_camera: RefCell::new(None),
            surfaces: RefCell::new(Vec::new()),
            frame_callbacks: RefCell::new(Vec::new()),
            next_callback: Cell::new(0),
        });
        *root.inner.scene.borrow_mut() = Rc::downgrade(&inner);
        Scene { inner }
    }

    pub fn root(&self) -> Node {
        self.inner.root.clone()
    }

    pub fn set_background(&self, color: Color) {
        self.inner.background.set(color);
        self.inner.request_frame();
    }

    pub fn set_ambient_light(&self, color: Color, intensity: f32) {
        self.inner.ambient.set((color, intensity));
        self.inner.request_frame();
    }

    /// Light from every direction, `sky` from above blending into `ground` from below, added to
    /// the ambient light.
    pub fn set_hemisphere_light(&self, sky: Color, ground: Color, intensity: f32) {
        self.inner.hemisphere.set((sky, ground, intensity));
        self.inner.request_frame();
    }

    /// How lit colors, scaled by `exposure`, are mapped into the displayable range.
    pub fn set_tone_mapping(&self, tone_mapping: ToneMapping, exposure: f32) {
        self.inner.tone_mapping.set(tone_mapping);
        self.inner.exposure.set(exposure);
        self.inner.request_frame();
    }

    /// Fades surfaces into a color with distance from the camera.
    pub fn set_fog(&self, fog: Option<Fog>) {
        self.inner.fog.set(fog);
        self.inner.request_frame();
    }

    /// Lights surfaces with light from every direction, scaled by `intensity`, and reflects it.
    pub fn set_environment(&self, environment: Option<Environment>, intensity: f32) {
        *self.inner.environment.borrow_mut() = environment.map(|e| (e, intensity));
        self.inner.request_frame();
    }

    /// Draws the environment behind the scene in place of the background color.
    pub fn set_environment_background(&self, shown: bool) {
        self.inner.environment_background.set(shown);
        self.inner.request_frame();
    }

    pub fn set_post_processing(&self, post_processing: PostProcessing) {
        *self.inner.post_processing.borrow_mut() = post_processing;
        self.inner.request_frame();
    }

    /// The camera to draw from; without one, the first camera node in the tree is used.
    pub fn set_active_camera(&self, camera: Option<&Node>) {
        *self.inner.active_camera.borrow_mut() = camera.cloned();
        self.inner.request_frame();
    }

    pub fn active_camera(&self) -> Option<Node> {
        self.inner.active_camera.borrow().clone()
    }

    /// A surface that draws this scene; several surfaces may show one scene.
    pub fn surface(&self) -> Surface {
        let scene = self.clone();
        let surface = whisker_wgpu::surface(move |gpu, format| {
            crate::render::SceneRenderer::new(gpu, format, scene)
        });
        if !self.inner.frame_callbacks.borrow().is_empty() {
            surface.set_continuous(true);
        }
        self.inner.surfaces.borrow_mut().push(surface.downgrade());
        surface
    }

    /// Calls `callback` at the start of every frame until the returned handle is dropped.
    pub fn on_frame(&self, callback: impl FnMut(&FrameInfo) + 'static) -> FrameSubscription {
        let id = self.inner.next_callback.get();
        self.inner.next_callback.set(id + 1);
        let first = {
            let mut callbacks = self.inner.frame_callbacks.borrow_mut();
            callbacks.push((id, Rc::new(RefCell::new(callback))));
            callbacks.len() == 1
        };
        if first {
            self.inner.set_continuous(true);
        }
        FrameSubscription {
            scene: Rc::downgrade(&self.inner),
            id,
        }
    }

    pub(crate) fn inner(&self) -> &SceneInner {
        &self.inner
    }
}

/// Keeps a [`Scene::on_frame`] callback registered while alive.
pub struct FrameSubscription {
    scene: Weak<SceneInner>,
    id: u64,
}

impl Drop for FrameSubscription {
    fn drop(&mut self) {
        let Some(scene) = self.scene.upgrade() else {
            return;
        };
        let empty = {
            let mut callbacks = scene.frame_callbacks.borrow_mut();
            callbacks.retain(|(id, _)| *id != self.id);
            callbacks.is_empty()
        };
        if empty {
            scene.set_continuous(false);
        }
    }
}

impl SceneInner {
    fn surfaces(&self) -> Vec<Surface> {
        let mut surfaces = self.surfaces.borrow_mut();
        surfaces.retain(|surface| surface.upgrade().is_some());
        surfaces.iter().filter_map(WeakSurface::upgrade).collect()
    }

    pub(crate) fn request_frame(&self) {
        for surface in self.surfaces() {
            surface.request_frame();
        }
    }

    fn set_continuous(&self, continuous: bool) {
        for surface in self.surfaces() {
            surface.set_continuous(continuous);
        }
    }

    pub(crate) fn run_frame_callbacks(&self, frame: &FrameInfo) {
        // Cloned out so a callback may register or drop subscriptions.
        let callbacks: Vec<FrameCallback> = self
            .frame_callbacks
            .borrow()
            .iter()
            .map(|(_, callback)| callback.clone())
            .collect();
        for callback in callbacks {
            if let Ok(mut callback) = callback.try_borrow_mut() {
                callback(frame);
            }
        }
    }

    pub(crate) fn root(&self) -> &Node {
        &self.root
    }

    pub(crate) fn background(&self) -> Color {
        self.background.get()
    }

    pub(crate) fn ambient(&self) -> (Color, f32) {
        self.ambient.get()
    }

    pub(crate) fn active_camera(&self) -> Option<Node> {
        self.active_camera.borrow().clone()
    }

    pub(crate) fn hemisphere(&self) -> (Color, Color, f32) {
        self.hemisphere.get()
    }

    pub(crate) fn tone_mapping(&self) -> (ToneMapping, f32) {
        (self.tone_mapping.get(), self.exposure.get())
    }

    pub(crate) fn fog(&self) -> Option<Fog> {
        self.fog.get()
    }

    pub(crate) fn environment(&self) -> Option<(Environment, f32)> {
        self.environment.borrow().clone()
    }

    pub(crate) fn environment_background(&self) -> bool {
        self.environment_background.get()
    }

    pub(crate) fn post_processing(&self) -> PostProcessing {
        self.post_processing.borrow().clone()
    }
}

/// How lit colors are mapped into the range a display shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToneMapping {
    /// Colors past white are clipped.
    #[default]
    None,
    Reinhard,
    /// The ACES filmic curve, with more contrast and saturated highlights.
    Aces,
    /// The AgX curve, which desaturates highlights toward white as they brighten.
    AgX,
}

/// Fades surfaces into `color` with distance from the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Fog {
    /// No fog nearer than `near`, rising evenly to full fog at `far`.
    Linear { color: Color, near: f32, far: f32 },
    /// Fog that thickens as `1 - exp(-(density · distance)²)`.
    Exponential { color: Color, density: f32 },
}

/// One node of a [`Scene`]: a transform with an optional mesh, light, or camera.
#[derive(Clone)]
pub struct Node {
    inner: Rc<NodeInner>,
}

#[derive(Clone)]
pub(crate) struct WeakNode(Weak<NodeInner>);

impl WeakNode {
    pub(crate) fn upgrade(&self) -> Option<Node> {
        self.0.upgrade().map(|inner| Node { inner })
    }
}

struct NodeInner {
    parent: RefCell<Weak<NodeInner>>,
    children: RefCell<Vec<Node>>,
    // Set only on a scene's root.
    scene: RefCell<Weak<SceneInner>>,
    transform: Cell<Transform>,
    visible: Cell<bool>,
    cast_shadow: Cell<bool>,
    receive_shadow: Cell<bool>,
    mesh: RefCell<Option<(Geometry, Material)>>,
    mesh_instances: RefCell<Option<Instances>>,
    visible_distance: Cell<Option<(f32, f32)>>,
    light: Cell<Option<Light>>,
    camera: Cell<Option<Projection>>,
    name: RefCell<Option<String>>,
    deform: RefCell<Option<Rc<Deform>>>,
    on_tap: RefCell<Option<TapHandler>>,
    instance: RefCell<Option<Rc<crate::model::Instance>>>,
}

pub(crate) type TapHandler = Rc<dyn Fn(crate::raycast::SceneHit)>;

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for Node {}

impl Default for Node {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("transform", &self.transform())
            .field("children", &self.inner.children.borrow().len())
            .finish()
    }
}

impl Node {
    pub fn new() -> Node {
        Node {
            inner: Rc::new(NodeInner {
                parent: RefCell::new(Weak::new()),
                children: RefCell::new(Vec::new()),
                scene: RefCell::new(Weak::new()),
                transform: Cell::new(Transform::IDENTITY),
                visible: Cell::new(true),
                cast_shadow: Cell::new(true),
                receive_shadow: Cell::new(true),
                mesh: RefCell::new(None),
                mesh_instances: RefCell::new(None),
                visible_distance: Cell::new(None),
                light: Cell::new(None),
                camera: Cell::new(None),
                name: RefCell::new(None),
                deform: RefCell::new(None),
                on_tap: RefCell::new(None),
                instance: RefCell::new(None),
            }),
        }
    }

    /// Moves `child` under this node, detaching it from any previous parent.
    ///
    /// # Panics
    ///
    /// If `child` is this node or one of its ancestors.
    pub fn add_child(&self, child: &Node) {
        let len = self.inner.children.borrow().len();
        self.insert_child(len, child);
    }

    /// Like [`add_child`](Self::add_child), at `index` among the children (clamped to their count).
    pub fn insert_child(&self, index: usize, child: &Node) {
        assert!(
            !self.is_or_descends_from(child),
            "a node cannot become a child of itself or of its own descendant"
        );
        child.detach();
        let mut children = self.inner.children.borrow_mut();
        let index = index.min(children.len());
        children.insert(index, child.clone());
        drop(children);
        *child.inner.parent.borrow_mut() = Rc::downgrade(&self.inner);
        self.invalidate();
    }

    pub fn remove_from_parent(&self) {
        let parent = self.inner.parent.borrow().upgrade();
        self.detach();
        if let Some(parent) = parent {
            Node { inner: parent }.invalidate();
        }
    }

    pub fn parent(&self) -> Option<Node> {
        self.inner
            .parent
            .borrow()
            .upgrade()
            .map(|inner| Node { inner })
    }

    pub fn children(&self) -> Vec<Node> {
        self.inner.children.borrow().clone()
    }

    pub fn set_transform(&self, transform: Transform) {
        self.inner.transform.set(transform);
        self.invalidate();
    }

    pub fn transform(&self) -> Transform {
        self.inner.transform.get()
    }

    /// Hides this node and everything under it while false.
    pub fn set_visible(&self, visible: bool) {
        self.inner.visible.set(visible);
        self.invalidate();
    }

    pub fn visible(&self) -> bool {
        self.inner.visible.get()
    }

    /// Hides this node and everything under it while the camera's distance from the node's origin
    /// is outside `range`.
    pub fn set_visible_distance(&self, range: Option<Range<f32>>) {
        self.inner
            .visible_distance
            .set(range.map(|range| (range.start, range.end)));
        self.invalidate();
    }

    pub fn visible_distance(&self) -> Option<Range<f32>> {
        self.inner
            .visible_distance
            .get()
            .map(|(start, end)| start..end)
    }

    /// Whether meshes in this subtree block shadowing lights; false anywhere above wins.
    pub fn set_cast_shadow(&self, cast: bool) {
        self.inner.cast_shadow.set(cast);
        self.invalidate();
    }

    pub fn cast_shadow(&self) -> bool {
        self.inner.cast_shadow.get()
    }

    /// Whether meshes in this subtree are darkened by shadows; false anywhere above wins.
    pub fn set_receive_shadow(&self, receive: bool) {
        self.inner.receive_shadow.set(receive);
        self.invalidate();
    }

    pub fn receive_shadow(&self) -> bool {
        self.inner.receive_shadow.get()
    }

    pub fn set_mesh(&self, mesh: Option<(Geometry, Material)>) {
        *self.inner.mesh.borrow_mut() = mesh;
        self.invalidate();
    }

    pub fn mesh(&self) -> Option<(Geometry, Material)> {
        self.inner.mesh.borrow().clone()
    }

    /// Draws this node's mesh once per instance instead of once.
    pub fn set_instances(&self, instances: Option<Instances>) {
        *self.inner.mesh_instances.borrow_mut() = instances;
        self.invalidate();
    }

    pub fn instances(&self) -> Option<Instances> {
        self.inner.mesh_instances.borrow().clone()
    }

    pub fn set_light(&self, light: Option<Light>) {
        self.inner.light.set(light);
        self.invalidate();
    }

    pub fn light(&self) -> Option<Light> {
        self.inner.light.get()
    }

    /// Makes this node a camera that looks down its local -Z axis.
    pub fn set_camera(&self, camera: Option<Projection>) {
        self.inner.camera.set(camera);
        self.invalidate();
    }

    pub fn camera(&self) -> Option<Projection> {
        self.inner.camera.get()
    }

    pub fn set_name(&self, name: Option<String>) {
        *self.inner.name.borrow_mut() = name;
    }

    pub fn name(&self) -> Option<String> {
        self.inner.name.borrow().clone()
    }

    /// The first node named `name` in this subtree, this node included, searched depth first.
    pub fn find(&self, name: &str) -> Option<Node> {
        if self.inner.name.borrow().as_deref() == Some(name) {
            return Some(self.clone());
        }
        self.inner
            .children
            .borrow()
            .iter()
            .find_map(|child| child.find(name))
    }

    /// The transform from this node's space to the space of the tree's root.
    pub(crate) fn world_matrix(&self) -> glam::Mat4 {
        let local = self.transform().to_matrix();
        match self.parent() {
            Some(parent) => parent.world_matrix() * local,
            None => local,
        }
    }

    pub(crate) fn set_deform(&self, deform: Option<Rc<Deform>>) {
        *self.inner.deform.borrow_mut() = deform;
        self.invalidate();
    }

    pub(crate) fn deform(&self) -> Option<Rc<Deform>> {
        self.inner.deform.borrow().clone()
    }

    pub(crate) fn set_on_tap(&self, handler: Option<TapHandler>) {
        *self.inner.on_tap.borrow_mut() = handler;
    }

    pub(crate) fn on_tap(&self) -> Option<TapHandler> {
        self.inner.on_tap.borrow().clone()
    }

    pub(crate) fn set_instance(&self, instance: Option<Rc<crate::model::Instance>>) {
        *self.inner.instance.borrow_mut() = instance;
    }

    pub(crate) fn instance(&self) -> Option<Rc<crate::model::Instance>> {
        self.inner.instance.borrow().clone()
    }

    pub(crate) fn request_frame(&self) {
        self.invalidate();
    }

    pub(crate) fn downgrade(&self) -> WeakNode {
        WeakNode(Rc::downgrade(&self.inner))
    }

    fn detach(&self) {
        let Some(parent) = self.inner.parent.replace(Weak::new()).upgrade() else {
            return;
        };
        parent
            .children
            .borrow_mut()
            .retain(|child| !Rc::ptr_eq(&child.inner, &self.inner));
    }

    fn is_or_descends_from(&self, ancestor: &Node) -> bool {
        let mut current = Some(self.clone());
        while let Some(node) = current {
            if node == *ancestor {
                return true;
            }
            current = node.parent();
        }
        false
    }

    fn invalidate(&self) {
        let mut node = self.clone();
        while let Some(parent) = node.parent() {
            node = parent;
        }
        if let Some(scene) = node.inner.scene.borrow().upgrade() {
            scene.request_frame();
        }
    }
}

/// A node's placement relative to its parent, applied as scale, then rotation, then translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn from_translation(translation: Vec3) -> Transform {
        Transform {
            translation,
            ..Self::IDENTITY
        }
    }

    /// Placed at `eye` with its -Z axis toward `target`.
    pub fn looking_at(eye: Vec3, target: Vec3, up: Vec3) -> Transform {
        Transform {
            translation: eye,
            rotation: rotation_toward(target - eye, up),
            scale: Vec3::ONE,
        }
    }

    pub fn to_matrix(&self) -> glam::Mat4 {
        glam::Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

/// The rotation that turns -Z toward `direction`, keeping +Y as close to `up` as possible.
pub(crate) fn rotation_toward(direction: Vec3, up: Vec3) -> Quat {
    let Some(forward) = direction.try_normalize() else {
        return Quat::IDENTITY;
    };
    let right = forward
        .cross(up)
        .try_normalize()
        .or_else(|| forward.cross(Vec3::Z).try_normalize())
        .unwrap_or(Vec3::X);
    let up = right.cross(forward);
    Quat::from_mat3(&Mat3::from_cols(right, up, -forward))
}

/// How a mesh's surface responds to light.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Material {
    /// A flat color that ignores lights.
    Unlit {
        color: Color,
    },
    Standard(StandardMaterial),
    /// Surface inputs computed by a WGSL function of your own, lit like [`Material::Standard`].
    Custom(CustomMaterial),
}

impl From<StandardMaterial> for Material {
    fn from(material: StandardMaterial) -> Self {
        Material::Standard(material)
    }
}

/// Metallic-roughness physically based shading, as glTF defines it.
#[derive(Clone, Debug, PartialEq)]
pub struct StandardMaterial {
    pub base_color: Color,
    /// `0.0..=1.0`
    pub metallic: f32,
    /// `0.0..=1.0`
    pub roughness: f32,
    pub emissive: Color,
    /// Scales `emissive`, allowing light brighter than its color can hold.
    pub emissive_intensity: f32,
    /// Multiplied with `base_color`.
    pub base_color_texture: Option<Texture>,
    /// Roughness from its green channel and metalness from its blue, multiplied with the factors.
    pub metallic_roughness_texture: Option<Texture>,
    /// A tangent-space normal map.
    pub normal_texture: Option<Texture>,
    /// Multiplied with `emissive`.
    pub emissive_texture: Option<Texture>,
    /// Ambient occlusion in its red channel, darkening ambient light.
    pub occlusion_texture: Option<Texture>,
    /// Applied to the texture coordinates of every texture.
    pub texture_transform: TextureTransform,
    pub alpha_mode: AlphaMode,
    /// Draws back faces too instead of culling them.
    pub double_sided: bool,
    /// The strength of a clear glossy layer over the surface, `0.0..=1.0`.
    pub clearcoat: f32,
    /// `0.0..=1.0`
    pub clearcoat_roughness: f32,
    /// The color of a soft velvet-like sheen at grazing angles; black for none.
    pub sheen_color: Color,
    /// `0.0..=1.0`
    pub sheen_roughness: f32,
    /// How much light passes through the surface instead of being diffused, `0.0..=1.0`.
    pub transmission: f32,
    /// Index of refraction, bending what is seen through a transmissive surface.
    pub ior: f32,
    /// How far, in the mesh's units, light travels inside a transmissive surface; 0 for a thin one.
    pub thickness: f32,
    /// Stretches highlights along the tangent when positive and across it when negative,
    /// `-1.0..=1.0`.
    pub anisotropy: f32,
    /// Turns the direction of `anisotropy` from the tangent, in radians.
    pub anisotropy_rotation: f32,
}

impl Default for StandardMaterial {
    fn default() -> Self {
        StandardMaterial {
            base_color: Color::rgb(255, 255, 255),
            metallic: 0.0,
            roughness: 0.5,
            emissive: Color::rgb(0, 0, 0),
            emissive_intensity: 1.0,
            base_color_texture: None,
            metallic_roughness_texture: None,
            normal_texture: None,
            emissive_texture: None,
            occlusion_texture: None,
            texture_transform: TextureTransform::default(),
            alpha_mode: AlphaMode::default(),
            double_sided: false,
            clearcoat: 0.0,
            clearcoat_roughness: 0.0,
            sheen_color: Color::rgb(0, 0, 0),
            sheen_roughness: 0.0,
            transmission: 0.0,
            ior: 1.5,
            thickness: 0.0,
            anisotropy: 0.0,
            anisotropy_rotation: 0.0,
        }
    }
}

/// WGSL source defining `fn surface(input: SurfaceInput) -> Surface`, compiled once per value.
///
/// The function may call `standard_surface(input)`, read `draw.custom[0..4]` and `frame.time.x`,
/// and sample `texture0`..`texture4` with `material_sampler`.
#[derive(Clone)]
pub struct MaterialShader {
    source: Rc<str>,
}

impl MaterialShader {
    /// Checks the source compiles together with the renderer's shader.
    pub fn new(source: impl Into<String>) -> Result<MaterialShader, MaterialShaderError> {
        let source: Rc<str> = Rc::from(source.into());
        let whole = crate::render::material_source(&source);
        let module = naga::front::wgsl::parse_str(&whole)
            .map_err(|error| MaterialShaderError(error.emit_to_string(&whole)))?;
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .map_err(|error| MaterialShaderError(error.emit_to_string(&whole)))?;
        Ok(MaterialShader { source })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn id(&self) -> usize {
        Rc::as_ptr(&self.source) as *const u8 as usize
    }
}

impl PartialEq for MaterialShader {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.source, &other.source)
    }
}

impl std::fmt::Debug for MaterialShader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaterialShader").finish_non_exhaustive()
    }
}

/// Why a [`MaterialShader`]'s source was rejected.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialShaderError(String);

impl std::fmt::Display for MaterialShaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MaterialShaderError {}

/// A material whose surface a [`MaterialShader`] computes.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomMaterial {
    pub shader: MaterialShader,
    /// Read in the shader as `draw.custom`.
    pub params: [Vec4; 4],
    /// Bound as `texture0`.. in order, at most five, sampled without sRGB decoding.
    pub textures: Vec<Texture>,
    pub alpha_mode: AlphaMode,
    pub double_sided: bool,
}

impl CustomMaterial {
    pub fn new(shader: MaterialShader) -> CustomMaterial {
        CustomMaterial {
            shader,
            params: [Vec4::ZERO; 4],
            textures: Vec::new(),
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        }
    }
}

/// How the alpha of a material's base color is used.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AlphaMode {
    /// Alpha is ignored.
    Opaque,
    /// Drawn where alpha is at least the cutoff and discarded elsewhere.
    Mask(f32),
    /// Blended with what is behind wherever alpha is below 1.
    #[default]
    Blend,
}

/// Maps texture coordinates as scale, then rotation in radians, then offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureTransform {
    pub offset: Vec2,
    pub rotation: f32,
    pub scale: Vec2,
}

impl Default for TextureTransform {
    fn default() -> Self {
        TextureTransform {
            offset: Vec2::ZERO,
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }
}

impl TextureTransform {
    pub(crate) fn rows(&self) -> [[f32; 4]; 2] {
        let (sin, cos) = self.rotation.sin_cos();
        let (sx, sy) = (self.scale.x, self.scale.y);
        [
            [cos * sx, sin * sy, self.offset.x, 0.0],
            [-sin * sx, cos * sy, self.offset.y, 0.0],
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Light {
    /// Parallel rays along the node's -Z axis, like sunlight.
    Directional {
        color: Color,
        intensity: f32,
        shadow: Option<Shadow>,
    },
    /// Light from the node's position that fades out by `range`.
    Point {
        color: Color,
        intensity: f32,
        range: f32,
        shadow: Option<Shadow>,
    },
    /// A point light limited to a cone along the node's -Z axis, full inside `inner_angle` and
    /// fading out by `outer_angle`, both measured in radians from the axis.
    Spot {
        color: Color,
        intensity: f32,
        range: f32,
        inner_angle: f32,
        outer_angle: f32,
        shadow: Option<Shadow>,
    },
}

/// Shadows a light casts from meshes onto other meshes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// Width and height in texels of each shadow map; a point light renders six.
    pub map_size: u32,
    /// How many maps, `1..=4`, split a directional light's `distance`, finest nearest the camera.
    pub cascades: u32,
    /// How far from the camera a directional light's shadows reach.
    pub distance: f32,
    /// Blur radius of shadow edges in texels.
    pub softness: f32,
    /// How far, in texels, surfaces are pushed toward the light before testing, against acne.
    pub bias: f32,
}

impl Default for Shadow {
    fn default() -> Self {
        Shadow {
            map_size: 1024,
            cascades: 3,
            distance: 50.0,
            softness: 1.0,
            bias: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Projection {
    /// `fov_y` is the vertical field of view in radians.
    Perspective { fov_y: f32, near: f32, far: f32 },
    /// `height` is the visible height in scene units.
    Orthographic { height: f32, near: f32, far: f32 },
}

impl Projection {
    pub fn to_matrix(&self, aspect: f32) -> glam::Mat4 {
        match *self {
            Projection::Perspective { fov_y, near, far } => {
                glam::Mat4::perspective_rh(fov_y, aspect, near, far)
            }
            Projection::Orthographic { height, near, far } => {
                let half_height = height / 2.0;
                let half_width = half_height * aspect;
                glam::Mat4::orthographic_rh(
                    -half_width,
                    half_width,
                    -half_height,
                    half_height,
                    near,
                    far,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests;
