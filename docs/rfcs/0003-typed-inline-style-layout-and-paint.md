# RFC 0003: Typed Inline Style, Layout, and Paint

- Status: Draft
- Authors: Whisker maintainers
- Created: 2026-08-18
- Discussion: TBD
- Tracking issue: TBD
- Depends on: [RFC 0001](0001-runtime-modules-and-build-plugins.md),
  [RFC 0002](0002-renderer-interface-and-frame-protocol.md)
- Supersedes: None

## Summary

Whisker exposes one typed, inline `style:` input on every visual element. The
existing function-call-style `render!` DSL and `css!` macro remain the
authoring API:

```rust,ignore
render! {
    View(style: css!(
        flex_direction: FlexDirection::Column,
        padding: px(16),
        background_color: theme.surface,
    )) {
        Text(style: css!(font_size: px(18)), value: "Settings")
    }
}
```

Whisker does not expose stylesheets, selectors, specificity, `!important`, or
a general CSS cascade. Reuse and conditional styling use ordinary Rust values,
functions, composition, components, and signals. A small fixed set of text
properties inherit from parent nodes when a child does not specify them.

The Rust `whisker-style` subsystem resolves typed input into layout, paint,
compositing, text, and semantics data. Taffy computes layout. Rust motion
updates the same typed property slots. The renderer protocol sends only
changed resolved properties and geometry; the Host implements native text
shaping and concrete painting with Android Views, UIViews, or DOM nodes.

Supporting the styling surface of Lynx means covering its useful visual
properties and values, not preserving CSS text syntax or browser cascade
semantics.

## Motivation

Replacing Lynx requires replacing more than flexbox. Lynx currently supplies
style parsing and normalization, property applicability, inheritance, text
inputs, transforms, clipping, backgrounds, borders, filters, stacking,
overflow, scrolling, animation-related values, and the mapping from those
values to platform rendering.

A selector-based CSS implementation would add a parser, selector matcher,
specificity and cascade engine, invalidation rules, and global or scoped
stylesheet lifetime. Whisker's current applications instead attach typed
`css!` values directly to elements. Keeping that model:

- preserves the existing `render!` authoring style;
- makes invalidation local and predictable;
- rejects unsupported values at compile time where possible;
- avoids parsing CSS in Rust or in the Host hot path;
- makes scene, style, layout, and protocol behavior testable in Rust;
- keeps signal-driven and motion-driven updates on one property pipeline.

The design still needs a deliberate answer for the small amount of context
that text naturally receives from ancestors. Repeating `font_size` and `color`
on every nested text element is unnecessary, while inheriting layout or paint
properties would recreate surprising cascade behavior. This RFC therefore
defines limited text inheritance rather than a general cascade.

## Goals

- Preserve `render! { View(style: css!(...)) { ... } }` as the public form.
- Make all public common style properties typed.
- Define deterministic composition without selectors or specificity.
- Define a small, closed inherited text context.
- Separate specified, computed, and presentation values.
- Define which property changes invalidate measurement, layout, paint,
  compositing, or semantics.
- Keep Taffy as the authoritative layout engine on every interactive target.
- Make Host text measurement explicit, batched, cached, and revisable.
- Make Rust motion write through the same resolved-property pipeline.
- Cover Lynx's useful style, layout, and paint capabilities incrementally.
- Permit efficient Web rendering and SSR without making browser layout the
  semantic source of truth.

## Non-goals

- Parsing CSS source text at runtime.
- Supporting selectors, classes as styling hooks, specificity, `!important`,
  pseudo-classes, media queries, or global stylesheets.
- Exact compatibility with browser CSS cascade or Lynx CSS authoring.
- Letting each UI module define an independent layout or common paint system.
- Moving animation timelines to the Host.
- Requiring pixel-identical font rasterization across platforms.
- Fixing the final public names of every style type in this RFC.
- Defining the final binary encoding of resolved values; RFC 0002 owns the
  frame protocol information model.

## Authoring model

### Inline-only typed styles

Every visual scene node accepts at most one logical `style:` value. `css!`
constructs typed declarations; it does not produce CSS text in the new engine.
The existing `Css` name may remain for source compatibility, but its runtime
representation becomes typed property data.

```rust,ignore
fn card_style(theme: &Theme) -> Css {
    css!(
        padding: px(16),
        border_radius: px(12),
        background_color: theme.card,
    )
}

render! {
    View(style: card_style(&theme)) {
        Text(value: "Account")
    }
}
```

Raw `String` and `&str` conversion into `Style` is not part of the target API.
It may exist temporarily in the Lynx migration adapter, but the native engine
must not parse it. Removal should produce a migration diagnostic pointing to
typed `css!` properties.

The same `style:` slot accepts static and reactive values as it does today:

```rust,ignore
render! {
    View(style: computed(move || css!(
        opacity: if visible.get() { 1.0 } else { 0.0 },
        transform: translate_x(drag_x.get()),
    )))
}
```

This is ordinary signal authoring. A developer does not use a separate
"fast-animation style" API. Static declarations, computed signals, and
`whisker-motion` ultimately update identical typed property slots.

### Reuse and composition

Style reuse uses Rust rather than selector identity:

- functions returning `Css` or a typed style fragment;
- constants for values and statically constructible fragments;
- component variants represented by enums or ordinary arguments;
- a typed theme/context read by component code;
- explicit composition of fragments.

When fragments are composed, later declarations replace earlier declarations
for the same property. This is ordered map composition, not CSS cascade:

```rust,ignore
let style = card_style(&theme)
    .merge(if selected { selected_style(&theme) } else { Css::empty() })
    .merge(css!(opacity: opacity.get()));
```

The exact convenience methods may change, but these semantics must not:

```text
base fragment -> variant fragment -> local override
                                  last declaration wins per property
```

Composition is completed before inheritance and value computation. There is
no selector match, specificity comparison, origin, or `!important` stage.

### No public StyleSheet

There is no public `StyleSheet`, selector registration, or global style
registry. This avoids ambiguous ownership when several runtime modules or
embedded `WhiskerView` instances coexist.

The engine may internally intern equal immutable declaration sets and assign a
`StyleId`. The Web or SSR renderer may also emit an internal generated class
for repeated presentation data. These are storage and transport
optimizations, not observable selectors: application code cannot name them,
match descendants through them, or depend on their generated identifiers.

## Resolution model

The style pipeline has three representations:

```text
typed css! fragments / signals / motion
                  |
                  v
           SpecifiedStyle
                  |
       element applicability + fixed inheritance
       value normalization + environment resolution
                  v
            ComputedStyle
          /       |        \
     Taffy     text key   paint/composite
          \       |        /
                  v
          PresentationStyle
                  |
          RFC 0002 FramePacket
```

`SpecifiedStyle` stores only properties explicitly supplied after fragment
composition. `ComputedStyle` contains normalized values required for semantic
layout and rendering. `PresentationStyle` contains the current values after
motion and layout have been sampled for a frame, ready for dirty comparison
and protocol encoding.

The computed layout slice remains independent of Taffy. Absolute and relative
length units are normalized to logical pixels, while percentages and mixed
`calc()` expressions are retained as an affine `logical_px + fraction *
containing_block` value. The later `whisker-layout` module supplies the
containing-block constraint and maps these semantic values into its private
Taffy representation.

The conceptual interfaces are:

```rust,ignore
pub trait StyleResolver {
    fn resolve(
        &mut self,
        element: ElementTypeId,
        specified: &SpecifiedStyle,
        parent_text: &InheritedStyle,
        environment: &StyleEnvironment,
    ) -> ResolvedNodeStyle;
}

pub struct ResolvedNodeStyle {
    pub computed: ComputedStyle,
    pub inherited_for_children: InheritedStyle,
    pub invalidation: PropertyImpactSet,
}

pub struct PresentationStyle {
    pub layout: ResolvedLayout,
    pub paint: ResolvedPaint,
    pub composite: ResolvedComposite,
    pub text: ResolvedText,
    pub semantics: ResolvedSemantics,
}
```

These are information-level interfaces. Implementations should use dense
property storage, generated tables, bitsets, interning, and copy-on-write
values where useful rather than literal public structs with one field per
property.

### Limited text inheritance

Only the following properties inherit in version 1:

- `font_family`;
- `font_size`;
- `font_weight`;
- `font_style`;
- `line_height`;
- `letter_spacing`;
- `color`.

The initial inherited context is platform system font, `14px`, weight `400`,
`normal` font style, platform-normal line height, zero letter spacing, and
opaque black. A surface may provide a different root font size through its
`StyleEnvironment`; `rem` and an otherwise unspecified root `font_size` use
that value.

For each property, resolution is:

```text
explicit value on this node
    ?? computed value from the parent's InheritedStyle
    ?? engine initial value
```

The resulting seven computed values form the `InheritedStyle` passed to all
children, whether the current node itself draws text or is only a container.
This makes a parent's `font_size` or `color` the default for descendant text
without treating arbitrary properties as inherited.

Layout, box, background, border, opacity, transform, filter, clip, overflow,
pointer, and semantics properties do not inherit. Parent opacity and transform
still affect descendants through scene composition; that is geometric or
compositing ancestry, not inheritance.

Version 1 has no public `inherit`, `initial`, `unset`, or `revert` keywords.
Omitting a text property selects the inherited value. Explicitly specifying a
value overrides it. New inherited properties require a protocol-compatible
RFC change because they can invalidate entire subtrees.

### Element defaults and applicability

The common style registry declares, for each property:

- stable property ID and wire type;
- accepted Rust value types and normalization;
- initial value;
- applicable closed element categories;
- percentage and environment resolution rules;
- interpolation support;
- invalidation impacts;
- backend capability requirements.

An `ElementSchema` declares presentation, children, content, measurement, and
text-style-consumer categories as defined by RFC 0004. Its public module DSL
does not expose a Rust `Trait` list, and it does not repeat every common style
property. Invalid property/element combinations should be compile-time errors
when statically known and deterministic runtime diagnostics otherwise.
The seven inherited text properties are valid on containers because they also
define the inherited context for descendants, even when that container does
not paint text itself.

Element-specific behavior stays in the canonical element schema. For example,
`Video` uses common width, border, opacity, transform, and clip styles, while
`src`, `autoplay`, `muted`, and playback commands are element properties. A
third-party module that needs a truly custom visual parameter declares a typed
element property and Host binding; it does not extend a global style language.

## Invalidation and incremental updates

Every property has one or more impacts:

```rust,ignore
bitflags! {
    pub struct PropertyImpact: u8 {
        const INTRINSIC_MEASURE = 1 << 0;
        const LAYOUT            = 1 << 1;
        const PAINT             = 1 << 2;
        const COMPOSITE         = 1 << 3;
        const SEMANTICS         = 1 << 4;
    }
}
```

Examples:

| Change | Required work |
|---|---|
| `font_size`, `font_family`, `letter_spacing` | text measure, then layout and paint |
| `width`, `padding`, `flex_grow` | layout |
| `background_color`, `border_color` | paint |
| `opacity`, compositable transform | composite, unless backend capability requires paint |
| `overflow` or clip geometry | paint/composite and sometimes hit testing |
| accessibility visibility | semantics |

A signal update marks only the changed typed slots. Inherited text-property
changes walk only the affected descendant subtree and stop where descendants
explicitly override the property. Multiple writes before a frame collapse to
the last value.

Property impact is semantic, not a promise that every Host has the same fast
path. Capability negotiation may promote `COMPOSITE` to `PAINT`, but never
allows the Host to reinterpret a change with different layout semantics.

## Layout

Taffy is authoritative for box layout on Android, iOS, Web, and Desktop.
`whisker-style` produces renderer-independent `ComputedStyle`; `whisker-layout`
converts its layout slice into Taffy input and maintains a retained Taffy node
for each layout-participating scene node.

```text
ComputedStyle + child structure + intrinsic measurements
                         |
                         v
                       Taffy
                         |
                         v
          x, y, width, height, baseline, overflow data
                         |
                         v
              PresentationStyle / FramePacket
```

The Host does not independently choose flex or grid geometry. On DOM targets,
Whisker applies Rust-computed geometry to the managed subtree. CSS may be used
as the browser-facing encoding of paint, text, transforms, or explicit
geometry, but browser layout is not the semantic layout engine for an
interactive Whisker surface.

Property coverage should use Taffy's supported flexbox, grid, block, sizing,
alignment, position, aspect-ratio, gap, margin, padding, and border inputs. A
Lynx property that Taffy cannot represent requires an explicit engine
extension, preprocessing rule, or documented unsupported status; it must not
silently acquire target-dependent browser behavior.

The first retained implementation lives in the Rust-only `whisker-layout`
crate. Its public boundary consists of Whisker-owned `NodeId`,
`ComputedLayoutStyle`, viewport/measurement values, and `LayoutSnapshot`;
Taffy types remain private. The tree supports create, subtree removal,
reparent/reorder, style replacement, intrinsic-measure invalidation, and
viewport computation. Fractional logical coordinates are preserved so that
physical-pixel snapping remains a renderer concern.

Viewport computation uses an internal fixed-size flex-column `SurfaceRoot`
above the application root. The synthetic node supplies the viewport
containing block and normal flex-parent semantics, but is excluded from the
public retained tree and from every `LayoutSnapshot`. In particular,
`flex-grow: 1` on the application root fills the logical viewport while a root
with an explicit size keeps that size; percentage and absolute root geometry
resolve against the same viewport. Resize updates only the synthetic root's
constraints and then recomputes the user-authored subtree.

This first slice explicitly rejects, rather than approximates, backend gaps:
mixed non-zero length-plus-percentage values, intrinsic size keywords,
`flex-basis: content`, and fixed/sticky positioning. `linear` currently lowers
to flex and `relative` to block as migration-compatible layout modes. These
statuses are implementation milestones, not reductions of the RFC's Lynx
coverage goal; each rejection must later be replaced by a faithful Taffy
extension/preprocessing rule or remain recorded in the coverage registry.

## Text and intrinsic measurement

The Host owns shaping, fallback font selection, line breaking, glyph metrics,
and platform-native control intrinsic size. Rust owns the constraints under
which measurement occurs and the final box layout.

The measurement key includes all inputs that can affect the result:

```rust,ignore
pub struct TextMeasureKey {
    pub content_hash: u64,
    pub text_style: ResolvedTextStyle,
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
    pub scale: f32,
    pub font_environment_epoch: u64,
    pub locale_epoch: u64,
}
```

Measurements return width, height, and baselines. Requests are deduplicated,
batched, cached, and tagged with the scene and environment epochs as required
by RFC 0002.

Android and iOS may answer synchronously within the renderer binding when safe.
Web calls its JavaScript Host from WASM in the same event-loop turn and batches
browser text measurement. Desktop calls its native Rust Host directly; the
Host returns a measurement and `PreparedContentId` derived from the same
shaped/wrapped content that it later paints. The language does not erase the
Host boundary: only measurement protocol values cross into Whisker core, while
font databases, glyph runs, and raster resources remain in
`platforms/desktop`. Neither path requires native-process IPC. If a font,
image, or custom control is not ready, the Host returns a pending result. Rust
may present a provisional layout and relayout when the resource epoch changes.
The scheduler coalesces that correction into the next frame and prevents stale
responses from applying.

Exact prevention of visible correction for unavailable fonts is impossible on
any backend. Applications can preload fonts, reserve explicit dimensions, or
accept one resource-driven relayout. This is distinct from adding an
asynchronous IPC hop to every text measurement.

## Paint, clipping, and stacking

Rust resolves the semantic values for:

- backgrounds and background images;
- borders, radii, outlines, and shadows;
- text paint and decoration;
- opacity and blend/compositing intent;
- transforms and transform origin;
- overflow and clip shapes;
- the single `backdrop-filter: blur(<length>)` extension;
- stacking order and isolated stacking contexts;
- visibility and hit-test participation.

For the common transform subset, specified function lists and
`transform-origin` remain typed computed values until layout is available.
The surface resolves percentage translations against border-box width or
height, resolves the origin against that same box, composes the functions in
CSS matrix order, and emits one `SetTransform` matrix. A Host applies that
matrix at its local border-box origin and does not repeat CSS resolution. This
slice accepts 2-D transforms plus 3-D functions and projective `matrix3d`
values applied to the node's flat local plane. Hosts flatten at each node;
Android derives the exact density-adjusted 3-by-3 homography for that `z = 0`
plane. In accordance with Lynx, `perspective` affects the current node rather
than its children: Rust resolves the length, clamps values below one logical
pixel for rendering, prepends the projection, and emits the same canonical
flat-plane matrix. Browser-style parent perspective, `perspective-origin`, and
shared `preserve-3d` descendant spaces are outside the current subset. Lynx
motion-path positioning is lowered through that same matrix operation: Rust
adaptively measures absolute `path()` lines, Bezier curves, and SVG elliptical
arcs (`M`, `L`, `Q`, `C`, `A`, and `Z`) and border-box-relative
`circle()`/`ellipse()`/`inset()`, clamps
normalized `offset-distance`, derives the analytic tangent for
`offset-rotate: auto` or uses a fixed angle, composes rotation before the
ordinary transform list, and applies path translation after transform-origin.
Circle percentage radii use the normalized box diagonal; ellipse percentages
resolve against their corresponding width or height axis. Rust applies those
semantics on every Host, including iOS where Lynx omits `ellipse()`. Rounded
inset paths use CSS overlap normalization and the standard top-edge clockwise
starting point, avoiding the different defaults of the native path libraries
used by Lynx Android and iOS. SVG arcs use the standard endpoint-to-center
conversion in Rust, including radii correction, absolute negative radii,
zero-radius line fallback, and omission of a segment whose endpoints coincide.
Rotated arcs use adaptive length measurement and an analytic tangent; Hosts do
not interpret their flags or geometry.

Protocol minor 1 groups those resolved meanings by Host capability rather
than by CSS spelling. `SetBackgroundLayers` carries resource-backed images,
linear/radial/conic gradients, positioning, sizing, repeat, origin, clip,
attachment, and layer blending. `SetVisualEffects` carries outlines, ordered
box shadows, normalized basic/path clips, mask layers, one backdrop blur, blend
and isolation intent, back-face visibility, and 3-D descendant behavior.
`SetText` paint includes decoration lines/style/thickness and ordered text
shadows; general `filter` functions and filter chains are deliberately out of
scope. `SetImage` and `SetCursor` carry replaced content and cursor resources.
All resource references use `ResourceId`; platform image objects, decoded
pixels, paths, and GPU handles never enter the common protocol.

Backdrop blur is negotiated independently from the remaining
`visual-effects` group. A Host may therefore support shadows, clips, opacity,
or compositing without falsely claiming it can sample content behind an
element. The initial profiles advertise backdrop blur as native on Web when
`CSS.supports("backdrop-filter", "blur(1px)")` succeeds, native on Desktop,
native on Android API 31 and later, emulated on iOS, and unsupported on older
Android versions.

Resource acquisition is a separate, non-frame channel. `ResourceCommand::Load`
binds a `ResourceId` and monotonic generation to a URL, bundled asset, or
one-time byte payload; `ResourceEvent::Ready` or `Failed` completes that exact
generation, and `ResourceCommand::Release` ends its Host lifetime after no
accepted frame references it. Ready events may carry intrinsic dimensions and
therefore schedule measurement invalidation and a later frame. Encoded image
bytes are never repeated in `FramePacket`.

The resource channel is implemented as a typed, non-frame runtime boundary.
Desktop and Web pass the protocol enums directly. Android and iOS use mobile
ABI 2.9's borrowed `WhiskerMobileResourceCommand` and
`WhiskerMobileResourceEvent`; each Host copies URL, asset, MIME, byte, and
diagnostic payloads during the callback before starting asynchronous work.
All four Hosts return generation-tagged completion to `RuntimeInstance`, which
retains current completion state, ignores replaced or released generations,
and wakes a running runtime only for a current Ready or Failed event. JSON and
encoded resource bytes never enter the frame transaction path.

Style lowering owns resource identity rather than assigning an ID to each
element slot. Equal `(ResourceKind, ResourceSource)` values share one
`ResourceId` across the surface, so repeated uses of the same URL issue one
load and one Host decode. A pending or failed generation is omitted from paint
operations; only a current `Ready` generation may be projected as
`PaintImage::Resource`. Removing the last reference first emits and commits the
frame that clears that resource from retained paint. Only after the Host
accepts that frame may Rust enqueue `ResourceCommand::Release`. A sink error or
`NeedSnapshot` therefore cannot release an object that the Host may still
reference. Reacquiring the source before that acceptance cancels retirement;
reacquiring it after release allocates a fresh ID. Automatic style lowering
never changes the content behind an ID that an accepted frame may reference,
because frame paint references intentionally contain only `ResourceId`, not a
generation. Generations remain available to the lower-level resource channel
for cancellation and stale-completion rejection before publication.

The first typed style slice applies this lifecycle to URL-backed
`background-image`. URL text remains unresolved in Rust and is passed as
`ResourceSource::Url`, leaving relative-URL bases, network policy, caching, and
decoding to the Host resource service. Background resources do not influence
Taffy layout; intrinsic dimensions returned by the Host are retained for
future replaced-content consumers but do not resize a CSS background.

Host-independent background geometry is lowered symmetrically for two-axis
explicit size, affine position, all repeat modes, border/padding/content origin
and clip boxes, and scroll attachment. Resource-backed images additionally
support intrinsic `auto`, one-axis `auto`, `cover`, and `contain` on all four
Hosts. Their natural dimensions remain in each Host resource cache: the frame
continues to identify the image only by `ResourceId`, and the receiver resolves
the final tile size from that cached metadata. This avoids duplicating mutable
resource facts in `FramePacket` while keeping the geometry operation semantic.
Gradients have no intrinsic dimensions, so only their initial `auto` and
two-axis explicit sizes are currently accepted. When `round` changes exactly
one tile axis of a resource-backed image and the opposite explicit-size axis is
`auto`, every Host rescales that opposite axis to restore the intrinsic aspect
ratio after rounding, as required by CSS Backgrounds. Fixed/local attachment,
text clipping, and non-normal blend modes remain deferred. Unsupported
combinations are rejected transactionally rather than silently approximated.

Adding the semantic value does not declare a Host implementation complete.
Until a Host advertises and implements the corresponding capability, its
receiver rejects the operation before mutating retained state. It must not
accept an unknown or protocol-only operation as a successful no-op. The
machine-readable implementation state lives in
`tests/host-conformance/capabilities.json`; `StyleProperty::domain()` keeps
every registry entry assigned to layout, motion, or one of these protocol
capabilities without creating a per-property Host dispatch layer.

### Typed custom properties

Whisker supports custom properties as inherited, case-sensitive semantic
values. A declaration stores a typed `StyleValue`, not an unparsed CSS token
stream. `var()` references are resolved in Rust before layout, measurement,
motion sampling, or paint lowering, so custom-property names and unresolved
references never cross the Host protocol boundary.

Whole-value references, typed fallbacks, forward references, cycle
invalidation, and `calc()` operands are supported. Typed references may also
occupy the color, angle, number, and length components of supported composite
values such as gradients, transforms, text paint, background shorthands, and
backdrop blur. If substitution produces a value incompatible with the
consuming grammar slot, that declaration alone becomes invalid at computed
value time; unrelated declarations and the frame remain valid.

Arbitrary CSS token-stream custom-property values are intentionally outside
the core. Supporting them would require retaining text, substituting tokens,
and reparsing each consuming property at runtime, contrary to the typed
inline-style architecture and footprint goals. A future optional stylesheet
compatibility layer may translate token streams into the same typed specified
values before they enter the runtime.

The Host maps these values to platform objects and drawing APIs. It does not
rerun style resolution. A renderer capability table classifies each feature as
native, emulated, or unsupported. Required capabilities are validated when the
surface binds, and unsupported dynamic values produce deterministic errors
rather than silent visual changes.

The Rust boundary exposes this table as `RenderCapabilities`. Optional
semantic groups include `elliptical-border-radius`, `background-layers`,
`visual-effects`, `backdrop-blur`, `text-effects`, `text-typography`,
`image-content`, `cursor`, and `resource-lifecycle`.
`FramePacket::required_capabilities()` derives the dynamic subset from semantic
operations. An omitted entry means unsupported; Hosts preflight the complete
packet before changing their retained projection. Native and emulated support
are distinct so conformance can require equal output without pretending every
platform uses the same rendering primitive.

Paint order is derived from the Rust scene, stacking-context rules, and
resolved order. A backend may realize it with native child order, layers, DOM
stacking, or a custom drawing primitive as long as the observable order,
clipping, hit testing, and accessibility relationship conform.

## Motion and gestures

Animation is owned by Rust. There is no Android animator, Core Animation, Web
Animation, or CSS Animation offload in the semantic model.

`whisker-motion` owns timelines, easing, keyframes, springs, decay, gesture
handoff, interruption, and cancellation. On each Host VSync,
`requestAnimationFrame`, or native Desktop frame callback it samples active
values, writes changed typed property slots, runs required layout at most once,
and emits at most one frame packet for the surface.

```text
Host VSync/rAF/Desktop frame
  -> Rust scheduler
  -> sample motion and gesture state
  -> update typed property slots
  -> measure/layout if impacts require it
  -> dirty comparison
  -> one FramePacket
  -> Host apply
```

An ordinary signal and a motion value therefore have the same destination and
can use the same `css!` declaration. Motion gets a scheduler fast path and
dense dirty storage internally, not a different application-facing style
language.

The engine may optimize compositable properties so a packet contains only
`SetOpacity` or `SetTransform`. It must not transfer ownership of the animation
timeline to the Host. Layout-affecting animation is supported but costs Rust
layout; documentation and diagnostics should distinguish it from composite-
only animation.

Typed equivalents of Lynx transition, keyframe, and imperative animation
capabilities may be convenience constructors over `whisker-motion`. They do
not require CSS parsing and must obey the same interruption and gesture
handoff semantics as direct motion APIs.

## Web Host and DOM renderer

Web uses a Whisker-owned JavaScript Host and DOM renderer. It does not include
the native Desktop renderer. Whisker Rust runs as WASM in the browser;
JavaScript enters WASM from `requestAnimationFrame`, and the renderer applies
the returned typed DOM deltas before that callback returns.

The DOM backend may cache normalized style applications, intern repeated
values, use generated classes for immutable repeated paint data, and apply
transforms or opacity through browser compositing. It must treat those as
backend implementation details. Dynamic properties are sent as typed deltas;
Whisker does not serialize the whole style or whole tree per frame.

DOM nodes belonging to a `WhiskerView` must be isolated from outer-page style
influence. The backend should use a shadow root, reset boundary, or equivalent
generated rules so external selectors and inherited page CSS cannot change
Whisker's computed semantics. Host-owned focus, IME, accessibility, and native
text behavior remain available through that boundary.

## Desktop native Host

Desktop uses a Whisker-owned native Rust Host rather than the Web Host, DOM,
WebView, or another application UI framework. The generated native executable
links the application, Whisker runtime, and Host in one process and mounts one
`DesktopSurface` in a native window or embedded region. Taffy remains
authoritative for all Whisker layout; the window system supplies only the
outer viewport and scale.

The Desktop Host lives below `platforms/desktop`. It accepts common
`FramePacket`s through a direct typed Rust `FrameSink` call and stores an
accepted Host projection containing render nodes and prepared resources. It
does not retain signals, style declarations, Taffy nodes, components, or the
runtime scene. Common paint meanings remain in `whisker-protocol`; quads,
paths, glyph runs, texture handles, atlases, command buffers, and native window
objects remain Desktop-only types.

The same boundary applies in the reverse direction. A Desktop text provider
implements `MeasurementProvider`, shapes and wraps text using its native font
environment, retains the prepared glyph content under a
`PreparedContentId`, and returns only protocol metrics to the engine. The
engine can then finish Taffy layout and include that handle in the final paint
packet without knowing the Host representation.

Presentation is implemented from focused low-level Rust facilities for GPU
access, shaping/rasterization, atlas allocation, paths, and accessibility.
Those common dependencies and the common `winit` window/event shell are owned
by `platforms/desktop`; Whisker core never depends on any Desktop Host crate.
The Desktop paint pass lowers the accepted Host projection
into quads, shadows, paths, glyph/image sprites, clips, layers, and external
surfaces, then submits GPU work. `platforms/macos`, `platforms/windows`, and
`platforms/linux` are symmetric OS-named target interfaces over that common
Host and the seams for genuine native-only integration; they do not fork the
common lifecycle, scene, or renderer.

Host source trees are organized by semantic capability rather than CSS
spelling: measurement/text, paint/box, paint/text, clip, transform,
compositing, image/effects, input, and accessibility. Android, iOS, Web, and
Desktop use the same logical groups and operation names even when their
implementation languages differ. Width, flex, margin, and other layout-only
properties have no corresponding Host property handler because Rust has
already resolved them to geometry. Shorthands and logical properties likewise
never cross the Host boundary.

This symmetry is an audit and testing property, not a reason to add runtime
indirection. Desktop keeps an exhaustive static operation dispatcher and
data-oriented batches within one crate. Hosts must not allocate a polymorphic
handler per CSS property or route every draw through an OS adapter.

The Desktop capability profile must track group opacity/compositing, general
transforms, rounded or path clips, filters, blend modes, hierarchical
accessibility, and external media surfaces. Missing required capabilities are
explicit conformance gaps, not permission to change style semantics. GPUI is
not used as the Desktop framework or renderer dependency.

## SSR and hydration

Inline-only typed authoring does not prevent SSR. A Rust renderer can traverse
the resolved scene and emit HTML plus browser-facing presentation. It may use
inline declarations or internally generated, content-addressed classes to
deduplicate repeated values. Neither form exposes a public stylesheet model.

Taffy remains the semantic layout engine for both SSR and interactive Web.
When the server has deterministic intrinsic measurements, it can emit resolved
geometry. When platform fonts or viewport facts are unavailable, SSR uses
declared fallback metrics or emits unresolved/provisional geometry. Hydration
then measures in the browser, updates Taffy, and sends only the resulting
deltas.

Consequently SSR is possible, but perfectly stable first paint requires the
same fonts, environment inputs, and measurement model on server and client.
This is an explicit SSR fidelity constraint, not a reason to let browser CSS
layout replace Taffy during normal execution.

Hydration identity is based on stable scene/node markers and application
state, not generated class names. The exact HTML format, streaming model, and
hydration protocol remain a later RFC.

## Styling coverage

The conformance target has two normative sources:

- layout properties follow the CSS semantics represented by Taffy 0.13;
- all other properties follow the standard, non-vendor Lynx 4.0 inventory
  pinned at `18a0a91009809de1d52a5637b82f573dc924e32a`.

This produces 158 target features: 157 properties plus CSS Custom Properties
as one non-fixed-property mechanism. Of the current 174 `StyleProperty`
entries, 154 remain in the target and 20 are deliberately unsupported. The
Taffy baseline additionally requires `float`, `clear`, and
`grid-template-areas`, which receive new stable IDs when registered.

Taffy-supported Block, Flexbox, and Grid semantics are in scope. Browser
layout modes or values that Taffy 0.13 cannot represent, including inline and
table layout, subgrid, masonry, `position: static/fixed/sticky`, and
`overflow: auto`, are deliberately outside the target. Taffy's feature-gated
float layout and `calc()` resolution are part of the target and must be
enabled by the Whisker layout integration.

The deprecated aliases `grid-column-gap`, `grid-row-gap`, and `word-wrap` are
not separate target features. Their canonical spellings are used. Retired
numeric property IDs stay reserved and must not be reassigned. The normative
machine-readable partition, including exclusion rationale, implementation
status, and browser-subset notes, is `tests/host-conformance/capabilities.json`.

Migration tracks semantic capability rather than CSS syntax. A generated
property registry and coverage table must classify every Lynx style feature
used or promised by Whisker into these groups:

| Group | Examples | Target owner |
|---|---|---|
| Layout | display, flex/grid, sizing, spacing, position, gap | Rust/Taffy |
| Text input | font, line height, spacing, alignment, decoration | Rust resolution + Host measure/paint |
| Box paint | color, backgrounds, borders, radii, shadows | Rust resolution + Host paint |
| Composite | opacity, transform, transform origin | Rust values + Host layers/DOM |
| Clip/overflow | overflow, radii clips, clip shapes | Rust semantics + Host realization |
| Effects | filters, blend/isolation where supported | Rust semantics + capability-gated Host |
| Stacking | order, z-index, stacking contexts | Rust order + Host realization |
| Motion | transitions, keyframes, springs, gesture-driven values | Rust `whisker-motion` |
| Interaction | visibility, pointer/hit-test effects, scrolling | Rust scene + Host input/scroll primitive |
| Environment | viewport units, scale, safe area, font/locale epochs | Rust environment resolution |

For each feature, the table records typed API shape, normalization,
inheritance, invalidation impacts, interpolation, Taffy mapping, renderer
operations, and Android/iOS/Web conformance. "Supported" requires tests for
semantic resolution and each required backend, not merely the presence of a
property name.

CSS-only concepts with no useful Whisker application semantics may be marked
deliberately unsupported. Such exclusions require rationale in the coverage
table. The goal is full useful Lynx visual capability, not implementation of a
general-purpose web browser.

## Rust subsystem boundaries

The implementation may be split into Rust-only crates. They remain internal
subsystems by default rather than ordinary Whisker runtime modules:

```text
whisker-style
  stable typed values, property IDs, declaration storage
  composition, applicability, inheritance, computed values, invalidation

whisker-layout
  retained Taffy tree, measurement requests, layout results

whisker-motion
  time, interpolation, springs, gestures, presentation updates

whisker-engine
  coordinates style/layout/motion and emits renderer operations

whisker-renderer
  versioned Host boundary defined by RFC 0002
```

UI element modules depend at compile time on `whisker-style` and stable element
schema types, not on `whisker-engine`, a concrete layout engine, or a renderer.
Style resolution is a deterministic Rust subsystem of `whisker-style`; the
retained `whisker-engine` owns per-node state and decides when to invoke it.
It does not require a separate `whisker-style-engine` crate or runtime module.
Whisker core coordinates these mechanisms directly. The module registry is
used at stable extension boundaries such as the selected Host renderer,
native services, and element providers; it is not inserted between core
style, layout, motion, scene, and event algorithms. RFC 0004 defines this
division.

The exact crate split is not normative. The ownership boundaries, internal
typed interfaces, and ability to substitute Rust-only recording/test
implementations are. Test substitution does not by itself imply a public
runtime-provider compatibility promise.

## Test strategy

Rust-only tests must cover:

- `css!` property typing and normalization;
- deterministic last-wins fragment composition;
- the fixed inheritance whitelist and override stopping behavior;
- property applicability and diagnostics;
- invalidation classification and subtree propagation;
- Taffy input/output fixtures;
- measurement cache keys, pending responses, and epoch rejection;
- motion and signal updates producing identical property deltas;
- stable frame-packet output from specified scene fixtures;
- SSR serialization from a resolved scene;
- the Lynx property coverage registry.

Shared renderer conformance fixtures then verify Android, iOS, Web, and Desktop
projection for geometry, text constraints, paint, clipping, stacking, and hit
testing. Each Host has a standalone runner that mocks the Rust-side scenario
source and event sink while exercising the shipped Host measurement,
presentation, paint, and native input paths. It must be possible to complete a
Host scenario without mounting `RuntimeInstance`.

Selected WPT CSS cases are adapted into this shared corpus with their upstream
path, revision, license, and adaptation recorded. The same case identifier is
used for a Rust style/layout-to-protocol assertion, every required Host-only
run, and a smaller final full-stack test. Canonical Host packets are derived
from the specification or WPT reference rather than recorded from the current
Rust implementation. Same-platform reftest image comparisons use explicit
tolerances; semantic cases assert retained projection and event observations.
Visual snapshots supplement but do not replace semantic assertions.

## Migration

1. Generate stable typed property metadata from the current `whisker-css`
   property inventory and establish the Lynx coverage table.
2. Change `Css`/`Style` internals from serialized strings to typed declaration
   storage while preserving `css!` and `render!` call syntax.
3. Add deterministic style composition, the seven-property
   `InheritedStyle`, Taffy-independent computed box/flex values, and Rust-only
   resolution tests.
4. Map computed layout values into a retained Taffy tree and connect RFC
   0002's measurement batches. The retained tree, generalized measurement
   protocol, keyed cache, explicit block/placeholder/retain-previous policies,
   pending completion with epoch rejection, and `SurfaceEngine` integration
   into incremental scene/frame production are implemented. Typed built-in
   payloads, strict batch validation, the Rust `MeasurementProvider` seam, and
   synchronous drive-to-final-layout orchestration are also implemented. Plain
   UTF-8 Text v1 now lowers computed inherited text style into matching
   measurement and `SetText` presentation payloads, including propagation of
   the accepted prepared Host object and resolved foreground color into final
   snapshot and delta frames. A Rust-only renderer adapter now binds existing
   `render!` Text output and typed `css!` fragments to the retained engine,
   preserving inherited text style and avoiding remeasurement for paint-only
   updates. The generated renderer ABI, platform Host providers, and the
   explicitly diagnosed backend gaps remain.
5. Resolve box paint, clip, stacking, transforms, text decoration, and
   semantics into typed renderer operations. Text foreground paint,
   background color, four-sided border width/color/style, corner radius,
   opacity, visibility, z-order, and overflow clipping are implemented and
   exercised from `render!` through `FrameSink`. Shadows, images, general
   transforms, filters, blend/group compositing, and text decoration remain.
   Layout frames now include border-box and content-box geometry, preventing
   Hosts from reconstructing text origins from layout styles. The initial
   macOS Host consumes the retained box/text/clip subset with native font
   shaping and Metal/wgpu painting, including normalized per-corner rounded
   backgrounds and border outlines. Rounded descendant/path clipping, exact
   non-solid borders, and group compositing remain Desktop conformance work.
   Protocol minor 1 now defines the remaining Host-independent
   background-layer, outline/shadow, shape-clip, mask, filter, compositing,
   text-decoration/shadow, image, and cursor value shapes with transactional
   validation. It also retains elliptical radius axes, OpenType
   feature/variation/optical-sizing measurement inputs, image-rendering intent,
   optional capability profiles, and a generation-safe resource lifecycle.
   Existing Desktop and Web Hosts advertise elliptical radius support, while
   the common background-layer subset is now represented in every Host ABI.
   URL-backed backgrounds and their supported layer geometry are emitted by
   style lowering; remaining operation groups are rejected before retained
   state mutation. Each property group lands as a complete specified-value ->
   computed-value -> operation slice and then advances the per-Host checklist.
6. Route signal and `whisker-motion` writes through the shared property slots
   and incremental dirty classifier.
7. Add the shared Host conformance scenario format, standalone runners, and
   pinned WPT-derived property corpus. Implement and conform Android, iOS, the
   JavaScript DOM Web path, and the Whisker-owned native Rust Desktop path
   against the same case identifiers.
8. Move the portable first macOS Host implementation to `platforms/desktop`
   and keep OS crates as thin adapters. This extraction and the first
   WPT-derived background/radius Host scenarios, real Desktop measurement,
   and the recording input-sink contract are implemented. Continue Desktop
   lowering conformance for paint, text, clipping, compositing,
   accessibility, and external surfaces without making Desktop render types
   part of the common protocol.
9. Add the optional SSR serializer and hydration contract in a follow-up RFC.
10. Remove raw CSS string inputs, Lynx inline-style serialization, Lynx style
   ownership, and temporary migration adapters.

Steps may overlap, but raw-string removal must not land without diagnostics and
typed replacements for properties used by maintained Whisker packages.

## Invariants

1. The public common styling API is typed and inline-only.
2. `render!` remains function-call-style and `style: css!(...)` remains valid.
3. No public selector, stylesheet, specificity, `!important`, or global
   cascade participates in rendering.
4. Explicit style fragments compose in order with last declaration winning.
5. Only the seven listed text properties inherit in version 1.
6. Taffy is authoritative for interactive layout on every target.
7. The Host owns shaping and painting, but not style or layout semantics.
8. Signal and motion updates target the same typed property slots.
9. One changed frame sends typed deltas, not a full style string or full tree.
10. The Host does not own or autonomously advance animation timelines; it only
    applies values sampled by Rust.
11. Internal style interning or generated Web classes are unobservable
    optimizations.
12. Third-party UI modules use common styles plus typed canonical element
    properties; they do not extend a global CSS language.
13. Lynx compatibility is measured by semantic feature coverage, not CSS text
    compatibility.

## Open questions

The following must be resolved before this RFC becomes `Accepted`:

- the final property IDs, value encodings, and generated-registry format;
- whether the public typed declaration value keeps the name `Css` or becomes
  `Style` while `css!` remains the macro name;
- the exact fragment composition helpers and constant-style support;
- the element applicability of every inherited property;
- the baseline and multi-line text measurement representation;
- the minimum blend, shadow, backdrop-blur, and clip capability required of all
  interactive renderers;
- which low-level Desktop paint, text, compositing, accessibility, and external-
  surface facilities are required to satisfy that minimum profile;
- scrolling ownership for platform-native and fully Rust-coordinated modes;
- the fallback font-metrics policy for SSR and pre-font-load Web layout;
- which Lynx style features are deliberately excluded as browser-oriented
  rather than application-oriented;
- the follow-up SSR HTML and hydration protocol.
