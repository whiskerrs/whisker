# Render Layer Design

How one wgpu renderer draws inside another's frame, sharing its depth, so
that a `whisker-scene` model can stand on a `whisker-map` street, hidden by
the buildings in front of it and under the labels above it, without either
crate depending on the other.

> **Status.** Proposal. Nothing here is implemented yet.

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

/// Lets a layer ask its host for frames.
#[derive(Clone)]
pub struct LayerHost { /* the host's WeakSurface, plus per-layer continuity */ }

impl LayerHost {
    pub fn request_frame(&self);
    pub fn set_continuous(&self, continuous: bool);
}

/// A shared, type-erased layer, cheap to clone into components.
#[derive(Clone)]
pub struct LayerHandle(Rc<RefCell<dyn Layer>>);
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

1. It calls `prepare` for every layer before beginning the pass that `draw`
   runs in. Both run in the same frame, with the same `LayerTarget`.
2. It does not clear color or depth between its own draws and a layer's.
3. After `draw` returns, it restores every piece of pass state it relies on:
   pipeline, bind groups, vertex and index buffers, stencil reference, viewport
   and scissor. A layer may leave any of it set. This replaces three.js's
   `resetState()` and is the host's job, not the layer's.
4. It submits the encoder. A layer never submits.
5. It holds no borrow of its own state across `prepare`, `draw` or `tap`. A
   layer may run app code there (frame callbacks, tap handlers) that writes
   signals the host reads; such writes take effect on the host's next frame.

Rules a layer follows:

1. Its pipelines match `color_format`, `depth_format` and `samples`. A depth
   format with stencil needs a pipeline that names it, even if the layer never
   uses the stencil.
2. It never clears, and it draws nothing that is outside its own geometry
   (no sky, no fullscreen background).

`set_continuous` is per layer: the host's surface draws continuously while any
of its layers asks, or while the host itself does.

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
heights scale at the camera center's latitude, the factor fill extrusions use,
so a model and a building of the same height stay level. Precision stays good to centimeters
across a city. Things far from the anchor drift with Mercator's scale change,
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
  `draw` when it reaches the step. Labels and markers are drawn after every
  layer, so they stay on top.
- Only fill extrusions write depth (a depth-only prepass, then color with
  `Equal`), so a model drawn after the building layer is hidden behind
  buildings, and one drawn before it hides the buildings behind it. Flat layers
  test with `Always`, so a flat layer drawn after a layer paints over it: the
  usual place for `before` is the first label or extrusion layer.
- After a layer draws, the map re-binds group 0 and its current pipeline.

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

## Open questions

1. **`LayerHandle` vs `Rc<RefCell<dyn Layer>>` in props.** A named type
   reads better and lets us change the inside later.
2. **Scale of `pixels_per_meter`.** Is the anchor's latitude enough, or does
   the map need a per-frame rebase when the camera wanders far from the
   anchor? Measure drift on a 10 km scene first.
3. **Depth precision.** The map's near plane is `height / 50` world pixels. At
   street level that may be coarse for small models; check z-fighting before
   adding a reversed-Z or a log depth.
4. **Hit testing through buildings.** `Layer::tap` doesn't know about map
   extrusions in front of the model. Fine for v1; a host could pass its depth
   at the point later.

---

## Plan

0. Spike, thrown away: draw a scene into an offscreen pass with a map-built
   `view` and `projection`, to settle the matrix split, the winding and the
   shadow planning before the API is fixed.
1. `whisker-wgpu`: `Layer`, `LayerTarget`, `LayerHost`, `LayerHandle`, with a
   test host that draws a layer into an offscreen target.
2. `whisker-scene`: split the renderer; `Scene::layer()`, `SceneContent`;
   GPU tests that a layer draws into a supplied pass and respects its depth.
3. `whisker-map`: `MapLayer`, the local-space matrix (Mercator, then globe),
   `Step::Layer`, tap forwarding; tests for the matrix, for a single-sided mesh
   facing the camera, and for ordering under labels and against extrusions.
4. Live Cities demo on top.

Steps 2 and 3 only depend on step 1 and can be reviewed in parallel.
