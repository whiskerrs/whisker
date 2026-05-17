# Reactivity — user guide

Whisker uses Leptos-style fine-grained reactivity. The mental model
is simple once you accept its single non-obvious rule:

> **A component function runs exactly once.** The reactive primitives
> it creates — signals, effects, memos — form a graph that lives on
> after the function returns, and that graph is what makes the UI
> respond to state changes.

This guide walks the primitives, how they compose, and the patterns
that fall out. The companion `docs/render-macro.md` covers the
`render!` macro syntax in detail.

For the architecture rationale behind this design, see
`docs/reactivity-design.md`.

---

## Signals

A signal is a reactive value. Reading it inside a tracked context
(an effect, memo, or `render!` interpolation) registers a
subscription; writing it schedules every subscriber to re-run.

Two equivalent ways to create one:

```rust
use whisker::prelude::*;

// Solid-style tuple: read half + write half are distinct types.
// Lets you give a child component a read-only signal without
// also handing over write capability.
let (count, set_count) = signal(0);
count.get();              // 0
set_count.set(7);
count.get();              // 7

// Unified handle: read and write through the same `Copy` value.
let count = RwSignal::new(0);
count.get();
count.set(7);
count.update(|n| *n += 1);

// Round-trip between the two forms.
let (read, write) = count.split();
```

All four types (`ReadSignal<T>`, `WriteSignal<T>`, `RwSignal<T>`,
`Memo<T>`) are `Copy`. Hand them to closures freely:

```rust
let (count, set_count) = signal(0);
view! { /* ... */ }                   // `count` works
let increment = move || set_count.set(count.get() + 1);
```

### Borrowed reads

`.get()` clones the value. For non-`Clone` types or expensive
clones, use `.with(|v: &T| -> R) -> R`:

```rust
let (names, _) = signal(vec!["a".to_string(), "b".to_string()]);
let len = names.with(|v| v.len());
```

### Untracked reads

By default reads inside an effect / memo / `{expr}` register a
dependency. Sometimes you want to read a signal without subscribing
— `get_untracked` / `with_untracked` skip the tracking step:

```rust
effect(move || {
    let tracked = a.get();              // re-runs when a changes
    let snapshot = b.get_untracked();   // does NOT re-run when b changes
    log::info!("{tracked} {snapshot}");
});
```

### Choosing `set` vs `update`

```rust
write.set(new_value);            // replace whole value
write.update(|v| *v += 1);       // mutate in place — better for collections
write.update(|v| v.push(item));
```

`update` is essential for non-`Clone` types: `set` requires you to
already have the new value, whereas `update` hands you `&mut T`.

---

## Effects

`effect(|| ...)` runs the closure once on creation, then again every
time any signal it read changes:

```rust
let (count, set_count) = signal(0);

effect(move || {
    log::info!("count is now {}", count.get());
});
//  ^ logs "count is now 0" on registration

set_count.set(5);   // logs "count is now 5"
set_count.set(8);   // logs "count is now 8"
```

Effects are the bridge from reactive state to "the outside world" —
logging, persistence, animations, opening a connection. They never
return a value other subscribers can read; for that, use `memo`.

### Effects and ownership

Effects belong to the current reactive owner — usually the
`#[component]` they were created in. When the component unmounts,
its effects stop running and are freed.

### When effects fire

Effects don't run synchronously when you set a signal. They're
queued and drained at a flush boundary — typically the end of an
event handler, an `on_mount` callback, or a `tick()`. This means:

```rust
on_tap: move || {
    set_a.set(1);
    set_b.set(2);
    set_c.set(3);
    // ↑ effects depending on a/b/c run ONCE here when the handler
    //   returns, not three times.
}
```

This is **microtask batching** in Solid / Leptos terminology.

---

## Memos

A memo is an effect with a cached return value. Other reactive code
reads it like a signal:

```rust
let (count, set_count) = signal(0);
let doubled = memo(move || count.get() * 2);

doubled.get();                // 0
set_count.set(7);
doubled.get();                // 14
```

Memos are **lazy** (recompute only when a subscriber is reading)
and **value-stable** (don't notify subscribers if the new computed
value equals the previous one). The latter is what makes:

```rust
let bucket = memo(move || count.get() / 10);
effect(move || log::info!("bucket = {}", bucket.get()));
```

…log just twice when `count` goes `0 → 5 → 12` rather than three
times — because the bucket value didn't change on the `0 → 5`
transition.

---

## Components

`#[component]` wraps a function so it runs inside a fresh reactive
owner. Hot-reload uses the owner registry to remount affected
components when their function bodies are patched (#14 / Strategy
C).

```rust
use whisker::prelude::*;
use whisker::runtime::view::ElementHandle;

#[component]
fn counter(initial: i32) -> ElementHandle {
    let count = RwSignal::new(initial);

    effect(move || log::info!("count is {}", count.get()));

    render! {
        view {
            text { "Count: " {count.get()} }
            view {
                on_tap: move || count.update(|n| *n += 1),
                text { "+1" }
            }
        }
    }
}
```

The body runs **once**, when the component mounts. Reactivity is
provided by the signals + effects you create inside.

### Props

Props are positional arguments. There's no separate "props struct";
just write the function:

```rust
#[component]
fn greeting(name: &'static str, count: ReadSignal<i32>) -> ElementHandle {
    render! {
        text { "Hello, " {name} " ×" {count.get()} }
    }
}
```

To share writable state with a child, pass `WriteSignal<T>` (or the
whole `RwSignal<T>` if both halves are needed).

#### Each prop type must be `Clone`

This is the only framework-imposed bound on props. Every prop type
must implement `Clone` (or `Copy`, which implies it).

```rust
#[component]
fn user_card(user: User, badges: Vec<Badge>) -> ElementHandle { /* ... */ }

#[derive(Clone)]                    // ← required
struct User { name: String, id: u64 }

#[derive(Clone)]                    // ← required
struct Badge { label: String }
```

**Why.** When subsecond patches a component's function body during
hot reload (tier 1), the runtime disposes the old owner and
re-invokes the body with the *same* props. To call the body again,
it clones the stored props.

**This clone never runs in production.** Hot reload is a
development-only path; release builds don't enable the subsecond
patch loop. The `Clone` bound is a static contract for the dev
loop, not a runtime cost on shipped apps. (This is the key
difference from Dioxus, where `Clone` on props is required because
the framework clones on every parent re-render.)

**What naturally satisfies the bound.** Almost every type you'd
want to put in a prop is already `Clone`:

| Prop type | `Clone`? | Cost |
|---|---|---|
| Primitives (`i32`, `bool`, `f64`, ...) | yes (`Copy`) | free |
| `String`, `Vec<T>`, `HashMap<K, V>` | yes | heap alloc, but only at patch time |
| `&'static str`, `&'static T` | yes (`Copy`) | free |
| `Rc<T>`, `Arc<T>` | yes | refcount inc only |
| Signal handles (`RwSignal`, `ReadSignal`, ...) | yes (`Copy`) | free |
| Your own structs with `#[derive(Clone)]` | yes | one derive line |

**For non-`Clone` types**, wrap them in `Rc<T>` (single-threaded) or
`Arc<T>` (cross-thread):

```rust
// `Box<dyn Fn() + 'static>` is not Clone, but Rc<dyn Fn() + 'static> is.
#[component]
fn button(label: &'static str, on_click: Rc<dyn Fn() + 'static>) -> ElementHandle {
    render! {
        view {
            on_tap: move || on_click(),
            text { {label} }
        }
    }
}
```

`File`, `MutexGuard`, sockets, and similar move-only resources
shouldn't be passed as props anyway — keep them inside a signal or
context, not in the prop list.

---

## Lifecycle hooks

```rust
on_mount(|| {
    log::info!("view is now in the tree");
});

on_cleanup(|| {
    log::info!("component unmounting");
});
```

- `on_mount` fires after the component's view has been attached to
  its parent (so things like "measure the rendered size" work).
- `on_cleanup` fires when the owner is disposed — typically when
  the parent re-renders past this component (e.g. `Show`'s
  condition flipped, `For`'s key removed).

Both are LIFO — multiple registrations in the same component fire
last-registered-first.

---

## Context

Pass values down through the owner tree without prop-drilling:

```rust
#[component]
fn app() -> ElementHandle {
    provide_context(Theme::Dark);
    render! { my_inner_component {} }
}

#[component]
fn my_inner_component() -> ElementHandle {
    let theme = use_context::<Theme>().unwrap_or(Theme::Light);
    /* ... */
}
```

- `provide_context::<T>(value)` stores a `T` in the current owner.
- `use_context::<T>()` walks the owner chain upward to find the
  nearest provided `T`, returning a clone.
- `with_context::<T, R>(f)` is the borrowed-access version.

A closer descendant's `provide_context::<T>` shadows an outer one,
exactly like CSS cascading.

---

## Control flow: `Show` and `For`

Plain Rust control flow (`if`, `match`, `Vec::iter().map()`) runs
during the component body's single execution — it can't react to
later signal changes. For reactive control flow, use `Show` (if /
else) and `For` (keyed list).

### `Show`

```rust
render! {
    Show {
        when: move || count.get() > 5,
        fallback: || render! { text { "small" } },
        text { "big!" }
    }
}
```

When `when()` flips, the inactive branch's owner is fully disposed
(all signals, effects, cleanups inside it run) and the new branch is
mounted fresh.

`fallback:` is optional. With no fallback, the false case renders
nothing.

### `For`

```rust
render! {
    For {
        each: move || items.get(),
        key: |item: &Item| item.id,
        children: move |item: Item| render! { text { {item.name} } },
    }
}
```

- `each` returns the list. Re-evaluated on every dep change.
- `key` derives a stable identity per item.
- `children` is called once per *new* key. Existing items keep
  their owner (and reactive state) when their key reappears.
- Reordered items are re-attached to reflect the new position.
- Removed items have their owners disposed.

For a fixed-shape, non-reactive list, regular `.iter().map()` in a
closure passed to `{...}` still works:

```rust
view {
    {tabs.iter().map(|t| tab_button(*t)).collect::<Vec<_>>()}
}
```

But each render runs once at mount — changes to `tabs` won't
re-render. Use `For` whenever the list might change.

---

## Patterns

### Lift state up

When two siblings need to share state, lift the signal to their
common parent and pass it down. The `Copy`-handle shape means this
is cheap:

```rust
#[component]
fn parent() -> ElementHandle {
    let count = RwSignal::new(0);
    render! {
        view {
            display(count)
            controls(count)
        }
    }
}

#[component]
fn display(count: RwSignal<i32>) -> ElementHandle {
    render! { text { {count.get()} } }
}

#[component]
fn controls(count: RwSignal<i32>) -> ElementHandle {
    render! {
        view {
            on_tap: move || count.update(|n| *n += 1),
            text { "+1" }
        }
    }
}
```

### Long-lived app state via context

Top-level signals declared in your root component become "app
state". Make them available to deep descendants via context:

```rust
#[derive(Copy, Clone)]
struct AppState {
    count: RwSignal<i32>,
    theme: RwSignal<Theme>,
}

#[component]
fn app() -> ElementHandle {
    provide_context(AppState {
        count: RwSignal::new(0),
        theme: RwSignal::new(Theme::Dark),
    });
    render! { /* ... */ }
}

#[component]
fn some_deep_descendant() -> ElementHandle {
    let state = use_context::<AppState>().unwrap();
    render! { text { {state.count.get()} } }
}
```

### Avoid signals that never change

If a value is decided once at mount and never updates, just use a
plain Rust binding — not a signal. Signals incur reactive
book-keeping; static values shouldn't pay for it.

```rust
let banner_text = compute_banner_text(&props);   // not a signal
render! { text { {banner_text} } }
```

---

## Rules and gotchas

- **`signal()` / `effect()` / `memo()` must be called inside a
  component or other reactive owner.** Calling them outside emits a
  debug-build warning and falls back to a detached owner that
  doesn't clean up cleanly.

- **Calling reactive primitives in conditionals or loops is
  allowed.** Each call creates an independent node. (Unlike React,
  Whisker doesn't have "rules of hooks" — there's no positional
  slot system because the component body runs once.)

- **Don't write a signal an effect reads from inside that same
  effect.** This is a feedback loop; the scheduler caps it at 256
  iterations and warns, but the right fix is to break the cycle.

- **Cloning vs Copying handles.** Signal handles are `Copy`, so
  `let a = signal_handle; let b = signal_handle;` works. The
  underlying value isn't cloned by this — both handles point at the
  same arena slot.

- **The macro re-evaluates `{expr}` blocks inside an effect on every
  signal change.** So `{count.get()}` re-runs when `count` changes.
  But the surrounding component body does NOT re-run — that's the
  fine-grained property.

---

## See also

- `docs/render-macro.md` — `render!` syntax reference.
- `docs/reactivity-design.md` — internal architecture: arena, owner
  tree, batching, hot-reload Strategy C.
- `examples/counter` — minimum-working example.
- `examples/hello-world` — substantial sample exercising most of
  the surface.
