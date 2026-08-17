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
- DOM nodes through JavaScript inside the system WebView on Desktop v1.

The Host supplies frame callbacks, native text and intrinsic measurement,
input, viewport changes, resource readiness, and concrete element factories.
Rust replies once per changed frame with one packed `FramePacket`. The packet is
a transaction containing only changes since the last accepted scene revision;
Whisker does not send the whole screen every frame.

The renderer is an ordinary runtime module implementing
`whisker.renderer@1`. UI modules are ordinary modules that contribute element
types. Individual `View`, `Text`, or custom-view instances are scene nodes, not
module instances.

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
- Keep Web and Desktop animation within one JavaScript animation-frame callback
  without asynchronous native IPC.
- Permit custom native/DOM views while retaining a single Host per platform.
- Use the same `#[whisker::main]` Application descriptor and surface runtime
  for a generated standalone root and an optional embedded Host container.

## Non-goals

- Defining the supported CSS surface, cascade, selector matching, or exact
  layout and paint semantics. RFC 0003 owns those decisions.
- Standardizing the binary encoding byte-for-byte in this RFC. This RFC fixes
  the information model and invariants; implementation work will assign final
  opcodes, alignments, and field widths.
- Requiring every renderer to use platform view objects. A future canvas or GPU
  renderer may implement the same scene contract if its capabilities satisfy
  the application.
- Preserving Lynx element handles, raw inline-style strings, or the current
  imperative animation extension.
- Introducing a second native Host for Desktop. Desktop v1 has a JavaScript
  Host inside its WebView.
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
- CSS parsing, selector matching, cascade, inheritance, and resolved values;
- layout constraints and the Taffy layout tree;
- animation timelines, interpolation, springs, decay, and gesture handoff;
- classification of changes as layout, paint, composite, text, or semantics;
- generation of minimal scene changes;
- event propagation, capture/bubble, gesture state, and listener lifetimes;
- accessibility semantics as a platform-independent logical tree;
- the current accepted scene revision for each surface.

### The Host owns

- concrete Android View, UIView, or DOM object allocation and destruction;
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
| Desktop v1 | Rust/WASM in WebView | WebView JavaScript | DOM |

The Desktop launcher creates a system WebView and loads the application. It is
generated build infrastructure, not a second Whisker Host. In particular, a
normal Desktop frame does not travel from WASM to JavaScript, through IPC to a
native Rust process, and back.

## Runtime concepts

### Renderer module

A renderer provider implements the singular `whisker.renderer@1` interface for
a surface. The runtime resolves it once during bootstrap and holds a typed
handle. No module-name or method-name lookup occurs while constructing a frame.

### Surface

A `Surface` is one independently presented root: an Android Activity content
root, iOS window/root view, browser document root, or Desktop WebView document.
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
| Host `WhiskerView` | Android/iOS/JavaScript Host | Container into which one Whisker surface is mounted |
| Whisker `view` | Rust scene | Ordinary box/container element inside that surface |

The platform shape is:

```text
Android   WhiskerView : Host ViewGroup
iOS       WhiskerView : Host UIView
Web       DOM custom element (for example `whisker-view`) or JavaScript object
Desktop   system-WebView-backed container, with JavaScript as the Host
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

### Module dependency graph

UI modules do not require a concrete renderer and do not call `present`,
`measure_batch`, or Host factories. The Scene Runtime is the coordinator that
requires both sides:

```text
Application Module
    |
    | requires exactly one SceneV1
    v
Scene Runtime Module
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

Conceptually, the Scene Runtime declares:

```rust,ignore
ModuleDescriptor {
    id: "whisker.scene",
    provides: [interface("whisker.scene", "1")],
    requires: [
        exactly_one("whisker.renderer", "^1"),
        many("whisker.element-provider", "^1"),
    ],
    lifecycle: LifecycleScope::Surface,
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

The registry starts the renderer and element providers before the Scene
Runtime, then starts the Application module. The Scene Runtime collects the
schemas, validates duplicate canonical element keys, and passes registrations
to the resolved renderer. Shutdown occurs in reverse order.

The renderer does not resolve the Scene Runtime to send events back. During
`attach_surface`, the Scene Runtime passes a typed `RendererEventSink`. This
keeps the registry graph directed and avoids a `Scene -> Renderer -> Scene`
cycle:

```text
Scene --requires RendererV1---------> Renderer
Scene --passes RendererEventSink----> Renderer
Scene <-------events through sink---- Renderer
```

Likewise, the renderer does not resolve `ElementProvider` modules itself. It
receives normalized `ElementRegistration` values from the Scene Runtime. This
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
        requests: &[MeasureRequest],
        responses: &mut Vec<MeasureResponse>,
    ) -> Result<(), RenderError>;

    fn present(
        &mut self,
        surface: SurfaceId,
        packet: FramePacket<'_>,
    ) -> Result<PresentResult, RenderError>;

    fn detach_surface(&mut self, surface: SurfaceId);
}
```

This is an information-level contract, not a final Rust trait signature. A
generated binding may use function tables, FFI functions, JNI methods, or
WASM imports. The semantics and ordering remain the same.

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

### Web and Desktop timing

On Web and Desktop, JavaScript calls into WASM from a
`requestAnimationFrame` callback. Rust advances motion and calls
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

There is no Tauri command or native-process IPC on this path. DOM measurement
can still trigger expensive browser layout, so reads are batched and cached,
but it is not forced to be asynchronous by the Desktop architecture.

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

It does not serialize a `Vec<WhiskerValue>` per property and does not encode
method names in the hot path. WASM JavaScript reads a borrowed `Uint8Array`
view over linear memory during `present`; native bindings receive a borrowed
pointer and length. The packet is valid only for the synchronous duration of
`present` unless a provider explicitly copies it.

Unknown major protocol versions are rejected. A minor version may add ignorable
sections or opcodes only when their skip length and optional semantics are
encoded. Required unknown behavior must fail capability negotiation instead of
being silently skipped.

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
  SetLayout(node, x, y, width, height)
  SetTransform(node, transform)
  SetOpacity(node, opacity)
  SetClip(node, clip)
  SetVisibility(node, visibility)
  SetZOrder(node, z_order)

Paint and content
  SetBackground(node, paint)
  SetBorder(node, border)
  SetShadow(node, shadow)
  SetText(node, text_run)
  SetImage(node, resource)
  SetProperty(node, property_id, typed_value)
  ClearProperty(node, property_id)

Interaction and semantics
  SetEventMask(node, event_mask)
  SetHitTest(node, hit_test_behavior)
  SetAccessibility(node, semantics)
  SetPointerCapture(node, pointer_id)
  ReleasePointerCapture(node, pointer_id)

Commands
  InvokeCommand(node, command_id, arguments, result_id?)
```

The final opcode set may combine common operations for compactness. The
semantic separation matters because it enables dirty classification,
validation, and backend-specific fast paths.

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

An element provider defines what an element *is*; it does not implement CSS
parsing, selectors, cascade, inheritance, layout, or common paint properties.
The Scene Runtime connects independent style and layout services with element
providers and the renderer:

```text
ElementProvider modules -------+
Style Engine ------------------+--> Scene Runtime --> FramePacket --> Renderer
Layout Engine -----------------+
```

Element schemas declare semantic traits used by the common style system:

```rust,ignore
enum ElementTrait {
    Box,
    Container,
    TextContent,
    Replaced,
    ScrollContainer,
    HitTestable,
    Accessible,
}
```

The final trait set belongs to RFC 0003. The boundary is fixed here: a `View`
can declare `Box + Container`, `Text` can declare `Box + TextContent`, and
`Image` or `Video` can declare `Box + Replaced`; the common Style Engine uses
those declarations to determine property applicability.

Standard CSS properties such as width, padding, background, opacity,
transform, border, and clip are not repeated in each element schema. An
element schema contains only element-specific properties such as a video's
source, autoplay behavior, or playback rate. Element defaults may be supplied
as precompiled typed declarations, but the Style Engine performs their cascade.

A UI package that truly needs selector- and cascade-aware custom properties
may additionally provide a collection-valued `StyleExtensionProvider`
interface. Behavioral configuration should remain an element property rather
than becoming CSS merely because it affects a visual component.

At the Cargo level, a UI crate may depend on small `element-api` and
`style-types` crates for stable IDs and schema types. That compile-time
dependency is not a runtime dependency on a concrete Style Engine or renderer.

## Standard View example

The built-in UI package provides an Application-scoped module such as
`whisker.ui.primitives`. That module provides an `ElementProvider` collection
containing schemas for `View`, `Text`, and other standard element types.

Conceptually, `View` declares:

```rust,ignore
ElementSchema {
    key: "whisker.ui/View@1",
    children: ChildrenPolicy::Multiple,
    measure: MeasurePolicy::None,
    properties: &[ACCESSIBILITY_LABEL, POINTER_EVENTS, FOCUSABLE],
    events: &[POINTER_DOWN, POINTER_MOVE, POINTER_UP, CLICK, FOCUS, BLUR],
    commands: &[FOCUS, BLUR],
}
```

Width, padding, background, transform, and other CSS properties are common
resolved-style data, not bespoke `View` properties.

One module instance registers the type. Every rendered `View` creates a cheap
scene node:

```text
whisker.ui.primitives module instance
  `- element type whisker.ui/View@1
       |- NodeId 41
       |- NodeId 42
       `- NodeId 43
```

A first frame might contain:

```text
CreateNode(42, VIEW)
InsertChild(10, 42, 0)
SetLayout(42, 16, 24, 320, 80)
SetBackground(42, white)
SetEventMask(42, CLICK)
```

The Host binding maps `VIEW` to its registered standard factory:

```text
Android -> standard Whisker ViewGroup
iOS     -> UIView
Web     -> div
Desktop -> div in the WebView DOM
```

## Custom UI modules

A third-party UI package such as `whisker-video` commonly distributes all of
these pieces together:

```text
whisker-video package
|- Rust declarative component API
|- Rust typed ElementHandle, props, events, and commands
|- runtime module providing an ElementProvider
|- versioned whisker.video/Video@1 element schema
|- Android Host element factory
|- iOS Host element factory
|- JavaScript Host element factory
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
commands, measurement policy, child policy, and traits. The exact macro syntax
is deliberately not fixed by this RFC; conceptually the declaration produces:

- the Rust component builder and props;
- a generation-checked `ElementHandle` API;
- typed event and command payloads;
- `ElementSchema` and stable symbolic property/event/command IDs;
- FramePacket encoders and decoders;
- Kotlin, Swift, and TypeScript binding inputs;
- module and debug-symbol descriptors.

An illustrative generated schema is:

```rust,ignore
ElementSchema {
    key: "whisker.video/Video@1",
    traits: &[BOX, REPLACED, ACCESSIBLE, MEDIA],
    children: ChildrenPolicy::None,
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
transform, border radius, and clip are common styles supplied by the traits and
the Style Engine.

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
trait HostElementFactory {
    fn create(&mut self, context: ElementContext) -> HostElement;
    fn apply_properties(&mut self, element: &mut HostElement, patch: PropertyPatch);
    fn invoke_command(&mut self, element: &mut HostElement, command: Command);
    fn measure(&mut self, element: &HostElement, request: MeasureRequest)
        -> MeasureResponse;
    fn destroy(&mut self, element: HostElement);
}
```

This is a cross-language behavioral shape, not a Rust trait that Kotlin,
Swift, or JavaScript literally implements. Binding generation supplies native
typed payloads and verifies the common schema so authors do not manually keep
wire IDs synchronized.

The same element key can map to different concrete objects:

```text
whisker.video/Video@1
  Android -> PlayerView / Media3 player
  iOS     -> AVPlayerLayer-backed UIView
  Web     -> HTMLVideoElement
  Desktop -> HTMLVideoElement in the WebView
```

Factories register through the one Host renderer registry. They do not create
a module-specific bridge, renderer, or second Host.

### Bootstrap binding

The package metadata or companion plugin causes the selected target factory
and generated registration to be included in Project IR. At runtime:

```text
Video module
  -> provides ElementSchema("whisker.video/Video@1")

Scene Runtime
  -> collects and normalizes the schema
  -> Renderer.register_elements(...)

Host Renderer
  -> looks up embedded Host factory by canonical key and major version
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

Web / Desktop
  JavaScriptProvider(whisker-video/host)
  HostElementFactory(VideoElementFactory)
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
  whisker.video/Video@1 is selected
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
  -> create AVPlayer / Media3 player / HTMLVideoElement

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
Custom           versioned element-specific measurement payload
```

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

Android, iOS, Web, and Desktop v1 are expected to provide synchronous `Ready`
results for ordinary text measurement. Web/Desktop may do synchronous work in
JavaScript during the WASM frame callback; they are not forced through native
IPC. Requests are batched to avoid interleaved DOM reads and writes.

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
Rust CSS/style engine
  -> ResolvedBackground / ResolvedBorder / ResolvedClip / ...
  -> typed frame operations
  -> Host platform implementation
```

Backend capability negotiation determines whether a feature is native,
emulated, or unsupported. The exact required CSS-to-paint mapping belongs to
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
- `NeedSnapshot { host_revision }`;
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

The protocol decoder should be fuzzed independently. Each Host backend gets a
conformance suite that replays shared packet fixtures and reports its projected
tree. Visual/platform tests remain necessary for text and paint fidelity but
are not required to test engine or protocol correctness.

## SSR and headless renderers

Because the renderer is an interface, a Rust-only serializer or headless
renderer can consume the same logical scene without a platform Host. This RFC
does not define HTML serialization, CSS emission, or hydration markers, but it
does not make SSR impossible.

An SSR renderer may require different capabilities and output rules from the
interactive DOM renderer. Both can implement renderer interfaces without
making runtime DOM layout the semantic source of truth. RFC 0003 will define
how server-emitted CSS relates to Rust-resolved interactive styling.

## Mapping from the current implementation

| Current concept | Direction under this RFC |
|---|---|
| Lynx `FiberElement` stored in `Element` | Replace with Rust `NodeId` into the retained scene |
| Direct `SetAttribute` / `SetRawInlineStyles` effects | Accumulate typed dirty properties into one frame transaction |
| Full inline CSS string on dynamic style change | Typed property operations; no CSS parsing in the Host hot path |
| Lynx frame/tick | Host `Frame` event feeding the Rust scheduler |
| `bounding_client_rect` Host query | Read Rust layout for ordinary geometry; explicit Host query only where necessary |
| Lynx element methods | Typed element commands ordered in the frame protocol |
| Native `ModuleDefinition.View` / `Prop` | Host element factory and versioned element schema registration |
| Lynx CSS animation and imperative extension | Rust motion updating typed property slots before packet generation |
| Lynx text/layout | Taffy layout plus batched Host intrinsic measurement |

## Migration outline

1. Introduce `NodeId`, the Rust retained scene, element schemas, and a
   `RecordingRenderer` without changing the shipped Lynx backend.
2. Implement the frame encoder, validator, revision model, and Rust-only tests.
3. Add a temporary Lynx-backed `RendererV1` adapter so the runtime stops
   storing or directly calling Lynx elements before native replacement
   renderers are complete.
4. Move signals and motion from complete inline-style strings to typed dirty
   property slots and one transaction per frame.
5. Implement standard element factories, measurement, events, and packet
   application for Android and iOS.
6. Implement the JavaScript DOM provider shared by Web and Desktop v1.
7. Migrate custom UI modules to versioned element schemas, generated Host
   factories, and typed commands.
8. Remove the temporary Lynx renderer, C++ bridge, fork artifacts, and obsolete
   distribution paths after conformance and visual parity are reached.

## Invariants

1. Rust owns the logical scene and accepted revision.
2. The Host owns concrete platform objects but not CSS cascade or animation
   state.
3. One surface has one ordered renderer lane and one Whisker-visible Host.
4. One changed frame produces at most one normal `present` call per surface.
5. Unchanged nodes are not resent and idle applications request no frames.
6. A frame packet is an ordered transaction, not a bag of independent calls.
7. Element instances are scene nodes, not module instances.
8. Custom UI modules use registered element types and the same frame protocol.
9. Ordinary Web/Desktop frames do not cross native IPC.
10. Host-dependent measurement is batched, keyed, cached, and epoch-validated.
11. Renderer and protocol behavior can be tested with a Rust-only provider.
12. The Scene Runtime, not UI modules, requires and coordinates the renderer.
13. Renderer callbacks use the event sink passed during binding and do not
    create a reverse registry dependency on the Scene Runtime.
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
- exact text measurement inputs, baseline representation, and font fallback
  identity;
- whether accessibility uses frame operations or a separately versioned but
  transaction-linked semantics packet;
- command cancellation and result ordering when an element is deleted;
- the final standard `ElementTrait` and `StyleExtensionProvider` contracts,
  which RFC 0003 must define consistently with this boundary;
- debug symbol-table format for compact element, property, event, and command
  IDs;
- hydration and event attachment requirements for a future SSR DOM renderer.
- the binary ABI and generated Host representation of an
  `ApplicationDescriptor`, including collision-free selection when several
  independently built applications are linked into one Host;
- intrinsic embedded-surface sizing and cross-boundary focus traversal beyond
  the required constrained sizing mode.
