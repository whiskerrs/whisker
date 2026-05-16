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

```rust
render! {
    view {
        // attributes (key: value, …)
        style: "color: red;",
        on_tap: || println!("hi"),

        // children: more elements, components, text literals,
        // or `{expr}` blocks.
        text { "Hello" }
        text { {count.get()} }
    }
}
```

Recognised tag names: `page`, `view`, `text`, `raw_text`, `image`,
`scroll_view`. Anything else is a compile error. (Custom / x-* tags
are a Phase 7 add.)

### Attributes

```rust
view {
    // Reserved: `style:` → set_inline_styles.
    style: "padding: 16px;",
    // Reserved: `on_*` → event listener (one-shot registration).
    on_tap: move |_| count.update(|n| *n += 1),
    // Reserved: `key:` → silently accepted (used by `For`
    // reconciliation — see below). Macro emits no code.
    key: item.id,
    // Anything else → set_attribute(name, value.to_string()).
    src: "https://example.com/a.png",
    alt: "example",
    "scroll-orientation": "horizontal",  // raw string key
    scroll_orientation: "vertical",       // identifier key
}
```

All attribute values flow through `to_string()`. They're wrapped in
an `effect` so signal reads inside the expression re-fire the
attribute set on every dep change:

```rust
view {
    // re-runs whenever `color` changes
    style: format!("color: {};", color.get()),
}
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
    view {
        style: { btn_style() },
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

Capitalised tag names dispatch to component constructors. Phase
6.5a v1 supports two built-in components: `Show` and `For`.
User-defined `#[component]` functions are invoked as plain function
calls outside `render!` (and their return value can be embedded via
`{...}`):

```rust
render! {
    view {
        {my_component(props)}       // user component
        Show { /* … */ }            // built-in
        For { /* … */ }             // built-in
    }
}
```

### `Show`

```rust
Show {
    when: move || cond.get(),                       // Fn() -> bool, required
    fallback: || render! { text { "loading…" } },   // optional
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
For {
    each: move || items.get(),                  // Fn() -> Vec<T>, required
    key: |item: &Item| item.id,                 // Fn(&T) -> K, required
    children: move |item: Item| render! { /* ... */ },  // Fn(T) -> impl IntoView, required
}
```

For takes **no positional children** — the template is the
`children:` kwarg.

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
// "unknown render! tag `foo`"
render! { foo {} }

// "Show requires `when:` kwarg"
render! { Show { text { "x" } } }

// "unknown kwarg `bar` on Show; allowed: when, fallback"
render! { Show { when: || true, bar: 1, text { "x" } } }

// "For requires `each:` kwarg"
render! { For { key: |x| x, children: |x| x } }

// "For takes no positional children; pass them via `children:`"
render! { For { each: || vec![], key: |x| 0, text { "stray" } } }

// "unknown component `Foo` in render!"
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
- **Iterator-spreading via `for` / `while`.** Use `For { ... }`.
- **Slot-style composition (named children).** Phase 7+ if needed.
- **Custom component invocation in macro position.** Use a function
  call wrapped in `{...}`.

---

## How the macro expansion works (informally)

For
```rust
render! {
    view {
        style: "padding: 16px;",
        on_tap: || count.update(|n| *n += 1),
        text { "Count: " {count.get()} }
    }
}
```

the macro emits roughly
```rust
{
    let __h = view::create_element(ElementTag::View);
    effect(move || view::set_inline_styles(__h, "padding: 16px;"));
    view::set_event_listener(__h, "tap", Box::new(|| count.update(|n| *n += 1)));
    {
        let __child = {
            let __h = view::create_element(ElementTag::Text);
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

For the gory details, see `crates/whisker-macros/src/render.rs`.
