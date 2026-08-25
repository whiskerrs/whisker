# RFC 0002: Renderer Interface and Frame Protocol

- Status: Draft
- Authors: Whisker maintainers
- Created: 2026-08-18
- Discussion: TBD
- Tracking issue: TBD
- Depends on: [RFC 0001](0001-runtime-modules-and-build-plugins.md)
- Supersedes: None

## Summary

Whisker owns the logical UI tree, reactivity, resolved style, layout, motion,
event routing, and frame transactions in Rust. A versioned `Renderer` interface
projects the resulting retained scene onto a platform Host:

- Android Views through Kotlin/Java;
- UIViews through Swift/Objective-C;
- DOM nodes through JavaScript on Web;
- a Whisker-owned native Rust Host that lowers the retained scene to window,
  GPU, text, input, and accessibility primitives on Desktop.

The Host supplies frame callbacks, native text and intrinsic measurement,
input, viewport changes, resource readiness, and concrete element factories.
Rust replies once per changed frame with one versioned semantic `FramePacket`.
Bindings may encode that model when a language or memory boundary requires it;
Desktop borrows it directly. The packet is a transaction containing only
changes since the last accepted scene revision, so Whisker does not send the
whole screen every frame.

The renderer is an ordinary runtime module implementing
`whisker.renderer@1`. UI modules contribute element types through the narrow
provider contract refined by
[RFC 0004](0004-native-modules-and-host-elements.md). The scene coordinator,
style resolution, Taffy layout, frame generation, measurement transaction,
and event propagation remain Whisker core. Individual `View`, `Text`, or
custom-view instances are scene nodes, not module instances.

## Motivation

Whisker currently stores Lynx `FiberElement` handles in its view layer and
applies dynamic attributes and complete inline-style strings directly through
the Lynx bridge. This makes the runtime's nominal renderer abstraction depend
on Lynx's tree, style, layout, animation, and scheduling behavior.

Removing Lynx requires a boundary that:

- supports native Android/iOS views and DOM without making one backend the
  semantic reference implementation;
- keeps reactive updates and animation values observable in Rust;
- batches fine-grained changes rather than crossing FFI or WASM/JS for every
  property;
- supports Host-dependent text and custom-view measurement without giving the
  Host ownership of the entire layout algorithm;
- can be recorded, validated, and tested entirely in Rust;
- allows custom UI packages to add element types without adding special cases
  to the renderer kernel;
- makes call direction, ownership, ordering, and synchronization explicit.

## Goals

- Define the versioned runtime interface between Whisker's Rust scene and a
  renderer provider.
- Define the bootstrap, frame, measurement, event, resource, and teardown
  sequences in both call directions.
- Define a compact, transactional, forward-evolvable frame protocol.
- Make full-tree initialization and incremental updates use the same protocol.
- Distinguish runtime modules, element types, and element instances.
- Support Rust-only recording, test, headless, and future SSR renderers.
- Keep Web animation within one JavaScript animation-frame callback and
  Desktop animation within one native window frame callback, without
  asynchronous native-process IPC.
- Permit custom native, DOM, and Desktop Rust elements while retaining a
  single Host per platform.
- Use the same `#[whisker::main]` Application descriptor and surface runtime
  for a generated standalone root and an optional embedded Host container.

## Non-goals

- Defining the supported typed style surface or exact layout and paint
  semantics. RFC 0003 owns those decisions.
- Standardizing the binary encoding byte-for-byte in this RFC. This RFC fixes
  the information model and invariants; implementation work will assign final
  opcodes, alignments, and field widths.
- Requiring every renderer to use platform view objects. A future canvas or GPU
  renderer may implement the same scene contract if its capabilities satisfy
  the application.
- Preserving Lynx element handles, raw inline-style strings, or the current
  imperative animation extension.
- Defining a DOM, WebView, or third-party application-framework fallback
  renderer for Desktop. Desktop v1 uses a Whisker-owned native Rust Host.
- Making every element instance a Whisker module.
- Changing Whisker's existing function-call-style `render!` syntax. The new
  scene and renderer pipeline remains behind the current authoring API.
- Defining how an embedded Whisker application is packaged, linked, or added to
  an existing Xcode, Gradle, Web, or Desktop project. This RFC defines the
  runtime surface contract only.

## Ownership boundary

### Rust owns

- the logical element tree and stable `NodeId`s;
- component and reactive-owner lifetimes;
- signal dependency tracking and dirty propagation;
- typed inline-style composition, limited text-property inheritance, and
  resolved values;
- layout constraints and the Taffy layout tree;
- animation timelines, interpolation, springs, decay, and gesture handoff;
- classification of changes as layout, paint, composite, text, or semantics;
- generation of minimal scene changes;
- event propagation, capture/bubble, gesture state, and listener lifetimes;
- accessibility semantics as a platform-independent logical tree;
- the current accepted scene revision for each surface.

### The Host owns

- concrete Android View, UIView, DOM object, or Desktop GPU/text resource
  allocation and destruction;
- applying geometry, paint, clipping, transforms, content, and accessibility
  changes to those objects;
- native text shaping, line breaking, glyph metrics, and intrinsic measurement;
- platform input collection, hit-test results, focus, IME, and accessibility
  actions;
- viewport, scale, safe-area, font-environment, and lifecycle notifications;
- platform resource decoding where selected by the element/resource contract;
- the VSync or `requestAnimationFrame` callback;
- target-specific element factories registered by embedded UI modules.

Rust decides *what* the resolved UI means and *when* it changes. The Host
performs the platform operation that makes that state visible. For example,
Rust resolves `overflow: hidden` into a clip description; the Host implements
the corresponding Android, UIKit, or DOM clip.

## Platform model

| Target | Whisker runtime | Whisker-visible Host | Concrete backend |
|---|---|---|---|
| Android | Native Rust | Kotlin/Java | Android Views |
| iOS | Native Rust | Swift/Objective-C | UIViews |
| Web | Rust/WASM | JavaScript | DOM |
| Desktop v1 | Native Rust | Native Rust | Whisker-owned window and GPU renderer |

Desktop v1 links the Whisker runtime and the Desktop Host into the same native
Rust process. The runtime calls `MeasurementProvider::measure_batch` and
`FrameSink::present(&FramePacket)` directly through typed Rust interfaces. A
normal frame therefore requires neither packet serialization nor an FFI, WASM,
JavaScript, WebView, command-channel, or process boundary.

This remains a real Host boundary even though both sides are Rust. The boundary
separates information, ownership, and dependency direction: the Host receives
only versioned measurement and frame protocol values, and it cannot inspect
signals, resolved style storage, Taffy nodes, component ownership, or runtime
scene internals. The engine cannot inspect a window handle, GPU resource, text
atlas, or native input object.

### Desktop native Host boundary

The Desktop Host owns one `DesktopSurface` for each mounted Whisker surface.
It validates and applies `FramePacket`s to an accepted Host-side projection,
maps `NodeId`s to Desktop render nodes, retains `PreparedContentId` resources,
and marks the native window for redraw. The projection contains only data
needed for presentation; it is not a second style, layout, or reactive tree.

The native frame path is:

```text
window event/frame callback
  -> Whisker runtime scheduler
       -> MeasurementProvider::measure_batch(...) on cache misses
       -> Taffy layout and frame preparation
       -> FrameSink::present(&FramePacket)
            -> DesktopSurface accepts Host projection
  -> Desktop paint scene construction and GPU submission
```

The Desktop Host lowers common semantic operations into its own platform
representation:

```text
Whisker box/shadow     -> Desktop quad, shadow, or path primitive
Whisker text           -> shaped glyph run plus glyph-atlas references
Whisker image          -> decoded image and texture-atlas references
Whisker clip/layer     -> Desktop clip and compositing nodes
Whisker pointer input  -> native window event -> Whisker hit testing
Whisker keyboard/IME   -> native focus/IME adapter -> Whisker event routing
```

Taffy remains authoritative for every Whisker node. Window-system layout only
sets the outer surface viewport. The Desktop Host must not reconstruct Whisker
content in another declarative UI framework or run a competing inner layout.

### Desktop package and dependency policy

Desktop has one common native Host implementation under `platforms/desktop`.
It owns GPU surface lifecycle, accepted Host projection, text shaping and
rasterization, glyph and image atlases, the `wgpu` renderer and shaders,
common frame driving, common input translation, and the common accessibility
projection. GPU backend selection is static and target-specific: Metal on
macOS, Direct3D 12 on Windows, and Vulkan, with an explicit fallback only
where required, on Linux.

Thin crates under `platforms/macos`, `platforms/windows`, and
`platforms/linux` are application shells. Each owns its `winit` lifecycle,
window creation, viewport and scale sampling, redraw scheduling, application
activation and packaging hooks, target-specific window extensions, IME and
clipboard integration, native menus, accessibility attachment, and other OS
services. They must not copy the scene projection, paint lowering, shaders,
batching, glyph preparation, or GPU resource lifetime code merely to preserve
an OS directory boundary.

Common semantic values remain in `whisker-protocol`; OS-native handles and
Desktop render primitives must not leak back into protocol, engine, runtime,
or application crates. Conversely, `platforms/desktop` is a Host crate and
must not expose its GPU or window types to Whisker core. Its narrow
surface-target constructor is only a contract with the three OS application
shells.

The intended internal ownership is:

```text
platforms/desktop/
  Cargo.toml
  src/
    lib.rs
    surface.rs
    scene.rs
    measurement/{mod.rs, text.rs}
    paint/{mod.rs, box.rs, text.rs, clip.rs, transform.rs,
           composite.rs, image.rs, effects.rs}
    gpu/{mod.rs, renderer.rs, pipeline.rs, atlas.rs, shaders/...}
    input/{mod.rs, pointer.rs, keyboard.rs, ime.rs}
    accessibility.rs

platforms/macos/                 # OS application shells; same shape for peers
  src/{lib.rs, app.rs}
platforms/windows/
  src/{lib.rs, app.rs}
platforms/linux/
  src/{lib.rs, app.rs}
```

This is a responsibility map rather than a permanently fixed module layout.
The paint modules correspond to Host capabilities and protocol operations,
not to individual CSS property spellings. Layout-only properties have no Host
implementation, while shorthands and logical properties are normalized in
Rust before a Host sees a frame. This keeps the Android, iOS, Web, and Desktop
Host trees visually symmetric without introducing one object, trait call, or
allocation per property.

Source-file boundaries are not runtime boundaries. The Desktop frame hot path
stays in one crate, uses an exhaustive static match over protocol operations,
builds data-oriented paint batches, and submits them directly to `wgpu`.
Dynamic dispatch between property handlers and per-operation calls through an
OS adapter are prohibited. An OS adapter may be entered at lifecycle, native
event, or whole-frame boundaries where the cost is not proportional to scene
size.

The dependency direction is one-way:

```text
generated Desktop executable
  -> application and Whisker runtime/engine
  -> selected platforms/{macos,windows,linux} adapter
       -> platforms/desktop common Host
       -> whisker-protocol and the narrow engine Host traits
       -> window, GPU, text, geometry, and accessibility libraries
```

Whisker core crates never depend on an OS Host crate. CNG emits a complete,
Cargo-based platform project at `gen/macos`, `gen/windows`, or `gen/linux`.
That generated executable is the composition root that links the application,
`SurfaceRuntime`, and selected platform Host. The same generated project is
consumed by both `whisker run` and `whisker build`; it is not a development-only
launcher. macOS packaging produces an `.app` bundle from this Cargo project,
with `Info.plist`, entitlements, resources, signing, and any optional Xcode
integration treated as packaging concerns. GPUI is not a Desktop framework or
renderer dependency.

The initial implementation may assemble focused low-level Rust libraries such
as `winit` for windows and events, `wgpu` for GPU access, `cosmic-text` and
`swash` for shaping/rasterization, `etagere` for atlas allocation, `lyon` for
paths, and AccessKit for accessibility. This list records the intended level
of abstraction, not a protocol guarantee; exact choices and version pins are
implementation decisions. Release builds must record binary size, compile
time, enabled features, and per-platform capability coverage.

## Runtime concepts

### Renderer module

A renderer provider implements the singular `whisker.renderer@1` interface for
a surface. The runtime resolves it once during bootstrap and holds a typed
handle. No module-name or method-name lookup occurs while constructing a frame.

### Surface

A `Surface` is one independently presented root: an Android Activity content
root, iOS window/root view, browser document root, or native window/embedded
region owned by a `DesktopSurface` on Desktop.
Every node, frame, measurement request, and input event belongs to exactly one
surface.

Multi-window applications create distinct Window- and Surface-scoped renderer
instances. A `NodeId` only needs to be unique within its surface and scene
epoch.

### Host WhiskerView and embedding

Every interactive surface is rooted in a Host-side container called
`WhiskerView` in this RFC. It is distinct from the built-in Whisker `view`
element:

| Concept | Ownership | Purpose |
|---|---|---|
| Host `WhiskerView` | Android/iOS/JavaScript/Desktop Host | Container into which one Whisker surface is mounted |
| Whisker `view` | Rust scene | Ordinary box/container element inside that surface |

The platform shape is:

```text
Android   WhiskerView : Host ViewGroup
iOS       WhiskerView : Host UIView
Web       DOM custom element (for example `whisker-view`) or JavaScript object
Desktop   DesktopSurface mounted in a Whisker-owned native window or region
```

`WhiskerView` is a Host SDK/bootstrap primitive, not a scene element and not a
runtime module. It supplies the `HostSurfaceId` passed to `attach_surface` and
owns the connection between Host view lifecycle and the Surface scope.

The normal standalone Host shell uses this same primitive:

```text
generated standalone Host shell
  -> create one root WhiskerView
  -> mount the ApplicationDescriptor generated by #[whisker::main]
```

An existing application may use the alternate embedded form:

```text
existing Host screen
  |- existing Host UI
  `- WhiskerView
       `- mount the same #[whisker::main] ApplicationDescriptor
```

Embedding is intentionally a secondary integration mode. It does not add named
entry points: one Whisker application crate still has one `#[whisker::main]`.
The difference between standalone and embedded execution is who creates and
places the Host `WhiskerView`, not which Rust root function is called.

At the information level, the Host lifecycle is:

```text
WhiskerView.mount(application_descriptor, environment)
  -> create isolated RuntimeInstance
  -> construct and resolve its module registry
  -> attach this WhiskerView as a Surface
  -> start the Application module generated by #[whisker::main]
  -> request the initial frame

Host container attached/resumed
  -> resume Surface and frame delivery

Host container hidden/paused
  -> pause Surface and idle its frame source

WhiskerView.unmount()
  -> stop Application and scoped modules in reverse order
  -> delete Host nodes and release pending commands/resources
  -> detach Surface and destroy RuntimeInstance
```

The exact Kotlin, Swift, JavaScript, and Desktop wrapper method names are not
fixed by this RFC.

Multiple Host `WhiskerView`s may mount the same application descriptor. By
default each creates an isolated `RuntimeInstance`:

```text
Host process
|- WhiskerView A -> RuntimeInstance A -> Surface A
`- WhiskerView B -> RuntimeInstance B -> Surface B
```

They may share immutable executable code and explicitly Process-scoped
services, but not signals, reactive owners, Application-scoped modules,
routers, scenes, animation state, or revisions. Sharing one RuntimeInstance
across several surfaces is a future advanced API, not the default embedding
behavior.

Host-to-Whisker inputs and Whisker-to-Host actions use ordinary versioned
module interfaces registered before Application startup. They do not require a
special `main` signature or a parallel untyped embedding event system.

The outer Host controls the container's placement. Constrained sizing is the
required initial mode:

```text
Host layout determines WhiskerView bounds
  -> ViewportChanged(width, height, scale, environment_epoch)
  -> Rust/Taffy lays out the internal scene within those constraints
```

Content-driven intrinsic sizing may be added using the existing versioned
measurement and epoch mechanisms, but it is optional and must detect feedback
oscillation between Host layout and Rust layout.

On Web, an embedded provider should isolate Whisker-owned DOM from the existing
site, normally with Shadow DOM. Font, locale, writing direction, scale, and
color-scheme inputs cross the surface environment explicitly rather than
depending on accidental outer-page CSS inheritance. Focus and accessibility
must still cross the container boundary according to Host platform rules.

This runtime contract deliberately does not choose AAR, XCFramework, SwiftPM,
npm, WASM bundle, Desktop wrapper, CLI, or Project IR integration formats.
Those build and distribution decisions are deferred.

### Scene

The scene is the Rust-owned retained representation of the logical UI. The
Host retains a projection of its last accepted revision. `FramePacket`s advance
that projection transactionally.

### Element type

An element type is a versioned contract contributed by a UI module. It defines
properties, events, allowed children, measurement policy, commands, and Host
backing requirements. At bootstrap, canonical element keys are resolved to
compact `ElementTypeId`s used in frame packets.

### Element instance

An element instance is a scene node identified by `NodeId`. Thousands of nodes
may be created from one registered element type. Nodes follow scene lifecycle,
not module-registry lifecycle.

### Composition graph

UI modules do not require a concrete renderer and do not call `present`,
`measure_batch`, or Host factories. Whisker core is the coordinator that binds
both sides. It may use the module registry to resolve providers without making
the scene engine itself a third-party-replaceable module:

```text
Application
    |
    v
Whisker Core Scene
    |
    | requires exactly one RendererV1
    +---------------------------> DOM / Android / iOS Renderer
    |
    | requires many ElementProviderV1
    +---------------------------> View Module
    +---------------------------> Text Module
    +---------------------------> Image Module
    `---------------------------> Video Module
```

Conceptually, core requests these provider collections during bootstrap:

```rust,ignore
ProviderRequirements {
    renderer: exactly_one("whisker.renderer", "^1"),
    elements: many("whisker.element-provider", "^1"),
}
```

A UI module declares only the element-provider side unless it has independent
service dependencies:

```rust,ignore
ModuleDescriptor {
    id: "whisker.ui.text",
    provides: [interface("whisker.element-provider", "1")],
    requires: [],
    lifecycle: LifecycleScope::Application,
}
```

The registry starts the renderer and element providers before core starts the
Application. Core collects the schemas, validates duplicate canonical element
keys, and passes registrations to the resolved renderer. Shutdown occurs in
reverse order.

The renderer does not resolve core to send events back. During
`attach_surface`, core passes a typed `RendererEventSink`. This keeps the
registry graph directed and avoids a `Core -> Renderer -> Core` cycle:

```text
Core --requires RendererV1---------> Renderer
Core --passes RendererEventSink----> Renderer
Core <-------events through sink---- Renderer
```

Likewise, the renderer does not resolve `ElementProvider` modules itself. It
receives normalized `ElementRegistration` values from core. This
keeps provider discovery, duplicate detection, and schema policy in Rust while
the renderer remains responsible for binding schemas to Host factories.

## Renderer interface

The conceptual typed Rust-facing interface is:

```rust,ignore
trait RendererV1 {
    fn capabilities(&self) -> RenderCapabilities;

    fn attach_surface(
        &mut self,
        host_surface: HostSurfaceId,
        config: SurfaceConfig,
        events: RendererEventSink,
    ) -> Result<SurfaceInfo, RenderError>;

    fn register_elements(
        &mut self,
        surface: SurfaceId,
        elements: &[ElementRegistration],
    ) -> Result<Vec<ElementBinding>, RenderError>;

    fn request_frame(&mut self, surface: SurfaceId);

    fn measure_batch(
        &mut self,
        surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), RenderError>;

    fn present(
        &mut self,
        surface: SurfaceId,
        packet: &FramePacket,
    ) -> Result<PresentResult, RenderError>;

    fn detach_surface(&mut self, surface: SurfaceId);
}
```

This is an information-level contract, not a final Rust trait signature. A
generated binding may use function tables, FFI functions, JNI methods, or
WASM imports. The semantics and ordering remain the same.

The native Rust implementation may expose the measurement and presentation
parts as the narrower `MeasurementProvider` and `FrameSink` traits and compose them
at the surface boundary. This is the Desktop v1 path and avoids a transport
adapter without weakening the information-level `RendererV1` contract.

The semantic layer deliberately does not mandate one serialized measurement
packet for every platform. Generated bindings may lower the same typed batch
to JNI arrays, C-compatible tables, or WASM linear-memory views without an
intermediate encoding. Each generated transport must round-trip the shared
Rust conformance fixtures and preserve the batch validation rules. This keeps
transport allocation and binary-format versioning out of the common hot path
while `MeasurementProvider` remains the final common Rust-facing seam.

`attach_surface` attaches to a Host root already supplied by bootstrap; it does
not require Rust to create an operating-system window. `RendererEventSink` is
the reverse-direction callback interface registered once for that surface.

### Capabilities

Capabilities report behavior that can legitimately vary by backend, such as:

- supported standard and extension element types;
- synchronous measurement availability;
- maximum texture/resource sizes;
- filter, blend, clip, and accessibility features;
- preferred packet alignment and maximum packet size;
- platform font and color-space information.

Capabilities do not replace required interface contracts. If an application
requires a feature that the selected provider cannot implement or emulate,
composition or surface attachment fails with a diagnostic. Whisker must not
silently drop a required visual or interaction behavior.

## Bootstrap and surface attachment

The normal startup order is:

```text
Host
  1. create or locate a Host WhiskerView container
  2. load the Rust library or WASM module
  3. mount the #[whisker::main] ApplicationDescriptor with its environment
       |
       v
Rust kernel
  4. create a RuntimeInstance and construct its module registry
  5. resolve whisker.renderer@1
  6. attach_surface(host_surface, config, event_sink)
       |
       v
Host renderer
  7. return SurfaceInfo and capabilities
       |
       v
Rust runtime
  8. collect ElementProvider interfaces from UI modules
  9. register element schemas and negotiate compact IDs
 10. start the Application module generated by #[whisker::main]
  11. request_frame(surface)
```

All runtime module compatibility and required Host element factories should be
validated before the first application frame. Missing native registration is a
startup error naming the package, module, interface, target, and generated
project contribution expected to provide it.

## Frame scheduling

Rust requests a frame only when there is work: a dirty signal, active motion,
pending layout, queued command, resource completion, or an explicitly requested
continuous frame. Multiple requests before the next callback are coalesced by
the Host.

The implemented native Rust boundary represents this edge-triggered request as
an any-thread `RuntimeWakeHandle`. The Host callback only posts or coalesces a
drive of its UI event loop; it never enters application code inline. Each
`RuntimeInstance` owns an isolated `RuntimeContext` containing its reactive
arena, local future pool, view bookkeeping, and animation state. Entering a
frame or event temporarily activates that context on the Host UI thread, so
several surfaces may share one thread without sharing runtime state.

The frame sequence is:

```text
Rust: request_frame(surface) ------------------------------> Host

Host VSync / requestAnimationFrame
  -> RendererEvent::Frame { surface, timestamp, interval } -> Rust

Rust
  1. advance motion using the Host timestamp
  2. flush reactive changes
  3. resolve dirty style
  4. collect and perform required measurement
  5. recompute dirty layout subtrees
  6. build one FramePacket from accumulated scene changes
  7. renderer.present(surface, packet) --------------------> Host

Host
  8. validate and apply the packet as one ordered transaction
  9. return accepted revision or recovery request ---------> Rust

Rust
 10. request another frame only if work remains
```

`Frame` is a scheduling callback, not permission for the Host to calculate
animation values. Motion state remains in Rust.

Background work may wake the same instance without polling while idle. Local
future wakers retain the instance's `RuntimeWakeHandle`; arbitrary worker or
Tokio tasks capture a `RuntimeDispatcher`, post an owned `Send` closure, and
wake the Host. The closure runs only when the Host next enters that instance on
the UI thread. Neither mechanism gives a worker direct access to the retained
scene or UI-thread reactive arena.

Pause closes the wake gate without destroying state: completions stay queued
and cannot spin the Host until resume explicitly schedules a drive. Permanent
unmount disposes the owner, local futures, queued input, and reactive/view
state, then closes the dispatcher so handles retained by workers reject new
posts instead of retaining callbacks after teardown. Host callbacks that occur
synchronously during a Rust call are bounded and deferred to the current event
boundary rather than re-entering application code.

### Web timing

On Web, JavaScript calls into WASM from a `requestAnimationFrame` callback.
Rust advances motion and calls
`Renderer::present`; the generated binding invokes JavaScript synchronously,
and JavaScript applies the DOM patch before the same animation-frame callback
returns:

```text
JavaScript rAF
  -> WASM frame entry
       -> Rust motion/reactivity/layout
       -> WASM-to-JS present(packet)
            -> DOM updates
  <- return
```

DOM measurement can still trigger expensive browser layout, so reads are
batched and cached.

### Desktop timing

On Desktop, the native window frame callback enters the Rust Whisker runtime.
Rust advances motion, resolves any missing intrinsic sizes, completes layout,
and presents one packet before the Desktop Host builds and submits GPU work:

```text
native window frame callback
  -> Rust motion/reactivity/style
  -> direct MeasurementProvider::measure_batch(...) on cache misses
  -> Taffy layout and frame preparation
  -> direct FrameSink::present(&FramePacket)
       -> update DesktopSurface Host projection
  -> build Desktop paint scene and submit GPU commands
```

Both Host calls are synchronous typed Rust calls. The semantic protocol remains
versioned and testable, but Desktop v1 does not encode or copy the packet merely
to preserve that boundary. Text shaping and intrinsic measurement can use the
same prepared Host resource that later supplies glyphs for paint.

## Frame packet

### Transaction model

A packet advances one surface from `base_revision` to `target_revision`:

```rust,ignore
struct FrameHeader {
    magic: u32,
    protocol_major: u16,
    protocol_minor: u16,
    surface: SurfaceId,
    scene_epoch: u32,
    frame_id: u64,
    base_revision: u64,
    target_revision: u64,
    viewport_epoch: u32,
    flags: FrameFlags,
}
```

The Host validates the complete packet before mutating visible state where
practical. Operations are then applied in packet order. A rejected packet does
not advance the accepted revision.

`base_revision` detects a dropped, duplicated, or out-of-order delta. If it
does not match the Host revision, the Host returns `NeedSnapshot`; Rust emits a
full retained-scene snapshot with a new scene epoch. Normal synchronous
backends should never need this recovery path, but defining it makes protocol
failure diagnosable.

### Encoding

The transport uses a packed opcode buffer plus tables for repeated or
variable-size values:

```text
FramePacket
|- fixed header
|- opcode stream
|- string table
|- typed value table
|- resource references
`- optional diagnostics table in debug builds
```

It does not encode method names or issue a dynamic module call per property.
The semantic model uses the shared `WhiskerValue` tagged union while the packed
transport stores repeated and variable-size values in tables. WASM JavaScript reads a borrowed `Uint8Array`
view over linear memory during `present`; native bindings receive a borrowed
pointer and length. The packet is valid only for the synchronous duration of
`present` unless a provider explicitly copies it.

Unknown major protocol versions are rejected. A minor version may add ignorable
sections or opcodes only when their skip length and optional semantics are
encoded. Required unknown behavior must fail capability negotiation instead of
being silently skipped.

The typed Rust protocol exposes optional semantic groups through
`RenderCapabilities`. A Host classifies each group as native, emulated, or
unsupported; omission is unsupported. A frame can derive its requirements
without inspecting CSS spellings. Capability preflight occurs before retained
state mutation, so a packet cannot partially apply before discovering an
unsupported background, effect, text, image, cursor, or elliptical-radius
operation.

### Operation groups

The protocol supports at least these operation groups:

```text
Tree
  CreateNode(node, element_type)
  DeleteNode(node)
  InsertChild(parent, child, index)
  RemoveChild(parent, child)
  MoveChild(parent, child, index)

Geometry and compositing
  SetLayout(node, border_box, content_box)
  SetTransform(node, transform)
  SetOpacity(node, opacity)
  SetClip(node, clip)
  SetVisibility(node, visibility)
  SetZOrder(node, z_order)

Paint and content
  SetBoxPaint(node, background, borders, radii)
  SetBackgroundLayers(node, layers)
  SetVisualEffects(node, outline, shadows, clip_path, masks, backdrop_blur, compositing)
  SetText(node, text_run)
  SetImage(node, resource, fit, position)
  SetProperty(node, property_id, typed_value)
  ClearProperty(node, property_id)

Interaction and semantics
  SetEventMask(node, event_mask)
  SetHitTest(node, hit_test_behavior)
  SetCursor(node, cursor)
  SetAccessibility(node, semantics)
  SetPointerCapture(node, pointer_id)
  ReleasePointerCapture(node, pointer_id)

Commands
  InvokeCommand(node, command_id, arguments, result_id?)
```

`SetTransform` always carries a resolved column-major 4-by-4 matrix around
the node's local border-box origin. Rust retains typed transform functions,
length-percentage translations, and `transform-origin` until Taffy has
produced the border-box size, then bakes them into that matrix. Hosts therefore
do not parse CSS or independently resolve percentages and origin semantics.
Changing a node's border-box size recomputes its matrix even when its specified
transform is unchanged. The current common subset includes CSS/Lynx 2-D
transforms and 3-D functions or `matrix3d` values that project a node's flat
local plane. Each Host applies the projective result and flattens at that node;
Android maps the `z = 0` slice exactly to a density-adjusted 3-by-3 homography.
Parent `perspective`, shared `preserve-3d` descendant spaces, and motion paths
remain later capability slices rather than silently degrading on any Host.

The final opcode set may combine common operations for compactness. The
semantic separation matters because it enables dirty classification,
validation, and backend-specific fast paths.

Resource acquisition is ordered on a separate typed channel:

```text
ResourceCommand::Load(id, generation, kind, source)
  -> Host fetch/decode/register
  -> ResourceEvent::Ready(id, generation, intrinsic_dimensions?)
     or ResourceEvent::Failed(id, generation, code)

ResourceCommand::Release(id, generation)
```

Frames carry only `ResourceId`. A generation prevents stale asynchronous
completion from replacing a newer load, and release happens only after no
accepted frame references that generation. Inline bytes, when used, cross the
resource channel once and are not embedded into every frame transaction.

Structural operations must establish a node before later operations reference
it. Deleting a subtree invalidates its Host handles, subscriptions, pending
commands, and element-instance state. A `NodeId` is not reused within the same
scene epoch.

### Incremental updates

Initial presentation uses snapshot mode and creates the complete current tree.
Later frames contain only accumulated changes:

```text
signal changes opacity on Node 42
  -> no component-tree diff
  -> no full style string
  -> no full-scene serialization
  -> SetOpacity(42, new_value)
```

Several changes to the same property before a frame collapse to the final
value. Structural changes preserve required ordering. Static nodes produce no
packet operations while unchanged. An idle application requests no frames.

## Styling boundary for UI modules

An element provider defines what an element *is*; it does not implement common
style resolution, limited text-property inheritance, layout, or paint
properties. Whisker core connects its style and layout subsystems with element
providers and the renderer:

```text
ElementProvider modules -------+
Style Engine ------------------+--> Whisker Core --> FramePacket --> Renderer
Layout Engine -----------------+
```

RFC 0004 supersedes the original public content-category sketch. The common
module declaration contains identity, intrinsic measurement, and generated
property/event/command members:

```rust,ignore
#[whisker::module_component(
    name = "example.controls/Toggle",
    measurement = None,
)]
pub fn toggle(checked: Signal<bool>, on_change: ChangeEvent) {}
```

Child acceptance is inferred from a `Children` parameter; the Host definition
owns the actual mount target. Built-in text may retain private renderer
capabilities, but public schemas do not expose content or child-mount enums.

Common typed style properties such as width, padding, background, opacity,
transform, border, and clip are not repeated in each element schema. An
element schema contains only element-specific properties such as a video's
source, autoplay behavior, or playback rate. Public authoring is inline-only;
there is no public stylesheet, selector, specificity, or general CSS cascade.
Element defaults and the fixed inherited text context are resolved before the
node's inline style.

Third-party UI packages express visual extensions as typed element properties
in their versioned schema. They do not extend a selector language or register
custom cascading properties. Behavioral configuration remains an element
property even when it affects a visual component.

At the Cargo level, a UI crate may depend on small `element-api` and
`whisker-style` crates for stable IDs and schema types. That compile-time
dependency is not a runtime dependency on a concrete Style Engine or renderer.

## Standard View example

The built-in UI package provides an Application-scoped module such as
`whisker.ui.primitives`. That module provides an `ElementProvider` collection
containing schemas for `View`, `Text`, and other standard element types.

Conceptually, `View` declares:

```rust,ignore
ElementSchema {
    key: "whisker.ui/View",
    children: ChildrenPolicy::Multiple,
    measure: MeasurePolicy::None,
    properties: &[ACCESSIBILITY_LABEL, POINTER_EVENTS, FOCUSABLE],
    events: &[POINTER_DOWN, POINTER_MOVE, POINTER_UP, CLICK, FOCUS, BLUR],
    commands: &[FOCUS, BLUR],
}
```

Width, padding, background, transform, and other typed style properties are
common resolved-style data, not bespoke `View` properties.

One module instance registers the type. Every rendered `View` creates a cheap
scene node:

```text
whisker.ui.primitives module instance
  `- element type whisker.ui/View
       |- NodeId 41
       |- NodeId 42
       `- NodeId 43
```

A first frame might contain:

```text
CreateNode(42, VIEW)
InsertChild(10, 42, 0)
SetLayout(42, border=(16, 24, 320, 80), content=(0, 0, 320, 80))
SetBoxPaint(42, background=white, ...)
SetEventMask(42, CLICK)
```

The Host binding maps `VIEW` to its registered standard factory:

```text
Android -> standard Whisker ViewGroup
iOS     -> UIView
Web     -> div
Desktop -> retained box lowered to Whisker Desktop paint primitives
```

## Custom UI modules

A third-party UI package such as `whisker-video` commonly distributes all of
these pieces together:

```text
whisker-video package
|- Rust declarative component API
|- Rust typed ElementHandle, props, events, and commands
|- runtime module providing an ElementProvider
|- canonical whisker.video/Video element schema
|- Android Host element factory
|- iOS Host element factory
|- JavaScript Host element factory
|- Desktop Rust Host element factory
|- generated bindings and registration metadata
`- optional build plugin for dependencies, permissions, and lifecycle hooks
```

These are distribution contents, not one runtime object. The Rust module is
typically Application-scoped and registers one element type. Each rendered
video is a Scene-owned node with separate Host state.

### Application-author surface

Application code should see an ordinary Rust crate, a declarative component,
typed events, and a typed element handle:

```rust,ignore
fn configure(app: &mut App) {
    app.module::<WhiskerVideo>();
}

fn player() -> Element {
    let video = VideoHandle::new();

    render! {
        view {
            Video(
                ref: video.r(),
                src: "https://example.com/movie.mp4",
                autoplay: true,
                muted: false,
                style: css!(
                    width: percent(100),
                    height: px(240),
                    border_radius: px(12),
                ),
                on_ready: move |event| log_duration(event.duration),
                on_ended: move |_| play_next(),
                on_error: move |error| report(error),
            )

            view(on_tap: move |_| video.play()) {
                text(value: "Play")
            }
        }
    }
}
```

Official and third-party typed APIs must not require application authors to
spell module names, method strings, numeric property IDs, or raw
`WhiskerValue`s. The generic dynamic module API remains an escape hatch.

Selecting the module makes its companion build requirements eligible for
activation under RFC 0001. An application may configure the companion plugin
explicitly when it has options, but ordinary use should not require redundant
module and plugin declarations.

### Module-author contract

A module author declares a canonical element key and typed properties, events,
commands, presentation/content category, measurement policy, and child policy.
The exact macro syntax
is deliberately not fixed by this RFC; conceptually the declaration produces:

- the Rust component builder and props;
- a generation-checked `ElementHandle` API;
- typed event and command payloads;
- `ElementSchema` and stable symbolic property/event/command IDs;
- FramePacket encoders and decoders;
- Kotlin, Swift, TypeScript, and Desktop Rust binding inputs;
- module and debug-symbol descriptors.

An illustrative generated schema is:

```rust,ignore
ElementSchema {
    key: "whisker.video/Video",
    presentation: Presentation::Box,
    children: ChildrenPolicy::None,
    content: Content::Native,
    measure: MeasurePolicy::FixedAspectRatioOrHostIntrinsic,
    properties: &[
        VIDEO_SOURCE,
        VIDEO_AUTOPLAY,
        VIDEO_MUTED,
        VIDEO_LOOPING,
        VIDEO_CONTROLS,
        VIDEO_PLAYBACK_RATE,
    ],
    events: &[
        VIDEO_READY,
        VIDEO_PLAYING,
        VIDEO_PAUSED,
        VIDEO_PROGRESS,
        VIDEO_ENDED,
        VIDEO_ERROR,
    ],
    commands: &[VIDEO_PLAY, VIDEO_PAUSE, VIDEO_SEEK, VIDEO_SNAPSHOT],
}
```

Source, autoplay, muted, looping, controls, and playback rate are
element-specific properties. Width, height, aspect ratio, object fit, opacity,
transform, border radius, and clip are common styles supplied by the closed
semantic channels and the Style Engine.

### Typed updates, events, and commands

Creating and updating a custom element uses the ordinary frame path:

```text
CreateNode(100, VIDEO)
SetProperty(100, VIDEO_SOURCE, resource)
SetProperty(100, VIDEO_MUTED, false)
SetLayout(100, ...)
InvokeCommand(100, VIDEO_PLAY, ...)
```

Property changes are typed slots and are collapsed to their final value before
the frame. A Host factory receives a property patch, not arbitrary method
names. Player callbacks return typed events through the renderer event sink,
which routes them by `NodeId` to Rust listeners.

View-bound imperative methods use a generation-checked handle:

```rust,ignore
video.play();
video.pause();
video.seek(Duration::from_secs(30));
let image = video.snapshot().await?;
```

Fire-and-forget commands are ordered inside the frame packet after any
properties or geometry they depend on. Result-bearing commands allocate a
typed `ResultId` and complete through `CommandCompleted`. Calling a handle
after its node generation was deleted returns a stale-element error rather
than targeting a reused node.

### Host element factory

Every supported target provides a factory conforming to the generated element
binding. At the information level it implements:

```rust,ignore
trait HostElementMeasurer {
    fn measure(&mut self, request: &MeasurementRequest) -> MeasurementResponse;
}

trait HostElementFactory {
    fn create(&mut self, context: ElementContext) -> HostElement;
    fn apply_properties(&mut self, element: &mut HostElement, patch: PropertyPatch);
    fn invoke_command(&mut self, element: &mut HostElement, command: Command);
    fn destroy(&mut self, element: HostElement);
}
```

This is a cross-language behavioral shape, not a Rust trait that Kotlin,
Swift, or JavaScript literally implements. Binding generation supplies native
typed payloads and verifies the common schema so authors do not manually keep
wire IDs synchronized. Measurement is separate because layout can need an
answer before the first `CreateNode` has created a live Host element. The
measurer receives all content, property, resolved-text-style, constraint, and
environment inputs in the request and may share prepared resources with the
later factory through `PreparedContentId`.

The same element key can map to different concrete objects:

```text
whisker.video/Video
  Android -> PlayerView / Media3 player
  iOS     -> AVPlayerLayer-backed UIView
  Web     -> HTMLVideoElement
  Desktop -> native media provider composited as a Desktop external surface
```

Factories register through the one Host renderer registry. They do not create
a module-specific bridge, renderer, or second Host.

### Bootstrap binding

The package metadata or companion plugin causes the selected target factory
and generated registration to be included in Project IR. At runtime:

```text
Video module
  -> provides ElementSchema("whisker.video/Video")

Whisker Core
  -> collects and normalizes the schema
  -> Renderer.register_elements(...)

Host Renderer
  -> looks up embedded Host factory by name
  -> verifies supported properties, events, commands, and measurement
  -> assigns compact ElementTypeId::VIDEO
```

Missing or incompatible factories fail before the first application frame and
name both the runtime provider and expected build contribution.

### Build companion

A package-specific plugin is required only for target work that generic module
integration metadata cannot express. For a video package, Project IR
contributions might include:

```text
Android
  MavenDependency(androidx.media3:media3-exoplayer:...)
  MavenDependency(androidx.media3:media3-ui:...)
  HostElementFactory(VideoElementFactory)

iOS
  SystemFramework(AVFoundation)
  HostSourceModule(WhiskerVideo)
  HostElementFactory(VideoElementFactory)

Web
  JavaScriptProvider(whisker-video/host)
  HostElementFactory(VideoElementFactory)

Desktop
  NativeRustProvider(whisker_video_desktop)
  NativeMediaDependency(...)
  DesktopElementFactory(VideoElementFactory)
```

Generic target plugins can handle ordinary source inclusion, binding
generation, and registration directly from module-provider metadata. A custom
build plugin adds permissions, dependencies, resources, lifecycle hooks, or
configurable behavior beyond that generic path.

The relationship is:

```text
select WhiskerVideo runtime provider
  -> activate declared companion build requirements
  -> compose target Project IR
  -> generate and build Host project with Video factory
  -> validate and bind Video element during runtime bootstrap
```

Discovery alone does not execute a third-party plugin, as specified by RFC
0001.

### Unsupported targets and capability variants

If a selected UI module has no compatible Host factory for the target, project
composition fails. It must not silently emit an empty view or ignore commands:

```text
cannot compose target `web`:
  whisker.video/Video is selected
  available Host providers: android, ios
  missing Host provider: web
```

A package may deliberately provide a reduced-capability target variant, but
the variant advertises those capabilities and required application behavior is
validated before use.

### Module and node lifecycle

The Video module is normally started once per Application. Each scene node
owns a separate Host player lifecycle:

```text
Application start
  -> start Video module and register schema once

CreateNode(Video)
  -> create AVPlayer / Media3 player / HTMLVideoElement / Desktop media player

DeleteNode(Video)
  -> detach observers, stop playback, release per-node resources

Application shutdown
  -> stop Video module
```

An independent media session is a different runtime concept. A package may
also provide `VideoSessionV1` for preloading or background playback not owned
by a scene node. View-bound operations use `ElementHandle` commands; detached
resources use the service interface and its own lifecycle.

## Measurement

### Why measurement crosses the boundary

Rust can calculate ordinary box layout, but exact text shaping, line wrapping,
native controls, replaced content, and some custom views depend on Host
facilities. An element schema therefore declares one of these policies:

```text
None             Rust constraints determine size; no Host query
HostIntrinsic    Host measures under Rust-provided constraints
FixedAspectRatio Rust layout uses provider metadata
DeferredResource size may become known after a resource event
Custom           typed element-specific measurement payload
```

The request also carries a semantic provider category independent of the
fallback policy:

```text
Text             shaped/wrapped text and inline attachments
ReplacedContent  auto-sized images and other resource-backed content
NativeControl    switches, progress indicators, pickers, and similar controls
EmbeddedSurface  content-sized child Whisker surfaces
Custom           versioned module-defined measurement
```

An explicitly sized image, video, WebView, or ordinary box does not enter this
path. A module registers intrinsic measurement only when its size cannot be
derived from style, children, or already available Rust-side metadata.

### Batch contract

Rust sends all currently missing measurements in one `measure_batch` call. A
request includes:

- `NodeId` and element type;
- min/max width and height constraints;
- text, font, locale, direction, scale, and line-limit inputs where relevant;
- relevant custom properties or resource metadata;
- a measurement key and environment epoch.

Each response is either:

```text
Ready { key, width, height, baselines, overflow }
Pending { key, request_id, provisional_size? }
Unsupported { key, reason }
```

The semantic Rust contract uses a closed `MeasurementPayload` enum. Text
contains UTF-8 content, an ordered font fallback list, logical font size,
numeric weight, posture, line height, letter spacing, locale, direction,
wrapping, maximum lines, and overflow behavior. Replaced content, native
controls, and embedded surfaces have separate typed records. Only custom and
provider-owned native-control state remains opaque, as versioned bytes.

The Host must return exactly one response for every request. Rust accepts
response reordering, but rejects duplicate, missing, unexpected, or
wrong-environment keys before applying any result from the batch.

Android, iOS, Web, and Desktop v1 are expected to provide synchronous `Ready`
results for ordinary text measurement. Web may do synchronous browser text
measurement during the WASM frame callback; requests are batched to avoid
interleaved DOM reads and writes. Desktop calls its native Rust text provider
directly and returns metrics plus a `PreparedContentId` for the shaped/wrapped
content retained by `DesktopSurface`. Neither path requires native-process IPC.

Asynchronous `Pending` is reserved for resources and custom providers that
cannot answer immediately. The Host later emits `MeasurementReady`; Rust
validates its request and environment epochs, updates the cache, marks the
smallest affected layout subtree dirty, and requests another frame.

### Caching and stability

Measurement is cached by all inputs affecting the result, not merely by
`NodeId`. Font availability, locale, scale, writing direction, and viewport
changes increment environment epochs and invalidate relevant entries.

If a synchronous measurement is available, Rust completes layout before the
first visible packet and no corrective frame is needed. For a deferred result,
the element schema chooses an explicit provisional policy: retain previous
geometry, use a declared placeholder, or withhold the new subtree until ready.
This makes layout shifts a known element behavior rather than an accidental
result of transport timing.

The runtime detects repeated measure/layout oscillation for unchanged inputs
and reports it as a provider error.

The Host-independent semantic request/response types, typed built-in payloads,
strict batch validator, retained engine state machine, and Rust-facing
`MeasurementProvider::measure_batch` seam are implemented. `SurfaceEngine` can
drive all synchronous batches to a final Taffy layout before frame preparation
and returns to the event boundary only for `Pending`. Plain UTF-8 Text v1 can
now lower computed inherited text style into the shared measurement payload;
the accepted prepared-content handle, shaping inputs, and resolved foreground
paint are retained in final snapshot and delta `SetText` operations. The
existing `render!` Text element and typed `css!` declarations populate this
pipeline through `SurfaceRuntime`, including text inheritance, common box
paint, overflow clips, and paint-only deltas without remeasurement.

`RuntimeInstance` now implements the Host-driven mount, pause, resume, unmount,
frame, deferred-measurement, and input boundaries. `RuntimeContext` isolates
multiple instances on one UI thread. Instance-specific future wakers and
`RuntimeDispatcher` cover idle async completion without a busy tick. Typed
Host input is validated, hit-tested against the retained Rust scene, and routed
through Rust capture and bubble listeners; synchronous re-entry is queued until
the current event/frame boundary. Android and UIKit now have their first
retained-renderer vertical slice: CNG emits Lynx-free native surfaces,
`whisker run` builds and embeds the user Rust library/framework, and the Host
mounts and ticks `RuntimeInstance` while consuming semantic View/Text frame
operations. Android and iOS now use the same versioned, borrowed C ABI for
bootstrap registrations, batched measurement, frame operations, element
events, module calls, and module events. `WhiskerValueRaw` carries every
open-ended value; no JSON serialization exists on this retained mobile path.
The attach sequence is mount, synchronous registration negotiation, Host
factory binding, then measurement/frame scheduling, so Host measurement is
never requested against an unbound element table. UIKit text bounding and
Android `StaticLayout` replace the original zero-size/estimated provider;
module factories may supply the same pre-mount custom measurer and unsupported
kinds return `Unsupported` rather than a successful zero size.

Mobile frame application is revision-aware and transactional at the retained
scene boundary. Each Host stages and validates the complete operation graph
before mutating UIKit/Android views, returns `NeedSnapshot` on epoch or base
revision drift, and acknowledges the committed target revision. Native events
raised during commit are queued until the transaction exits, preventing
synchronous Rust re-entry through a live borrowed frame. Initial DOM
and macOS Host slices now provide CNG composition
roots, Host-driven frame scheduling, measurement, and frame consumption; DOM
cover the built-in box/text paint subset. The first Desktop implementation has
been extracted to `platforms/desktop`: it retains accepted packets in a native
Host projection, measures and shapes text with `cosmic-text`, reuses the
resulting `PreparedContentId` for glyph paint, and submits common box, rounded
background and border-outline, rectangular clip, and text draws through
`wgpu`. Window lifecycle and scheduling now live in symmetric
`platforms/macos`, `platforms/windows`, and `platforms/linux` application
shells; only macOS is wired into CNG/build/run today. Percentage corner radii
resolve independently against both box axes and are normalized when
adjacent radii exceed an edge. Layout packets carry both border-box and
content-box geometry so Hosts never reconstruct padding or borders from style
inputs. Shared Host scenarios now drive Desktop measurement and frame
presentation without `RuntimeInstance`; the first WPT-derived background and
radius cases include offscreen `wgpu` pixel checkpoints. Rounded
descendant/path clips, exact non-solid borders, transforms, group compositing,
ellipsis/forced-direction text behavior, input, and accessibility remain
explicit Desktop conformance gaps.
Each Host samples its current logical viewport and scale before a frame.
`RuntimeInstance` applies that `StyleEnvironment`, transactionally
re-resolves retained `vw`, `vh`, `rpx`, and other environment-dependent styles,
derives the Taffy root constraints from the same values, and uses Host-owned
environment and viewport epochs to invalidate measurements and identify the
resulting packet.

Layout represents the viewport as a private `SurfaceRoot`: a fixed-size
flex-column node whose only child is the application root produced by
`render!`. The node is owned entirely by `whisker-layout`; it has no public
`NodeId`, is never included in `LayoutSnapshot`, and therefore never appears in
the frame protocol. This gives the application root ordinary child semantics:
cross-axis stretch, `flex-grow`, percentage sizing, and absolute positioning
resolve against the current viewport without mutating or overriding the
application's computed style. A viewport change updates the private root and
recomputes the application subtree before a packet is prepared.

## Renderer events: Host to Rust

The reverse callback interface carries typed events:

```rust,ignore
enum RendererEvent {
    Frame {
        surface: SurfaceId,
        timestamp: FrameTime,
        interval: Duration,
    },
    Input(InputEvent),
    ViewportChanged(ViewportMetrics),
    FocusChanged(FocusEvent),
    Ime(ImeEvent),
    AccessibilityAction(AccessibilityAction),
    MeasurementReady {
        surface: SurfaceId,
        request_id: RequestId,
        response: MeasureResponse,
    },
    ResourceReady(ResourceEvent),
    CommandCompleted(CommandResult),
    SurfaceLost {
        surface: SurfaceId,
        reason: SurfaceLossReason,
    },
}
```

Events identify logical nodes with `NodeId`; Host object pointers are never
exposed to application code. Rust performs listener lookup and capture/bubble
propagation. Host-native gesture recognizers or controls may emit higher-level
typed events when declared by their element interface.

Events received while Rust is applying another event are queued at the runtime
boundary. A Host must not synchronously re-enter arbitrary application code
from inside `present`. This gives each frame transaction a deterministic
reactive flush boundary.

## Commands and queries

Commands that must be ordered relative to visual mutations, such as focus,
scroll, video playback, or pointer capture, are encoded in the frame packet
after the properties on which they depend.

A fire-and-forget command has no result ID. A result-bearing command allocates
a typed `ResultId`; the Host may complete it immediately after accepting the
packet or later through `CommandCompleted`. Completion never changes the
already accepted scene revision.

Read queries that require up-to-date Host state must declare their scheduling
semantics. They cannot silently force a present in the middle of reactive
evaluation. Geometry queries should normally read Rust's accepted layout;
Host queries are for genuinely platform-owned state.

## Paint and clipping

Rust resolves paint values, stacking order, clip descriptions, filters, and
compositing intent. The Host performs actual painting or configures platform
objects that do so.

This means paint is not implemented twice as independent style logic:

```text
Rust typed style engine
  -> ResolvedBackground / ResolvedBorder / ResolvedClip / ...
  -> typed frame operations
  -> Host platform implementation
```

Backend capability negotiation determines whether a feature is native,
emulated, or unsupported. The exact required style-to-paint mapping belongs to
RFC 0003.

## Backpressure and failure

The primary Android, iOS, Web, and Desktop v1 providers accept and apply a
packet synchronously. This bounds packet lifetime, preserves frame order, and
avoids queues of stale animation frames.

A future asynchronous renderer must expose that capability explicitly and
maintain a bounded queue. It may coalesce property updates, but may not discard
structural dependencies or report a revision accepted before it can preserve
that revision's order. Such a provider is not required for the initial
architecture.

Recoverable protocol outcomes include:

- `Accepted { revision }`;
- `NeedSnapshot { receiver_revision }`;
- `SurfaceUnavailable`;
- `UnsupportedElement` or `UnsupportedProperty` detected before mutation;
- malformed packet or protocol-version failure.

A provider exception, invalid Host registration, or partial application is a
surface failure. Debug builds include enough opcode, node, element, provider,
and module information to attribute the error. Production builds may use
compact IDs backed by a separately emitted symbol table.

## Testability

The scene, style, layout, motion, module registry, element registry, packet
encoder, and packet validator must run without a Host.

A Rust-only `RecordingRenderer` implementing `whisker.renderer@1` records
packets and can maintain a reference scene projection. Tests can assert:

- exact incremental operations after a signal change;
- no packet when the scene is idle;
- ordering and transactional revision rules;
- layout invalidation after measurement;
- event routing from synthetic Host events;
- custom element registration and command encoding;
- full snapshot recovery after a simulated dropped packet.

The protocol decoder should be fuzzed independently.

### Host-only conformance driver

Every Host must also be executable as a system under test without mounting a
Whisker application or starting `RuntimeInstance`. A test-only Host
conformance driver replaces only the two peers at the Host boundary:

- a scenario source stands in for Rust and supplies surface facts,
  `MeasurementRequest` batches, accepted `FramePacket`s, clock advances, and
  synthetic native input;
- a recording event sink stands in for Rust in the reverse direction and
  captures frame requests, viewport/resource notifications, and typed input
  events.

The driver must call the production surface attachment, measurement, packet
application, paint, and native-event conversion paths. A second test renderer,
test-only CSS interpreter, or alternate Host scene is not conforming because
it could pass while the shipped path fails. Test fixture decoding and
observation hooks are excluded from release builds and do not constrain the
production transport.

The information-level scenario API is:

```text
HostCommand
  AttachSurface(surface facts and deterministic resources)
  Resize(viewport, scale, environment epoch)
  Measure(batch)
  Present(frame packet)
  AdvanceClock(timestamp)
  InjectInput(platform-neutral or native fixture input)
  Checkpoint(kind)
  DetachSurface

HostObservation
  SurfaceInfo
  MeasurementResponses
  PresentResult
  RequestedFrame
  RendererEvents
  SemanticProjection
  PixelSnapshot
```

This is not a new production renderer interface. Each backend implements the
smallest natural test entry point in its own environment:

- Desktop uses direct Rust calls and either a real window surface or a `wgpu`
  offscreen target. The common suite runs on macOS, Windows, and Linux; OS
  adapters additionally test lifecycle, scale, and native input integration.
- Web runs the same scenarios in a real browser against the production DOM
  Host and captures DOM state, events, and screenshots.
- Android runs instrumentation tests against the production View Host with a
  mock Rust callback/event sink.
- iOS runs XCTest against the production UIView Host with a mock Rust
  callback/event sink.

Portable fixture files are allowed to use a stable, readable test envelope
even when the production binding uses direct Rust calls, JNI tables, a C ABI,
or WASM memory. The envelope is decoded before the production Host entry point
and must not become an extra serialization step in shipped frames.

Measurement scenarios send the real request to the Host, record the real
response, and may bind the returned `PreparedContentId` into later text paint
steps. Absolute font metrics are not required to match across operating
systems. Tests instead use platform-pinned fonts, same-platform reference
rendering, or explicit metric relationships and tolerances. Event scenarios
inject input below the Host adapter and assert the typed event emitted above
it; Rust capture/bubble routing remains covered by the Rust-only suite.

### WPT-derived corpus and three-layer composition

Whisker uses selected Web Platform Tests as the authoritative behavioral
source for standard CSS features. Native Hosts cannot execute WPT HTML and
CSS directly, and Whisker deliberately has no browser cascade, so these cases
are imported as a WPT-derived corpus rather than reported as unmodified WPT
passes. Every imported case records the upstream path, pinned revision,
license, test kind, required capabilities, and any deliberate adaptation.

One case identifier is exercised at three independent layers:

1. A Rust semantic test lowers typed style and layout input to the expected
   measurement requests, geometry, and protocol operations.
2. Each Host-only suite consumes the canonical request/frame scenario without
   running the Rust runtime and checks measurement, retained projection,
   event output, and test-versus-reference pixels.
3. A smaller full-stack suite mounts the corresponding Whisker fixture and
   verifies that the Rust output and real Host compose successfully.

Canonical Host scenarios are authored from the specification or WPT
reference, not recorded blindly from current Rust output. Otherwise the same
Rust bug would be baked into every Host expectation. Reftests render the test
and reference scenario on the same Host and compare them with a declared
tolerance; semantic testharness cases assert structured observations instead
of pixels.

The shared corpus and per-Host runners live at predictable locations:

```text
tests/host-conformance/             # schemas, manifest, shared scenarios
  wpt/<upstream-path>/

platforms/desktop/tests/host_conformance/
platforms/web/tests/conformance/
platforms/android/.../androidTest/.../conformance/
platforms/ios/Tests/WhiskerHostConformance/
```

The manifest maps every supported style feature to its Rust semantic tests,
protocol capability, Host scenarios, and required backend runners. A feature
is not marked supported until the semantic suite and all required Host suites
pass. Visual/platform tests remain necessary for text and paint fidelity but
are not used as a substitute for protocol, projection, or event assertions.

## SSR and headless renderers

Because the renderer is an interface, a Rust-only serializer or headless
renderer can consume the same logical scene without a platform Host. This RFC
does not define HTML serialization, style emission, or hydration markers, but it
does not make SSR impossible.

An SSR renderer may require different capabilities and output rules from the
interactive DOM renderer. Both can implement renderer interfaces without
making runtime DOM layout the semantic source of truth. RFC 0003 will define
how server-emitted presentation relates to Rust-resolved interactive styling.

## Mapping from the current implementation

| Current concept | Direction under this RFC |
|---|---|
| Lynx `FiberElement` stored in `Element` | Replace with Rust `NodeId` into the retained scene |
| Direct `SetAttribute` / `SetRawInlineStyles` effects | Accumulate typed dirty properties into one frame transaction |
| Full inline CSS string on dynamic style change | Typed property operations; no CSS parsing in the Host hot path |
| Lynx frame/tick | Host `Frame` event feeding the Rust scheduler |
| `bounding_client_rect` Host query | Read Rust layout for ordinary geometry; explicit Host query only where necessary |
| Lynx element methods | Typed element commands ordered in the frame protocol |
| Native `ModuleDefinition.View` / `Prop` | Host element factory and canonical element schema registration |
| Lynx CSS animation and imperative extension | Rust motion updating typed property slots before packet generation |
| Lynx text/layout | Taffy layout plus batched Host intrinsic measurement |

## Migration outline

1. Introduce `NodeId`, the Rust retained scene, element schemas, and a
   `RecordingRenderer` without changing the shipped Lynx backend.
2. Implement the frame encoder, validator, revision model, and Rust-only tests.
3. Connect `render!` directly to `SurfaceRuntime`, without a Lynx compatibility
   renderer in the new path.
4. Move signals and motion from complete inline-style strings to typed dirty
   property slots and one transaction per frame.
5. Generate and launch minimal Lynx-free Android and iOS applications, then
   connect their native Rust library ABI and implement standard element
   factories, measurement, events, and packet application. The launch-shell
   portion is complete; retained rendering remains.
6. Implement the JavaScript DOM provider for Web.
7. Extract the portable implementation from the first macOS slice into
   `platforms/desktop`. Keep OS lifecycle in `platforms/macos` and add
   equivalent Windows and Linux application shells plus CNG-generated
   `gen/<os>` composition roots. The three shells now exist; CNG/build/run
   wiring for Windows and Linux remains. Scene projection, measurement, paint
   lowering, batching, shaders, and GPU resources remain common; lifecycle
   and native services remain visible at the OS boundary.
8. Establish the shared Host scenario schema, recording event sink, per-Host
   conformance runners, and the pinned WPT-derived corpus before expanding
   property support. The Host-only runner must exercise production
   measurement, presentation, paint, and input paths without `RuntimeInstance`.
9. Complete Desktop capability coverage for hierarchical accessibility, group
   compositing, filters, path clipping, and external surfaces without leaking
   Desktop render types into the common protocol.
10. Migrate custom UI modules to canonical element schemas, generated Host
   factories, and typed commands.
11. Remove the separate legacy Lynx production path, C++ bridge, fork artifacts,
   and obsolete distribution paths after Host conformance and visual parity are
   reached. No Lynx adapter is introduced into the retained path.

## Invariants

1. Rust owns the logical scene and accepted revision.
2. The Host owns concrete platform objects but not style resolution or animation
   state.
3. One surface has one ordered renderer lane and one Whisker-visible Host.
4. One changed frame produces at most one normal `present` call per surface.
5. Unchanged nodes are not resent and idle applications request no frames.
6. A frame packet is an ordered transaction, not a bag of independent calls.
7. Element instances are scene nodes, not module instances.
8. Custom UI modules use registered element types and the same frame protocol.
9. Ordinary Web frames use an in-callback WASM/JavaScript call. Ordinary
   Desktop frames use direct typed Rust calls across the semantic Host
   boundary. Neither crosses native-process IPC.
10. Host-dependent measurement is batched, keyed, cached, and epoch-validated.
11. Renderer and protocol behavior can be tested with a Rust-only provider.
12. Whisker core, not UI modules, requires and coordinates the renderer.
13. Renderer callbacks use the event sink passed during binding and do not
    create a reverse registry dependency on core.
14. A third-party UI element uses the same schema registration, frame,
    measurement, event, and command paths as a built-in element.
15. A Host `WhiskerView` is a surface/bootstrap container, not a scene element
    or runtime module.
16. Standalone and embedded forms mount the same Application descriptor
    generated by the single `#[whisker::main]` root.
17. Separate `WhiskerView` mounts create isolated `RuntimeInstance`s by
    default.

## Open questions

The following must be resolved before this RFC becomes `Accepted`:

- final data structures for normalized `ElementRegistration`, Host factory
  capabilities, and compact binding results at the now-defined
  Scene/Renderer/ElementProvider boundary;
- final binary header, opcode numbering, alignment, and table encoding;
- whether scene revisions acknowledge decoded state or completed visible
  platform application when a backend internally defers work;
- the minimum standard element set every interactive renderer must provide;
- rich-text run and inline-attachment representation beyond the current plain
  UTF-8 Text v1 payload;
- whether accessibility uses frame operations or a separately versioned but
  transaction-linked semantics packet;
- command cancellation and result ordering when an element is deleted;
- the final normalized element-category encoding and typed custom-property
  contracts, consistently with RFC 0004's public domain vocabulary;
- debug symbol-table format for compact element, property, event, and command
  IDs;
- hydration and event attachment requirements for a future SSR DOM renderer;
- exact low-level Desktop library and version choices, target support matrix,
  and the binary-size and compile-time budgets they must satisfy;
- the minimum Desktop shader, compositing, clip, accessibility, and external-
  surface feature set required by the conformance suite;
- generated OS shells and embedded-region packaging while retaining
  `platforms/{macos,windows,linux}` as the Host implementation boundaries;
- the binary ABI and generated Host representation of an
  `ApplicationDescriptor`, including collision-free selection when several
  independently built applications are linked into one Host;
- intrinsic embedded-surface sizing and cross-boundary focus traversal beyond
  the required constrained sizing mode.
