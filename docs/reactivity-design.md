# Reactivity Design

Whisker's reactive layer is modelled on **Solid.js / Leptos**: components
run **once** at mount, and dynamic UI is driven by `effect` subscriptions
to signals. An update patches the Lynx element tree at exactly the
affected property — there is no virtual DOM and no diff pass.

This doc is the *design* of the runtime — the "why" and "how it's built".
For the user-facing "how to write components" side, see the guide on
[whisker.rs/docs](https://whisker.rs/docs). The implementation lives in
`crates/whisker-runtime/src/reactive/*` (primitives) and
`crates/whisker-runtime/src/view/*` (the renderer that wires effects to
Lynx handles).

## Why fine-grained

1. **Lynx's `FiberElement::SetAttribute(name, value)` is already
   per-property granular.** A fine-grained reactive model maps directly
   onto it — there's no need to build a virtual DOM, diff it, and emit
   patches that target individual attributes. The effect *is* the patch:
   a dynamic `style` / `value` / text node is just an `effect` that calls
   `SetAttribute` / `SetRawInlineStyles` when its dependency changes.
2. **Mobile CPU sensitivity.** Whisker targets Android / iOS, where
   weaker per-core throughput makes virtual-DOM diffing more visible.
3. **Smaller runtime.** No value-tree `Element` representation and no
   diff machinery shrinks the runtime and the shipped dylib.

## Primitives

The reactive module is a single thread-local `ReactiveRuntime` (Whisker
runs all reactive work on Lynx's TASM thread). Every primitive funnels
through it. Public surface is re-exported from `whisker::prelude`.

### Signal

```rust
// Solid-style tuple — read/write capability split into two types
let (count, set_count) = signal(0);
count.get();            // 0  — registers a dependency if inside an effect
set_count.set(1);

// Unified handle — get + set on one value
let count = RwSignal::new(0);
count.get();
count.set(1);
count.update(|n| *n += 1);
let (read, write) = count.split();   // project to halves any time
```

`ReadSignal<T>`, `WriteSignal<T>`, and `RwSignal<T>` are all `Copy`
newtypes over a `NodeId` — the value lives in the runtime arena, the
handle is one machine word. Cloning is free; moving one into a `move ||`
closure ties no lifetime. `computed()` also returns a `ReadSignal<T>`
(it's a compute-driven signal); there is no separate `Computed<T>` /
`Memo<T>` type.

API surface:

```rust
// ReadSignal<T> (also what computed() returns)
fn get(self) -> T where T: Clone;                    // tracked read
fn with<R>(self, f: impl FnOnce(&T) -> R) -> R;      // tracked, no clone
fn get_untracked(self) -> T where T: Clone;          // no dep registration
fn with_untracked<R>(self, f: impl FnOnce(&T) -> R) -> R;

// WriteSignal<T> (and RwSignal<T>)
fn set(self, value: T);
fn update(self, f: impl FnOnce(&mut T));
fn update_untracked(self, f: impl FnOnce(&mut T));   // no subscriber notify
```

`get` / `with` register the currently-running effect (or computed) as a
subscriber. The `_untracked` variants read without subscribing — used
inside an effect when you want to *read* a value but not *react* to it.
`RwSignal` additionally has `try_set` / `try_update` (no-op + `false`
when the slot has been disposed).

`update` is fully generic over the closure (`FnOnce(&mut T)`) — there is
no numeric/`Add` constraint.

There is also an **Arc-backed** family (`arc_signal()` →
`ArcReadSignal` / `ArcWriteSignal` / `ArcRwSignal`) for values that must
outlive the arena / cross owner boundaries by refcount rather than by
`NodeId`. These don't live in the arena; nodes that read them record an
`ArcSubscription` so the back-reference can be severed on re-run /
disposal while the signal's value lives on as long as an Arc is held.

### Effect

```rust
effect(move || {
    log::info!("count is {}", count.get());
});
```

`effect(f)` registers `f` against the current owner and runs it **once
synchronously** (so dependencies are recorded and the side effect has
happened by the time `effect()` returns). Whatever signals it read become
its dependencies; later runs are scheduled through the
[scheduler](#batching--scheduling) and drained at flush time. The effect
is disposed when its owner is disposed.

The closure is `FnMut() + 'static` and takes **no arguments** — the
Solid/Leptos `Option<prev>` previous-value variant is intentionally not
part of this API. Derive-from-previous can be done with a captured local
or a `StoredValue`.

### Computed

```rust
let doubled: ReadSignal<i32> = computed(move || count.get() * 2);
doubled.get();           // 0
set_count.set(5);
flush();
doubled.get();           // 10
```

An effect that caches its return value. The returned handle is a
`ReadSignal<T>` — identical to what a primitive `signal()` hands out — so
any component prop that takes "a readable reactive value" accepts
`ReadSignal<T>` regardless of whether the source is `signal()` or
`computed()`.

Two design points:

- **Seeded under `untrack`.** The construction-time run that initialises
  the cache happens inside `untrack`, so its reads don't leak as
  dependencies of whatever outer effect/computed the `computed()` call
  sits inside. Real dependency edges are registered by the immediate
  scheduler-driven run. Without this, `computed(move || sig.get())` from
  inside a component-mount effect would subscribe that mount to `sig` and
  leak a fresh node on every write.
- **`PartialEq`-gated notification.** Subscribers are only notified when
  the recomputed value *differs* from the cached one (`T: PartialEq`), so
  a computed whose result is unchanged costs nothing downstream.

### Resource

Async data fetched off the reactive graph and exposed through a
signal-shaped handle:

```rust
let stories = resource(|| async {
    run_blocking(|| ureq::get(url).call()?.into_string())
        .await
        .and_then(parse)
});
```

`resource(fetcher)` spawns the `async` fetcher on Whisker's
single-threaded local pool (the TASM thread). Blocking IO inside the
fetcher should be wrapped in `tasks::run_blocking`, which offloads to a
worker thread and marshals the result back to the main thread.

`Resource<T>` is `Copy` and exposes `state() -> ResourceState<T>`
(`Loading | Ready(T) | Error(String)`), plus the conveniences
`loading() -> bool`, `error() -> Option<String>`, and `get() ->
Option<T>` (`None` while loading). Read these in an `effect` / `computed`
/ `{expr}` to drive loading and ready UI; `resource_sync` is the
non-async variant for a pre-computed value. (A `Suspense` boundary is not
yet implemented — handle loading state explicitly via `loading()` /
`get()`.)

### `StoredValue<T>`

A `Copy`, owner-bound, **non-reactive** arena slot — the role
`Rc<RefCell<…>>` plays in vanilla Rust, but tied to a scope so it's freed
on dispose. Internally a signal-shaped node minus the subscribers/sources
plumbing: reads and writes never tick the dependency graph.

```rust
let history: StoredValue<Vec<String>> = StoredValue::new(Vec::new());
```

### `Signal<T>` — the prop-value type

A 2-variant sum used by built-in tag builders, `#[component]`, and
`#[whisker::module_component]` to receive prop values that may be either a
static `T` or a reactive `ReadSignal<T>`. One unified type lets all three
component surfaces share one calling convention.

```rust
pub enum Signal<T: 'static> {
    Static(T),                // set once, no subscription
    Dynamic(ReadSignal<T>),   // tracked: builder wraps the read in effect
}
```

`From` impls drive the implicit call-site conversion (builders take
`impl Into<Signal<T>>`):

| Source                  | Variant                    |
|-------------------------|----------------------------|
| `T`                     | `Static(value)`            |
| `ReadSignal<T>`         | `Dynamic(signal)`          |
| `RwSignal<T>`           | `Dynamic(rw.read_only())`  |
| `&str` (when T=String)  | `Static(s.to_string())`    |
| `computed(…)` result    | `Dynamic(…)` (it's a `ReadSignal<T>`) |

So the **Static vs Dynamic decision is visible at the call site**:

```rust
text(value: "literal")            // → Static
text(value: my_string)            // → Static
text(value: my_signal)            // → Dynamic (reactive)
text(value: my_rw_signal)         // → Dynamic (reactive)
text(value: computed(move || …))  // → Dynamic (memoised derivation)
text(value: my_signal.get())      // → Static (snapshot — read happens at
                                  //   the call site, before any effect
                                  //   is on the observer stack)
```

Inside the builder, the dispatch happens once per setter call:

```rust
match v.into() {
    Signal::Static(t)   => set_attribute(h, name, &t.to_string()),
    Signal::Dynamic(sig) => effect(move || set_attribute(h, name, &sig.get().to_string())),
}
```

`Signal::<T>::get()` reads a prop inside a component body: `Static`
returns a clone; `Dynamic` forwards to `ReadSignal::get`, registering the
underlying signal with whatever effect/computed is on the observer stack.
`Signal<T>` is `Clone` when `T: Clone` — the `Dynamic` arm just copies the
`ReadSignal` handle — which matters because `#[component]` re-clones every
prop on each body invocation.

#### Why not auto-wrap kwargs in `render!`?

An earlier design had the macro silently wrap each kwarg in
`move || …` so the builder always received a closure, making
`text(value: signal.get())` reactive with no user effort. That was
dropped because it was (1) asymmetric — built-in tags got the auto-wrap
but `#[component]` calls didn't; (2) hidden DX — no syntactic marker for
where the reactive boundary was; and (3) closure-only — static values
couldn't skip the effect overhead. The explicit model puts the reactive
boundary on the call site (`signal` vs `signal.get()`), exactly as Leptos
(`MaybeSignal<T>`) and Solid's JSX (`prop={signal}` vs `prop={signal()}`)
do.

## Arena + Owner

All reactive state lives in the thread-local `ReactiveRuntime`. Whisker
runs on a single Lynx TASM thread, so single-threaded (no `Arc`, no
locks) is both correct and borrow-checker-clean.

```rust
struct ReactiveRuntime {
    owners: SlotMap<Owner, Scope>,
    nodes:  SlotMap<NodeId, ReactiveNode>,  // signals + effects + computeds
    owner_stack:     Vec<Owner>,            // top = current owner
    current_tracker: Option<NodeId>,        // effect/computed being run
    pending:         Vec<NodeId>,           // batched re-run queue
    component_owners: HashMap<*const (), Vec<Owner>>,  // for hot reload
    // … flushing flag, deferred queue (paused scopes), pending_mounts …
}

struct Scope {                              // the owner record
    parent:   Option<Owner>,
    children: Vec<Owner>,
    nodes:    Vec<NodeId>,                  // reactive nodes freed on dispose
    contexts: HashMap<TypeId, Rc<dyn Any>>, // provide/use_context bag
    cleanups: Vec<Box<dyn FnOnce()>>,       // on_cleanup, LIFO
    mount_fn: Option<*const ()>,            // component fn-ptr, for hot reload
    elements: Vec<Element>,                 // Lynx handles to release on dispose
    paused:   bool,                         // pause/resume — see below
}

struct ReactiveNode {
    owner: Owner,
    data:  NodeData,                        // Signal | Effect | Computed
    sources:     HashSet<NodeId>,           // who I read last run
    subscribers: HashSet<NodeId>,           // who reads me
    arc_sources: Vec<Rc<dyn ArcSubscription>>,  // Arc-signal back-refs
}
```

`Owner` is a `Copy` slotmap key — the public handle. The `Scope` record
it dereferences to is never named by user code. The owner stack's top is
the "current" owner: new signals/effects/computeds and lifecycle hooks
register against it. `#[component]` and `Owner::with` push/pop it.

**Disposal cascades.** `Owner::dispose` recursively disposes children,
runs `cleanups` LIFO, frees the scope's reactive nodes (severing
subscriber links and Arc back-refs), and releases the scope's Lynx
`Element` handles back to the renderer — preventing the bridge's element
map from accumulating dangling pointers across `Show` flips, `ForEach`
removals, and per-component remounts.

**Pause / resume.** `Owner::pause` / `resume` set the `paused` flag and
cascade it down the subtree; effects/computeds owned by a paused scope
skip flush (deferred until resume). `whisker-router`'s `StackLayout` uses
this to freeze mounted-but-off-screen back-stack entries.

## Component model

```rust
#[component]
fn counter(initial: i32, on_change: WriteSignal<i32>) -> Element {
    let (count, set_count) = signal(initial);

    effect(move || on_change.set(count.get()));
    on_cleanup(|| log::info!("counter unmounted"));

    render! {
        view(style: "flex-direction: column; padding: 16px;") {
            text(value: computed(move || format!("Count: {}", count.get())))
            view(on_tap: move |_| set_count.update(|n| *n += 1)) {
                text(value: "+")
            }
        }
    }
}
```

The `#[component]` macro generates, for `fn xxx(...)`:

1. A `XxxProps` struct mirroring the parameters + a hand-rolled
   `XxxPropsBuilder` (one setter per field). Required fields take
   `impl Into<Type>`, so call sites omit conversions; for `Signal<T>`
   props that's the `Into<Signal<T>>` coercion above. `#[prop(default =
   …)]` marks optional props. (Hand-rolled rather than `#[derive(
   TypedBuilder)]` so only two types surface — the `Props` struct and one
   builder — instead of typed-builder's per-field type-state markers.)
2. A rewritten `fn xxx(__props: XxxProps) -> Element` that creates a
   fresh owner, runs the user body inside it (under `untrack`, so ambient
   `signal.get()` reads in the body don't contaminate an outer node), and
   returns the view via `mount_component_remountable`.
3. A PascalCase alias (`Xxx`) the `render!` macro calls as
   `Xxx(XxxProps::builder().k(v).build())`.

Lifecycle hooks register against the current owner:

```rust
on_mount(|| /* after the view is appended to its parent */);
on_cleanup(|| /* on owner disposal, LIFO */);
```

Context walks the owner tree:

```rust
provide_context(ThemeMode::Dark);             // store on current scope
let theme = use_context::<ThemeMode>();       // walk parents for a TypeId
with_context::<ThemeMode, _>(|t| /* … */);    // borrow without cloning out
```

### Control flow: `Show` and `ForEach`

The control-flow primitives are `Show` (conditional) and `ForEach`
(keyed list) — written as **ordinary `#[component]` functions** in
`crates/whisker/src/control_flow.rs`. There's no `ControlFlow` trait, no
special `View` variant, and no special path through the surrounding
builder — `render!` treats `Show(…)` / `ForEach(…)` like any other
component invocation.

Each one allocates a **phantom element** (`create_phantom_element`, no
on-screen footprint) and installs a reactive `effect` that mounts /
disposes children under it. The phantom's hoisting machinery routes each
child mount/detach to the nearest *real* (non-phantom) Lynx ancestor, so
the on-screen tree is wrapper-less while user code keeps its hierarchical
mental model.

```rust
Show {
    when: move || cond.get(),
    fallback: || render! { /* optional */ },
    /* children */
}

ForEach {
    each: move || items.get(),
    key:  |item| item.id,
    children: move |item| render! { /* per item */ },
}
```

- **`Show`** watches `when` in an effect. On each flip it disposes the
  previously-mounted branch's owner (cascading cleanup) before mounting
  the other branch, so reactive state can't leak between branches.
- **`ForEach`** re-keys against the previous frame: survivors (same key)
  keep their per-item owner and any reactive state inside; new keys get a
  fresh owner + `children(item)` run; missing keys are disposed.
  Reordering detaches every surviving handle and re-attaches in the new
  order. This is the closest thing to "vDOM diff" in the model, but it's
  scoped to one list and keyed — efficient.

Custom control flow follows the same recipe (phantom + effect); the
`#[component]` form in `control_flow.rs` is the reference.

### `IntoView`

Components return `impl IntoView`; the renderer (or parent's `render!`
expansion) calls `.into_view()` to get a `View` — either a single
`Element`, a fragment of children, or a marker view used by control flow.
`Element` itself, `()` (empty fragment), tuples, and `Option<T>` all
implement it; the conventional `children: Children` prop is a cheap
`Rc<dyn Fn() -> View>` so it survives hot-reload re-invocation.

## Batching / scheduling

A signal `set` does **not** run subscribers immediately. It appends them
to the runtime's `pending` queue. The queue is drained by `flush`:

1. Explicitly, and at the end of the current event handler / effect — the
   Solid/Leptos microtask-batching model.
2. Implicitly: the first enqueue on an empty queue pings the host's
   request-frame callback (`host_wake::wake_runtime`) so the runtime can
   wake out of idle.

Within a batch, `flush` reentrantly drains until the queue is empty (an
effect that writes a signal appends more work; a `FLUSH_ITERATION_CAP`
guards against runaway feedback loops). A node already in the queue is
not re-enqueued, so a signal written several times in one batch produces
one re-run of each subscriber:

```rust
on_tap: move |_| {
    set_a.set(1);
    set_b.set(2);
    set_c.set(3);
}   // → one flush at handler exit; an effect reading (a,b,c) runs once
```

## Hot reload — per-component remount

When subsecond patches functions, the runtime:

1. Receives the patched fn pointers from `subsecond::apply_patch`.
2. For each ptr, finds matching live owners via `component_owners`
   (populated by `mount_component`, which records `mount_fn`).
3. For each match: **dispose** the previous component owner (cascading
   cleanup + node freeing), **re-invoke** the body closure under a fresh
   owner, and **re-attach** the new body root in place.

`#[component]` wraps the user body in a re-callable closure
(`mount_component_remountable`), capturing props by move into a factory
scope and re-cloning them on each invocation — so a `Copy` prop is a
copy, a `Clone` prop is a real clone (paid only on remount), and a
non-`Clone` prop is a compile error (wrap in `Rc`/`Arc`).

Because signals are **named, not positional**, this never suffers the
slot-shift state corruption that a re-run-and-diff model has when you add
a `signal()` at the top of a function. The trade-off is that structural
edits (adding an element, adding a `signal`/`effect`, editing static
styles) remount the component and lose its *local* state. State that
should survive hot-reload belongs in a higher owner — typically an
`AppState` held by the top-level component and shared via
`provide_context`; leaf edits then wipe only leaf-local signals.

| Edit | Outcome |
|---|---|
| Body of an existing `effect` / `computed` / event handler | New code runs next time; state preserved |
| Body of an existing dynamic `{expr}` in `render!` | Updates next time deps change; state preserved |
| Adding an element / `signal` / `effect` in a `#[component]` body | Component remounted; local state lost; parent attachment + sibling order preserved |
| Editing static styles / attributes | Component remounted; local state lost |
| Edit to a non-`#[component]` helper invoked via `{helper()}` | Effect re-fires with the patched body; state preserved |
| Edit to top-level `app()` (`#[whisker::main]`) | Needs a manual restart |
</content>
