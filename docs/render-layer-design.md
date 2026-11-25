# Render Layer Design

How one wgpu renderer draws inside another's frame, sharing its depth, so
that a `whisker-scene` model can stand on a `whisker-map` street, hidden by
the buildings in front of it and under the labels above it, without either
crate depending on the other.

> **Status.** Step 1 (the `whisker-wgpu` contract) is implemented.

---

## TL;DR

- `whisker-wgpu` gains a small contract, `Layer`: draw into a render pass the
  host owns, with the host's view and projection matrices and attachments.
- `whisker-map` hosts layers: `MapLayer(layer, anchor, before)` draws a layer
  between two style layers, in meters around `anchor`.
- `whisker-scene` provides one: `Scene::layer()` draws the scene into the host's
  pass instead of its own surface. `SceneContent(scene)` declares its nodes
  without a view.
- The app joins them. Neither crate names the other.

```rust
let scene = Scene::new();
render! {
    MapView(map_style: style, camera: camera) {
        MapLayer(layer: scene.layer(), anchor: tokyo, before: Some("poi-label".into()))
    }
    SceneContent(scene: scene.clone()) {
        DirectionalLight(direction: Vec3::new(-0.4, -1.0, -0.6))
        ModelView(model: avatar, position: Vec3::new(120.0, 0.0, -40.0))
    }
}
```

The shape follows Mapbox GL JS's `CustomLayerInterface` (`prerender` /
`render` with the map's matrix, depth shared in `renderingMode: "3d"`) and
Google Maps' `WebGLOverlayView`. On WebGL those work because three.js borrows
the map's GL context and cleans up with `resetState()`. wgpu has no ambient
context to borrow, so the contract names the pass and the attachment formats.

---

## Crate graph

```
whisker-surface
      ↑
whisker-wgpu      Layer, LayerTarget, LayerHost, LayerHandle
   ↑       ↑
whisker-map   whisker-scene
 (hosts)       (implements)
      ↖     ↗
        app
```

Any other wgpu renderer can be a map layer, and a scene can be a layer of any
other host, such as a future video or AR surface.

---

## `whisker-wgpu`: the contract

```rust
/// What a host draws a layer into, for one frame.
pub struct LayerTarget {
    /// The layer's local space to the camera's view space, and view space to
    /// clip space. The local space's units and axes are the host's to document.
    pub view: [[f32; 4]; 4],
    pub projection: [[f32; 4]; 4],
    /// The frame's time, the same one the host's own animations use.
    pub frame: FrameInfo,
    pub color_format: wgpu::TextureFormat,
    pub depth_format: wgpu::TextureFormat,
    pub samples: u32,
    /// Physical pixels, and physical pixels per logical one.
    pub size: (u32, u32),
    pub scale: f32,
}

pub trait Layer: 'static {
    /// Called once, before the first frame, with the host that draws it.
    fn attach(&mut self, _host: LayerHost) {}
    /// Before the host's pass begins: offscreen passes such as shadow maps.
    fn prepare(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, target: &LayerTarget);
    /// Inside the host's pass, depth-tested against what the host drew so far.
    fn draw(&mut self, gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>, target: &LayerTarget);
    /// A tap at `point` (logical pixels), before the host's own handling.
    /// Returns whether the layer took it.
    fn tap(&mut self, _point: [f32; 2], _target: &LayerTarget) -> bool { false }
}

/// Asks a host's surface for frames on behalf of one layer.
#[derive(Clone)]
pub struct LayerHost { /* the host's WeakSurface, plus this handle's continuity vote */ }

impl LayerHost {
    pub fn new(surface: WeakSurface) -> LayerHost;
    /// Another handle on the same surface, whose continuous request counts on its own.
    pub fn separate(&self) -> LayerHost;
    pub fn request_frame(&self);
    pub fn set_continuous(&self, continuous: bool);
}

/// A shared, type-erased layer, cheap to clone into components. Equal by identity.
#[derive(Clone, PartialEq)]
pub struct LayerHandle(Rc<RefCell<dyn Layer>>);

impl LayerHandle {
    pub fn new(layer: impl Layer) -> LayerHandle;
    pub fn borrow_mut(&self) -> RefMut<'_, dyn Layer>;
}
```

`view` and `projection` are separate because a lit renderer needs more than
their product: the eye position (specular and fog), the view matrix (sorting
blended draws), and the view frustum alone (fitting shadow cascades). Mapbox
passes only the product, which is why three.js examples on it light from a
wrong eye.

The local-to-clip transform has a positive determinant: front faces wind
counter-clockwise, as in a `SceneView`. A host whose own space is mirrored
(the map's is) folds the mirror into `view`.

Rules a host follows:

1. Its `view` is a similarity: rotation, translation and one uniform scale.
2. It calls `prepare` for every layer before beginning the pass that `draw`
   runs in. Both run in the same frame, with the same `LayerTarget`.
3. It does not clear color or depth between its own draws and a layer's.
4. After `draw` returns, it restores every piece of pass state it relies on:
   pipeline, bind groups, vertex and index buffers, stencil reference, viewport
   and scissor. A layer may leave any of it set. This replaces three.js's
   `resetState()` and is the host's job, not the layer's.
5. It submits the encoder. A layer never submits.
6. It holds no borrow of its own state across `prepare`, `draw` or `tap`. A
   layer may run app code there (frame callbacks, tap handlers) that writes
   signals the host reads; such writes take effect on the host's next frame.

Rules a layer follows:

1. Its pipelines match `color_format`, `depth_format` and `samples`. A depth
   format with stencil needs a pipeline that names it, even if the layer never
   uses the stencil.
2. It never clears, and it draws nothing that is outside its own geometry
   (no sky, no fullscreen background).

`set_continuous` is per handle: the host's surface draws continuously while
any `LayerHost` on it asks. The host gives each layer a `separate` handle and
asks for its own continuous frames through one too, not through the surface,
or it would turn off a layer's request.

---

## `whisker-map`: hosting

### Component

```rust
#[component]
pub fn map_layer(
    layer: LayerHandle,
    /// The origin of the layer's local space.
    anchor: Signal<LatLng>,
    /// The style layer to draw beneath; `None` draws above every style layer,
    /// still under labels and markers.
    #[prop(default = None.into())] before: Signal<Option<String>>,
) -> Element
```

`MapLayer` is legal only inside `MapView`, like `Marker`. There is no
imperative form yet; `MapRef::add_layer` keeps taking style JSON.

### Local space

The layer's local space is meters, Y up, right-handed, anchored at `anchor`
on the ground:

| Axis | Direction |
|---|---|
| +X | east |
| +Y | up |
| −Z | north |

This matches `whisker-scene`'s default camera, which looks down −Z, so "in
front of the default camera" is "north".

The map builds `view` in `f64` and narrows to `f32` last: the map's eye
transform, then the anchor's world pixel, then the meters-to-pixels scale,
with the axis swap. `projection` is the map's perspective alone. Horizontal
meters scale at the anchor's latitude, so distances on the ground are true;
heights use the same scale, so `view` stays a similarity. Precision stays good
to centimeters across a city. Things far from the anchor drift with Mercator's scale change,
the same trade-off Mapbox's three.js example makes. Apps with content spread
over a country use several anchors.

### Globe

While the map is (partly) a globe, the anchor's frame is the tangent plane
at the anchor: `globe_matrix` × (east, up, north) scaled to meters. Content
near the anchor sits right; content far off it floats off the curve. During
the Mercator–globe morph, the two matrices are blended by `view.globe`, the
same factor tiles use.

### Drawing

- `record` inserts a `Step::Layer(index)` at the position of `before` in
  style order (or after the last style layer), skipping layers whose anchor is
  behind the globe's horizon.
- `render` calls `prepare` for each layer before beginning the pass, and runs
  `draw` when it reaches the step. Labels are drawn with their symbol layer, in
  style order, so `before: None` draws over them; markers are views and stay on
  top.
- Only fill extrusions write depth (a depth-only prepass, then color with
  `Equal`), so a model drawn after the building layer is hidden behind
  buildings, and one drawn before it hides the buildings behind it. Flat layers
  test with `Always`, so a flat layer drawn after a layer paints over it: the
  usual place for `before` is the first symbol layer after the extrusions.
- After a layer draws, the map re-binds groups 0 and 2 and forgets its current
  pipeline.

### Taps

On a tap, the map offers the point to its layers from the top down, and only
calls `on_tap` and its own feature query if none took it.

---

## `whisker-scene`: being a layer

### Public surface

```rust
impl Scene {
    /// Draws this scene inside another renderer's pass, in the host's space.
    pub fn layer(&self) -> LayerHandle;
}

/// Declares a scene's nodes without showing it, for scenes drawn as layers.
#[component]
pub fn scene_content(scene: Scene, children: Children) -> Element;
```

`SceneContent` provides the same context `SceneView` does, so every scene
component (`Mesh`, `ModelView`, lights, `Group`, `on_tap`) works unchanged
inside it. A scene may be both shown in a `SceneView` and drawn as a layer.

### What changes inside

- `SceneRenderer::render` splits into `prepare` (frame callbacks, collection,
  uniforms, shadow maps) and `draw` (opaque then blended draws, into a given
  pass). The surface path calls both around its own pass; the layer path calls
  them from `Layer::prepare` and `Layer::draw`.
- `PipelineKey` gains `depth_format`, because the map's depth has a stencil.
- `view` may carry one uniform scale; the scene divides it out to keep its eye
  rigid and its distances in scene units.
- A layer's shadows reach `Shadow::distance`, not clamped to the host's far
  plane.
- The camera comes from `LayerTarget::view` and `projection`. Camera nodes in
  the scene are ignored while it is drawn as a layer.
- Shadow planning fits cascades from the inverse view-projection matrix rather
  than from a `Projection` (field of view, near, far), so it works for any
  host's camera. The `SceneView` path goes through the same code.
- Frame callbacks (`use_frame`, animations) run in `prepare` with the host's
  `FrameInfo`.
- `LayerHost` joins the scene's list of surfaces, so `request_frame` and
  continuous drawing for animations reach the map unchanged.
- `Layer::tap` casts the ray from `view` and `projection` and reuses the node `on_tap`
  dispatch `SceneView` already has.

### What a layer cannot do

Each of these needs a pass of its own, which a layer doesn't have:

- Post-processing (bloom, ambient occlusion, depth of field, FXAA). It is
  ignored. Tone mapping, exposure and fog still apply, because the fragment
  shader does them.
- Transmission. Transmissive surfaces draw as if nothing were behind them.
- Background and sky. The host's pixels are the background.

Shadows work: the shadow maps are drawn in `prepare`, and models shadow each
other. Map buildings don't cast onto models, since they are not in the scene.

---

## Spike findings

The spike (branch `spike/render-layer`) drew a `whisker-scene` scene inside the
`whisker-map` pass on macOS, anchored at Tokyo Station: a platform, a 120 m
tower, a cone 60 m north and a sphere 60 m east, lit by a shadowed sun.

What held:

- **Matrix split.** `view` = map eye × anchor translation × axis swap and
  meter scale, built in `f64`; `projection` = the map's perspective alone. North
  is up and east is right from above.
- **Winding.** The axis swap and the map's Y flip are two mirrors that cancel, so
  the scene's counter-clockwise pipelines draw single-sided meshes unchanged.
- **Depth.** Buildings in front hide the model, labels draw over it, and the
  model hides what is behind it.
- **Shadow cascades from the projection matrix.** Unprojecting NDC corners
  replaces `Projection`'s field of view; the existing shadow tests pass on it.

What changes in the design:

1. **`view` is a similarity: rotation, translation and one uniform scale.**
   The scene divides the scale out (cube root of the determinant) so its eye is
   rigid and lighting, fog and shadow distances stay in meters, and folds it into
   the projection. The map uses the anchor latitude's scale for height too; the
   mismatch with extrusions is the cosine ratio between anchor and camera
   center, negligible within a city.
1. **Shadow reach ignores the host's far plane.** The scene clamps
   `Shadow::distance` to the camera's far plane and fades shadows over the last
   tenth of it. The map's far plane sits just past the ground (1.01× its depth
   looking straight down), so every shadow on the ground faded out. A layer uses
   `distance` as given.
2. **Labels are not drawn after every layer.** The map records labels per symbol
   layer, in style order. In OpenFreeMap Liberty the first symbol layer (61) comes
   before `building-3d` (84), so a layer inserted before the first label draws
   before the extrusions, and the translucent buildings in front tint the model.
   `before: None` draws above every style layer, labels included, as in Mapbox;
   an app that wants labels on top names the first symbol layer after its
   extrusions.
3. **The pipeline cache is keyed by depth format**, since the map's depth has a
   stencil (`Depth24PlusStencil8`).
5. **The host resets its pipeline cache after a layer**, along with bind groups
   0 and 2. The map's step loop tracked the current pipeline in a closure, which
   had to take it as an argument instead.

Flat layers that come after the layer, such as an app's route line, paint over
the model, because they test depth with `Always`. This matches Mapbox.

---

## Open questions

1. **Scale of `pixels_per_meter`.** Is the anchor's latitude enough, or does
   the map need a per-frame rebase when the camera wanders far from the
   anchor? Measure drift on a 10 km scene first.
2. **Depth precision.** The map's near plane is `height / 50` world pixels. At
   street level that may be coarse for small models; check z-fighting before
   adding a reversed-Z or a log depth.
3. **Hit testing through buildings.** `Layer::tap` doesn't know about map
   extrusions in front of the model. Fine for v1; a host could pass its depth
   at the point later.

---

## Plan

0. Spike, thrown away (done; see the findings above).
1. `whisker-wgpu`: `Layer`, `LayerTarget`, `LayerHost`, `LayerHandle`, with a
   test host that draws a layer into an offscreen target.
2. `whisker-scene`: split the renderer; `Scene::layer()`, `SceneContent`;
   GPU tests that a layer draws into a supplied pass and respects its depth.
3. `whisker-map`: `MapLayer`, the local-space matrix (Mercator, then globe),
   `Step::Layer`, tap forwarding; tests for the matrix, for a single-sided mesh
   facing the camera, and for ordering under labels and against extrusions.
4. Live Cities demo on top.

Steps 2 and 3 only depend on step 1 and can be reviewed in parallel.
