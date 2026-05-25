# Phase L design: `ModuleDefinition` DSL

**Status**: design (no implementation yet). Tracking issue: #58. Parent epic: #55.

This doc proposes the implementation path for Whisker's `ModuleDefinition` DSL — an Expo-Modules-style API surface that replaces the current `@WhiskerComponent` / `@WhiskerModule` / `@WhiskerProp` / `@WhiskerUIMethod` annotation set on both iOS and Android. View-bearing and function-only modules will share the same `definition() -> ModuleDefinition` entry point; the View dispatch table becomes a feature of the DSL.

A secondary goal: stop routing through Lynx's `@LynxProp` / `@LynxUIMethod` reflection in the codegen output, and dispatch directly into the Lynx-internal prop / method registries instead. Removes the "method name must match exactly" footgun (the `lynxInvoke_pause` bug, PR #52), makes the dispatch table inspectable, and decouples Whisker's surface from changes in Lynx's annotation contract.

## 1. Target shape

**Swift**:

```swift
public final class VideoModule: WhiskerModule {
  public func definition() -> ModuleDefinition {
    Name("Video")

    Constants([
      "maxResolution": "1080p",
    ])

    View(WhiskerVideoView.self) {
      Prop("src") { (view, value: String) in view.setSrc(value) }
      Function("play") { (view) in view.play() }
      Function("pause") { (view) in view.pause() }
      Function("seek") { (view, seconds: Double) in view.seek(seconds) }
      Events("onCompleted")
    }
  }
}
```

**Kotlin**:

```kotlin
class VideoModule : WhiskerModule() {
  override fun definition() = ModuleDefinition {
    Name("Video")

    Constants("maxResolution" to "1080p")

    View(WhiskerVideoView::class) {
      Prop("src") { view, value: String -> view.setSrc(value) }
      Function("play") { view -> view.play() }
      Function("pause") { view -> view.pause() }
      Function("seek") { view, seconds: Double -> view.seek(seconds) }
      Events("onCompleted")
    }
  }
}
```

`Function(...)` is the sync form — the closure returns synchronously
and the caller awaits nothing. Use `AsyncFunction(...)` only when the
work is genuinely async (a network fetch, a permission prompt, a long
disk read) and the platform side wants to defer completion to a
later runloop turn. `play()` / `pause()` / `seek()` are immediate
state mutations on a Media3 / AVPlayer instance — sync.

**Function-only module** (same DSL, no `View(...)` block):

```swift
public final class LocalStoreModule: WhiskerModule {
  public func definition() -> ModuleDefinition {
    Name("WhiskerLocalStore")
    Function("save") { (key: String, value: String) in
      UserDefaults.standard.set(value, forKey: key)
      return true
    }
    Function("load") { (key: String) -> String? in
      UserDefaults.standard.string(forKey: key)
    }
  }
}
```

Single API. View is just a feature. Same shape on both platforms.

## 2. Lynx dispatch layer mapping

| DSL block | Maps to (Android) | Maps to (iOS) |
|---|---|---|
| `Prop("name") { view, value -> ... }` | a `LynxUISetter<T>` impl pre-registered in `PropsUpdater.PRE_REGISTER_MAP` | an Obj-C setter on the `LynxUI` subclass (`-(void)setNameProp:(Type)v`) emitted as a category |
| `Function("name") { ... }` (sync) / `AsyncFunction("name") { ... }` (callback) | a `LynxUIMethodInvoker<T>` impl pre-registered via `LynxUIMethodsExecutor.registerMethodInvoker(...)` | a `LYNX_UI_METHOD(name:withResult:)` macro expansion on the `LynxUI` subclass |
| `Events("name", ...)` | call `eventEmitter.dispatchCustomEvent(LynxCustomEvent(...))` directly when the module fires the event | same — `[eventEmitter dispatchCustomEvent:event]` directly |
| `Name("Foo")` | `addBehavior(Behavior("<crate>:Foo"))` registration (unchanged from today) | same |
| `Constants([...])` | per-class static map exposed via a new `getConstants()` accessor on `WhiskerModule` | same |
| `View(MyView.self) { ... }` | inner block scopes prop / method registrations to that view's `LynxUI<View>` subclass | same |

**Crucial property**: none of the codegen output references `@LynxProp` or `@LynxUIMethod`. Lynx's reflection layer is bypassed end-to-end for Whisker modules.

### 2a. Pre-registration vs lazy reflection

Both Lynx Android entrypoints currently look up generated `$$PropsSetter` / `$$MethodInvoker` classes by reflection at first use, falling back to the annotation-scanning `Fallback*` invoker when none is registered. The Whisker codegen plan:

1. KSP / Swift Macro processes `ModuleDefinition` declarations at build time.
2. Emits a hand-rolled implementation of `LynxUISetter<T>` and `LynxUIMethodInvoker<T>` per `WhiskerModule` class.
3. Emits a registration call that runs from the host's `WhiskerModuleBehaviors.registerAll()` — same plumbing PR #61 already wires up — but in addition to `addBehavior(...)`, also calls:
   - Android: `PropsUpdater.registerSetter(WhiskerVideoView.class, GeneratedSetter())` + `LynxUIMethodsExecutor.registerMethodInvoker(GeneratedInvoker())`
   - iOS: nothing extra. The `LYNX_UI_METHOD` macro + setter category methods are picked up at class load time by Lynx's first-use scan; we keep that scan since it's already efficient (one pass per class).

The Lynx engine's existing fallback paths stay intact for non-Whisker LynxUI subclasses, so this change is fully additive on the Lynx side.

### 2b. Lynx fork changes required

The Android registration APIs are currently package-private in the Lynx upstream. Whisker fork additions (small, targeted):

1. `PropsUpdater.registerSetter(Class<? extends LynxBaseUI>, LynxUISetter<?>)` — expose `PRE_REGISTER_MAP` insertion as a public static method. Currently `PRE_REGISTER_MAP` is `static final HashMap` populated only at class init — needs a `public synchronized` setter.
2. `LynxUIMethodsExecutor.registerMethodInvoker(Class<? extends LynxBaseUI>, LynxUIMethodInvoker<?>)` — same shape. Today the method exists but is package-private.

iOS needs no Lynx fork changes — `LYNX_UI_METHOD` macros and category methods are already first-class citizens of the public API.

These bumps go into a `v3.7.0-whisker.4` Lynx fork release alongside the `lynx_ui_invoke_method` symbol added in `.3` (PR #52's Lynx-side patch).

## 3. Code generation pipeline

### 3a. iOS (Swift Macro)

The `WhiskerModule` base class is annotated with `@WhiskerModuleMacro` (or similar) — a member-attached Swift Macro that:

1. Parses the `definition()` body's DSL via SwiftSyntax (matches calls to `Name`, `View`, `Prop`, `Function`, `AsyncFunction`, `Constants`, `Events`).
2. Emits per-DSL-block helper methods on the `WhiskerModule` subclass:
   - `Prop("src") { ... }` → emits a category method `-(void)setSrc:(NSString*)v` on the `View(WhiskerVideoView.self)` target. The closure body gets inlined.
   - `Function("pause") { ... }` → emits `LYNX_UI_METHOD(pauseWithResult:)` on the target, invoking the closure synchronously and packing the return value into the result block before returning. The `withResult:` block is called inline before `LYNX_UI_METHOD` returns — the caller (Rust via `ElementRef::invoke`) sees a synchronous round-trip.
   - `AsyncFunction("fetchThumbnail") { ... }` → also emits `LYNX_UI_METHOD(fetchThumbnailWithResult:)`, but the closure is wrapped in a `DispatchQueue.global().async { ... }` so the work runs off the dispatch thread; the `withResult:` block is captured and called from the worker once the closure completes. Same underlying Lynx hook; different scheduling.
   - `Name("Video")` → emits a `+ NSString *whiskerComponentTagName()` accessor the registry calls.
3. The aggregator scaffolding from PR #61 finds every `WhiskerModule` subclass and calls `WhiskerComponentRegistry.shared.register(MyModule.self)` at app launch.

### 3b. Android (KSP)

The existing `WhiskerComponentProcessor` is extended (or replaced):

1. Scans for classes extending `WhiskerModule`.
2. Parses the `definition() = ModuleDefinition { ... }` block by traversing the Kotlin AST — KSP gives us symbol-level access to lambda bodies and their captured `Prop("...") { ... }` invocations.
3. Emits a generated `<ModuleClassName>Generated.kt` file per module containing:
   - A `<Module>_PropSetter : LynxUISetter<TargetUI>` implementation with a `when` switch over prop names.
   - A `<Module>_MethodInvoker : LynxUIMethodInvoker<TargetUI>` implementation with a `when` switch over method names.
   - A `<Module>_registerAll()` function that calls `PropsUpdater.registerSetter(...)` + `LynxUIMethodsExecutor.registerMethodInvoker(...)` + `LynxEnv.inst().addBehavior(Behavior("<crate>:<name>") { ... })`.
4. The aggregator KSP entrypoint (also from PR #61) accumulates per-module `registerAll()` calls into the existing `WhiskerModuleBehaviors.registerAll()`.

### 3c. Rust shim

The Rust side adopts the **`ElementRef` + `Handle`** two-tier pattern
from the Phase N design (see `docs/phase-n-ref-api-design.md`). The
module author writes three things:

1. **`VideoHandle`** — a plain `Clone` struct holding signals; this is
   what end-users call methods on.
2. **`fn video(...)`** — a `#[whisker::component]` wrapper that owns an
   internal `ElementRef`, bridges `VideoHandle`'s command signals to
   `ElementRef::invoke(...)` via `effect(...)`, and pushes native
   events back into the handle's query signals.
3. **`fn video_sys(...)`** — the `#[whisker::modules::component]`
   platform-component declaration whose `ref: ElementRef` prop is what
   the wrapper feeds.

```rust
use std::cell::Cell;
use whisker::{ElementRef, WhiskerEvent, WhiskerValue};
use whisker::reactive::{RwSignal, Signal, effect, signal};

// 1. Handle: pure signals, no framework hooks.
#[derive(Clone)]
pub struct VideoHandle {
    play_tick:    RwSignal<u64>,
    pause_tick:   RwSignal<u64>,
    seek_to:      RwSignal<Option<f64>>,
    current_time: RwSignal<f64>,
    duration:     RwSignal<f64>,
}

impl VideoHandle {
    pub fn new() -> Self {
        Self {
            play_tick: signal(0), pause_tick: signal(0),
            seek_to: signal(None),
            current_time: signal(0.0), duration: signal(0.0),
        }
    }
    pub fn play(&self)         { self.play_tick.update(|n| *n += 1); }
    pub fn pause(&self)        { self.pause_tick.update(|n| *n += 1); }
    pub fn seek(&self, t: f64) { self.seek_to.set(Some(t)); }

    pub fn current_time(&self) -> Signal<f64> { self.current_time.read_only().into() }
    pub fn duration(&self)     -> Signal<f64> { self.duration.read_only().into() }
}

// 2. Wrapper component: bridge handle ↔ ElementRef.
#[whisker::component]
pub fn video(handle: VideoHandle, src: Signal<String>) -> Element {
    let sys = ElementRef::new();

    // play_tick → invoke("play"). Counter pattern needs first-fire
    // suppression (effect runs once on creation; without the guard it
    // would emit a stray play() on every mount).
    effect({
        let sys = sys.clone();
        let h = handle.clone();
        let first = Cell::new(true);
        move || {
            h.play_tick.get();
            if first.replace(false) { return; }
            let _ = sys.invoke("play", vec![]);
        }
    });
    effect({
        let sys = sys.clone();
        let h = handle.clone();
        let first = Cell::new(true);
        move || {
            h.pause_tick.get();
            if first.replace(false) { return; }
            let _ = sys.invoke("pause", vec![]);
        }
    });
    // Option pattern: dispatch then reset so re-setting same value re-dispatches.
    effect({
        let sys = sys.clone();
        let h = handle.clone();
        move || {
            if let Some(t) = h.seek_to.get() {
                let _ = sys.invoke("seek", vec![WhiskerValue::Float(t)]);
                h.seek_to.set(None);
            }
        }
    });

    // Native events → query signals.
    let on_time_update = {
        let h = handle.clone();
        move |e: WhiskerEvent| {
            if let Some(t) = e.get_float("currentTime") { h.current_time.set(t); }
        }
    };
    let on_loaded_metadata = {
        let h = handle.clone();
        move |e: WhiskerEvent| {
            if let Some(d) = e.get_float("duration") { h.duration.set(d); }
        }
    };

    render! {
        VideoSys(ref: sys, src: src,
                 on_time_update: on_time_update,
                 on_loaded_metadata: on_loaded_metadata)
    }
}

// 3. Platform-component declaration — the only place ElementRef appears
// in a #[modules::component] signature.
#[whisker::modules::component("Video")]
pub fn video_sys(
    r: ElementRef,
    src: Signal<String>,
    on_time_update: impl Fn(WhiskerEvent),
    on_loaded_metadata: impl Fn(WhiskerEvent),
) {}
```

End-user code only ever sees `VideoHandle` + `Video`:

```rust
let v = VideoHandle::new();
let progress = v.current_time();
let v_play = v.clone();

render! {
    Video(handle: v.clone(), src: signal("video.mp4".into()))
    text(value: move || format!("{:.1}s", progress.get()))
    text(on_tap: move || v_play.play(), value: "▶")
}
```

This replaces the current `#[whisker::element_methods] trait VideoSys
+ impl VideoControls for ElementRef<VideoProps>` triple-declaration
with three explicit Rust pieces — handle, wrapper component, platform
component — each doing one thing. Full design rationale and the four
command-signal patterns are in the [Phase N Ref API design
doc](./phase-n-ref-api-design.md). Highlights:

- **No `WhiskerRef` trait, no derive, no auto-binding.** A handle is
  just a `Clone` struct; the framework has no opinion about its shape.
  Only `#[modules::component]` recognizes `ref: ElementRef` and emits
  `__bind` / `__unbind` calls on the underlying native element.
- **Custom components without a native counterpart need no
  `ElementRef`.** `ModalHandle` (or any signal-only handle) drives a
  `#[component]` that reads its signals via `show!` / `effect`, and
  no bridge code is needed at all.
- **Symmetric with the Kotlin / Swift surface.** Platform-side
  `definition()` declares prop names + method names; Rust-side
  `impl VideoHandle` writes methods of the same names. The wrapper
  component is the routing table that ties the two sides together.
- **Composite handles are plain struct composition.** A
  `CustomInputHandle` holds a `TextInputHandle` as a field and
  forwards methods — see Phase N §5.

A future Rust-side declarative macro (`whisker::module! { ... }`)
that derives all three pieces from a single source-of-truth
declaration is **out of scope for Phase L** — the per-piece manual
declaration is fine for v1 and keeps the codegen story simpler.

## 4. Migration story

Phase L ships the new DSL **alongside** the existing annotation set:

1. PR L-1 (Lynx fork): expose `PropsUpdater.registerSetter` + `LynxUIMethodsExecutor.registerMethodInvoker` as public; bump fork to `v3.7.0-whisker.4`; bump Whisker's `LYNX_FORK_TAG`.
2. PR L-2 (Whisker): add `WhiskerModule` base class + `ModuleDefinition` DSL types + Swift Macro impl + KSP processor extensions. Both annotation and DSL paths work in parallel.
3. PR L-3 (samples): migrate `whisker-video`, `whisker-hello-element`, `whisker-local-store`, hello-world's video demo to the new DSL.
4. PR L-4 (Module Author Guide): update `docs/module-author-guide.md` to lead with the DSL; demote the annotation surface to a "legacy" section.

Phase M (#59) then deprecates → removes the annotation surface in a follow-up release window.

## 5. Open questions

- **Closure capture in KSP-emitted Kotlin code.** When `Prop("src") { view, value: String -> view.setSrc(value) }` is processed, KSP needs to extract the closure body and emit it as the body of the generated `<Module>_PropSetter.setSrc()` method. Two paths:
  - **A. Re-emit the closure verbatim** by reading the source range from KSP's `KSExpression`. Simple but rigid — anything referencing private members of the module class breaks.
  - **B. Keep the closure in the module class** and emit a generated `<Module>_PropSetter` that calls `module.definition().getPropClosure("src")(view, value)`. Loses static dispatch but is robust.
  - Lean toward **A** for the common case (small closures) and **B** as a fallback when KSP can't statically parse the body.
- **`AsyncFunction` semantics on iOS.** `Function(...)` is settled — sync round-trip; the `withResult:` block fires synchronously. `AsyncFunction(...)` is the open question: do we expose it as a Swift `async` closure (forwards-compatible with Swift concurrency), or as a callback closure that calls a handler when work completes? Expo uses Swift `async`. The simpler v1 is **callback-based** (a `(result) -> Void` parameter), with Swift `async` as a v2 sugar layer on top. Same question for Kotlin — `suspend` lambda vs `(callback) -> Unit`. v1 = callback.
- **`Constants` semantics.** Expo's constants are evaluated once at module init. Whisker has no JS host yet — constants are read from Rust at compile time via the `WhiskerModule.getConstants()` accessor. Q: do we want a way to declare *dynamic* constants (e.g. computed from the platform's locale)? Defer; start with static-only.
- **Should `View(...)` accept a closure or a class reference?** Expo uses `View(MyView.self) { ... }`. We follow that — the class reference scopes the inner DSL to a specific `LynxUI<V>` subclass. Multi-view modules are rare; if they come up, support `View(A.self) { ... }` + `View(B.self) { ... }` blocks in the same `definition()`.
- **Backward compatibility for `WhiskerCustomEvent.dispatch(...)`.** Today modules call `WhiskerCustomEvent.dispatch(from: ui, name: "...", params: [...])` to emit events. The new DSL declares `Events("onTap")` upfront but the *dispatch site* is still imperative. Keep `WhiskerCustomEvent.dispatch` as the dispatch helper (unchanged); `Events(...)` just records the event names for type-checking / future doc generation.

## 6. Risk + mitigations

| Risk | Mitigation |
|---|---|
| KSP closure-body extraction breaks under unusual Kotlin syntax (multi-line returns, type ascriptions, etc.) | Option B above as fallback; document the supported subset. |
| Swift Macro performance on large modules with many `Prop` / `Function` declarations | Profile during L-2; cap at e.g. 100 declarations per module; bigger modules can be split. |
| Lynx fork divergence — every Lynx upstream bump may touch `PropsUpdater` / `LynxUIMethodsExecutor` | Keep the registration APIs minimal (one method per registry); rebase fork patches each Lynx bump. |
| Module authors mix DSL and annotation styles in the same file | KSP / Swift Macro errors out with a clear diagnostic; document "pick one" up front. |
| Existing third-party Whisker modules break | Annotation surface stays alive through Phase L; deprecated via warning in Phase M; removed only in M.2. |

## 7. Out of scope for Phase L

- A Rust-side `whisker::module! { ... }` declarative macro that derives all three Rust pieces (Handle, wrapper component, platform-component declaration) from a single source-of-truth declaration. Rust authors write each piece by hand per the [Phase N Ref API design](./phase-n-ref-api-design.md); the platform-side `definition()` is declared separately. Single-source-of-truth comes later if the redundancy ever bites.
- View-less `Function(...)` blocks outside the `View(...) { ... }` scope (i.e. module-level functions). These map to the function-only flavor and need separate bridge plumbing — track as a sub-issue if it comes up.
- Code-generation perf optimizations beyond what's needed to ship (e.g. KSP incremental processing for individual module files). Defer to a polish PR if/when build times become noticeable.

## 8. Estimated work

| Sub-task | Estimate |
|---|---|
| L-1: Lynx fork API exposure + `v3.7.0-whisker.4` bump | 0.5 day |
| L-2: Swift Macro + KSP processor + `WhiskerModule` base class | 4–7 days |
| L-3: migrate three reference modules + hello-world | 1 day |
| L-4: Module Author Guide rewrite + samples | 0.5 day |
| Total | ~7–10 days |

Phase M is a follow-up; not included in this estimate.

## 9. Decision needed before L-2 starts

- [ ] Closure-body strategy (A vs B from §5).
- [ ] Async function semantics (callback vs Swift `async`).
- [ ] Whether to keep `WhiskerCustomEvent.dispatch` as the runtime emit API or fold it into the `WhiskerModule` base class.

Phase N's imperative-control API (`ElementRef` primitive + `Handle`
pattern) is **settled** — see [`docs/phase-n-ref-api-design.md`](./phase-n-ref-api-design.md). The
Rust shim path in §3c assumes that design and lands before or alongside L-2.

Once these are resolved, L-2 implementation can start.
