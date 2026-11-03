# Kiba design

Kiba is a general-purpose 3D game engine for the Web that works alongside Whisker.
It has three goals, in priority order when they conflict:

1. Light enough for a Web browser on a mid-range Android device
2. Affinity with Whisker
3. Photorealistic look

Evidence labels: **[verified]** was confirmed in code or a primary source, **[assumed]** is a
value to replace with a measurement, and **[unverified]** is not yet known to work.

## How the goals are interpreted

Kiba does not pursue "visuals only UE could produce" with UE's techniques. Nanite targets consoles
and desktops **[verified]**, and Godot also limits GI, volumetric fog, SSR, and TAA to its upper
renderer **[verified]**. Computing light at run time is judged infeasible on mid-range Android.

Kiba therefore **moves computation from run time to cook time**. Most of the realism comes from
baked light, PBR materials, IBL, and color grading.

This has three costs:

- Time-of-day changes are limited to blending a few baked variations.
- Terrain or buildings that change significantly, such as through destruction, do not receive
  correct indirect light.
- Indirect light from moving lights is not represented on the lowest tier.

## Decisions

### D1. The engine lives in `kiba`, and Hosts stay thin

| Crate | Responsibility |
|---|---|
| `kiba` | `Game` handle, World, schedule, renderer, asset loading. Depends on wgpu |
| Host implementations | Render target, frame timing, input collection, lifecycle |
| `kiba-cook` (CLI on the development machine) | Converts glTF and similar sources to the runtime format, and bakes light |

On Web, the app and modules link into one wasm, and Host crates can reference their core crate, as
`whisker-svg-web` does **[verified]**.

All platforms share the render pipeline and shaders through `kiba`'s wgpu implementation, with no
per-Host drawing code. A Host implementation is responsible for only four things:

- Supplying the render target, and reporting its size and scale
- Frame timing
- Collecting input
- Reporting foreground and background changes and render target destruction

Android passes a `SurfaceView` and iOS passes a `CAMetalLayer` to wgpu, with no changes to Whisker
**[verified]**. Android tries Vulkan and then falls back to GLES. A Vulkan surface takes exclusive
hold of the window, so both cannot be created in one instance **[verified]**.

Desktop shares the Host's wgpu device and draws into an element-sized texture. Whisker composites
it on the same device, so no readback occurs **[verified]**.

GPU constraints differ by backend, so tiers and adapter limits absorb the differences.

`SceneRenderer` writes display-encoded sRGB on every Host: it encodes in the shader unless the
target format already does **[verified]**.

The cost is that `kiba` is no longer a props-only module, which increases dependencies and wasm
size. Optimizations using OS-specific GPU features are also limited to what wgpu exposes.

### D2. Per-frame GPU updates do not go through Whisker's reactive graph

| Data | Path |
|---|---|
| Transforms, physics, animation | Update the World directly |
| HP, score, menu state | Signals, `set` only on change |
| Operations from the UI | Queue with `Game::send`, processed at the start of the next frame |

Values that change every frame do not go through `WhiskerValue` or props, which are UI paths.

### D3. 3D scenes are not written as `render!` children

`Viewport` is a leaf element, and scenes are built in ordinary Rust.

The React Three Fiber form is rejected because `module_element` children are child views attached
to the Host **[verified]**, with nothing to represent 3D nodes. Adding a separate tree would impose
reconcile costs and a second lifecycle on large numbers of entities. Whisker also builds Keyframes
in ordinary Rust rather than a macro **[verified]**.

UI is layered as siblings of `Viewport`.

### D4. Entities use an ECS (first candidate `hecs`), and Kiba owns a fixed schedule

An ECS is adopted because many similar objects and streaming are assumed. The schedule has the
fixed order `input → fixed_update → update → extract → render`.

`bevy_ecs` also works standalone **[verified]**, but `hecs` stays. For the same spawn-and-query wasm,
`hecs` 0.10 is 18 KB gzipped and `bevy_ecs` 0.16 is 51 KB **[verified]**. `extract` walks every entity
each frame without change detection, which costs about 1.5 ms for 10,000 entities and leaves no need
for `bevy_ecs`'s built-in tracking.

### D5. Game logic and rendering run on the main thread

Signals are not `Send`, so moving the game to a Worker would turn D2's bridge into asynchronous
messages. Wasm threads also require COOP and COEP headers, and no mention of them was found in
Whisker's Web code (searched `platforms/web` and `whisker-build`) **[verified]**.

Only asset decoding runs in dedicated Workers, without shared memory.

The cost is that heavy game logic consumes the same frame budget as the UI. P3 measures and
revisits this.

### D6. On Web, the GPU baseline is WebGPU only, without WebGL2

WebGPU on Android is enabled by default from Chrome 121 on Android 12 or later with Qualcomm or Arm
GPUs **[verified]**. Firefox for Android is reported as not yet shipped, and older devices are not
reached. This is accepted.

WebGL2 has no compute or storage buffers, so supporting it would mean maintaining a separate
renderer.

Chrome 146's compatibility mode (GLES 3.1) **[verified]** is out of scope through P3.

### D7. Rendering uses forward+, without deferred on the lowest tier

Memory bandwidth dominates on tile GPUs **[verified]**. Writing out a G-buffer is expensive, and core
WebGPU has no subpass for reading a G-buffer within a tile. The closest addition, transient attachments
(Chrome intent to ship, January 2026) **[verified]**, keeps one pass's attachments in tile memory but does
not let a later pass read them there.

| Tier | Target | Frame time | Longest side | Lowest scale | Features |
|---|---|---|---|---|---|
| T0 | Mid-range Android | 33 ms | 1280 px | 0.5 | forward, CPU culling, instancing, probes, two sun cascades, bloom, fog, particles |
| T1 | High-end phones | 16.7 ms | 1920 px | 0.7 | T0 + half-resolution AO |
| T2 | Desktop | 16.7 ms | native | 0.7 | T0 + stronger AO |

All tiers use the same assets. T0's image is tuned as the product baseline, not as a compromise.

The tier is chosen while the game runs, not by a benchmark at startup, because a benchmark draws a scene
other than the game's and a phone that heats up slows minutes later. A game starts at T2. Whenever a
one-second window averages more than 110% of the tier's frame time, internal resolution drops by a tenth,
and at the tier's lowest scale the tier drops. Windows are judged by their median frame, so a load's hitch
does not count. A display's refresh caps how fast frames arrive, so meeting the budget for three windows
is enough to try a tenth more resolution, and each try that fails doubles the wait for the next. A tier
never climbs back, so the image does not pulse. Frames over 250 ms, from a hidden tab or a
load, do not count. `GameSetup::set_tier` fixes the tier for a settings menu, and resolution still
adapts. The scene renders at the scaled size, and post-processing upscales it with bilinear filtering
and a light sharpen.

Deferred, with reasons:

- **GPU culling.** CPU culling held 20,000 instances within about 10 ms under a 6x throttle, so it is not
  the bottleneck yet, and multi-draw indirect, which would make one culled draw per batch cheap, is still an experimental
Chrome feature behind a flag **[verified]**.
- **Temporal upscaling.** It needs motion vectors from every pass, a jittered projection, and history
  rejection. Spatial upscaling is what T0 needs, and whether T1 needs more waits for a device measurement.
- **Additional shadows.** Kiba has no point or spot light yet; they come when a game asks for them.
- **SSR and volumetrics.** Nothing in Kiba is glossy and flat enough for screen-space reflections to beat
  the sky cube yet, and exponential fog stands in for volumetric fog, which would need a compute froxel
  grid.

Every tier uses reversed `Depth32Float` depth, which keeps precision over open-world distances.

Every tier shades with the metallic-roughness model: GGX, height-correlated Smith visibility, and
Schlick Fresnel. It matches glTF, so cooked assets need no material conversion. Material factors are
per-instance values, so entities that share a mesh and textures stay in one draw call. Normal maps
derive their tangent frame from screen derivatives, so meshes carry no tangent attribute.

The sun casts shadows through two cascades in one 1024-texel depth array. The near cascade covers the
first fifth of `Sun::shadow_distance`, entities are culled per cascade, and entities off screen still
cast. Each cascade snaps to whole texels so that edges do not crawl, and lookups take four rotated
hardware-filtered taps.

`Sky` is an analytic gradient, with image-based lighting calculated from it on the GPU: 128 samples per
texel into a 32-texel radiance cube of five roughness levels and a 16-texel irradiance cube. This
happens once per change of the sky, so a frame pays only two cube lookups per fragment. The sun is left
out of the cubes, because surfaces receive it directly.

`Sky::from_sun` derives sky colors from the sun's height for a day cycle, and `Fog` fades surfaces into
the air with distance. Ambient occlusion reads the depth of the main pass at half resolution with eight
samples per pixel and is blurred while post-processing reads it. Bloom keeps light above 1.0, halves it
four times, and adds the chain back up. Particles are CPU-simulated sprites drawn as one instanced draw
that adds light, so smoke that darkens waits for sorted transparency.

Skinning runs in the vertex shader from a uniform palette at a dynamic offset. A uniform rather than a
storage buffer keeps vertex shaders within what GLES guarantees. Skin data is a separate vertex stream,
so unskinned meshes do not pay for it, and each posed mesh draws alone.

### D8. The world is delivered in cells and controlled by memory budgets

The world is split into grid cells, loaded and evicted with LRU by distance and budget. Distant
content is replaced with HLOD and impostors. Upload volume per frame is also capped.

### D9. Assets are cooked, and run-time glTF is for development only

Run-time glTF sits behind the `gltf` feature so that shipped games do not carry its parser. It reads
binary glTF geometry, vertex colors, material factors, base color, metallic-roughness, and normal
textures, skins of up to 128 joints, and animation clips, with cubic curves played as straight lines. Occlusion, emission, transparency, double-sided materials, and texture samplers are not
read yet. Images decode and build mipmaps on the main thread, which D5 allows only for development.

Distribution uses per-cell `.kiba` packs. Compressed textures are cooked into a separate pack for
each GPU format, with no run-time transcoding.

Play device support is over 80% for ASTC and over 95% for ETC2 **[verified]**. ASTC is preferred, with
ETC2 and BC7 also provided.

On T0, baked light is primarily probe volumes, without per-texel lightmaps across all terrain.
Lightmaps are limited to important buildings and close range.

The cost is larger delivery size and longer builds, accepted to avoid the transcoder's wasm and CPU
load.

### D10. Of the 18 engine areas, Kiba builds only rendering and delivery itself

| Area | Approach | Timing |
|---|---|---|
| Rendering | Own | P1–P4 |
| Streaming / LOD | Own | P3 |
| Asset pipeline | Own (using `meshoptimizer` and similar) | P1–P3 |
| Game loop | Own (fixed step + interpolation, capped dt) | P1 |
| Entity model | `hecs` | P1 |
| Input | Own (including action mapping) | P1–P2 |
| Camera | Own (basic controllers) | P1 |
| Animation | Own (skinning, blending, IK later) | P2 |
| Physics / collision | Integrate Rapier as first candidate | P3 |
| Particles / VFX | Own: CPU-simulated additive sprites in one instanced draw | P4 |
| Audio | Thin wrapper over Web Audio; relationship to `whisker-audio` decided in P3 | P3 |
| UI / text | Whisker | Done |
| Scripting | Rust only | Out of scope |
| AI / navigation | External crate for navmesh | P5 or later |
| Networking | Game's responsibility | P5 or later |
| Save | Game's responsibility (`whisker-local-store`) | Out of scope |
| Editor | Not built. Blender + cook + debug overlay + hot reload | Out of scope |
| Platform | Web, iOS, Android, and Desktop Hosts provided in P0 | P0 |

## Target API

This is what a Whisker app author writes today. A cooked `.kiba` asset replaces the `.glb` once
`kiba-cook` exists. Mutable game state lives in components, not in variables shared between callbacks,
because each callback owns its captures.

```rust
use kiba::prelude::*;
use whisker::css::PositionKind;
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_asset::asset;

#[derive(Clone, Copy)]
enum Command {
    Boost,
}

struct Spin {
    speed: f32,
}

#[whisker::main]
pub fn app() -> Element {
    let turns = signal(0);

    let game = Game::<Command>::new(move |setup| {
        setup.world.spawn((
            Camera::default(),
            Transform::default(),
            OrbitCamera::default(),
        ));
        setup.world.spawn((Sun::default(),));
        setup.world.spawn((Sky::default(),));
        setup.world.spawn((
            Transform::default(),
            Model::load(asset!("crate.glb")),
            Spin { speed: 1.0 },
        ));

        setup.on_command(|context, command| match command {
            Command::Boost => {
                for (_, spin) in context.world.query_mut::<&mut Spin>() {
                    spin.speed *= 2.0;
                }
            }
        });
        let mut angle = 0.0;
        setup.on_update(move |context| {
            for (_, (transform, spin)) in context.world.query_mut::<(&mut Transform, &Spin)>() {
                angle += spin.speed * context.time.delta_seconds();
                transform.rotation = Quat::from_rotation_y(angle);
            }
            let completed = (angle / std::f32::consts::TAU) as i32;
            if completed != turns.get_untracked() {
                turns.set(completed);
            }
        });
    });

    render! {
        View(style: css!(width: percent(100), height: percent(100))) {
            Viewport(game: game, style: css!(width: percent(100), height: percent(100)))
            View(
                style: css!(position: PositionKind::Absolute, left: px(16), top: px(16)),
                on_tap: move |_| game.send(Command::Boost),
            ) {
                Text(value: computed(move || format!("{} turns", turns.get())))
            }
        }
    }
}
```

`Game` is a `Copy` key, like `VideoHandle` **[verified]**. A thread-local registry in `kiba` holds the
real object, and `Viewport` passes the key to the Host as a string prop, because `module_element`
props reach the Host stringified whatever their Rust type **[verified]**. The key passes directly into
closures such as `on_tap`, avoiding the problem of `#[component]` arguments that cannot be moved.

## Performance budgets

All values are **[assumed]** and are replaced by device measurements in P1.

A first measurement on a Mac, in a web build, is **[verified]** only for that machine: 10,000 cube
instances (120,000 triangles, one draw call) all moving each frame take 2.3 ms of main-thread time,
and 4.3 ms under Chrome's 6x CPU throttling, at the display's 60 fps. Throttling does not slow the
GPU, so this shows only that the CPU side has room. It does not replace the device check in P1.

| Item | Initial T0 value |
|---|---|
| Frame rate | Stable 30 fps (33 ms, without thermal throttling) |
| Internal resolution | Adjusted dynamically from about 1280 px on the long side |
| Draw calls | 300 or fewer |
| Triangles per frame | 500,000 or fewer |
| Render passes | 3–4: shadows + main + post-processing |
| GPU textures | 300 MB or less |
| Baked light data | 2 MB or less per cell (compressed, assuming 256 m cells) |
| Total tab memory | 1 GB or less (no primary source found for the limit) |
| Initial download | 20 MB or less (gzipped, including wasm) |

## Phases and exit criteria

This document is complete when every phase row has its implementation, a Web measurement, and an API
review recorded under [Phase results](#phase-results); every spike has a result; every open question
is answered; and no **[assumed]** or **[unverified]** label is left without a deferral reason.

All measurements use the Web. The stand-in for the reference device is Chrome on the development Mac,
with a 6x CPU throttle and an internal resolution capped at 1280 px on the long side. It proves CPU
headroom and correctness only, because throttling does not slow the GPU. Each criterion below that
names the reference device is met through this stand-in, and checking on a real mid-range Android
device is deferred to the project owner.

Each phase ends with an API review for humans and for AI: a fresh agent given only the public docs
writes a small game, and its misreadings are recorded and fixed. The review also counts the concepts
and imports needed to draw one lit, textured model.

Changes outside `packages/kiba`, such as to `crates/` or `platforms/`, need the project owner's
approval first.

| Phase | Scope | Exit criteria |
|---|---|---|
| P0 | `Viewport`, render targets, Web / iOS / Android / Desktop Hosts | Verified on Web, iOS Simulator, Android Emulator (GLES), and macOS. Android Vulkan remains to be verified on a device |
| P1 | Loop, ECS, camera, glTF, PBR, sun | A 10,000-instance scene is within budget on the reference device, and the target API works |
| P2 | IBL, shadows, light cooking, skinning, input actions | A static outdoor scene looks photorealistic on the reference device |
| P3 | Streaming, LOD, HLOD, physics, audio | Moving through a 4 km square for 30 minutes on the reference device holds the memory limit and frame rate |
| P4 | Tiers, upscaling, VFX, atmosphere, T1/T2 features | Automatic tier selection works per device |
| P5 | Navmesh and networking as needed | Decided by requests from real games |

## Phase results

### P1

**Built:** the fixed schedule, `Interpolated`, `Input`, `OrbitCamera`, `FlyCamera`, async `Model::load`
for binary glTF, PBR with base color, metallic-roughness, and normal textures, frustum culling, an
HDR target with tone mapping, and `FrameStats`.

**Measured** with the example's `stress` feature at 1280x720 in Chrome on the development Mac: 10,000
moving cubes (120,000 triangles, one draw call) hold 60 fps with about 3 ms of main-thread time, and
about 5 ms under a 6x CPU throttle. The GPU cost on a mid-range phone is unknown.

**API review:** an agent given only the docs wrote a 75-line game using 16 Kiba concepts, with one
compile error, which came from this document's example. Fixed as a result: a crate-level guide with a
table of component sets, documented defaults and sizes, `Context::drawn_transform` so a camera can
follow an interpolated entity without jitter, and the missing import. Callbacks taking one `Context`
drew no complaints. Shared state between callbacks was a trap found while writing the example: a
captured `Copy` value silently forks, so the guide now directs state into components.

### P2

**Built:** two-cascade sun shadows, `Sky` with GPU-calculated image-based lighting, skinning and blended
animation clips through `Animator`, input actions with on-screen sources, and `kiba-cook`, which traces
rays through still models into a `ProbeVolume` of sky visibility.

Light cooking is sky visibility only, not baked bounce light or color. Probes store what can see the sky,
so a sky can change without a second cook, and one bounce or per-texel lightmaps are not implemented.
Only one volume applies at a time, until cells bring their own in P3.

**Measured** the same way as P1: 10,000 moving cubes that all cast shadows, under a `Sky` (240,660
triangles, three draw calls because shadow passes now count) hold 60 fps with about 4 ms of main-thread
time, and about 10 ms under a 6x CPU throttle. Throttling does not slow the GPU, so the cost of shadow
passes, two cube lookups, and a 3D texture lookup per fragment on a phone is unmeasured. Skinning was
checked with Khronos's CesiumMan, one character, and not measured at scale.

**The exit criterion is not met by this stand-in.** Whether a static outdoor scene looks photorealistic
needs authored art and a judgment on the reference device. The renderer has the features it needs, and
this judgment is deferred to the project owner. Native Hosts still draw only the placeholder.

**API review:** an agent given only the docs wrote a game with a walking character, a follow camera,
on-screen jump, and probes, using 15 Kiba concepts and `kiba::prelude::*`. Every Kiba call compiled the
first time. All four compile failures came from `asset!`, whose crate and `assets/` rule were undocumented.
It also found a trap: an action pressed only through `Game::set_action` must still be bound, or reading it
panics in debug builds. Fixed as a result: documented `asset!`, the binding rule, and clip restarts, and
added crate-level recipes for a character with a follow camera and for an on-screen button.

## Spikes to run first

Each result can change the decisions above. They run at the start of P1.

1. **Android device baseline** [unverified]: Collect WebGPU adapter limits, PBR frame rate, and ASTC
   support on the reference device. Affects D6, D7, and all budgets.
2. **Frame integration** [verified on Web]: Kiba rides Whisker's frame driver, so `on_update` can
   call `signal.set`, and the Web Host's own loop is gone. Each frame posts the next one through
   `RuntimeDispatcher`, which requires a drain to run only what was queued when it started. A drain
   that also runs newly posted work draws up to 4096 frames without yielding and hangs the page.
   Dispatches also drain on input, so a frame can occasionally run off the display's cadence.
   Native Hosts do not run a `Game` yet.
3. **Tab memory limit** [unverified]: Increase allocation on the reference device and measure where
   context loss and tab discard occur.
4. **Wasm size** [partly verified]: The example is about 0.9 MB gzipped without `gltf` content and
   1.12 MB with it. `hecs` adds 18 KB. Rapier is measured in P3.
5. **Hot reload** [unverified]: Check the behavior when editing `Game::new`'s closure. Web uses side
   modules **[verified]**, so also confirm whether the thread-local registry refers to the same
   object after a patch.

## Open questions

- The reference device must be chosen. Android 12 or later with Adreno 6xx-class or Mali-G6x-class
  is assumed.
- Iteration speed during development is a concern. `whisker build web` has a long wait each time, so
  `kiba` tests run natively with wgpu headless rendering, separate from browser checks.
- wgpu is pinned to 25.0.2 to match `platforms/desktop`. Bevy 0.20 uses wgpu 30 **[verified]**, and
  whether to upgrade together with Whisker is undecided.
