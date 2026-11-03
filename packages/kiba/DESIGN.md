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

`bevy_ecs` also works standalone **[verified]**, but the configuration with the smaller API and wasm
size is tried first. In exchange, `extract` cannot rely on `bevy_ecs`'s built-in change detection.
P1 compares their size and the work to implement change detection, and switches if `hecs` falls
short.

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
WebGPU has no subpass for reading a G-buffer within a tile **[unverified]**.

| Tier | Target | Features |
|---|---|---|
| T0 | Mid-range Android | forward+, CPU culling, instancing, baked light + probes, 1–2 sun shadow cascades, lightweight bloom, no temporal upscale |
| T1 | High-end phones | T0 + GPU culling, additional shadows, temporal upscale, lightweight AO |
| T2 | Desktop | T1 + SSR, volumetrics, high-quality AO and shadows |

All tiers use the same assets. T0's image is tuned as the product baseline, not as a compromise.

The tier is chosen by a benchmark at startup and maintained with dynamic resolution.

Every tier uses reversed `Depth32Float` depth, which keeps precision over open-world distances.

Every tier shades with the metallic-roughness model: GGX, height-correlated Smith visibility, and
Schlick Fresnel. It matches glTF, so cooked assets need no material conversion. Materials are
per-instance values, so entities that share a mesh stay in one draw call. Output is not yet tone
mapped, so highlights clip at display white.

### D8. The world is delivered in cells and controlled by memory budgets

The world is split into grid cells, loaded and evicted with LRU by distance and budget. Distant
content is replaced with HLOD and impostors. Upload volume per frame is also capped.

### D9. Assets are cooked, and run-time glTF is for development only

Run-time glTF sits behind the `gltf` feature so that shipped games do not carry its parser. It reads
binary glTF geometry, vertex colors, and material factors. Textures, transparency, emission, and
double-sided materials are not read yet.

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
| Particles / VFX | Own (starting from instancing) | P4 |
| Audio | Thin wrapper over Web Audio; relationship to `whisker-audio` decided in P3 | P3 |
| UI / text | Whisker | Done |
| Scripting | Rust only | Out of scope |
| AI / navigation | External crate for navmesh | P5 or later |
| Networking | Game's responsibility | P5 or later |
| Save | Game's responsibility (`whisker-local-store`) | Out of scope |
| Editor | Not built. Blender + cook + debug overlay + hot reload | Out of scope |
| Platform | Web, iOS, Android, and Desktop Hosts provided in P0 | P0 |

## Target API

This is what a Whisker app author writes. It is the target shape: `Game`, `GameSetup::on_update`,
`Game::send`, `Transform`, `Camera`, `Sun`, `Mesh`, `Material`, `Model`, and `Viewport` exist today,
with `Model::from_glb(include_bytes!(...))` in place of a cooked asset. Hook names are finalized in P1.

```rust
use kiba::prelude::*;
use whisker::prelude::*;

#[derive(Clone, Copy)]
enum Command {
    Boost,
}

#[whisker::main]
pub fn app() -> Element {
    let turns = signal(0);

    let game = Game::<Command>::new(move |setup| {
        let world = &mut setup.world;
        world.spawn((
            Camera::default(),
            Transform::looking_at(Vec3::new(0.0, 1.5, 4.0), Vec3::ZERO, Vec3::Y),
        ));
        world.spawn((Sun::default(),));
        let crate_id = world.spawn((Transform::default(), Model::load(asset!("crate.kiba"))));

        let mut speed = 1.0;
        let mut angle = 0.0;
        setup.on_update(move |world, time, commands| {
            for command in commands {
                match command {
                    Command::Boost => speed *= 2.0,
                }
            }
            angle += speed * time.delta_seconds();
            world
                .get::<&mut Transform>(crate_id)
                .expect("the crate is never despawned")
                .rotation = Quat::from_rotation_y(angle);

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

Each phase's exit criteria include on-device checks. If they are not met, budgets or features are
revisited before moving on.

| Phase | Scope | Exit criteria |
|---|---|---|
| P0 | `Viewport`, render targets, Web / iOS / Android / Desktop Hosts | Verified on Web, iOS Simulator, Android Emulator (GLES), and macOS. Android Vulkan remains to be verified on a device |
| P1 | Loop, ECS, camera, glTF, PBR, sun | A 10,000-instance scene is within budget on the reference device, and the target API works |
| P2 | IBL, shadows, light cooking, skinning, input actions | A static outdoor scene looks photorealistic on the reference device |
| P3 | Streaming, LOD, HLOD, physics, audio | Moving through a 4 km square for 30 minutes on the reference device holds the memory limit and frame rate |
| P4 | Tiers, upscaling, VFX, atmosphere, T1/T2 features | Automatic tier selection works per device |
| P5 | Navmesh and networking as needed | Decided by requests from real games |

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
4. **Wasm size** [unverified]: Measure the release-optimized value of the current 1.6 MB (gzipped),
   and the increase from `hecs` and Rapier.
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
