# The `render!` macro — syntax reference

`render!` is Whisker's view-construction macro. It looks JSX-like
but emits imperative element-creation calls + `effect`s wired to
the installed `DynRenderer` (Phase 6.5a `view::*`). Each
`{expr}` interpolation becomes an effect, so reading a signal
inside it makes that one piece of the tree update reactively when
the signal changes.

The user guide is `docs/reactivity.md`; this doc is the syntax
cheat sheet.

---

## Top-level shape

```rust
render! {
    // Exactly one root node. Element / component / `{expr}` /
    // string literal all valid.
}
```

The macro returns an `ElementHandle` — the root of the produced
tree. For component invocations and `{expr}` it returns whatever
the inner code returns.

---

## Element nodes

Compose-shaped syntax: props go in `()`, children in `{}`. Either
block can be omitted.

```rust
render! {
    view(
        // attributes go inside the parens, comma-separated.
        style: "color: red;",
        on_tap: || println!("hi"),
    ) {
        // children inside the braces — more elements, components,
        // text literals, or `{expr}` blocks.
        text { "Hello" }
        text { {count.get()} }
    }
}
```

Recognised tag names: `page`, `view`, `text`, `raw_text`, `image`,
`scroll_view`. Anything else is dispatched as a user component.

```rust
// Forms:
view(style: "x")           // props only — no children, drop the `{…}`
view { text { "hi" } }     // children only — no props, drop the `(…)`
view(style: "x") { text { "hi" } }  // both
view()                     // neither (also: `view {}`, `view`)
```

### Attributes

```rust
view(
    // Reserved: `style:` → set_inline_styles.
    style: "padding: 16px;",
    // Reserved: `on_*` → event listener (one-shot registration).
    on_tap: move || count.update(|n| *n += 1),
    // Anything else → set_attribute(name, value.to_string()).
    src: "https://example.com/a.png",
    alt: "example",
    scroll_orientation: "vertical",  // snake_case → kebab-case
)
```

All attribute values flow through `to_string()`. They're wrapped in
an `effect` so signal reads inside the expression re-fire the
attribute set on every dep change:

```rust
view(
    // re-runs whenever `color` changes
    style: format!("color: {};", color.get()),
)
```

Static values (string literals, constant expressions) just run
their effect once and never re-fire.

### Event handlers

`on_<event>` and `on<Event>` are both accepted; the macro lowercases
the first character of the camelCase form:

```rust
on_tap: || {}            // event name "tap"
onTap:  || {}            // event name "tap"
on_long_press: || {}     // event name "long_press"
onLongPress: || {}       // event name "longPress" (first char only lowercased)
```

The closure must be `Fn() + 'static`. Captures by `move` are typical.

Handlers register **once** at element creation — they are not
wrapped in an effect. Subsecond hot-reload patches the closure's
body in place; you don't need to do anything special for handler
edits to take effect.

---

## Text children

A string literal renders as a `raw_text` element:

```rust
text { "Hello, world" }
```

Multiple string literals at sibling positions are each a separate
`raw_text` — fine for trivial cases, but if you want a single text
node with mixed static + dynamic parts use `{expr}`:

```rust
text {
    "Count: "        // static raw_text
    {count.get()}    // dynamic raw_text driven by an effect
}
```

---

## `{expr}` interpolation

```rust
view {
    {header()}         // ElementHandle: insert as child
    {count.get()}      // i32: render as text
    {name.clone()}     // String: render as text
    {tuple_of_three}   // (A, B, C): each child inserted in order
    {Some(handle)}     // Option: Some → child, None → nothing
}
```

`{expr}` interpolation only makes sense in the children block, so
it lives inside `{…}` (never inside `(…)` which is for kwargs).

The expression is dispatched through `IntoView`. Built-in impls
cover:

- `ElementHandle` — attached as a child.
- `View` — attached as-is (Element / Fragment / Text / Empty).
- `&str`, `String`, `&String` — rendered as a `raw_text` element.
- All numeric primitives + `bool` / `char` — same, via `Display`.
- `()` — no-op.
- `Option<T>` — `Some(v)` mounts `v`, `None` mounts nothing.
- Tuples of 1–8 elements — each child mounted in declaration order.

Each `{expr}` is wrapped in an `effect`. Reading signals inside it
makes the surrounding bit of the tree update when those signals
change. A signal-less expression's effect just runs once at
registration.

On every effect re-run, previously-attached elements are detached
and replaced with whatever the expression produces this time. For
the static element case (e.g. `{header()}`), the effect runs once
and the cost is zero on subsequent flushes.

### Reactivity through closures

A common pattern: when a *non-signal* value depends on a signal, wrap
the computation in a closure:

```rust
let glyph = move || if playing.get() { "▌▌" } else { "▶" };
let color = move || if playing.get() { ACCENT } else { TEXT_MUTED };
let btn_style = move || format!("color: {};", color());

render! {
    view(style: { btn_style() }) {
        text { {glyph()} }
    }
}
```

The closure call inside `{...}` makes the signal read happen *inside
the effect closure*, so the dep is tracked. Without the closure
wrap, the value would only be evaluated once during mount and never
update.

---

## Components

User-defined `#[component]` functions are invoked with the same
Compose-shaped syntax as built-in elements:

```rust
render! {
    view {
        my_component(src: "https://…", alt: "logo")        // user component
        Show(when: …) { /* children */ }                    // built-in
        For(each: …, key: …, children: |i| …)               // built-in
    }
}
```

The kwarg block in `(…)` maps name-by-name onto fields of the
`XxxProps` struct that `#[component]` auto-generates from the
function's parameter list (see `docs/reactivity.md`). At expansion
the call above lowers to:

```rust
my_component(
    MyComponentProps::builder()
        .src("https://…")
        .alt("logo")
        .build(),
)
```

`typed-builder`'s `setter(into)` handles common coercions at the
call site — `&'static str` → `String`, `i32` → `f64`, etc. — so
callers write the natural value and the macro takes care of the
rest.

Children passed inside `{…}` (not the `(…)` kwarg block) are
bundled into a `children:` closure of type
[`whisker::Children`](../crates/whisker-runtime/src/view/into_view.rs):

```rust
render! {
    card(title: "About") {
        // routed into the card's `children: Children` prop.
        text { "First child" }
        text { "Second child" }
    }
}
```

Components that don't declare a `children` prop compile-error when
called with positional children, telling the user which props the
component does accept.

> **Positional calls no longer compile.** A `#[component]` function
> always takes a single Props argument; bare `my_component("…", "…")`
> at the call site is a type error. Components must be invoked
> through `render!`'s brace syntax (or, in the rare case the user
> needs to bypass the macro, by constructing the Props struct
> explicitly).

Two component names are reserved as built-ins and must use the
capitalised form: `Show` and `For`.

### `Show`

```rust
Show(
    when: move || cond.get(),                       // Fn() -> bool, required
    fallback: || render! { text { "loading…" } },   // optional
) {
    /* one or more children — the "true" branch */
    text { "ready" }
}
```

`when` is called inside an effect. When the bool flips, the
previously-mounted branch's owner is disposed (cascading cleanups +
freeing reactive nodes) and the other branch is mounted fresh.

Without a `fallback:` kwarg, the false case mounts nothing.

Unknown kwargs are a compile error: `when` and `fallback` are the
only accepted ones.

### `For`

```rust
For(
    each: move || items.get(),                  // Fn() -> Vec<T>, required
    key: |item: &Item| item.id,                 // Fn(&T) -> K, required
    children: move |item: Item| render! { /* ... */ },  // Fn(T) -> impl IntoView, required
)
```

For takes **no children block** — the per-item template is the
`children:` kwarg closure.

Reconciliation rules:

- Items whose key matches a previous render keep their owner (and
  per-item reactive state) intact.
- New keys get a fresh owner + child mount.
- Removed keys have their owners disposed.
- Reordered keys are detached + re-attached so the wrapper's
  child list reflects the new order.

Unknown kwargs are a compile error: only `each`, `key`, `children`
are accepted.

---

## Error grammar

The macro tries hard to give specific compile errors:

```rust
// `foo` resolves to a user component. If `foo` isn't in scope you
// get the normal Rust "cannot find function `foo`" error. The
// macro does NOT treat lowercase identifiers as built-in tags
// (the whitelist is `page`, `view`, `text`, `raw_text`, `image`,
// `scroll_view`).
render! { foo() }

// "Show requires `when:` kwarg"
render! { Show { text { "x" } } }

// "unknown kwarg `bar` on Show; allowed: when, fallback"
render! { Show(when: || true, bar: 1) { text { "x" } } }

// "For requires `each:` kwarg"
render! { For(key: |x| x, children: |x| x) }

// "For takes no children block; pass per-item template via `children:`"
render! { For(each: || vec![], key: |x| 0) { text { "stray" } } }

// PascalCase is reserved for built-in components only. Writing
// `Foo { ... }` for a user component is a macro error suggesting
// the snake_case spelling.
render! { Foo { text { "x" } } }
```

---

## What `render!` does NOT support

- **Conditional `if` / `match` outside `Show` / `For`.** Plain Rust
  control flow doesn't take part in the reactive graph. Inside
  `{...}` you can still write conditional Rust expressions:
  ```rust
  view {
      {if cond.get() { "yes" } else { "no" }}
  }
  ```
  …but the macro itself doesn't parse `if`/`match` as macro syntax.
- **Iterator-spreading via `for` / `while`.** Use `For(…)`.
- **Slot-style composition (named children).** Phase 7+ if needed.
- **Positional component invocation.** Components only accept the
  Compose-shaped kwarg syntax (`my_component(src: "…")`).
  Positional function calls don't compile.

---

## How the macro expansion works (informally)

For
```rust
render! {
    view(
        style: "padding: 16px;",
        on_tap: || count.update(|n| *n += 1),
    ) {
        text { "Count: " {count.get()} }
    }
}
```

the macro emits roughly
```rust
{
    let __b: __tags::view = __tags::view();
    let __h = __b
        .style(move || "padding: 16px;".to_string())
        .on_tap(|| count.update(|n| *n += 1))
        .__h();
    {
        let __child = {
            let __b: __tags::text = __tags::text();
            let __h = __b.__h();
            // static "Count: "
            {
                let __child = {
                    let __h = view::create_element(ElementTag::RawText);
                    view::set_attribute(__h, "text", "Count: ");
                    __h
                };
                view::append_child(__h, __child);
            }
            // dynamic {count.get()}
            {
                let __interp_parent = __h;
                let __interp_last = Rc::new(RefCell::new(Vec::new()));
                effect(move || {
                    /* detach previous, into_view(count.get()), attach new */
                });
            }
            __h
        };
        view::append_child(__h, __child);
    }
    __h
}
```

The builder-chain shape (`__tags::view().style(…).on_tap(…).__h()`)
is what enables rust-analyzer's method completion on prop kwargs:
the chain's receiver type is known, so `view(s|`'s method-name
slot resolves to `view`'s methods (`style`, `class`, `on_tap`, …)
as completion candidates.

For the gory details, see `crates/whisker-macros/src/render.rs`.
