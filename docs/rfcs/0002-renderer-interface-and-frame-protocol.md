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
  1. create Activity/root View, UIWindow/root UIView, or DOM root
  2. load the Rust library or WASM module
  3. call whisker_start(BootstrapInput { host_surface, environment })
       |
       v
Rust kernel
  4. construct module registry
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
 10. start the Application module
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

A custom UI package such as `whisker-video` commonly contains:

```text
whisker-video package
|- Rust component and typed ElementHandle API
|- runtime module providing whisker.video/Video@1
|- Android Host element factory
|- iOS Host element factory
|- JavaScript Host element factory
`- optional build plugin for dependencies, permissions, and registration
```

The runtime module contributes an element schema. The package metadata or its
companion plugin causes the target Host factory and generated registration to
be included in the project. During bootstrap, `register_elements` verifies that
the Rust schema and Host factory agree on interface version and assigns a
compact `ElementTypeId`.

Creating and updating a video still uses the ordinary frame path:

```text
CreateNode(100, VIDEO)
SetProperty(100, VIDEO_SOURCE, resource)
SetLayout(100, ...)
InvokeCommand(100, VIDEO_PLAY, ...)
```

The Host factory maps this to `PlayerView`, an `AVPlayerLayer`-backed view, or
an HTML `video` element. It may hold per-node platform state, but that state is
destroyed with the scene node. The runtime module itself is not instantiated
once per video element.

Custom factories register through the one platform Host. They do not create a
second bridge or module registry.

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

## Open questions

The following must be resolved before this RFC becomes `Accepted`:

- the exact boundary between `Renderer` and the collection-valued
  `ElementProvider` interface;
- final binary header, opcode numbering, alignment, and table encoding;
- whether scene revisions acknowledge decoded state or completed visible
  platform application when a backend internally defers work;
- the minimum standard element set every interactive renderer must provide;
- exact text measurement inputs, baseline representation, and font fallback
  identity;
- whether accessibility uses frame operations or a separately versioned but
  transaction-linked semantics packet;
- command cancellation and result ordering when an element is deleted;
- debug symbol-table format for compact element, property, event, and command
  IDs;
- hydration and event attachment requirements for a future SSR DOM renderer.
