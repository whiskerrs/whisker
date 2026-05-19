# Reactivity Design (Phase 6.5a)

Whisker's reactive layer is modelled on **Solid.js / Leptos**: components
run **once** at mount, and dynamic UI is driven by `effect` subscriptions
to `Signal`s. Updates patch the Lynx element tree at exactly the affected
properties — no virtual DOM, no diff.

This doc captures the architecture decisions for the Phase 6.5a rewrite.
It is the source of truth that the implementation issues (#9–#14) and
future contributors should read first.

## Why fine-grained

1. **Lynx's `FiberElement::SetAttribute(name, value)` is already
   per-property granular.** A fine-grained reactive model maps directly
   onto it — there's no need to build a virtual DOM, diff it, and emit
   patches that target individual attributes. The effect *is* the patch.
2. **Mobile CPU sensitivity.** Whisker targets Android / iOS where
   weaker per-core throughput makes the virtual-DOM cost more visible.
3. **Smaller runtime.** Dropping `diff.rs` (~300 LOC) and the value-tree
   `Element` representation simplifies the runtime and shrinks the
   dylib.

## Primitives

### Signal

Two forms — both legal, choose by ergonomics:

```rust
// Solid-style tuple — read/write separation
let (count, set_count) = signal(0);
count.get();           // 0
set_count.set(1);

// Unified handle — get/set on the same value
let count = RwSignal::new(0);
count.get();
count.set(1);
count.update(|n| *n += 1);
let (read, write) = count.split();   // any time
```

All four types (`ReadSignal<T>`, `WriteSignal<T>`, `RwSignal<T>`,
`Memo<T>`) are `Copy` arena handles — clone is free, `'static`,
moves into closures without lifetime annotations.

API surface:

```rust
// ReadSignal<T> (and Memo<T>, which derefs to ReadSignal)
fn get(&self) -> T where T: Clone;
fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R;
fn get_untracked(&self) -> T where T: Clone;        // no dep registration
fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R;

// WriteSignal<T> (and RwSignal<T>)
fn set(&self, value: T);
fn update(&self, f: impl FnOnce(&mut T));
fn update_untracked(&self, f: impl FnOnce(&mut T));  // no subscriber notify
```

`get` / `with` register the current effect as a subscriber.
`get_untracked` / `with_untracked` read without subscribing — used inside
effects when you want to *read* a value but not *react* to it.

### Effect

```rust
effect(move || {
    log::info!("count is {}", count.get());
});
```

Runs once at registration. Whatever signals it reads become its
dependencies. When any dependency changes, the effect re-runs (after the
microtask flush — see [Batching](#batching)). Disposed when its owner
is disposed.

The closure receives `Option<R>` of the previous return value if needed:

```rust
effect(|prev: Option<i32>| {
    let new = count.get();
    log::info!("delta = {}", new - prev.unwrap_or(0));
    new
});
```

### Memo

```rust
let doubled = memo(move || count.get() * 2);
log::info!("{}", doubled.get());           // 0
set_count.set(5);
log::info!("{}", doubled.get());           // 10
```

Like an effect that caches its return value. Other reactive nodes read
it through `get` / `with` as if it were a signal. **Lazy**: re-runs only
when dependencies change *and* it has at least one subscriber.

### Resource (Phase A4)

Async data fetched off the reactive graph:

```rust
let id = signal(1);
let user = resource(
    move || id.get(),                       // re-fetch when this changes
    |id| async move { fetch_user(id).await },
);

render! {
    Suspense {
        fallback: || render! { text { "loading..." } },
        text { "User: " {move || user.get().map(|u| u.name)} }
    }
}
```

Exposes `loading() -> bool`, `error() -> Option<Error>`, and
`get() -> Option<T>` (`None` while loading, `Some` on success). Inside a
`<Suspense>` boundary, reading `user.get()` suspends until ready.

### `StoredValue<T>`

Non-reactive arena slot — handy for non-`Copy` types you need to share
across closures without `Rc<RefCell<…>>`. Owned by the current owner,
cleaned up with it.

```rust
let history: StoredValue<Vec<String>> = StoredValue::new(Vec::new());
effect(move || history.update(|h| h.push(format!("count={}", count.get()))));
```

## Arena + Owner

All reactive state lives in a thread-local `ReactiveRuntime`. Whisker
runs on a single Lynx TASM thread, so single-threaded is fine.

```rust
struct ReactiveRuntime {
    owners: SlotMap<OwnerId, Owner>,
    nodes:  SlotMap<NodeId, ReactiveNode>,    // signals + effects + memos
    current_owner:   Option<OwnerId>,         // stack top
    current_tracker: Option<NodeId>,          // effect being run
    pending: Vec<NodeId>,                     // batched effect queue
    component_owners: HashMap<FnPtr, Vec<OwnerId>>,   // for hot reload
}

struct Owner {
    parent:   Option<OwnerId>,
    children: Vec<OwnerId>,
    nodes:    Vec<NodeId>,
    contexts: HashMap<TypeId, Box<dyn Any>>,
    cleanups: Vec<Box<dyn FnOnce()>>,
    mount_fn: Option<FnPtr>,                  // for components only
}

struct ReactiveNode {
    owner:  OwnerId,
    kind:   NodeKind,                          // Signal | Effect | Memo
    value:  Option<Box<dyn Any>>,
    sources:     HashSet<NodeId>,              // who I depend on
    subscribers: HashSet<NodeId>,              // who depends on me
}
```

**Disposal cascades.** Disposing an owner cleans up its children
recursively, then its nodes, then runs its `cleanups` (LIFO). A disposed
signal read returns its last value in `release` builds with a warning
log; in `debug` builds it panics.

## Component model

```rust
#[component]
fn counter(initial: i32, on_change: WriteSignal<i32>) -> impl IntoView {
    let count = signal(initial);

    effect(move || on_change.set(count.0.get()));
    on_cleanup(|| log::info!("counter unmounted"));

    render! {
        view {
            style: "padding: 16px;",
            text { "Count: " {count.0} }
            view {
                on_tap: move |_| count.1.update(|n| *n += 1),
                text { "+" }
            }
        }
    }
}
```

The `#[component]` macro:

1. Wraps the body so a fresh `Owner` is created, pushed as
   `current_owner`, popped on return.
2. Records the fn pointer in `component_owners[fn_ptr]` so hot reload
   can find owners that ran this function.
3. Returns `impl IntoView` (see [Type erasure](#type-erasure-intoview)).

Lifecycle hooks register against `current_owner`:

```rust
on_mount(|| /* … */);          // after render, before first paint
on_cleanup(|| /* … */);        // on owner disposal
```

Context (parent → descendant value passing):

```rust
#[component]
fn app() -> impl IntoView {
    provide_context(ThemeMode::Dark);
    render! { my_inner_comp {} }
}

#[component]
fn my_inner_comp() -> impl IntoView {
    let theme = use_context::<ThemeMode>().unwrap();  // walks owner tree
    /* … */
}
```

## render! macro

Replaces the current `rsx!`. Generates **effects**, not value trees.
Renamed for clarity now that the macro's job is "render this view into
the Lynx tree", not "build an expression of nested rsx values".

### Input

```rust
render! {
    view {
        style: format!("color: {}", color.get()),     // dynamic
        on_tap: move |_| count.update(|n| *n += 1),

        text { "Count: " {count} }                    // dynamic interp

        Show {
            when: move || count.get() > 5,
            fallback: || render! { text { "small" } },
            text { "big!" }
        }

        For {
            each: move || items.get(),
            key: |i| i.id,
            children: move |i| render! { text { {i.name} } },
        }
    }
}
```

### Compilation (conceptual)

```rust
{
    let view_el = create_view();
    view_el.set_event_handler("tap", move |_| count.update(|n| *n += 1));
    effect(move || view_el.set_inline_styles(&format!("color: {}", color.get())));

    let text_el = create_text();
    let static_part  = create_raw_text("Count: ");
    let dynamic_part = create_raw_text("");
    text_el.append_child(static_part);
    text_el.append_child(dynamic_part);
    effect(move || dynamic_part.set_text(&count.get().to_string()));
    view_el.append_child(text_el);

    let show = Show::mount(view_el, /* when */, /* fallback */, /* children */);
    let for_ = For::mount(view_el, /* each */, /* key */, /* children */);

    view_el
}
```

Rules:

- **Static attributes / styles**: set at element creation. No effect.
- **Dynamic `{expr}` or `format!(...)` values**: wrapped in an `effect`
  that updates the affected attribute / style / text node.
- **Event handlers**: registered at creation. The handler closure body
  itself can be hot-patched, but the registration is a one-shot.

### Control flow

`Show` and `For` are the **only** control flow primitives in v1.
`Switch / Match / ErrorBoundary` come later.

#### `Show`

```rust
Show {
    when: move || cond.get(),
    fallback: || render! { /* optional */ },
    /* main children */
}
```

Implementation: a small component with an `effect` watching `when`. On
transition, it mounts / disposes the relevant branch's children.

#### `For`

```rust
For {
    each: move || items.get(),
    key: |item| item.id,
    children: move |item: Item| render! { /* per item */ }
}
```

Keyed list reconciliation:

- Diff prev vs new keys (LCS-ish or simpler "moved / added / removed").
- Reused items: stay mounted, owner intact, signal state preserved.
- Added items: new owner + mount.
- Removed items: dispose owner.

This is the closest thing to "vDOM diff" in a fine-grained model, but
scoped to one list and keyed (= efficient).

## Type erasure: `IntoView`

```rust
pub trait IntoView {
    fn into_element(self) -> ElementHandle;
}
```

Implementations:
- `ElementHandle` — identity
- `()` — empty fragment
- `(A, B, C, …)` tuples — fragment of children
- `Option<T: IntoView>` — empty when `None`
- Closures `Fn() -> impl IntoView` used as children of `Show` / `For`

Components implicitly return `impl IntoView` via the `#[component]` macro.

## Batching / scheduling

A signal `set` does **not** run subscribers immediately. It enqueues
them in `pending`. The queue is flushed:

1. At the **end of the current event handler / effect** (synchronous
   microtask). This is the Solid/Leptos model.
2. When the runtime is otherwise idle and `wake_runtime()` fires the
   host's "request frame" callback.

Within a single batch:
- Effects are run in topological order over their dependency graph
  (cycles are detected and warn-logged).
- A signal written multiple times within the same batch coalesces into
  one notification to subscribers.

This means inside an event handler:

```rust
on_tap: move |_| {
    set_a.set(1);
    set_b.set(2);
    set_c.set(3);
}
```

…produces **one** flush at the end, regardless of how many signals
were touched, and effects depending on multiple of `(a, b, c)` run
exactly once.

## Hot reload — Strategy C (per-component remount)

When subsecond patches functions, the runtime:

1. Receives the list of patched fn pointers from
   `subsecond::apply_patch`'s `Ok(Vec<*const ()>)` return.
2. For each ptr, finds matching mount sites in
   `runtime.fn_ptr_mounts`.
3. For each site: **detach** the previous body root from the
   permanent wrapper element, **dispose** the previous component
   owner (cascading cleanup + reactive node freeing), **re-invoke**
   the body closure under a fresh owner, **re-attach** the new
   body root to the same wrapper.

The wrapper element is created at first mount and lives under the
*parent*'s owner — it survives every remount, so the parent's
child list is untouched and navigation / scroll position / sibling
order are preserved.

### Macro-emitted body shape

`#[component]` wraps the user body in a `Box<dyn Fn() -> ElementHandle>`,
capturing props by move into a factory scope and re-cloning them on
each body invocation:

```rust
// User writes:
#[component]
fn screen(name: String, count: ReadSignal<i32>) -> ElementHandle { … }

// Macro emits (roughly):
fn screen(name: String, count: ReadSignal<i32>) -> ElementHandle {
    let __whisker_prop_name = name;
    let __whisker_prop_count = count;
    let __body: Box<dyn Fn() -> ElementHandle + 'static> = Box::new(move || {
        let name  = Clone::clone(&__whisker_prop_name);
        let count = Clone::clone(&__whisker_prop_count);
        // user body
    });
    mount_component_remountable(screen as *const (), __body)
}
```

For `Copy` types `Clone::clone` is a copy. For `Clone`-not-`Copy`
types it's a real clone (paid only on remount). For non-`Clone`
types it's a compile error — wrap in `Rc<T>` / `Arc<T>` if needed.

### Coverage and limitations

| Edit | Outcome |
|---|---|
| Body of an existing `effect` / `memo` / event handler | New code runs next time; state preserved |
| Body of an existing dynamic `{expr}` in `render!` | Updates next time deps change; state preserved |
| Adding a new static element in `#[component]`'s `render!` | Component remounted; local state lost; parent attachment + sibling order preserved |
| Adding a new `signal()` / `effect()` / `memo()` inside the component body | Component remounted; local state lost |
| Editing static styles / attributes | Component remounted; local state lost |
| Edits to a non-`#[component]` helper invoked via `{helper()}` | Effect re-fires with patched helper body; state preserved |
| Edits to the top-level `app()` (`#[whisker::main]`) | Not currently re-invoked; needs manual restart |

**Best practice for users**: keep state that should survive
hot-reload in higher owners — typically an `AppState` struct held
by the top-level component and made available to descendants via
`provide_context`. When you iterate on leaf component bodies,
their local signals get wiped but the context-stored state is
unaffected.

### Comparison with current (Dioxus-style coarse)

Current Whisker re-runs `app()` on any change and diffs the resulting
tree. State preservation depends on `use_signal` slot stability —
adding a signal at the start of the function shifts every later slot
and corrupts unrelated state.

Strategy C never has slot-shift corruption (signals are named, not
positional), but loses state more often for structural edits. In
practice, well-composed apps lose less *total* state under Strategy C
because the blast radius is scoped to one component subtree.

## Migration & deletion plan

- `crates/whisker-runtime/src/diff.rs` — **delete** (replaced by
  fine-grained effect updates).
- `crates/whisker-runtime/src/render.rs` — **rewrite** as the renderer
  that walks `IntoView` and wires effects to FiberElement handles.
- `crates/whisker-runtime/src/element.rs` — `Element` value-tree type
  becomes internal-only; `ElementHandle` (the FiberElement-wrapping
  Copy handle) is the new public type returned from `into_element`.
- `crates/whisker-macros/src/rsx.rs` — **rewrite** as `render.rs` (new
  macro name).
- `crates/whisker-runtime/src/signal.rs` — extend with effect / memo /
  owner / scheduler. The current `Signal` API survives in spirit but
  the closed `T: Add + From<i32>` constraint on `update` goes away.
- `crates/whisker/src/prelude.rs` — re-export the new surface.

## Implementation order

Tracked in #8 with sub-issues:

- **A1** (#9) Reactive primitives + arena + batching
- **A2** (#10) Component model + lifecycle + context
- **A3** (#11) `render!` macro rewrite + `Show` + `For` + `IntoView`
- **A4** (#12) `Resource` + `Suspense`
- **A5** (#13) Examples + docs
- **A6** (#14) Hot reload Strategy C
