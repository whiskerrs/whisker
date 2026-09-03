# next-architecture core review fix inventory

Status captured on 2026-09-03. This document inventories the 77 open pull
requests targeting `next-architecture` that were created while acting on the
external core review. It exists to make the proposed consolidation reviewable;
it is not a statement that every patch below should be merged unchanged.

## Why there are 77 pull requests

The review findings were incorrectly treated as pull-request boundaries. A
small implementation concern such as one DOM child index, one event-loop
control-flow transition, or one Kotlin validation lookup became its own branch.
That retained good commit-level isolation, but produced excessive review and CI
overhead and hid dependencies between related fixes.

The replacement boundary is seven responsibility-oriented rollups:

1. Core style, layout, paint lowering, and protocol projection
2. Runtime / Host transaction and mobile ABI contracts
3. Desktop Host
4. Web Host
5. Android Host
6. iOS Host
7. Tooling and CI

The original commits should remain as commit-level history inside the rollups.
Superseded commits must not be copied blindly.

## Known duplication and conflicts

- #595 and #608 implement the same Desktop `ControlFlow::Wait` restoration.
  Keep #608; drop #595.
- #574 combines three unrelated concerns: mobile runtime lifecycle, module-name
  documentation, and a broad Rust 1.85 compatibility rewrite. #587 later sets
  the actual MSRV to Rust 1.88. The lifecycle and contract pieces need to be
  extracted; the 1.85 workspace-wide rewrites and package changes should not be
  carried into the rollup.
- #583 rejects percentage border widths at style resolution because CSS border
  widths are length-only. #649 independently resolves percentage widths in the
  Web Host. Keep #649's `hidden => 0` defensive Host behavior; reconsider its
  percentage path after #583 is applied.
- The unsubmitted Web baseline change (`a83c959e`) is based on #600 because the
  marker reads must participate in the same batched DOM layout. It is not an
  additional open pull request.
- #572, #575, #605, #607, #614, and #644 intentionally touch more than one Host.
  They belong to the shared Runtime / Host contract rollup rather than being
  duplicated in each platform rollup.

## 1. Core style, layout, paint, and protocol

### #571 — canonical radial gradients

- `crates/whisker-engine/src/radial_gradient.rs`: centralizes radial-gradient
  geometry normalization before Host projection.
- `crates/whisker-engine/src/surface.rs`: emits the canonical gradient form.
- `crates/whisker-protocol/src/capability.rs`: aligns capability requirements
  with the lowered representation.
- `crates/whisker/tests/runtime_instance.rs`: covers end-to-end lowering.

### #573 — bounded measurement validity

- `crates/whisker-engine/src/measurement.rs`: expires stale prepared and cached
  measurements rather than retaining them indefinitely.
- `crates/whisker-layout/src/lib.rs`: keeps layout invalidation synchronized
  with measurement generations.

### #576 — custom-property failure isolation

- `crates/whisker-style/src/resolution.rs`: rejects only the declaration whose
  `var()` substitution fails instead of poisoning unrelated declarations.
- `crates/whisker-style/src/value_tree.rs`: preserves structured substitution
  errors and bounded traversal.
- `crates/whisker-style/src/resolution/tests.rs`: tests sibling declaration
  isolation.

### #577 — logical margin and padding

- `crates/whisker-css/src/prop/box_model.rs`: exposes logical box properties.
- `crates/whisker-style/src/layout.rs` and `layout/resolution.rs`: resolve
  logical edges using writing direction before Taffy input is built.
- `crates/whisker-style/src/layout/tests.rs`: covers LTR/RTL mappings.

### #578 — non-painting border widths

- `crates/whisker-style/src/resolution.rs`: collapses used border widths to zero
  for `none` and `hidden`.
- paint/resolution tests cover the layout and paint contract.

### #579 — inheritance contract

- `crates/whisker-style/src/property.rs`: corrects which typed properties are
  inherited.
- `docs/rfcs/0003-typed-inline-style-layout-and-paint.md`: records the contract.

### #580 — composite color validation

- `crates/whisker-style/src/paint.rs`: validates colors nested in composite
  paint values consistently with standalone colors.
- resolution and paint tests cover invalid channels and finite values.

### #581 — `steps(..., jump-start)` sampling

- `crates/whisker-style/src/motion.rs`: fixes the exact-boundary sample for
  jump-start timing functions.

### #582 — `calc()` length semantics

- `crates/whisker-style/src/resolution.rs` and `resolution/values.rs`: keep
  length/percentage arithmetic typed and reject invalid combinations.
- `crates/whisker-engine/src/text.rs`: consumes the corrected resolved values.
- resolution tests cover nested and failing expressions.

### #583 — length-only border widths

- `crates/whisker-style/src/layout/resolution.rs`: rejects percentage terms for
  CSS border widths instead of sending invalid used values to Hosts.
- layout tests cover valid lengths and rejected percentages.

### #584 — Unicode custom-property names

- `crates/whisker-style/src/value.rs`: validates CSS custom-property names
  without incorrectly restricting them to ASCII.

### #585 — delta projection copy-on-write

- `crates/whisker-protocol/src/validation.rs`: avoids cloning every retained
  node for small deltas while preserving atomic validation.

### #586 — bounded retired-node tracking

- `crates/whisker-protocol/src/validation.rs`: replaces the epoch-long set of
  every allocated node ID with one high-water mark.
- The original patch was incomplete: monotonic allocation does not imply that
  a snapshot emitted from a `HashMap` is ordered. The Core rollup additionally
  sorts snapshot `CreateNode` operations by ID and tests that producer-side
  protocol invariant before enabling constant-space validation.

### #589 — background-position axis diagnostics

- `crates/whisker-css/src/prop/background.rs`: rejects axis-incompatible or
  ambiguous position combinations with a typed diagnostic.

### #591 — `order` display scope

- `crates/whisker-layout/src/lib.rs`: applies `order` only under flex/grid
  parents, not ordinary block layout.

### #592 — reject error-valued frame data

- `crates/whisker-protocol/src/validation.rs`: prevents
  `WhiskerValue::Error` from crossing the presentation protocol as application
  data.

### #593 — background capability completeness

- `crates/whisker-protocol/src/capability.rs`: requires the complete set of
  background capabilities represented by an operation instead of accepting a
  partial Host claim.

### #594 — degenerate motion paths

- `crates/whisker-engine/src/paint/motion_path.rs`: gives zero-length paths a
  finite, deterministic transform rather than propagating invalid geometry.
- paint tests cover the degenerate result.

## 2. Runtime / Host transactions and mobile ABI

### #572 — Rust-authoritative hit testing

- `crates/whisker-engine/src/scene/hit_test.rs`: performs retained-scene hit
  testing in Rust using paint order, clipping, transforms, and visibility.
- `crates/whisker-engine/src/scene.rs` and `surface.rs`: retain hit-test data and
  expose the query through the surface.
- `crates/whisker-runtime/src/runtime_instance.rs`: resolves untargeted pointer
  events in Rust while preserving explicit targets for native elements.
- `crates/whisker-protocol/src/input.rs`: defines Host input metadata without
  requiring a Host-selected target.
- `crates/whisker-driver*` and Android/iOS bridge files: carry the input
  coordinates and lazy external scroll state through the existing ABI.
- all four Hosts receive adapter changes and conformance tests.

### #574 — mixed contract/MSRV/lifecycle patch; split before use

Keep only these concepts:

- `crates/whisker-driver*` and mobile bridge headers: O(1) runtime pause/resume
  entry points.
- Android `WhiskerView.kt` and iOS `WhiskerView.swift`: temporary detach pauses
  a RuntimeInstance; final owner destruction tears it down.
- module-name documentation: canonical custom element names remain explicit.

Do not copy the broad Rust 1.85 rewrites across 48 files. They touch core,
examples, packages, every Host, and `Cargo.lock`, and are superseded by #587's
Rust 1.88 decision.

### #575 — custom Host element failure isolation

- `crates` contract docs: core presentation failure rejects a frame; a custom
  element factory/property/command failure disables only that element.
- Desktop `element.rs` / `scene/transaction.rs`: validates metadata before
  mutation and stores a failed custom presentation state.
- Web `frame_sink.rs`: preserves common layout/paint while disabling a failed
  native element callback.
- Android/iOS module registrars and Host scenes: implement the same failure
  boundary.
- each Host has regression coverage.

### #605 — defer element events during present

- Android/iOS `HostEventGate` files: queue synchronous native callbacks raised
  during frame application and flush them afterward in FIFO order.
- Host scenes use the gate so Rust cannot be re-entered while a transaction is
  mid-commit.

### #607 — mobile structural preflight

- Android/iOS `HostScene`: validates create/delete/insert/move/layout references
  before mutating native view hierarchies.
- conformance tests assert rejection leaves the prior scene intact.

### #611 — length-prefixed ABI strings

- `crates/whisker-driver/src/ffi_runtime*.rs`: decodes strings from pointer plus
  length rather than NUL termination, preserving interior NUL bytes.
- `value_codec.rs`: applies the same contract to `WhiskerValue` strings.

### #612 — linear measurement response matching

- `crates/whisker-driver/src/ffi_runtime/measurement.rs`: indexes one Host
  response batch once instead of repeatedly searching it, removing quadratic
  work.

### #613 — canonical empty ABI slices

- driver frame/measurement encoders: represent an empty slice as null pointer
  plus zero length and test the ABI shape.

### #614 — pooled mobile visibility reset

- Android/iOS module registrars: reset hidden state when a native element is
  reused for a new Whisker node.
- both mobile conformance suites cover reuse.

### #632 — atomic Android bootstrap exception handling

- `crates/whisker-driver-sys/bridge/src/whisker_mobile_android.c`: treats a JNI
  exception during bootstrap as a terminal failure and does not publish a
  partially initialized runtime.

### #644 — shared min-content text measurement

- `tests/host-conformance/core/text-measure-min-content.json`: one common
  measurement case.
- Android/iOS/Desktop measurement implementations: use native narrow intrinsic
  measurement for `AvailableSpace::MinContent`.
- Web keeps CSS `min-content` behavior; all four runners execute the fixture.

## 3. Desktop Host

### #595 and #608 — duplicate idle control-flow fix

- Both change `platforms/desktop/src/app.rs` so the winit loop uses
  `WaitUntil(deadline)` only while scroll settling and returns to blocking
  `Wait` afterward.
- Keep #608 and its focused test. Close #595 as superseded.

### #596 — prepared text lifetime

- `platforms/desktop/src/text.rs`: identifies prepared text by generation and
  releases stale glyph/layout buffers.
- `scene.rs`, `scene/transaction.rs`, and `surface.rs`: synchronize prepared
  content ownership with node deletion and snapshots.

### #609 — pointer-move invalidation

- `platforms/desktop/src/app.rs`: requests redraw only when pointer movement is
  handled or changes hover/capture state, avoiding idle mouse redraws.

### #615 — snapshot scroll state

- `platforms/desktop/src/scene/transaction.rs`: cancels stale smooth-scroll
  animations and offsets when a full snapshot replaces the scene.

### #616 — duplicate pointer-capture map removal

- `platforms/desktop/src/scene.rs` and `scene/transaction.rs`: removes Host-side
  state that duplicated the Rust runtime's authoritative capture state.

### #617 — borrowed element binding lookup

- `platforms/desktop/src/element.rs`: returns shared registrations by reference
  on the hot path instead of cloning binding metadata per operation.

### #618 — pure element preflight

- Desktop `element.rs` and `scene/transaction.rs`: separates validation from
  factory invocation so preflight cannot create native/custom content.

### #619 — safe presentation pooling

- Desktop element/transaction code: pools only built-in presentations with a
  known reset contract; custom elements are not speculatively recycled.

### #620 — executable-relative assets

- `platforms/desktop/src/lib.rs`: resolves packaged assets relative to the
  executable/application bundle instead of the process working directory.

### #621 — bounded resource requests

- `platforms/desktop/src/resource.rs`: reuses one `ureq::Agent` and applies
  connect/read timeouts to remote image/resource acquisition.

### #622 — 2D shader depth contract

- `platforms/desktop/src/gpu/shaders.rs`: emits normalized zero clip-space Z for
  the 2D renderer instead of an invalid or backend-dependent depth.

### #628 — Linux/Windows shell API symmetry

- `platforms/linux` and `platforms/windows`: add the same application-hash and
  hot-reload entry points already used by the macOS shell.

## 4. Web Host

### #597 — resulting-index `MoveChild`

- `platforms/web/src/scene/frame_sink.rs`: translates protocol indexes, which
  are defined after removal, into correct live DOM insertion indexes.
- the Web conformance test covers forward movement.

### #598 — ScrollView clip composition

- `platforms/web/src/paint/clip.rs`: keeps orientation and `enable-scroll`
  overflow state separate from visual clipping.
- the test applies a later clip update to a horizontal ScrollView.

### #599 — intrinsic text width

- `platforms/web/src/measure/text.rs`: reports used text width rather than the
  definite constraint width while still measuring wrapping under the
  constraint.
- measurement assertions no longer allow the pinned-width bug.

### #600 — batched text measurement

- `platforms/web/src/measure/text.rs`: creates/appends all probes, performs all
  geometry reads, then removes all probes, reducing N forced layouts to one.
- the test verifies response order and cleanup.

### Unsubmitted commit a83c959e — real Web baselines

- `platforms/web/src/measure/text.rs`: adds zero-size inline baseline markers at
  the first and last line and reads their actual positions in #600's read batch.
- `platforms/web/src/tests/host_conformance.rs`: verifies single- and multi-line
  baseline behavior.

### #601 — DOM style isolation

- `platforms/web/src/scene/frame_sink.rs`: establishes an isolated baseline for
  Whisker nodes so page-level selectors cannot alter layout or typography.
- `platforms/web/Cargo.toml`: enables the required Shadow DOM bindings.

### #602 — direct child index

- `platforms/web/src/scene/frame_sink.rs`: maintains child adjacency so layout,
  paint, and deletion do not scan every parent entry for every node.

### #610 — reusable animation-frame callback

- `platforms/web/src/application.rs`: replaces one leaked wasm closure per rAF
  with a retained reusable callback.

### #647 — partial DOM transaction recovery

- `platforms/web/src/scene/frame_sink.rs`: keeps mutation-free preflight
  separate, clears a partially applied DOM tree after an exception, and returns
  `NeedSnapshot` for automatic recovery.
- the Web test forces a DOM pointer-capture failure and verifies no duplicate
  node survives the next snapshot.

### #648 — advertised text/cursor payloads

- `platforms/web/src/paint/text.rs`: supports combined decoration lines,
  explicit/from-font thickness, and multiple shadows.
- new `platforms/web/src/paint/cursor.rs`: maps resource cursor candidates,
  hotspots, and keyword fallback to CSS.
- `frame_sink.rs`: preflights missing cursor resources.

### #649 — Web border used values

- `platforms/web/src/paint/box.rs`: writes resolved pixel border widths instead
  of invalid percentage `calc()` values.
- `frame_sink.rs`: recomputes used widths when layout changes and treats
  `hidden` as zero.
- retain only behavior still necessary after #578/#583 are combined.

## 5. Android Host

### #629 — z-order without elevation

- `HostScene.kt`: reorders physical siblings according to protocol z-order
  instead of using Android elevation, preserving clipping and shadows.
- only parents whose children changed are revisited.

### #630 — outer box shadows

- `paint/BoxShadow.kt`: gives outer shadows drawing space beyond the border box
  while preserving inset clipping.

### #631 — native transform geometry

- `HostNode.kt`: applies protocol transforms through the Android View animation
  matrix so native input/accessibility geometry matches rendering; keeps a
  Canvas fallback for devices lacking the method.

### #633 — compact member validation

- `WhiskerModuleRegistrar.kt` and `HostScene.kt`: build compact property/command
  membership indexes and reject unknown IDs before callbacks.

### #634 — hidden native input

- `HostNode.kt`: prevents a protocol-hidden node from receiving native touch.

### #635 — capture through nested scrollers

- `HostScene.kt`: routes captured pointers through ancestor ScrollViews and
  reapplies capture correctly for multiple pointers.

### #636 — physical layout rounding

- `HostScene.kt`: rounds both edges in physical pixels and derives size from the
  rounded edges, preventing seams and drift.

### #637 — test hook visibility

- `WhiskerView.kt`: makes conformance-only entry points internal and names them
  explicitly `ForTesting`.

### #638 — module DSL cleanup

- Android module `Module.kt` / `ModuleDefinition.kt`: removes an impossible
  `View(Class, block)` overload and updates stale examples/comments.

### #639 — surface-local element bindings

- `WhiskerModuleRegistrar.kt`: keeps declarations global but creates compact
  element-ID bindings per `WhiskerView` surface.
- `WhiskerView.kt`, bootstrap, measurement, and scene code use the local map so
  multiple surfaces cannot overwrite each other's IDs.

### #640 — generated Activity deep links

- `crates/whisker-cng` Android template: forwards both cold-start and warm
  intents into `WhiskerAppContext`.
- `WhiskerAppContext.kt`: exposes the route/deep-link event used by modules.

### #642 — pointer action normalization

- `input/PointerInput.kt`: maps hover enter/exit and generic motion/scroll into
  the protocol pointer stream while preserving wheel semantic scroll events.

### #643 — actual Android text width

- `measure/TextMeasurement.kt`: returns the maximum `StaticLayout` line width,
  not the width constraint.
- a JVM test covers the allocation-free line scan.

### #645 — centered CSS line height

- `CenteredLineHeightSpan.kt` and `LineHeightMetrics.kt`: distribute extra or
  negative leading around the font metrics to match CSS line boxes.
- `WhiskerTextView`, built-ins, and measurement use the same implementation.
- JVM and emulator tests cover rendering and measurement.

## 6. iOS Host

### #603 — bootstrap enum validation

- `HostElementBootstrap.swift`: rejects invalid raw enum discriminants instead
  of force-mapping them into module schema state.

### #604 — multiline ellipsis measurement

- `measure/TextMeasurement.swift`: applies line-limit/ellipsis height without
  incorrectly forcing every ellipsized text to one line.

### #606 — incremental z-order

- `HostScene.swift`: reorders only affected sibling groups rather than walking
  and sorting the complete native tree after every accepted frame.

### #623 — generic image data URLs

- `resource/HostResourceService.swift`: accepts supported `image/*;base64`
  media types rather than one hard-coded image format.

### #624 — hidden scroll chrome only

- `WhiskerBuiltInElements.swift` and `HostNodeView.swift`: maps scrollbar
  visibility to UIKit indicators without disabling scrolling itself.

### #625 — duplicate pointer-capture map removal

- `HostScene.swift`: removes native state duplicating Rust's authoritative
  pointer-capture ownership.

### #626 — conformance ABI signatures

- `WhiskerHostConformanceStubs/stubs.c`: keeps test stubs exactly aligned with
  the production runtime ABI.

### #627 — main-thread callback contract

- `bridge/RuntimeABI.swift`: asserts that UI-affecting Host callbacks arrive on
  the main thread.

### #646 — one-axis clips during ancestor scrolling

- `HostNodeView.swift`: observes only relevant ancestor ScrollViews and
  translates an existing one-axis mask as offsets change, without rebuilding
  paths or crossing FFI on every scroll.
- iOS conformance tests cover scrolling under one-axis overflow clipping.

## 7. Tooling and CI

### #587 — Rust 1.88 MSRV

- root `Cargo.toml` and `deny.toml`: declare the real supported compiler.
- `.github/workflows/ci.yml`: checks Rust 1.88 explicitly.
- `whisker-macros/component.rs`: contains the small compatibility adjustment
  needed by that compiler.

### #588 — iOS publish pin check

- `.github/workflows/publish-ios.yml`: verifies the canonical SDK version pin
  instead of a stale path/value.

### #641 — non-blocking Gradle process capture

- `WhiskerPlugin.kt`: drains child stdout and stderr concurrently so a full pipe
  cannot deadlock module discovery; retained stderr is bounded to 64 KiB.
- Gradle plugin tests exercise large stderr output.

## Proposed disposition

| Original PRs | Disposition |
| --- | --- |
| #595 | Close as duplicate of #608 |
| #574 | Do not merge directly; extract lifecycle and contract pieces |
| #649 | Fold only non-redundant Host defense into the Web rollup |
| #571–#594 otherwise | Core / shared-contract rollups |
| #595–#622 otherwise | Desktop or Web rollups |
| #623–#646 | iOS, Android, shared mobile, or tooling rollups |
| #647–#649 | Web rollup |
| a83c959e | Web rollup after #600 |

Before closing any original pull request, each rollup must preserve the chosen
commits, pass the relevant Host conformance suite, and include a PR-number to
commit mapping in its description.

## Re-audit decisions after composing all seven rollups

The seven branches were also merged locally in their intended order. This
found interactions that were invisible while each original patch was tested
alone and one external-review recommendation that does not match CSS syntax.

- Reject #584's Unicode-whitespace restriction. CSS identifier code points
  include every non-ASCII code point, including characters that Rust classifies
  as Unicode whitespace or controls. The Core rollup keeps the existing
  non-ASCII behavior and adds explicit coverage for it.
- Keep #586's constant-space retired-node validation only after making the
  producer satisfy its stricter contract. Engine recovery snapshots now emit
  `CreateNode` operations in ascending allocation order instead of arbitrary
  `HashMap` order; protocol and Engine tests cover both sides of the invariant.
- Keep built-in element failures fail-fast and isolate only package/custom
  elements. The Web rollup now preserves that distinction after declared
  factories are bound; the Android rollup preserves the same boundary while
  moving bindings from process-global state to each surface.
- Remove the Web Host's outer full-scene projection clone. Protocol validation
  is already transactional; after a DOM apply failure the live DOM is cleared,
  so the matching projection is reset to revision zero and requests a full
  snapshot. Successful delta frames now retain only the protocol validator's
  changed-node undo state.
- Ignore a stale scroll-offset update for a node already absent from the Rust
  scene, while still validating every numeric payload and applying current
  updates in the same batch. This avoids discarding a valid pointer sample for
  a normal Host/scene race and adds no per-scroll FFI call.
- Treat Android detach as temporary even when no `ViewTreeLifecycleOwner` is
  installed. Ownerless embedders must call `WhiskerView.destroy()` for final
  teardown because `onDetachedFromWindow()` cannot distinguish reparenting
  from destruction.
- Keep Android's already-posted module-event flush alive across temporary
  detach. Cancelling the runnable while retaining its `flushScheduled` flag
  permanently wedges future delivery; the paused Rust runtime can safely
  accept and retain the completion without scheduling a frame.
- Reset Rust's mirrored Host scroll offsets when a recovery snapshot is
  required. All four Hosts rebuild native scroll presentation at offset zero,
  and scroll offsets are intentionally absent from snapshot operations; the
  Rust mirror must therefore cross the same rare O(node-count) recovery
  boundary so authoritative hit testing stays aligned.
- Reject out-of-range insert and move indices in Android and iOS before native
  mutation instead of clamping them to a different tree. The validators track
  only one child-count integer per retained parent; no extra frame copy or
  per-operation child-vector allocation is introduced.
- Accept the ABI's canonical empty-frame representation on iOS: zero
  operations with a null operation pointer. Android already accepted this
  representation and the Rust encoder intentionally emits it.
- Reuse `InputDispatch.target` for Desktop cursor, native text focus, wheel
  routing, and snap settling. This removes the second Host hit test from normal
  pointer movement and prevents the weaker Host geometry walk from disagreeing
  with Rust over clip paths or rounded overflow.
- Do not add Host-side pause queues for module or resource completions. The
  runtime already accepts them while paused into retained state, suppresses
  wakeups, and defers reactive effects. Adding another queue would duplicate
  payloads and introduce a new overflow policy; RFC 0002 is clarified to state
  the implemented contract.
- The Runtime/ABI rollup owns the shared contract and the corresponding thin
  adapters in all four Hosts, including generated mobile ABI mirrors and the
  common fixture. The four Host rollups are stacked directly on that branch
  and contain only the remaining platform-specific changes. This keeps the
  Runtime/ABI pull request independently buildable while preventing its shared
  adapter edits from appearing as duplicate review work in every Host pull
  request. Once Runtime/ABI is merged, the four Host pull requests can be
  retargeted to `next-architecture` without changing their effective diffs.
