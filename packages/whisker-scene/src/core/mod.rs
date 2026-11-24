//! The imperative scene graph that the components in the crate root are built on.

mod color;
mod deform;
mod geometry;
mod texture;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use glam::{Mat3, Quat, Vec2, Vec3};
use whisker::css::Color;
use whisker_surface::{FrameInfo, Surface, WeakSurface};

pub use geometry::Geometry;
pub use texture::{Texture, TextureError};

pub(crate) use color::{linear_rgba, srgb_rgba};
pub(crate) use deform::Deform;
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
    mesh: RefCell<Option<(Geometry, Material)>>,
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
                mesh: RefCell::new(None),
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

    pub fn set_mesh(&self, mesh: Option<(Geometry, Material)>) {
        *self.inner.mesh.borrow_mut() = mesh;
        self.invalidate();
    }

    pub fn mesh(&self) -> Option<(Geometry, Material)> {
        self.inner.mesh.borrow().clone()
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
pub enum Material {
    /// A flat color that ignores lights.
    Unlit {
        color: Color,
    },
    Standard(StandardMaterial),
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
pub enum Light {
    /// Parallel rays along the node's -Z axis, like sunlight.
    Directional { color: Color, intensity: f32 },
    /// Light from the node's position that fades out by `range`.
    Point {
        color: Color,
        intensity: f32,
        range: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
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
