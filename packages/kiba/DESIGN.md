# Kiba design

Kiba is a general-purpose 3D game engine for the Web that works alongside Whisker.
It has three goals, in priority order when they conflict:

1. Light enough for a Web browser on a mid-range Android device
2. Affinity with Whisker
3. Photorealistic look

Evidence labels: **[verified]** was confirmed in code or a primary source, **[assumed]** is a
value to replace with a measurement, **[unverified]** is not yet known to work, and **[deferred]** marks
a check left to the project owner, with its reason beside it.

## How the goals are interpreted

Kiba does not pursue "visuals only UE could produce" with UE's techniques. Nanite targets consoles
and desktops **[verified]**, and Godot also limits GI, volumetric fog, SSR, and TAA to its upper
renderer **[verified]**. Computing light at run time is judged infeasible on mid-range Android.

Kiba therefore **moves computation from run time to cook time**. Most of the realism comes from
baked light, PBR materials, IBL, and color grading.

This has three costs:

- Time of day changes the sky, the sun, and its shadows, but baked sky visibility does not move.
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

### D4. Entities use the `hecs` ECS, and Kiba owns a fixed schedule

An ECS is adopted because many similar objects and streaming are assumed. The schedule has the
fixed order `commands → fixed updates (then physics) → updates → animation, particles, streaming, and
cameras → extract → render`.

`bevy_ecs` also works standalone **[verified]**, but `hecs` stays. For the same spawn-and-query wasm,
`hecs` 0.10 is 18 KB gzipped and `bevy_ecs` 0.16 is 51 KB **[verified]**. `extract` walks every entity
each frame without change detection, which costs about 1.5 ms for 10,000 entities and leaves no need
for `bevy_ecs`'s built-in tracking.

### D5. Game logic and rendering run on the main thread

Signals are not `Send`, so moving the game to a Worker would turn D2's bridge into asynchronous
messages. Wasm threads also require COOP and COEP headers, and no mention of them was found in
Whisker's Web code (searched `platforms/web` and `whisker-build`) **[verified]**.

The cost is that heavy game logic consumes the same frame budget as the UI.

Revisited in P3: packs are fetched asynchronously but parsed on the main thread, with uploads capped
at 8 MB per frame. The 30-minute soak never dropped below 59 fps under a 6x CPU throttle, so asset work
stays on the main thread. Workers are the next step if a device measurement shows parse hitches.

### D6. On Web, the GPU baseline is WebGPU only, without WebGL2

WebGPU on Android is enabled by default from Chrome 121 on Android 12 or later with Qualcomm or Arm
GPUs **[verified]**. Firefox for Android is reported as not yet shipped, and older devices are not
reached. This is accepted.

WebGL2 has no compute or storage buffers, so supporting it would mean maintaining a separate
renderer.

Chrome 146's compatibility mode (GLES 3.1) **[verified]** is out of scope, because every phase met its criteria on
WebGPU.

### D7. Rendering uses forward+, without deferred on the lowest tier

Memory bandwidth dominates on tile GPUs **[verified]**. Writing out a G-buffer is expensive, and core
WebGPU has no subpass for reading a G-buffer within a tile. The closest addition, transient attachments
(Chrome intent to ship, January 2026) **[verified]**, keeps one pass's attachments in tile memory but does
not let a later pass read them there.

| Tier | Target | Frame time | Longest side | Lowest scale | Features |
|---|---|---|---|---|---|
| T0 | Mid-range Android | 33 ms | 1280 px | 0.5 | forward, CPU culling, instancing, probes, 16 local lights, two 1024-texel sun cascades, FXAA, bloom, fog, particles |
| T1 | High-end phones | 16.7 ms | 1920 px | 0.7 | T0 + half-resolution AO, light shafts, three 2048-texel cascades with soft shadows |
| T2 | Desktop | 16.7 ms | native | 0.7 | T1 + stronger AO and a fourth cascade |

All tiers use the same assets. T0's image is tuned as the product baseline, not as a compromise.

The tier is chosen while the game runs, not by a benchmark at startup, because a benchmark draws a scene
other than the game's and a phone that heats up slows minutes later. A game starts at T2. Whenever a
one-second window has more than a tenth of its frames over 110% of the tier's frame time, internal
resolution drops by a tenth. Under vsync a late frame costs a whole extra refresh, so a median would hide
up to half the frames stuttering. At the tier's lowest scale the tier drops only after three such windows
in a row, so a burst of action does not cost features. A display's refresh caps how fast frames arrive,
so three windows with at most one late frame are enough to try a tenth more resolution, and a try that
fails within five windows doubles the wait for the next. A tier climbs back only when the output shrinks
below three quarters of the size at which it was lost, as when leaving full screen, so the image does
not pulse. Frames over 250 ms, from a hidden tab or a load, do not count. `GameSetup::set_tier` fixes the tier for a settings menu, and resolution still
adapts. The scene renders at the scaled size, and post-processing upscales it with bilinear filtering
and a light sharpen.

Deferred, with reasons:

- **GPU culling.** CPU culling held 20,000 instances within about 10 ms under a 6x throttle, so it is not
  the bottleneck yet, and multi-draw indirect, which would make one culled draw per batch cheap, is still an experimental
Chrome feature behind a flag **[verified]**.
- **Temporal upscaling.** It needs motion vectors from every pass, a jittered projection, and history
  rejection. Spatial upscaling is what T0 needs, and whether T1 needs more waits for a device measurement.
- **Shadows from point and spot lights.** Each needs its own depth map, six for a point light, and the
  caster pass is already the largest CPU cost per instance (see P6). They come when a game asks for them.
- **SSR and volumetrics.** Reflection probes cover rooms and streets, and screen-space reflections would
  add only what moves; exponential fog and screen-space light shafts stand in for volumetric fog, which
  would need a compute froxel grid.
- **Motion blur.** Like temporal upscaling, it needs motion vectors from every pass.
- **Sorting blended surfaces per triangle.** Blended surfaces sort back to front by entity, which is
  wrong only where two cross; order-independent transparency costs a second pass on every tier.

Every tier uses reversed `Depth32Float` depth, which keeps precision over open-world distances.

Every tier shades with the metallic-roughness model: GGX, height-correlated Smith visibility, and
Schlick Fresnel. It matches glTF, so cooked assets need no material conversion. Material factors are
per-instance values, so entities that share a mesh and textures stay in one draw call. Normal maps
derive their tangent frame from screen derivatives, so meshes carry no tangent attribute.

The sun casts shadows through two to four cascades in one depth array, split between a linear and a
logarithmic spacing of `Sun::shadow_distance`. Entities are culled per cascade, and entities off screen
still cast. Each cascade snaps to whole texels so that edges do not crawl, blends into the next over its
last 15%, and the last fades out. Lookups push off the surface by 1.5 texels along its normal. T0 takes
four rotated hardware-filtered taps. T1 and T2 use percentage-closer soft shadows: eight taps find the
average blocker, whose distance sizes a 16-tap disk by the sun's angular radius, so a penumbra widens
away from contact. Interleaved gradient noise rotates the disk per pixel, trading banding for grain.

Up to 16 `PointLight`s and `SpotLight`s, nearest the camera among those whose range reaches the view, are
shaded per fragment with an inverse-square falloff windowed to zero at their range. They cast no shadows.

Materials have glTF's alpha modes. Masked surfaces discard below a cutoff and cast shadows with their
holes. Blended surfaces draw after the sky, back to front by entity, premultiplied so reflections keep
their strength, and cast no shadow. Double-sided materials light back faces as if they faced the viewer,
and `transmission` lets the sun behind a thin surface through, shadowed from its far side. Emission is
added after lighting and lights nothing else, so a lamp pairs it with a `PointLight`.

A `ReflectionProbe` captures the scene around its center into six 128-texel layers once, and again on
`recapture`, then filters them by roughness into a 64-texel cube of five levels with 64 importance samples
per texel. Inside its box, lookups are corrected toward the box's walls, which keeps near walls and
floors in place, and fade to the sky cube at the edges. A frame uses one probe, the first holding the
camera, so the capture costs nothing per frame after the first.

`Sky` is an analytic gradient, with image-based lighting calculated from it on the GPU: 128 samples per
texel into a 32-texel radiance cube of five roughness levels and a 16-texel irradiance cube. This
happens once per change of the sky, so a frame pays only two cube lookups per fragment. The sun is left
out of the cubes, because surfaces receive it directly.

`Sky::from_sun` derives sky colors from the sun's height for a day cycle, and `Fog` fades surfaces into
the air with distance. Post-processing adds FXAA on every tier, judged after tone mapping;
`Camera::depth_of_field`, a thin-lens blur gathered over 24 taps that keeps a blurred foreground spilling
over a sharp background; and on T1 and T2, light shafts that march 32 steps toward the sun and glow in the
sun's color as much as the way is clear, so a dim moon cannot wash out a dark sky. Ambient occlusion reads the depth of the main pass at half resolution with eight
samples per pixel and is blurred while post-processing reads it. Bloom keeps light above 1.0, halves it
four times, and adds the chain back up. Particles are CPU-simulated sprites drawn as one instanced draw
that adds light, so smoke that darkens waits for sorted transparency.

Skinning runs in the vertex shader from a uniform palette at a dynamic offset. A uniform rather than a
storage buffer keeps vertex shaders within what GLES guarantees. Skin data is a separate vertex stream,
so unskinned meshes do not pay for it, and each posed mesh draws alone.

### D8. The world is delivered in cells and controlled by memory budgets

The world is split into grid cells, loaded and evicted with LRU by distance and budget. Within
`StreamSettings::full_distance` a cell loads its full pack with mesh LODs, and out to `far_distance` it
loads a cooked far stand-in of a few simplified meshes, which serves as its HLOD. Impostors are not
built, because stand-ins already held about 400,000 triangles and 200 draw calls in the soak. Upload
volume per frame is also capped.

### D9. Assets are cooked, and run-time glTF is for development only

Run-time glTF sits behind the `gltf` feature so that shipped games do not carry its parser. It reads
binary glTF geometry, vertex colors, material factors, base color, metallic-roughness, and normal
textures, emission with its strength extension, alpha modes, double-sided materials, skins of up to
128 joints, and animation clips, with cubic curves played as straight lines. Occlusion textures, texture
samplers, and transmission are not read yet; glTF's transmission extension is refraction, not the thin
translucency `Material::transmission` gives. Images decode and build mipmaps on the main thread, which D5 allows only for development.

Distribution uses per-cell `.kiba` packs. Compressed textures are cooked into a separate pack for
each GPU format, with no run-time transcoding.

Play device support is over 80% for ASTC and over 95% for ETC2 **[verified]**. ASTC is preferred, with
ETC2 and BC7 also provided.

Packs hold raw RGBA textures for now. Texture compression is deferred, because it needs an encoder in
`kiba-cook` for each format and a device to check quality and the WebGPU features each GPU reports. The
soak's synthetic world peaked at 210 MB of GPU data, within the 300 MB budget, so raw textures do not
block anything measured so far.

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
| Audio | Own `Sound` over Web Audio for short effects with a 3D listener; music and background audio stay with `whisker-audio` | P3 |
| UI / text | Whisker | Done |
| Scripting | Rust only | Out of scope |
| AI / navigation | External crate for navmesh | P5 or later |
| Networking | Game's responsibility | P5 or later |
| Save | Game's responsibility (`whisker-local-store`) | Out of scope |
| Editor | Not built. Blender + cook + debug overlay + hot reload | Out of scope |
| Platform | Web, iOS, Android, and Desktop Hosts provided in P0 | P0 |

### D11. Games shape their own look through material shaders, not render passes

A game whose look is its identity, such as a water creature that bends what is behind it, needs code of
its own on the GPU. Unreal Engine gives that through materials: a node graph that fills fixed inputs of a
fixed pipeline, World Position Offset to move vertices and Refraction to read the scene behind
**[verified]**. Few of its games add render passes. Kiba takes the same shape, in text:

- A `Shader` is WGSL that a `Material` names. It may define `vertex`, which moves a vertex in the
  entity's space, and `surface`, which turns the material's inputs at a pixel into a surface's color,
  opacity, roughness, metalness, emission, and normal. Kiba shades the result as it shades any
  material, so tiers, shadows, lights, probes, fog, and exposure stay consistent. WGSL text rather than
  a node graph, because Kiba has no editor and AI writes text well.
- `Material::parameters` hands eight vectors to the shader per material, so a game drives the shape
  every frame without new code; entities whose parameters differ draw separately.
- A blended surface's shader can read the scene behind it, its color and its depth, copied once after
  opaque surfaces and the sky. That covers refraction, water depth, soft edges, and projected decals.
  Blended surfaces do not see each other through it. The copy is made only in frames that draw a
  blended surface with a shader, so a game without one pays nothing.
- A shader's vertex movement reaches its shadow, and `Shader::bounds_scale` widens culling for a
  vertex that moves past the mesh's bounds.
- A shader that fails to compile is logged and drawn as the plain material, so a typo does not stop
  the game.

Left out, with reasons:

- **Render passes from games.** They would bypass tier selection and the per-tier budgets, and the
  material inputs above cover what the slime game draws with passes of its own.
- **A final-color hook.** A surface writes emission with zero base color to replace what lighting
  would add, which keeps fog and exposure correct without exposing them.
- **A decal component.** A cube whose shader projects the scene's depth into it is a decal, so the
  component would add a name, not an ability.
- **Gameplay frameworks** such as Unreal's Gameplay Ability System. Combat rules belong to games and
  libraries, as D10 decides for AI and networking.

### D12. The whole slime game is ported as a game built on Kiba, and only its gaps reach the engine

P7 showed one effect. P8 ports the whole `slime` game, an open-world action game set in the real city of
Sasebo, onto Kiba, using the game's own assets. It is the first full game built on Kiba, so it tests the
claim that Kiba is general-purpose.

- **Kiba learns no slime formats.** slime's runtime files (`.owa` props, `.owk` characters, height tiles,
  and city cells) are read by a converter that belongs to the game, `example/slime/convert`. The
  converter writes glTF and inputs for `kiba-cook`. The game is the example's `slime-game` feature,
  because the repository's workspace lists its members by hand. Converted assets stay out of git and are read from the
  slime checkout.
- **Look, not technique.** slime renders with deferred shading, TAA, GTAO, SSR, and volumetrics. The
  port matches the look with Kiba's forward+ tiers and material shaders, and it does not port those
  passes.
- **The engine changes only for a gap the port hits**, one commit each, judged by whether another game
  would need the same thing.
- **Milestones**, each playable on the Web:
  - **M1:** the forest meadow and its boss. This covers the sword mage and slime forms, combo, skill,
    ultimate, just dodge, Elmgard's AI and BREAK, the HUD, sound, and effects.
  - **M2:** Sasebo streamed around the player.
  - **M3:** traffic, crowds, touch and gamepad controls, and the world reveal.
- **Not ported:** content the original does not have either, namely roaming monsters and online PvP.
  Generation that spends credits (Meshy, ElevenLabs) is not run again.

## Target API

This is what a Whisker app author writes today. A shipped game loads a `.kiba` pack cooked by
`kiba-cook model` in place of the `.glb`, through the same `Model::load`. Mutable game state lives in components, not in variables shared between callbacks,
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

These are targets for the reference device. Stand-in measurements under [Phase results](#phase-results)
stay within frame rate, internal resolution, draw calls, triangles, GPU textures, baked light (about 2.3 KB
of probes per synthetic cell), and download size. Total tab memory is inferred, not read, as P3 explains.
Device measurements are deferred to the project owner with spike 1.

A first measurement on a Mac, in a web build, is **[verified]** only for that machine: 10,000 cube
instances (120,000 triangles, one draw call) all moving each frame take 2.3 ms of main-thread time,
and 4.3 ms under Chrome's 6x CPU throttling, at the display's 60 fps. Throttling does not slow the
GPU, so this shows only that the CPU side has room. It does not replace the device check, which is deferred to the project owner.

| Item | Initial T0 value |
|---|---|
| Frame rate | Stable 30 fps (33 ms, without thermal throttling) |
| Internal resolution | Adjusted dynamically from about 1280 px on the long side |
| Draw calls | 300 or fewer |
| Triangles per frame | 500,000 or fewer |
| Render passes | Two shadow cascades, main, and post-processing, plus bloom's seven small passes |
| GPU textures | 300 MB or less |
| Baked light data | 2 MB or less per cell (compressed, assuming 256 m cells) |
| Total tab memory | 1 GB or less (no primary source found for the limit) |
| Initial download | 20 MB or less (gzipped, including wasm) |

## Phases and exit criteria

This document is complete when every phase row has its implementation, a Web measurement, and an API
review recorded under [Phase results](#phase-results); every spike has a result; every open question
is answered; and no **[assumed]** or **[unverified]** label is left without a deferral reason.

**Status: complete.** On 2026-11-04 the project owner accepted the deferrals that need a phone as these
items' results: spike 1, spike 3 beyond its desktop measurement, and the exit criteria that name the
reference device. They run once a reference device is chosen. P8 was added afterwards and is in progress.

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
| P6 | Bounce light, local lights, transparency and emission, reflection probes, shadow quality, FXAA, depth of field, light shafts, transmission | Each renders correctly on the Web, and the stress scene settles on a tier within its budget |
| P7 | Material shaders, material parameters, the scene behind blended surfaces (D11) | The water slime of the `slime` game, its body, ground ripples, and the light it focuses on the ground, is written in game code alone and runs on the Web |
| P8 | The slime game ported whole onto Kiba (D12) | Each of M1–M3 plays on the Web as the original does, from slime's assets, with no slime-specific code in `kiba` |

## Phase results

### P0

**Built:** the `Viewport` element, which a Whisker layout sizes like any view, the render targets, and Hosts
for the Web, iOS, Android, and Desktop. The exit criterion was checked when P0 landed (92cac9bb), before
measurement moved to the Web only: each Host drew one triangle through the shared `SceneRenderer` on the
Web, the iOS Simulator, the Android Emulator over GLES, and macOS. Native Hosts still draw that
placeholder; only the Web Host runs a `Game`.

**Measured** with the example at 1280x720 in Chrome on the development Mac, cache disabled, from a local
server: the GPU was ready about 40 ms after navigation, and the download is 1.25 MB of wasm and 15 KB of
JavaScript gzipped. The canvas followed its layout when the window went from 1280x720 to 800x600, and a tap on
UI layered over the viewport reached the game as a command, whose result came back through a signal.

**API review** (run after P6, with P5's): an agent given only the docs built a viewport with a HUD
over it and a panel below with a button, and it compiled on the first try. It guessed how to layer the
HUD, how state reaches the UI, what `Viewport`'s `Signal<String>` prop accepts, and whether the viewport
follows resizes; `Game::send` said commands reached the next update, though they run before fixed
updates. Fixed as a result: a crate-level recipe for a HUD, a panel, signals out, and commands in, with
where `PositionKind` comes from; `Viewport`'s docs on its prop and resizing; and `Game::send`'s timing.

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
Only one volume applies at a time: streamed cells carry their own, and the one around the camera lights
the frame, so surfaces just across a cell edge use the neighbor's volume only once the camera crosses.

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

### P3

**Built:** `.kiba` packs read without the glTF parser, `kiba-cook` commands for models, probes, and worlds,
mesh LODs by distance, `WorldStream` with far stand-ins and a byte budget, Rapier physics behind the
`physics` feature (`Physics`, `RigidBody`, `Collider`, `raycast`), and `Sound` over Web Audio. Streamed
cells have no colliders yet, so a game with physics spawns its own ground; the docs say so.

**Measured** with the example's `soak` feature at 1280x720 under a 6x CPU throttle: the camera flew back
and forth across a synthetic 4 km square of cooked cells (16x16 cells of 256 m, 734 MB on disk) at 20 m/s
for 33 minutes. Frame rate averaged 60 fps, and the worst one-second window was 59.2 fps. Loaded cell
data rose to the 300 MiB budget after about 15 minutes and held there, GPU data peaked at 210 MB, the JS
heap stayed under 44 MB, and the console showed no errors. A scene held 100–210 draw calls and up to
430,000 triangles. Rapier adds about 222 KB gzipped to the wasm.

Two limits remain. The world is synthetic boxes, so its textures and triangle density are lighter than
authored art. Wasm linear memory was not read, so total tab memory is inferred from loaded bytes plus GPU
data, about 0.5 GB, against the 1 GB budget. **The exit criterion is met by the stand-in only;** the
reference device run is deferred to the project owner.

**API review** (run once P4 had landed, covering both phases): an agent given only the docs wrote a game
with streaming, a physics ball, a raycast, a jump sound, a campfire, fog, a day cycle, and a tier readout,
using about 22 Kiba types through `kiba::prelude::*`. It compiled on the second try; the only error was
a placeholder it invented for placing a `Transform`. Fixed as a result: `Transform::at`, documented that
streamed cells have no colliders, that a ray starting inside a collider hits it, and how often a day cycle
should change `Sky`. Left as is: action names are strings, so a typo is found only at run time by the
debug panic, and `raycast` has no filter.

### P4

**Built:** tiers T0–T2 chosen while the game runs, dynamic resolution with a sharpened upscale, bloom,
half-resolution ambient occlusion on T1 and T2, `Fog`, `Sky::from_sun`, and `ParticleEmitter`. GPU
culling, temporal upscaling, SSR, volumetrics, and additional shadows are deferred with the reasons in D7.

**Measured** with the `stress` scene at 1280x720 in Chrome on the development Mac. Without throttling it
stayed at T2 and full resolution, and under a 6x throttle too. Under a 20x throttle it stepped from T2 to
T1 to T0 within about 30 seconds and settled at half resolution. That scene is CPU-bound there, with 39 ms
of main-thread work, so lowering resolution could not restore 30 fps; the governor sees only frame times and
cannot tell CPU from GPU cost. The example shows the campfire, bloom, and AO with no console errors.

A first Web run found that on a 60 Hz display frames never arrive faster than 16.7 ms, so resolution lost
to a load hitch never came back. The governor now probes upward whenever the budget is met. It first
judged each second by its median frame, until a Retina full screen showed a median of 16.7 ms over a
second at 53 fps, and a tier lost there stayed lost after leaving full screen; it now counts late frames.

**Automatic tier selection works on the stand-in.** Which tier a real mid-range phone settles on is
deferred to the project owner with spike 1.

**API review:** shared with P3 above. Its main finding was that the tier could be fixed only during setup,
which a settings menu cannot use, so `Context::set_tier` was added.

### P5

Navmesh and networking are not built, because no game built on Kiba has asked for them yet. D10's plan
stands: an external navmesh crate, and networking left to the game, each when a game requests it.

**API review:** the same agent wrote a chase game in which an enemy finds its way around boxes by a
breadth-first search on its own grid, and each fixed step writes a 14-byte snapshot of the two positions
as a stand-in for networking. It compiled on the first try without the `physics` feature, because
raycasts check a line of sight but cannot route around corners. It asked for the fixed step's contract,
what is promised about determinism, how entities are named across machines, a model's bounds, and a
statement of what Kiba leaves to the game. Fixed as a result: `set_fixed_rate` states that each step is
exactly `1 / hz` and that a frame advances at most 0.1 seconds, and a crate-level section on what Kiba
leaves to the game covers pathfinding, snapshots, entity ids, and the lack of a determinism promise.
Left as is: a model's bounds, which no game has needed yet.

**Measured** with that game at 1280x720 in Chrome on the development Mac: 60 fps with no console errors,
the enemy reached the player around the boxes, and Reset in the panel cleared the score.

Native Hosts (iOS, Android, and Desktop) still draw only the placeholder, because every phase was
measured on the Web. Running a `Game` on them is deferred to the project owner.

### P6

**Built:** one bounce of light, cooked into probe volumes as the color and facing of what hides the sky
and lit at run time by the current sun and sky; `PointLight` and `SpotLight`; `Material::emissive`,
`opacity` with `AlphaMode`, `double_sided`, and `transmission`; `ReflectionProbe`; shadow cascades and
soft shadows by tier; FXAA, `Camera::depth_of_field`, and light shafts. glTF import reads the new material
fields, and packs moved to version 3, so packs cooked before it must be cooked again.

**Measured** with the `stress` scene at 1280x720 in Chrome on the development Mac, against the commit
before P6 built the same way. Without throttling both hold T2 at 60 fps; P6 takes 3–5 ms of main-thread
time, against 3–4 ms, for 22,843 instances against 20,055, because the third and fourth cascades add
casters. Under a 6x throttle both fall to T0, unlike the P4 run, which stayed at T2; at T0 P6 takes
17–25 ms against 19 ms and runs at 46–57 fps. The first P6 run was 40% slower at T0, because every instance
hashed a batch key that now holds textures and pipeline state with SipHash once per pass; FxHash removed
that. The soak world, cooked again for version 3, streamed for two minutes under a 6x throttle at T2 with
about 2 ms of main-thread time and 120–140 draw calls. The example shows each feature on the Web with no
console errors. Throttling does not slow the GPU,
so the cost of soft shadows, 16 lights, and the post passes on a phone is unmeasured.

**The exit criterion is met on the stand-in.** Whether the result reads as photorealistic still needs
authored art and the reference device, as in P2.

**API review:** an agent given only the docs wrote a scene with every P6 feature and a lamp toggled by a
key, using 26 concepts, and it compiled on the first try. Its guesses were all about scale and defaults.
Fixed as a result: the docs now state that a `Sun::color` near 1 is daylight and the scale of every other
light, list the default values of the lights, `Sun`, and `DepthOfField`, say that `Sky::from_sun` takes
`Sun::direction`, that spot angles are half-angles, that a probe volume's box is where its models were
cooked, and that a reflection probe misses models still loading until `recapture`. Left as is: no
constructor returns a matching `Sun` and `Sky`, because `Sky::from_sun(sun.direction)` is one call.

### P7

**Built:** `Shader` with `vertex` and `surface` hooks, `Material::shader` and eight `parameters`, the
WGSL helpers, the scene behind blended surfaces, shader shadows, `bounds_scale`, and the fallback to the
plain material while a shader compiles or after it fails. The example's `slime` feature ports the water
slime of the `slime` game, using about 540 lines of Rust and 230 of WGSL in the example alone. It has
three parts:

- The body melts into a puddle at rest, gathers into a ball to move, and drips, splats, and trails.
- Ground ripples spread where it lands.
- A tinted shadow carries moving caustics.

Building it changed the engine once. A blended black metal still reflected the sky at grazing angles, so
reflectance at grazing angles now follows head-on reflectance as in Filament.

**Measured** with that example at 1280x720 in Chrome on the development Mac. With the slime walking and
jumping, it holds T2 at 60 fps with or without a 6x CPU throttle. It uses 15–40 draw calls, 19,000–28,000
triangles, and under 1.5 ms of main-thread time, and the console shows no errors. Copying the color and
depth behind takes no main-thread time worth measuring. Throttling does not slow the GPU, so the cost on
a phone of the copy, the second pass, and a 128x64 sphere moved per vertex is unmeasured.

**The exit criterion is met:** the slime is written in game code alone and runs on the Web.

**API review:** an agent given only the docs wrote a game with three shaders using 27 concepts:

- a wobbling jelly,
- a refracting puddle with rings,
- a blended spot of light.

Its Rust compiled on the first try, and all three shaders passed validation unchanged.

Its guesses were about these:

- Mesh layout.
- Whether emission is scaled by opacity.
- Whether fog applies to the scene behind.
- Whether moved vertices keep their normals.
- The clock behind `time()`.
- How a refracting surface can also fade at its edge.

Fixed as a result: `Mesh::plane` states its origin and uv corners, and `Shader` states that:

- the normal a `vertex` returns is kept;
- `emissive` is scaled by opacity under Blend;
- `time()` is the clock of `Time::elapsed_seconds`;
- fog applies to emission;
- a refracting surface fades by blending toward the unbent scene color.

`parameters` say that changes show the next frame.

Left as is, each found while writing the slime:

- Blended surfaces sort by entity origin. The ripples and shadow sit their entities 1 m below the ground
  and lift their vertices back so they draw behind the body.
- Blended surfaces do not see one another through `scene_color`.
- Additive light needs `emissive / opacity`.
- `Viewport`'s props are documented only in the crate guide.

### P8

#### M1: the forest meadow and its boss

**Built:** the example's `slime-game` feature plays the forest meadow and the fight with Elmgard from
slime's own assets. `slime/cook.sh` runs the converter and `kiba-cook`; its output is git-ignored, so the
feature builds only after it has run. The port has four layers:

- **Rules and logic** are slime's `gameplay` crate and walk-mode frame logic, copied with import paths
  changed: about 10,200 lines with their tests. The logic writes everything slime handed its renderer
  and page into one `Output` each frame.
- **Drawing** turns `Output` into entities: about 1,500 lines of Rust and 860 of WGSL.
  - The hero and boss are posed joint by joint through `Pose`, toon shaded with an inverted-hull outline,
    and flash and dissolve through `Restyle` parameters.
  - The slime body reuses the P7 shaders and gains its bubbles.
  - Every effect is ported from slime's effect shaders, one entity per effect: telegraphs, ripples and the
    magic circle, slash and wind arcs, drops, mist, glints, leaves, rock shards, the water sword and its
    trail, and the water figure.
- **The HUD** is rebuilt in Whisker from slime's page, about 1,800 lines, fed by the game's HUD JSON. Its
  sound cues play through `Sound` at the original's volumes.
- **Lighting** takes the original's 15:30 sun, its stylized sky, and a lower exposure.

Building it changed the engine in nine commits, each for a gap another game would hit:

- `Pose` and the Model and Animator methods that sample and read it, with rig node names in packs.
- `Restyle`, one shader over every part of a model, and `Pixel::front_facing` for outlines.
- Mouse buttons and function keys as `Key`s.
- `GameSetup::lock_pointer`, `Input::pointer_locked`, and locked movement through `Pointer::delta`,
  found inside the shadow root Whisker mounts the canvas in.
- `Context::pixel_ratio`, so UI over the viewport can lay out in layout pixels.
- `Material::casts_shadow`, for bubbles the sun shines through.

**Not matched,** for lack of an engine feature:

- Audio has no playback rate, effects bus, master gain, or synthesis. The original's pitch variation and
  reverb are gone, and its 11 synthesized cues, its wind, and its music are silent.
- Effects draw one entity each, with no instancing, and pass per-vertex data only through color and uv.
- Post effects are Whisker overlays: the hurt edge and the ultimate's tint are gradients, without
  desaturation.
- The HUD's ring and tick gradients do not animate, and off-center radial gradients are approximated.

**Measured:** not yet. The Browser pane used here draws frames only while the session's window is in
view, so frame times are pending a run in a visible browser with F3, which logs them.

**Unverified:** pointer lock. The Browser pane refuses `requestPointerLock` for any page, the original
included, so mouse look is checked only by the test of `PointerMotion`.

**API review:** a fresh agent, reading only the docs, wrote a toon character with an outline, a
glowing effect, and mouse look in two compile attempts across 22 concepts. Its guesses became docs:
that `vertex` runs before skinning, how to drop a pixel and shade unlit, how to add light under
Blend, what a partial `Pose` keeps, and the units of `Pointer::delta`. `Transform::matrix` became
public and `EulerRot` is re-exported. Left open: a pointer does not say whether it is a mouse or a
touch, and a game cannot release the pointer lock.

**Touch, and how the HUD talks to the game:** the original's phone pad (a stick on the left 45%, a
camera drag elsewhere, the skill discs as buttons, a sound toggle, a turn-sideways prompt) cost 280
lines in `hud/touch.rs` and 190 changed lines elsewhere. Checked at 844×390 with synthetic touch
pointers: stick and camera drag at once, a button held while its finger slides off and released
where it lifts, switching back on a key, and the sound toggle's round trip.

- With the grain: the game writes signals in `on_update` and the HUD reads them; buttons reach the
  game through `Game::set_action`, whose edges survive a tap shorter than a frame; Whisker's
  `TouchEvent` carries `pointer_id` and `pointer_type`, so `on_capture_touchstart` at the app's
  root tells a finger from a mouse without asking Kiba; and an interactive HUD node is the DOM's
  hit target too, so the canvas never sees the same finger.
- Against it, each worked around in the game:
  - The pad needs the `Game` to call `set_action`, and the game's setup needs the pad, so the pad
    takes the `Game` after both exist.
  - Actions are digital. The stick's tilt and the camera drag go through signals the game reads
    untracked, and the game resets the drag it took.
  - Whisker's `pointer_events: None` removes a whole subtree from hit testing, unlike CSS, so the
    HUD's root switches between `Auto` and `None` with the mode rather than enabling the buttons.
  - A mouse press is a `pointerdown`, which has no `on_` handler, so only `on_click` notices the
    mouse coming back.
  - Kiba asked for a pointer lock on every finger; it asks only for a mouse now.
- Fixed in Whisker after this check: `visibility` now inherits, so a hidden parent hides its
  children, and the Web surface root sets `touch-action: none`, so the browser does not claim a
  drag outside a scroll view. A finger was already captured by the node it landed on, so each
  button holds and releases its own key.
- Unverified: a real phone, including whether the browser still cancels any HUD drag. Safe-area
  insets and the fullscreen button are left out. A button's pressed look comes back through the
  game one frame late.

## Spikes to run first

Each result can change the decisions above.

1. **Android device baseline** [deferred]: Collect WebGPU adapter limits, PBR frame rate, and ASTC
   support on the reference device. Affects D6, D7, and all budgets. Deferred to the project owner,
   because measurements here are Web-only on the development Mac and no reference device is chosen.
2. **Frame integration** [verified on Web]: Kiba rides Whisker's frame driver, so `on_update` can
   call `signal.set`, and the Web Host's own loop is gone. Each frame posts the next one through
   `RuntimeDispatcher`, which requires a drain to run only what was queued when it started. A drain
   that also runs newly posted work draws up to 4096 frames without yielding and hangs the page.
   Dispatches also drain on input, so a frame can occasionally run off the display's cadence.
   Native Hosts do not run a `Game` yet.
3. **Tab memory limit** [deferred]: Increase allocation on the reference device and measure where
   context loss and tab discard occur. A desktop's limit says nothing about a phone's, so this waits
   for the device. The P3 soak shows the budgets keep Kiba's own use near 0.5 GB. Measured on the Web
   on the development Mac (Apple Metal 3, 32 GB): wasm memory grew to the 4 GB wasm32 ceiling, and 24 GB
   of WebGPU buffers allocated without an out-of-memory error or a lost device, so the desktop shows no
   limit below either budget and gives the phone nothing to calibrate against.
4. **Wasm size** [verified on Web]: The example is about 0.9 MB gzipped without `gltf` content and
   1.12 MB with it. `hecs` adds 18 KB, and Rapier about 222 KB. After P4 the example is 1.22 MB with `gltf`, well within
   the 20 MB download budget.
5. **Hot reload** [verified on Web]: With `whisker run web`, editing `Game::new`'s closure applied as a
   0.7 MB subsecond patch in under a second. The app remounts, so `Game::new` runs again into a new
   registry entry, and the old game is removed by its owner's cleanup. Game state starts over on every
   patch; keeping the world across a patch is not attempted.

## Open questions

- **The reference device.** Not chosen; it is the project owner's call. Android 12 or later with an
  Adreno 6xx-class or Mali-G6x-class GPU is the working target, and the Mac stand-in in the phase rules
  serves until then.
- **Iteration speed.** Answered: `kiba` tests run natively with wgpu headless rendering, and `whisker run
  web` hot-patches game code in under a second, so browser rebuilds are needed only for measurements.
- **wgpu version.** Kiba stays on 25.0.2 with `platforms/desktop`. No P1–P4 feature needed a newer
  version, and upgrading means changing Whisker itself, which the project owner decides with Whisker's
  own upgrade.
