# RFC 0004: Native Modules and Host Elements

- Status: Accepted
- Authors: Whisker maintainers
- Created: 2026-08-21
- Discussion: TBD
- Tracking issue: TBD
- Depends on: [RFC 0001](0001-runtime-modules-and-build-plugins.md),
  [RFC 0002](0002-renderer-interface-and-frame-protocol.md),
  [RFC 0003](0003-typed-inline-style-layout-and-paint.md)
- Supersedes: None

## Summary

Whisker modules are the public extension mechanism for native services and
Host-backed elements. They are not the decomposition mechanism for every
internal Rust subsystem.

Whisker core owns the mechanisms that must agree to produce one coherent
scene: reactive execution, the logical element tree, typed style resolution,
Taffy layout, intrinsic-measurement transactions, frame scheduling and
generation, and event routing. Built-in UI elements such as `View`, `Text`,
`Image`, `TextInput`, and `ScrollView` are nevertheless registered through the
same element-provider contract as a third-party element such as `GoogleMap`.
Core does not reserve hard-coded element IDs or a privileged registration path
for its built-ins.

Lynx's shell-only `Page` tag is not a Whisker element contract. The legacy Lynx
bootstrap may keep creating it as a private Host wrapper, but it is not present
in the normalized registry and is never exposed by `ModuleDefinition.View`.

Every visual element has two logical Host-side parts:

```text
HostNode
|- common presentation
|    layout, background, border, shadow, clip, opacity, transform,
|    stacking, hit testing, semantics
`- element-specific content
     empty box, shaped text, image, native input, native map, video, ...
```

The common Host renderer implements presentation once. An element provider
implements only its content, properties, commands, events, and optional
intrinsic measurement. This prevents the number of implementations from
growing as `element types x style properties` while allowing built-in and
third-party elements to use the same registration, frame, measurement, event,
and lifecycle paths.

The split is logical, not a requirement to allocate two platform views. A Host
may fuse presentation and content into one object when the observable result
is identical. It creates a wrapper or extra compositing layer when clipping,
shadows, native-view composition, accessibility, or event semantics require
one.

## Motivation

The previous module direction was broad enough to imply that style, layout,
motion, scene coordination, rendering, and other internal mechanisms should
all become replaceable runtime modules. That is aesthetically uniform but
puts versioned interfaces, lifecycle resolution, and possible indirection
between algorithms that must be tightly coordinated on every frame. It also
suggests a replaceability guarantee that Whisker is not ready to maintain.

At the other extreme, implementing `View`, `Text`, and every future custom
element directly in core makes native extension difficult and recreates a
different bridge for each platform. The desired boundary is narrower:

- core owns scene-wide mechanisms and semantic policy;
- a module exposes an addable native service or element type;
- a build plugin/CNG contribution makes its target implementation available;
- the Host renderer applies common presentation and delegates only
  element-specific content behavior.

This must support Android Views, UIViews, DOM, and the Rust Desktop Host without
making any one of them the semantic reference implementation.

## Goals

- Keep Whisker core small in responsibility and free from target UI classes.
- Keep the public module API simple enough for application and library authors.
- Register built-in and custom elements through one canonical contract.
- Preserve the Expo-like native `ModuleDefinition` authoring shape.
- Avoid exposing Rust implementation vocabulary such as `Trait` in the
  Swift/Kotlin/JavaScript module definition DSL.
- Apply common styles to every eligible element without reimplementing them in
  every element provider.
- Deliver resolved text styles, including inherited `font_size`, to native
  elements that render or edit text.
- Support synchronous and deferred intrinsic measurement without making the
  Host authoritative for layout.
- Keep frame, measurement, and event traffic typed, numeric, batched, and
  testable with mock peers.
- Apply the same semantic contract to Web and Desktop even when there is no
  language FFI boundary.

## Non-goals

- Turning every Rust crate, algorithm, or internal trait into a module.
- Allowing an element provider to replace Taffy, the style resolver, scene
  invalidation, frame ordering, or event propagation.
- Letting third-party modules add arbitrary properties to the common CSS
  registry or participate in a general cascade.
- Requiring one native object per logical node or requiring one wrapper around
  every native object.
- Reproducing Lynx's dynamic property-name dispatch or React Native's legacy
  serialized bridge.
- Guaranteeing that every native element supports every compositing
  combination on every target. Unsupported required combinations must be
  diagnosed rather than silently approximated.
- Defining a rich inline-text tree in version 1. Version 1 `Text` is a leaf
  text-content element; rich runs require a separate content model.

## Architectural boundary

### Whisker core

“Small core” means a narrow and stable responsibility, not the smallest
possible crate or line count. The following remain core because splitting
them behind independently replaceable runtime interfaces would weaken scene
correctness or add work to the frame hot path:

- runtime/event-loop integration and reactive scheduling;
- the retained logical tree, stable `NodeId`s, and node generations;
- element-schema registry and applicability validation;
- typed style composition, limited inheritance, and computed values;
- Taffy tree ownership and layout;
- measurement batching, cache keys, pending-result validation, and relayout;
- dirty classification and frame transaction generation;
- frame request coalescing and revision ownership;
- logical hit testing, capture/bubble propagation, and listener lifetime;
- focus, IME, accessibility, and scroll-state coordination contracts;
- protocol validation and recovery.

These responsibilities may be split into ordinary Rust crates. They are not
runtime modules merely because they are separate crates. Internal Rust traits
remain useful for tests and implementation boundaries without promising
third-party replacement compatibility.

### Whisker modules

A module is appropriate when functionality is addable, omittable,
target-backed, independently versioned, or supplied by an application/library
author. The initial public categories are:

1. **service modules**, such as haptics, notifications, secure storage, and
   sensors;
2. **element-provider modules**, such as `TextInput`, `GoogleMap`, a video
   player, or the built-in primitive element package.

A package may provide both categories. A map package, for example, can expose
a `GoogleMap` element and a detached geocoding service. A scene node uses the
element command/event path; an operation with no scene lifetime uses a service
interface.

The Host renderer remains the singular target projection selected at
bootstrap. It is a module boundary because the Host implementation is selected
per target, but it is not decomposed into one renderer per element.

## Element contract

### Registration and identity

An element has one versionless name, for example `whisker.ui/Text`
or `whisker.google-maps/GoogleMap`. Build composition embeds a matching Host
factory. During surface bootstrap, core:

1. collects element schemas from selected modules;
2. rejects duplicate names;
3. assigns an immutable compact `ElementTypeId` for the registry epoch;
4. asks the Host renderer to bind each normalized registration to an embedded
   factory;
5. validates properties, events, commands, measurement, and target
   capabilities before the first frame.

`FramePacket::CreateNode` carries the compact `ElementTypeId`. Element names
and module names are not repeated in per-node operations. `SetText` does not
implicitly decide that a node is a `Text`; type identity is established only
by `CreateNode`.

Built-in and custom element IDs use this same negotiation. Core must not
assume that `View == 1` or dispatch `Text` through a separate protocol.

### Public description, not public traits

The author-facing definition states only the cross-platform contract:

```rust,ignore
#[whisker::module_component(
    name = "example.controls/Toggle",
    measurement = None,
)]
pub fn toggle(
    checked: Signal<bool>,
    on_change: ChangeEvent,
) {}
```

Built-ins use a distinct internal declaration marker because their specialized
Rust builders and `ElementTag` lowering already exist, but the declaration
shape and schema compiler are the same:

```rust,ignore
#[whisker::builtin_component(
    name = "whisker.ui/View",
    measurement = None,
)]
fn view(
    style: Style,
    children: Children,
) {}

#[whisker::builtin_component(
    name = "whisker.ui/Text",
    measurement = Text,
)]
fn text(
    style: Style,
    children: TextChildren,
) {}
```

`module_component` emits a custom authoring builder and a name binding;
`builtin_component` retains the built-in builder and emits only the shared
schema module. Both generate `NAME`, `schema()`, property/event IDs,
`child_policy`, and measurement from the same compiler. The built-in
definition then binds that schema to its internal `ElementTag`; that tag is not
part of the public schema identity.

The custom builder owns its element handle from `builder()` onward and
implements the same internal `ElementBuilder` trait as built-ins. Universal
Rust authoring features such as `style`, `class`, `id`, accessibility props,
gesture/lifecycle events, arbitrary attributes, and `ElementRef` binding
therefore operate on both built-in and custom elements. They are not repeated
as properties or events in each custom element schema. Only the
component-specific parameters in the declaration become negotiated Host
properties and events. `Children` selects element children and `TextChildren`
selects normalized plain-text content. A declared
`style` parameter remains accepted for compatibility but is still excluded
from the schema.

The schema compiler derives property and event names from the function
signature. It emits one of three child policies:

- no children parameter -> `None`;
- `Children` -> `Elements`;
- `TextChildren` -> `PlainText`.

Where element children mount is deliberately not part of the common schema:
the Desktop, Web, Android, and iOS implementations own their presentation,
scroll-content, or native container. Plain-text fragments are normalized by
the Rust runtime and emitted as `SetText`; the Host never receives a
heterogeneous child union. Built-in and third-party plain-text elements use
the same policy and frame operation.

The only other explicit layout capability retained in the common contract is
the intrinsic measurement policy.

### Child policies

Whether an element contains logical children is independent of what it draws:

| Element | Child policy | Host-owned placement | Intrinsic measurement |
|---|---|---|---|
| `View` | elements | common presentation node | none |
| `Text` v1 | plain text | n/a | text |
| `Image` | none | n/a | known metadata or replaced content |
| `TextInput` | none | n/a | Host |
| `ScrollView` | elements | scroll-content container | none for viewport |
| `GoogleMap` | none | n/a | normally none |

Version 1 forbids ordinary flex children inside `Text`. Inline text is not a
tree of rectangular flex items: iOS and Android flatten styled ranges into an
attributed string/spannable representation. Rich text therefore needs a later
`TextRun` content model rather than pretending normal `View` children can be
laid out inline.

An intrinsic Host measure function is initially valid only for a leaf element.
A container derives its size from Taffy and its children. A future container
whose native content participates in sizing needs an explicit, cycle-safe
contract; it is not enabled by setting a flag.

### Child mount targets

An element with a `Children` parameter allows element tree operations.
`InsertChild`, `MoveChild`, and `RemoveChild` retain only the logical parent and
index. The common Host presenter mounts into its presentation node by default;
a Host-local target override lets `ScrollView` use its native scroll-content
container rather than its viewport wrapper. This target is never serialized in
the common schema.

A third-party element may contain Whisker element children only when its common
Rust signature contains `Children`. Whether content is below or above logical
children and how coordinates map into a custom platform container is target
implementation detail.
Arbitrary native content plus arbitrary children is unsupported by default;
otherwise paint order, clipping, hit testing, and accessibility would be
platform-dependent. A leaf provider such as `GoogleMap` simply has no child
mount target.

## Common presentation and element content

### Responsibility split

The common renderer consumes these operations for every box-presented node:

- `SetLayout` and stacking order;
- `SetBoxPaint` for backgrounds, borders, and radii;
- shadow, clip, opacity, transform, visibility, and hit-test state;
- common accessibility geometry and semantics.

The registered element factory consumes only content-specific operations:

- `SetText` or `SetImage` for a matching content category;
- `SetTextStyle` for an element that declares `TextStyle` consumption;
- negotiated `SetProperty(PropertyId, WhiskerValue)` patches;
- typed commands and node-scoped events;
- an optional resolved `TextStyleSnapshot`;
- optional intrinsic measurement requests.

Consequently, `background_color` works on `Text`, `TextInput`, `Image`, and
`GoogleMap` even though those factories do not implement background painting.
The common presentation implementation paints the background behind their
content. The element factory should normally make its own default background
and border transparent so there is only one owner.

### Logical wrapper and physical realization

The observable model is always presentation plus content, but the Host chooses
the cheapest correct realization per node and revision:

```text
fused
  one UIView/View/DOM element/GPU node owns presentation and content

wrapped
  presentation container or layer
    `- native content object

external surface
  presentation/compositor node
    `- platform surface or texture with explicit composition constraints
```

Fusion is allowed only while it preserves paint order, clipping, shadows,
transform origin, opacity grouping, z-order, hit testing, focus,
accessibility, and native-content behavior. In version 1 the realization is
stable for the node lifetime. A Host may fuse only when its factory and common
presenter can implement every supported dynamic presentation state for that
element without later reparenting it. Native controls and external surfaces
therefore normally start wrapped. A later protocol may permit state-preserving
fused/wrapped migration, but version 1 never risks losing focus, IME,
selection, scroll position, or native resource state to save a wrapper.

Wrapper elision is an optimization after correctness. A schema and current
presentation state form a conservative materialization key. A node with only
layout effects can often be flattened; a node with background, border, clip,
native ID/ref, accessibility, pointer handling, or an external surface is a
materialization barrier unless the Host proves exact fusion.

A physical presentation wrapper is accessibility-neutral unless the logical
node's resolved semantics require it to be the accessible object. The Host
must expose one logical accessibility node, not duplicate wrapper and content
nodes. The same rule applies to focus and hit testing: the wrapper may
translate coordinates and route events, but it must not create an extra
application-visible target.

### Platform examples

#### iOS

- `View`: a presentation `UIView`/`CALayer`; no separate content object when
  fusion is exact.
- `Text`: a presentation layer plus transparent CoreText/TextKit content. A
  simple case may fuse into a Whisker-owned text view/layer.
- `TextInput`: a presentation container around a transparent
  `UITextField`/`UITextView`. UIKit retains IME, caret, selection, and native
  editing behavior; Whisker owns the outer box. Native background, border,
  and content insets are disabled or explicitly represented so they do not
  silently add a second CSS box.
- `ScrollView`: an outer presentation node, `UIScrollView`, and content
  container. The native scroll object owns scrolling physics; Taffy owns the
  viewport and content geometry.
- `GoogleMap`: a presentation container around `GMSMapView`. Whisker paint is
  behind or around the map; rounded clipping is applied by a compatible layer
  or declared unsupported when the map surface cannot be composed correctly.

#### Android

- `View`: a Whisker presentation `View`/`ViewGroup`, fused when possible.
- `Text`: common presentation plus a transparent text-layout/content object.
- `TextInput`: common presentation plus transparent `EditText`; Android owns
  IME, cursor, selection, and platform autofill. Default native background and
  padding are disabled or explicitly represented by the element contract.
- `ScrollView`: common presentation, native scroll container, and content
  `ViewGroup`. Gesture arbitration is integrated with Whisker's event router.
- `GoogleMap`: common presentation around a Maps SDK view or surface, subject
  to the backend's z-order and clipping capabilities.

#### Web

The DOM Host uses the same logical split. It may use one element where CSS and
content semantics match, or a presentation wrapper plus an `input`, `img`,
`video`, map element, canvas, or custom element. Taffy remains authoritative;
browser APIs supply measurement and paint primitives but must not create a
browser-layout-to-Taffy feedback loop.

The provider may be implemented in Rust with `web-sys` or in JavaScript with
the same string-declared Host API. `web-sys` is an implementation detail, not part of the
module contract and not something to remove solely to make the architecture
look uniform. A JavaScript Host can reduce WASM crossings and improve direct
ecosystem integration, while a Rust provider can be smaller and simpler when
only selected `web-sys` features are linked.

Rust-authored Web providers use the same declaration model as FFI-backed
providers: one `WebModuleDefinition` contains zero or more named `View`
declarations with named `Prop`, event, and command callbacks. Bootstrap binds
those names to Rust-assigned IDs. Implementing the internal DOM Host dispatch
trait directly is not the module-authoring API.

#### Desktop

The Desktop Host implements the same semantic contract directly in Rust.
Common presentation lowers to retained GPU primitives and text lowers to
shaped glyph content. There is no serialization or language FFI merely to
preserve the abstraction.

Desktop providers likewise declare one `DesktopModuleDefinition` containing
zero or more named `View` declarations and named property, event, and command
callbacks. The Host may compile the negotiated result to internal Rust traits,
but those traits are an implementation detail rather than a different public
module shape.

A native child control or map-like provider is an external surface. Its
support depends on explicit compositor capabilities for clipping, transforms,
opacity, z-order, input, and accessibility. A target that cannot meet the
declared minimum fails composition or reports the unsupported combination; it
must not paint a plausible but incorrect result.

## Style delivery

### Closed common registry

RFC 0003's common property registry is closed and owned by core. Each property
is mapped once to a semantic channel:

| Channel | Examples | Consumer |
|---|---|---|
| layout | width, padding, flex, position | core/Taffy |
| box paint | background, border, radius, shadow | common Host presentation |
| composite | opacity, transform, clip, z-order | common Host presentation |
| text | font size/family/weight, line height, color | text-style consumer |
| replaced content | object fit/position | image/video content consumer |
| scroll | overflow/scroll behavior | core + scroll content consumer |
| semantics | accessibility, focusability, pointer behavior | core + Host adapter |

The element schema selects categories; it does not list or implement every
common property. Applicability is generated from the property table and the
normalized element categories. This makes the implementation approximately
`properties + element schemas`, not `properties x elements`.

A third-party visual setting such as `map_type`, `camera`, or
`shows_user_location` is a typed element prop. It does not become a CSS
property and does not inherit.

### Resolved text style

An element with `TextStyle` receives a typed, resolved snapshot, never raw CSS:

```rust,ignore
struct TextStyleSnapshot {
    revision: u64,
    font_families: FontFallbackList,
    font_size: LogicalPx,
    font_weight: u16,
    font_style: FontStyle,
    line_height: ResolvedLineHeight,
    letter_spacing: LogicalPx,
    color: Color,
    locale: LocaleId,
    direction: TextDirection,
}
```

This includes inherited values. A custom `TextInput` therefore receives
`font_size` through its typed `TextStyle` callback even though it did not
declare `font_size` as a prop. The same snapshot revision and shaping inputs
are included in its measurement key. A font change invalidates intrinsic
measurement before the new presentation is accepted.

Element props and common styles remain separate namespaces. A module must not
declare a second `font-size` prop to receive common text style.

## Expo-like `ModuleDefinition`

The existing authoring shape is retained. The exact syntax remains
language-idiomatic, but every Host definition uses stable strings:

```swift
public func definition() -> ModuleDefinition {
  Name("WhiskerTextInput")

  View("whisker.input/TextInput", TextInputView.self) {
    Prop("value") { view, value in view.reconcileValue(value) }
    Events("change", "selectionChange")
    Function("focus") { view, _ in view.focus(); return .null }
  }
}
```

Kotlin has the same form with Kotlin naming and types. Service-only
modules retain the existing `Name`, `Function`, `AsyncFunction`, module event,
and observer-lifecycle definition shape and do not emit element-provider
metadata. They do not need a `View` block.

The declarations map as follows:

- module-level `Function`/`AsyncFunction` -> typed native service interface;
- `View` -> Host factory named for one negotiated element schema;
- `Prop` -> numeric `PropertyId` patch;
- `Command` or view-local function -> numeric `CommandId` and optional
  `ResultId`;
- `Events` in a `View` -> node-scoped typed event IDs;
- module-level events -> service subscription lifecycle;
- `TextStyle` -> common resolved text-style channel;
- `Measurement` -> the element's intrinsic measurement provider.

### Independently compiled declarations and runtime negotiation

Rust and each Host implementation are independently compilable. A native
module must not require Kotlin or Swift source generated by `whisker run` or
`whisker build` before Gradle or Xcode can compile it. The mandatory contract
is the same loose coupling used by the original Whisker module API and Expo
Modules: both sides declare stable strings, and bootstrap matches them.

Rust remains authoritative for the application-facing schema and numeric wire
IDs. A Host `ModuleDefinition` independently declares:

- the versionless module and element names;
- property, event, and command names implemented by that Host;
- the native factory and callbacks;
- text/content handlers required by the Rust registration;
- Host-local child mount targets and native composition behavior.

At surface bootstrap Rust sends `ElementRegistration` records containing the
canonical names, member names, value kinds, capabilities, and Rust-assigned
compact IDs. The Host matches its catalog by string, validates the complete
shape, and builds an immutable lookup table from `ElementTypeId`, `PropertyId`,
`EventId`, and `CommandId` to native callbacks. A missing element, misspelled
property, incompatible value kind, missing plain-text updater, duplicate, or
extra required member is a bootstrap error before the first frame. Child
policy and measurement are not repeated in the Host declaration: they are
Rust-owned registration data. It is not a native compilation dependency.

Strings therefore exist in declarations and bootstrap diagnostics, not in
the frame hot path. `FramePacket::CreateNode` and subsequent property/event
traffic remain compact and numeric after negotiation. Numeric IDs generated
or assigned on one side must never be embedded as permanent IDs in a
hand-written Host implementation.

Public Host contracts are not generated. In particular Whisker does not emit
`TextInputContract`, `ViewBindings`, `TextBindings`, generated property/event
objects, or a generated `ModuleDefinition`. Such public generated types make
native source compilation depend on a prior Rust schema-export step and make
IDE completion sensitive to whether that step has already run.

Code generation is limited to invisible integration code: KSP and the Swift
build plugin discover `@WhiskerModule` declarations and generate provider or
registration lists. That code may instantiate a module and call the common
registrar; it must not inspect the executable `ModuleDefinition` body, copy a
Rust schema into native source, choose a view class, or define module behavior.
Schema drift is intentionally a deterministic bootstrap error. Host authors
may define ordinary hand-written constants in their own package, but those
constants are not a cross-language contract artifact.

### Native build ownership

Gradle and Xcode own production compilation. `whisker run` is a development
orchestrator and `whisker build` is a convenience entry point; neither is a
prerequisite of the native project. A developer can build the generated or
checked-in native project with `./gradlew assemble`, Android Studio,
`xcodebuild`, or Xcode's Run action.

Native build-system integration may invoke Whisker-owned helper executables as
ordinary build tools. That is different from requiring the user to run the
Whisker CLI first:

- Android's Gradle plugin owns Cargo compilation, native library staging, and
  module discovery as declared Gradle tasks.
- KSP owns Kotlin symbol discovery and emits the Host module provider from
  `@WhiskerModule` declarations.
- KSP cannot inspect the expressions and statements inside an executable
  `definition()` DSL body. It therefore does not reconstruct `Prop("...")` or
  `Events("...")`; completeness remains a bootstrap validation. This follows
  [KSP's documented symbol-level model](https://kotlinlang.org/docs/ksp-overview.html),
  which excludes expressions and statements.
- KSP does not launch Cargo or a Rust schema exporter. Gradle models Cargo as a
  separate build task, so Kotlin compilation and Host-only tests remain valid
  without it.
- Xcode owns the equivalent Rust compilation in a Run Script/build-tool phase.
  SwiftPM's build plugin discovers annotated Swift modules and emits only the
  Host provider/registration list.

From the application developer's perspective these helpers are internal to
the native build: one Gradle or Xcode build is sufficient. A module package
can also compile and run Host-only tests without Cargo or a Rust schema
artifact because the string-declared baseline remains complete.

A module package keeps this platform-neutral schema and its Rust authoring API
separate from every Host contribution. Android and iOS implementations live in
their Kotlin and Swift source trees; Web and Desktop implementations likewise
live in target-specific libraries. Using Rust on both sides of the Web or
Desktop boundary does not justify placing Host objects, DOM APIs, or Host
callbacks in the platform-neutral module source. Each Rust Host contribution
is an ordinary Cargo library crate rather than a source file injected with
`#[path]`:

```text
whisker-toggle/
  Cargo.toml              # platform-neutral module crate
  src/                    # schemas and authoring API only
  desktop/
    Cargo.toml            # whisker-toggle-desktop-host
    src/lib.rs
  web/
    Cargo.toml            # whisker-toggle-web-host
    src/lib.rs
  android/                # Gradle/Kotlin Host module
  ios/                    # SwiftPM/Swift Host module
```

The `[package.metadata.whisker]` marker identifies the outer package. Local and
git checkouts discover `desktop/Cargo.toml` and `web/Cargo.toml` by convention
and verify the package names declared in the outer metadata:

```toml
[package.metadata.whisker.desktop]
package = "whisker-toggle-desktop-host"

[package.metadata.whisker.web]
package = "whisker-toggle-web-host"
```

This small amount of metadata is package identity, not a source-file list or a
generated binding contract. It is necessary because Cargo intentionally omits
nested packages from an outer crates.io archive even when its `include` list
names them. The Desktop and Web Host libraries are therefore separately
published packages at the same version as the common module crate. Discovery
uses a nested path dependency when its manifest is present and an exact registry
version otherwise.

CNG adds the common crate and the selected Host crate as ordinary dependencies
of the generated Host Cargo project. The generated composition code calls the
common schema provider and the target Host's `#[WhiskerModule]` adapter. Cargo
therefore owns parsing, features, target selection, incremental compilation,
diagnostics, and IDE navigation; Whisker never asks rustc to compile a loose
source file outside Cargo's dependency graph.

Separate distribution packages do not imply a combined target binary: an
Android or iOS build does not depend on either Rust Host crate, and a Web build
depends on the Web Host package but not the Desktop Host package. The
platform-neutral crate does not import Host libraries or depend on a Host
runtime.

iOS and Android retain the established autolinking ownership. On iOS, CNG
creates the native project and the build pipeline discovers module
`Package.swift` manifests and stages a SwiftPM registration aggregator. On
Android, the generated Gradle project applies the Whisker settings/project
plugins; Gradle discovers module subprojects and KSP generates registration
providers. Re-running CNG after a clean checkout or after changing Cargo
dependencies is expected, but invoking `whisker run` or `whisker build` is not
required to compile an already generated Xcode or Gradle project.

Every platform uses an explicit `WhiskerModule` declaration signal. Rust
annotates an `impl WhiskerModule` block with `#[WhiskerModule]`; Android
annotates a concrete `Module` subclass with `@WhiskerModule`; iOS applies the
same spelling as an attached Swift macro. KSP and the SwiftPM build plugin
collect only annotated Host declarations and generate provider registration;
they do not need to understand Rust in order to do so. Inheritance alone is
not a registration mechanism.

Built-in `View`, `Text`, and `ScrollView` schemas remain platform-independent,
but each Rust platform binds them through its own `BuiltInElementModule`
implementation. Its `definition()` returns one ordinary multi-Element module
definition rather than a collection of one-Element modules. Built-ins are
selected automatically at bootstrap; after selection each contribution uses
the same registry path as package-provided elements. Mobile built-in modules
are checked-in Swift/Kotlin implementations declaring the canonical strings;
they do not have a privileged generated-source requirement.

`Host` describes an architectural role in this RFC, not a public API prefix or
suffix. Public APIs use the concrete role they expose (`WhiskerModule`,
`RuntimeOwner`, `MeasurementProvider`, `LayoutOptions`, and platform-specific
runtime/error names). Generated and private implementation details may still
refer to a Host when that makes the boundary clearer.

All module data uses the shared `whisker-value::WhiskerValue` model: property
values, event payloads, command/function arguments, and results. Numeric IDs
and frame batches still avoid name-based dynamic dispatch in the hot path;
using one value model does not turn each property update into a module call.
Schema value kinds optionally validate the top-level variant. Event payload
validation may be omitted, in which case any non-error `WhiskerValue` tree is
accepted and Rust callbacks may deserialize it into an authoring type.

`WhiskerValue` is not the entire rendering protocol. Control envelopes remain
typed structs: frame headers and operations, node and member IDs, layout and
paint records, measurement constraints/results, viewport and revision data,
resolved text-style snapshots, lifecycle state, and structured transport
errors. `WhiskerValue::Null` is an ordinary explicit value. It must never be
used to encode `ClearProperty`; clearing remains a distinct control operation
and reaches a distinct Host callback/default path.

## Intrinsic measurement

### Provider contract

Measurement occurs before the first `CreateNode` packet can be mounted, so it
must not require an already-created Host view. An element factory may expose a
separate lightweight measurer:

```rust,ignore
trait HostElementMeasurer {
    fn measure(
        &mut self,
        request: &MeasurementRequest,
    ) -> MeasurementResponse;
}

trait HostElementFactory {
    fn create(&mut self, context: ElementContext) -> HostElement;
    fn apply_content(&mut self, element: &mut HostElement, patch: ContentPatch);
    fn invoke_command(&mut self, element: &mut HostElement, command: Command);
    fn destroy(&mut self, element: HostElement);
}
```

These are information-level interfaces. A Host can share a text shaper,
control prototype, immutable resource metadata, or prepared content between
the measurer and factory. It must not create and leak a mounted native view
just to answer layout.

The request describes the content box constraints and contains all inputs
that can affect intrinsic size: element type, content and relevant props,
resolved text style, locale, direction, scale, resource metadata, content
revision, style revision, and environment epoch. The provider returns content
size, baselines, overflow, and optionally a `PreparedContentId`. Core adds
padding/border and applies width/height/min/max/aspect rules through Taffy.

### Synchronous and pending results

Requests are batched. Each answer is one of:

```text
Ready(key, metrics, prepared_content?)
Pending(key, request_id, provisional?)
Unsupported(key, reason)
```

The element declaration chooses the pending behavior:

- `Block`: withhold the new subtree until the initial answer is ready;
- `Placeholder(size)`: lay out using a declared provisional size;
- `RetainPrevious`: preserve the previous accepted size during an update.

An asynchronous resource completion emits `MeasurementReady`, wakes the
runtime, reruns the smallest affected Taffy subtree, and prepares another
frame. Completion is accepted only when request ID, measurement key, node
generation, content/style revisions, and environment epoch still match.
Deleted nodes cancel outstanding requests. The engine bounds immediate
measure/layout rounds and diagnoses oscillation for unchanged inputs.

The same resolved style snapshot used in measurement must be used to present
the accepted content. A `PreparedContentId` can bind a shaped glyph object,
decoded image metadata, or native layout object to that exact result and avoid
doing expensive work twice.

### Element policies

- `Text`: Host text shaping and line breaking; synchronous on primary Hosts.
- `TextInput`: Host control/text metrics; may use a lightweight prototype or
  platform text API rather than the live view.
- bundled image with known dimensions: `ReplacedContent` metadata; no Host FFI
  is needed.
- network/custom image: explicit dimensions or aspect ratio are preferred.
  A module may opt into pending intrinsic size with an explicit placeholder
  policy.
- `GoogleMap`, `WebView`, and video: normally have no useful intrinsic size;
  they require explicit, flex, or parent-constrained geometry. Resource
  readiness does not redefine their viewport unless the schema says so.
- a measuring container: unsupported in version 1 to prevent Host/Taffy child
  feedback cycles.

## Events, native state, and commands

Host events are node-scoped, typed, and include the node generation. Core owns
logical capture/bubble propagation and listener lifetimes. A native control
may perform platform gesture recognition, scrolling, text editing, or
accessibility actions locally, then report the corresponding typed state/event
through the surface event sink.

Host callbacks never synchronously re-enter arbitrary Rust application code
from inside `present`. They enqueue an event and request/wake the runtime.
Native objects are created, mutated, and destroyed on the Host UI thread.
Background resource work posts completion back to that boundary.

### TextInput reconciliation

IME composition, selection, caret movement, and rapid typing cannot be
round-tripped as naive controlled-value assignments. Every Host editing event
carries a monotonically increasing `host_state_revision`. A Rust value update
back to the Host identifies the revision on which it was based. The Host:

- ignores an update older than its accepted editing revision;
- applies a newer authoritative value without corrupting active composition;
- reports composition and selection changes explicitly;
- clears pending state when the node generation changes or is deleted.

The exact conflict policy is part of the typed editable-text contract, not
a generic `Prop("value")` convention. This prevents stale frames from
overwriting characters entered after Rust began producing them.

### Scroll state and gesture arbitration

`ScrollView` has a bounded viewport box and a distinct content extent. Taffy
lays out its content with an unbounded constraint only in the configured
scroll axis; the cross axis remains constrained. A scroll viewport without a
bounded size is a diagnostic.

The native scroll control owns transient offset, velocity, inertia, overscroll,
and platform gesture arbitration. It reports versioned scroll state to core so
hit testing, queries, events, sticky behavior, and accessibility observe a
coherent offset. Programmatic scroll is an ordered node command. Long lists
require a separate virtualized collection element; `ScrollView` does not imply
virtualization and must not eagerly materialize an unbounded data set by
accident.

## Performance contract

The module abstraction must not restore a bridge-shaped hot path:

- registry and canonical-name resolution happen at bootstrap;
- hot operations use compact element/property/event/command IDs;
- one changed frame crosses the Host boundary as one transaction;
- intrinsic requests and Host events are batched where latency permits;
- strings and variable payloads live in packet tables rather than one object
  allocation per property;
- Desktop passes borrowed typed data directly in Rust;
- WASM presents a borrowed linear-memory view for the duration of the call;
- the runtime ABI may use C-compatible tables/JNI arrays without
  JSON or per-property reflection;
- wrapper elision and lazy Host content creation are permitted behind the
  semantic model;
- a module callback is not invoked for every common style property.

Synchronous calls are allowed where correctness requires a result before
commit, especially text measurement, but they must be batched and must not
perform network or unbounded work. A provider that cannot meet this requirement
uses `Pending`.

## Lifecycle and failure

A module instance normally has Application or Process scope. An element
factory registration has surface availability; an element instance has node
scope. They are not the same lifetime.

Deleting a node must:

- detach it from presentation and content hierarchies;
- release or pool its Host objects;
- remove native observers and event subscriptions;
- cancel or invalidate pending measurements and command results;
- release prepared content and external surfaces;
- resign focus/IME and pointer capture safely;
- prevent stale callbacks from targeting a reused ID.

Unsupported targets fail during build composition. A schema/factory mismatch
fails during bootstrap. An unsupported style/content combination discovered
at runtime rejects the frame or emits a structured capability error according
to whether the capability was declared optional. Silent no-op behavior is not
allowed for required semantics.

## Prior-art and failure-mode review

This section is normative where it records a Whisker decision. It compares the
design against problems already exposed by Lynx and React Native rather than
claiming API compatibility with either project.

### Expo Modules

Expo modules keep their Kotlin and Swift `ModuleDefinition` implementations in
the native package and identify the module exposed to JavaScript by name.
Autolinking discovers packages and native module classes from package metadata;
it does not synthesize the native implementation. The autolinking CLI is in
turn invoked by Gradle and CocoaPods as part of their build integration. This
is the precedent for Whisker's independently compilable Host declarations and
native-build-owned discovery. See [Expo Autolinking](https://docs.expo.dev/modules/autolinking/),
the [module configuration format](https://docs.expo.dev/modules/module-config/),
and the [Module API](https://docs.expo.dev/modules/module-api/).

Whisker differs because the application side is Rust and its frame protocol
must stay numeric. It therefore adds an explicit bootstrap negotiation from
independently declared strings to compact IDs. Expo's package discovery is
analogous to KSP/Swift provider generation; it is not a reason for Rust schema
generation to become a prerequisite of native compilation.

### React Native Fabric

React Native's current renderer separates render, commit/layout, and UI-thread
mounting. Host-dependent `Text` and `TextInput` measurement participates in
layout, while atomic Host mutations are applied on the UI thread. Whisker keeps
the same important separation: the Rust scene and Taffy result are prepared
before one Host transaction, and native objects are mutated only by the Host
UI thread. See [Render, Commit, and Mount](https://reactnative.dev/architecture/render-pipeline)
and the [Threading Model](https://reactnative.dev/architecture/threading-model).

Fabric Native Components use a typed specification and Codegen for props,
events, and commands. Whisker deliberately chooses a different dependency
trade-off: Host modules compile independently and negotiate names to Rust IDs
at bootstrap, so Android Studio and Xcode never wait for a Rust-generated
public contract. See the
[Fabric Native Components introduction](https://reactnative.dev/docs/next/fabric-native-components-introduction)
and [native commands](https://reactnative.dev/docs/next/the-new-architecture/fabric-component-native-commands).

React Native's legacy serialized bridge became a bottleneck for frequent and
large updates. Whisker therefore does not use the friendly dynamic
`ModuleDefinition` dispatch as its frame transport. Bootstrap builds a typed
registry and the hot path stays numeric and batched. See React Native's
[New Architecture rationale](https://reactnative.dev/blog/2024/10/23/the-new-architecture-is-here).

Fabric flattens layout-only views during tree diffing. That demonstrates both
the performance value of avoiding unconditional wrappers and the need to make
flattening sensitive to paint and interaction props. Whisker's logical
presentation/content split permits conservative fusion, but background,
opacity, clipping, native refs, accessibility, and events are materialization
barriers unless exact equivalence is proven. See
[View Flattening](https://reactnative.dev/architecture/view-flattening).

React Native treats nested text as an inline attributed-text context rather
than ordinary flex children. Whisker v1 therefore omits `Children` from the
`Text` component and does not hide inline layout behind ordinary scene nodes. See React Native's
[Text container model](https://reactnative.dev/docs/text).

React Native requires network images to have dimensions instead of changing
layout after download, explicitly avoiding layout shift. Whisker permits
deferred intrinsic image sizing only as an opt-in schema policy with a declared
placeholder/retain/block behavior; explicit dimensions or aspect ratio remain
the default. See React Native's [Images guide](https://reactnative.dev/docs/images).

React Native's `ScrollView` requires bounded height and renders every child,
which makes it unsuitable as an implicit virtualized list. Whisker records
both constraints in the element contract instead of leaving them as usage
folklore. See the [ScrollView guide](https://reactnative.dev/docs/scrollview).

React Native editing events include an event count and expose Host-owned
selection/composition behavior. Whisker goes further by making Host state
revision part of the editable-text reconciliation contract. See the
[TextInput event API](https://reactnative.dev/docs/next/textinput).

### Lynx

Lynx custom elements separate the native UI object from a `ShadowNode` measure
provider. Its documentation limits custom intrinsic measurement to leaf
components and can pass prepared extra data from measure/layout to UI. Whisker
adopts the same safety constraints as a Host-independent measurer plus optional
`PreparedContentId`, while keeping Taffy and validation in Rust. See
[Lynx Custom Element](https://lynxjs.org/next/guide/custom-native-component.html).

Lynx's custom element API has distinct registration, property, event, method,
layout, and measurement hooks. That supports Whisker's separation of schema,
factory, commands/events, and measure provider. Whisker deliberately replaces
runtime string dispatch with Rust-assigned IDs in frequent paths.

Lynx supports event work on both main and background scripting threads and
warns that excessive main-thread handlers make the main thread busy. Whisker
does not initially expose arbitrary priority execution from module callbacks:
Host controls run locally, callbacks enqueue typed events, and Rust application
work runs at an explicit runtime boundary. Expensive work is delegated to
ordinary Rust background execution and wakes the Host loop when complete. See
[Lynx Event Handling](https://lynxjs.org/guide/interaction/event-handling).

### Risk audit

| Risk | Severity | RFC decision / required check |
|---|---:|---|
| element x style implementation explosion | blocker | common presentation owns box/composite styles; schemas select fixed channels |
| wrapper hierarchy cost | high | logical wrapper only; conservative fusion/flattening with materialization barriers |
| dynamic wrapper insertion loses control state | high | realization is stable for a node lifetime in v1; native controls/external surfaces start wrapped |
| native surface cannot clip/transform correctly | high | explicit external-surface capability matrix; reject unsupported required combinations |
| measure needs a view that does not exist yet | blocker | separate pre-mount measurer; no live Host element parameter |
| async measure returns stale data | high | key + request ID + node generation + content/style/environment revisions |
| measure/layout feedback oscillates | high | leaf-only v1, bounded rounds, identical-input oscillation diagnostic |
| measurement and paint shape text differently | high | one resolved snapshot and optional prepared-content identity |
| Text treated as an ordinary container | high | leaf `Text` v1; future rich-text content protocol |
| controlled TextInput loses keystrokes/IME state | blocker | Host state revision and explicit composition/selection reconciliation |
| ScrollView has circular/unbounded geometry | high | bounded viewport, one unbounded content axis, separate offset state |
| native control steals pointer/focus semantics | high | explicit gesture/focus ownership and typed state events; conformance tests |
| platform implementations drift | high | Rust-owned registration, strict bootstrap negotiation, and shared scenario IDs |
| dynamic module API leaks into frame hot path | high | numeric typed packets; dynamic API limited to compatibility/cold service paths |
| third-party CSS extensions break global semantics | medium | closed common registry; custom visuals are typed element props |
| background work re-enters UI/runtime unsafely | high | enqueue/wake boundary; Host UI objects remain UI-thread-affine |
| node deletion leaks native resources/callbacks | high | generation checks and normative teardown checklist |
| Web browser layout becomes second authority | high | Taffy authoritative; batch measure reads separately from DOM writes |
| built-ins gain hidden privileges over modules | medium | same registry, IDs, frame ops, events, lifecycle, and conformance harness |

### Review conclusion

The architecture is feasible, and its central presentation/content split is a
sound way to avoid combinatorial styling implementations. It is not safe if
“same treatment” is interpreted as “same capabilities”: containers, leaf
content, editable controls, scroll containers, text, replaced content, and
external surfaces need distinct closed semantic channels.

The following changes are required by this review and are incorporated above:

1. intrinsic measurement is pre-mount and separate from the live Host factory;
2. custom measurement is leaf-only in version 1;
3. `Text` is leaf-only until an explicit inline-run protocol exists;
4. TextInput uses revisioned Host state rather than naive controlled props;
5. ScrollView has a bounded viewport/content-extent contract and does not
   imply virtualization;
6. wrapper elision is semantics-aware and external surfaces negotiate
   composition capabilities;
7. Rust registration owns wire identity; Host strings are validated and bound once;
8. common style remains closed and element-specific visuals remain props.

With those constraints, no architectural blocker was found. The largest
implementation risks are native external-surface composition, IME correctness,
and keeping measurement/presentation text snapshots identical. These require
target conformance tests before a Host can claim support; they do not require a
different top-level architecture.

## Test strategy

### Rust core tests

- built-in and third-party schemas normalize through the same registry;
- duplicate keys and registration/member-ID mismatches fail bootstrap;
- property applicability is derived from closed channels;
- `CreateNode` type IDs, property IDs, event IDs, and command IDs are stable
  within a registry epoch;
- measurement batching, cache keys, pending policy, stale completion, and
  oscillation limits;
- resolved text style is identical between measure and frame content;
- TextInput and scroll Host-state revisions reject stale updates;
- node deletion invalidates every pending handle and callback;
- recording Host tests verify no per-style module dispatch occurs.

### Host-only tests

Each Host has a standalone scenario runner that mocks frame packets,
measurement requests, and the Rust event sink. Shared scenarios cover:

- background/border/clip/opacity on View, Text, TextInput, Image, and a mock
  custom native element;
- wrapper fusion and forced materialization producing equivalent pixels,
  geometry, focus, hit testing, and accessibility;
- font-size inheritance delivered to Text and TextInput measurement/content;
- Ready/Pending measurement and prepared-content reuse;
- native input composition, selection, stale value frames, focus, and teardown;
- ScrollView bounds, offset events, gesture arbitration, and command ordering;
- an external-surface mock for z-order, transform, opacity, and clip capability
  failures;
- event capture/bubble and deletion during callbacks;
- schema mismatch diagnostics before the first frame.

The scenario IDs are shared across Android, iOS, Web, and Desktop. WPT-derived
box/style cases remain useful for common presentation; element-specific native
state cases are Whisker-owned because WPT does not define UIKit/Android control
behavior.

### Full-stack tests

A smaller matrix runs `render!` through core, real measurement, FramePacket,
Host presentation, and returned events. It proves wiring, while the larger
Host-only and Rust-only suites localize failures and allow target development
without the other side running.

## Migration

The first implementation slice represents built-in primitives as the ordinary
`whisker.ui` Rust `ElementModuleDefinition` and independent target Host
declarations. Desktop and Web built-ins contain only stable names and Host
factories; they do not import `view_element_binding()` or
`text_element_binding()`. The built-in `ElementTag` authoring mapping remains
private to core registration. A missing, duplicate, or unmatched factory fails
bootstrap before mounting application UI. `Page` is not part of this module.

Android and iOS use the same structure without a prebuild dependency. Each
mobile Host has a checked-in `BuiltInElementModule`: its ordinary
`@WhiskerModule` definition declares three named `View` factory contributions
wired to `WhiskerBuiltInElements`. The concrete `ViewGroup`, `TextView`,
`UIScrollView`, and `UILabel` behavior is ordinary Kotlin or Swift Host source.
Third-party native implementations use the same string-based DSL and can be
compiled, registered, and Host-tested without running Whisker tooling.

At runtime the Rust registration catalog is matched to these Host declarations
by name. That bootstrap assigns the Rust-side numeric property/event IDs to
the corresponding native callbacks, after which frames contain no dynamic
name dispatch. KSP and Swift code generation emit registration providers only;
they do not emit public element/property/event contracts or copy Rust schemas
into native source.

The retained mobile presentation wrapper is common to built-ins and custom
elements. It applies layout and paint, while the resolved factory supplies the
element-specific native View. Child acceptance is checked from the negotiated
registration and child placement remains a Host concern. A missing factory fails
frame application instead of silently producing an empty wrapper. The mobile
shell and registrar have no Lynx renderer dependency.

Android uses a concrete `WhiskerContainerView : ViewGroup` whose measurement
and placement policy is limited to applying Rust-computed geometry. It does
not use `FrameLayout` as an implicit second layout engine. `ScrollView` mounts
logical children into a dedicated multi-child content container. iOS uses the
same separation with `WhiskerContainerView` and a `UIScrollView` content view;
Web marks its built-in scroll factory and preserves native DOM scrolling when
applying clip updates. Desktop retains the same logical container/content
distinction in its scene projection.

The common mobile frame presenter decodes `SetText`, but it does not reference
`TextView` or `UILabel`. Text mutation and content-box placement are callbacks
owned by the handwritten Kotlin/Swift `whisker.ui/Text` Host implementation,
matching Desktop and Web's element-specific content implementation boundary.

`whisker-toggle` is the first end-to-end native fixture. Its Rust declaration
owns the application-facing `whisker.toggle/Toggle`, `checked`, `disabled`,
and `change` schema; Android and iOS independently declare matching strings,
the target view implementation, and callbacks. Component commands remain on
the existing Whisker module function/handle path until the negotiated command
channel is implemented.

1. Use the string-matched `ModuleDefinition` contract and make native Host
   source compile without Rust-generated public bindings. KSP/Swift codegen
   only discovers and registers native declarations.
2. Negotiate the Rust `ElementRegistration` catalog against the Host catalog
   at bootstrap, fail on drift before the first frame, and compile the result
   to numeric callback tables.
3. Move Cargo compilation and optional schema export under declared
   Gradle/Xcode build tasks. Direct `./gradlew assemble` and `xcodebuild` must
   be first-class test paths; `whisker run/build` only orchestrate them.
4. Register built-in `View`, `Text`, and `ScrollView` through the same
   checked-in Host declarations as third-party providers.
5. Split common Host presentation from content factories and retain View/Text
   parity tests on all four Hosts.
6. Change the conceptual/live factory measurement API to the pre-mount
   measurer and preserve the current protocol's prepared-content path.
7. Add `Image`, `TextInput`, and `ScrollView` one at a time with their
   measurement/state contracts and shared Host scenarios.
8. Add a mock third-party native element, then a Google Maps proof of concept
   including CNG dependencies and capability failures.
9. Use compact IDs and frame batches for frequent updates while sharing the
   `WhiskerValue` data model with service modules and every Host language.
10. Add conservative wrapper fusion only after the non-fused implementation
   passes the same semantics and visual tests.

The mobile implementation now realizes steps 2, 4, 5, 6, and 9 at the ABI
boundary. Registration is a dedicated attach-time bootstrap callback rather
than snapshot metadata. Frame headers and operations are fixed-layout borrowed
C records; property values, element events, module arguments/results, and
module events all use `WhiskerValueRaw`. Android's JNI adapter converts those
records to typed Kotlin calls without a textual intermediate, while Swift
reads the same C records directly. Both Hosts batch intrinsic measurement,
perform real native text measurement, route custom requests to the bound
factory measurer, and stage/validate a complete frame before commit. ABI and
protocol major mismatches fail attach or frame application before native scene
mutation.

No Lynx migration adapter or `SurfaceEngine` compatibility layer is introduced
solely to preserve the old architecture. The temporary hard-coded built-in
path was removed directly rather than standardized.

## Invariants

1. Core owns one logical scene, style resolution, Taffy layout, frame
   revisions, measurement coordination, and event propagation.
2. Internal crate or Rust-trait boundaries are not automatically modules.
3. Built-in and third-party elements use the same registry and wire protocol.
4. Public `ModuleDefinition` does not expose Rust `Trait` terminology.
5. Every visual element has common box presentation independent of its content.
6. Element providers do not implement background, border, layout, or common
   compositing property resolution.
7. Physical wrappers are optional only when observable semantics are equal.
8. Common style is closed; module-specific behavior uses typed props.
9. Text-style consumers receive resolved inherited values, including
   `font_size`, through a dedicated typed channel.
10. Measurement can run before Host element creation and is leaf-only in v1.
11. Measurement and presentation use matching style/content revisions.
12. Host UI mutation is UI-thread-affine; callbacks enqueue rather than
    re-entering application code during presentation.
13. Frame, measurement, and frequent event traffic is typed and batched.
14. Unsupported required target or composition behavior fails visibly.
15. Node generation protects every event, command, measure, and Host resource
    from stale reuse.
16. A hand-written Host module compiles without Rust-generated Swift/Kotlin
    source; string/schema drift is a bootstrap error.
17. KSP/Swift code generation is limited to private discovery/registration and
    does not emit public Host contracts or Rust schema copies.
18. Gradle and Xcode own native build graphs and may invoke Whisker helper
    tools as declared tasks/phases.

## Deferred details

The following do not change the accepted module boundary but need later RFCs
or implementation decisions:

- the minimum external-surface composition profile required of every Host;
- the rich-text run/tree protocol and inline attachment model;
- exact editable-text conflict behavior during active marked-text composition;
- the common scroll-state record, nested scrolling contract, and gesture
  priority model;
- criteria and state-transfer protocol for any future in-place wrapper
  realization change;
- the initial cross-platform accessibility role/action schema;
- whether any custom container measurement use case justifies a cycle-safe
  version 2 contract.
