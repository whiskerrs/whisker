# Wrapper-less Control Flow — `For` / `Show` redesign

Closes [#84](https://github.com/whiskerrs/whisker/issues/84).
End-to-end acceptance: **`hn-reader` runs
`list(each, key, children, …)` with all 30 stories rendered through
Lynx's native virtualisation, no wrapper element in the tree, and
custom user-defined control flow uses the same author surface as the
built-in `ForEach` / `Show`**. Verified on iOS Simulator
(iPhone 17 Pro / iOS 26.3): header + 13+ visible stories render
through the new path on initial load, all 30 are reachable by
scroll.

---

## 1. Problem

Today `For` and `Show` each create a `<view>` wrapper element so the
runtime has a stable parent to mount their reactive contents into:

```text
view                   ← user wrote `view { For(...) text(...) }`
  view (For wrapper)   ← inserted by `for_each(...)`
    list-item × N
  text
```

Three consequences:

1. **Extra elements in Lynx.** Every `For` / `Show` is one more
   node than the markup suggests.
2. **`<list>` can't see the items.** The list builder's
   `.child(child: Element)` receives the `For` wrapper as a single
   child, so the native-item provider advertises a 1-element list.
3. **User-defined control flow is second-class.** Built-in
   `For` / `Show` got a special `ControlFlow` trait path; anything a
   user wrote couldn't reach the same wrapper-less surface — the
   only available authoring path produced an extra `<view>` of its
   own.

## 2. Goals

1. **Zero extra Lynx elements** for `For` / `Show` and any
   user-defined control flow.
2. **Reactive updates** continue to work, including across
   subsecond hot-reload remounts.
3. **`<list>` virtualisation** is reached through a render-props
   builder shape (`list(each, key, children, …)`); the macro
   forbids a `list { … }` body.
4. **User-defined control flow** is implemented exactly the same
   way as the built-ins — a `#[component]` function that
   allocates a fragment, installs an effect, returns the fragment.

## 3. Primitive: `fragment` (phantom-based transparent container)

Whisker exposes a single new primitive at the renderer + builder
layer:

- `whisker_runtime::view::create_phantom_element() -> Element` — an
  opaque ID the runtime registers in `CHILDREN_OF` (mirror) but
  **never** forwards to Lynx. The phantom owns no Lynx footprint.
- `whisker::__tags::fragment` — a built-in element builder whose
  ctor calls `create_phantom_element`. In `render!` syntax:
  `fragment { … }` (works exactly like `view { … }` from the
  author's perspective, just without the DOM element).
- Phantom-aware bridge dispatchers in `view/renderer.rs`:
  `append_child`, `remove_child`, `insert_child_at`,
  `release_element`, `set_attribute`, `set_inline_styles`,
  `set_event_listener`, `element_sign`,
  `set_update_list_info`, `install_list_native_item_provider`,
  `module_component_ptr` — all check `is_phantom(handle)` first
  and skip the Lynx FFI step accordingly.

### 3.1 Hoisting algorithm

When the user appends a real child under a phantom, the real
child is hoisted to the phantom's **nearest non-phantom mirror
ancestor** in the Lynx tree, inserted at the position the mirror's
DFS pre-order of real-only descendants puts it.

`append_child(parent, child)` (in `renderer.rs`):

```text
1. mirror update: CHILDREN_OF[parent] += child; PARENT_OF[child] = parent.
2. if !is_phantom(child):
     2a. if parent is phantom → find nearest_real_ancestor(parent);
         attach the real-descendant set of `child` (= just child
         when child is itself real, or its DFS pre-order real
         descendants when it's a phantom carrying children) into
         that ancestor at the right Lynx position, computed via
         count_real_descendants_before.
     2b. else → bridge append, possibly followed by a rotation if
         `child` should sit before any existing real siblings
         (the rotation reuses remove_child + append_child).
3. on_component_root_attached(parent, child).
```

### 3.2 Deferred attach (phantom-without-real-ancestor)

When `append_child(phantom_p, real_c)` runs and `phantom_p` has no
real ancestor yet (the whole chain is detached), step 2a finds
`None` and nothing is told to Lynx. The mirror records the edge,
though. When the topmost phantom is later attached to a real
parent, that `append_child` call recursively replays the entire
transparent subtree into Lynx in DFS pre-order — the same machinery
that handles the simpler "phantom-with-real-ancestor" case.

This is what lets `for_each` / `show` run their effects eagerly at
construction time without knowing whether their fragment has been
mounted yet — they freely `append_child(frag, item)` and the real
attach happens when the surrounding render! chain calls
`view.child(frag)`.

## 4. `ForEach` / `Show` as `#[component]` functions

`crates/whisker/src/control_flow.rs` defines both built-ins as
ordinary `#[component]` functions. The `render!` macro's
PascalCase-name `Show` / `For` special-case is **gone** — both
take the standard `UserComponent` path through `PropsBuilder`.

```rust
#[component]
pub fn for_each<T, K>(
    each: EachFn<T>,           // Fn() -> Vec<T> newtype
    key: KeyFn<T, K>,           // Fn(&T) -> K newtype
    children: ItemFn<T>,        // Fn(T) -> Element newtype
) -> Element 
where T: 'static, K: Eq + Hash + Clone + 'static
{
    let frag = create_phantom_element();
    // install effect that diffs each() against per-key entries,
    // mounts new items under `frag` (which hoists them to the
    // real ancestor), disposes removed item owners, reorders by
    // detach+re-attach in new order.
    frag
}

#[component]
pub fn show(
    when: WhenFn,                                              // Fn() -> bool newtype
    children: Children,                                         // body, Fn() -> View
    #[prop(default = Default::default())] fallback: Fallback,   // Option<Fn() -> Element>
) -> Element {
    let frag = create_phantom_element();
    // install effect that tears down the previously-mounted
    // branch + mounts the active branch (children() returns a
    // View attached via attach_to(frag); fallback() returns an
    // Element appended directly to frag).
    frag
}
```

PascalCase aliases generated by `#[component]`: `ForEach`, `Show`.

### 4.1 Function-shaped prop newtypes

Closures can't be `Clone` blanket-impl'd to flow through
`#[component]`'s typed-builder, so the function-shaped props live
behind `Rc<dyn Fn>` newtypes that each have a single
`impl<F: Fn(…) -> …> From<F>` so closure literals convert via
`Into` in the setter:

  - `EachFn<T>` — `Fn() -> Vec<T>`.
  - `KeyFn<T, K>` — `Fn(&T) -> K`.
  - `ItemFn<T>` — `Fn(T) -> Element`.
  - `WhenFn` — `Fn() -> bool`.
  - `Fallback` — `Option<Fn() -> Element>`, with `Default = None`
    so `#[prop(default = Default::default())]` resolves to
    "no fallback".

These live in `whisker_runtime::view::into_view` and are
re-exported through `whisker::prelude::*` so user-defined control
flow code reaches them through the same import path the built-ins
use.

### 4.2 User-defined control flow

Identical author surface:

```rust
#[whisker::component]
pub fn animated_show(
    when: WhenFn,
    duration_ms: u32,
    children: Children,
) -> Element {
    let frag = whisker::runtime::view::create_phantom_element();
    let state: Rc<RefCell<…>> = Rc::new(RefCell::new(…));
    let when = when.clone();
    let children = children.clone();
    effect(move || {
        // … same shape as `show` ↑ but with animation tweens before
        //   the mount / unmount calls.
    });
    frag
}
```

User code in `render!`:

```rust
AnimatedShow(when: move || cond.get(), duration_ms: 300) {
    main_content()
}
```

Same builder pattern, same `Children` body slot, same prop types.
Nothing in the macro or runtime treats user control flow
differently from `Show`.

## 4.3 `<list-item>` auto-wrap + removal from user surface

`list_item` is no longer a user-writable tag in the `render!`
macro nor a prelude re-export. The `list` render-props builder
calls `create_element_by_name("list-item")` directly from its
`__h()` effect and wraps every `children(item)` result in a fresh
`<list-item>` before attaching it to the `<list>` parent.

The `list_item` struct in `crates/whisker/src/__tags` is now
`pub(crate)`; the macro's `is_builtin_tag` whitelist drops it, and
the prelude no longer re-exports it. Source compatibility with any
user code that previously wrote `render! { list { list_item { … } … } }`
is intentionally broken — the migration is mechanical (drop the
body, pass an `each` / `key` / `children` closure triple instead),
and the doc above describes the new shape.



User code passes a `children: |item| render! { … }` closure that
returns the *content* of one list slot (a `story_row` view, a
custom component, whatever). The list builder wraps each returned
`Element` in a fresh `<list-item>` before attaching it to the
`<list>` parent.

### Why the wrap is mandatory

The Whisker fork's list machinery has two layers:

  - **C++ fiber layer** (`core/renderer/dom/fiber/list_element.cc`):
    when a child is appended to `<list>`, the element calls
    `child->MarkAsListItem()` *regardless of tag*. The fiber side
    treats any direct child as a list-item.
  - **Platform UI layer** (iOS `LynxUIListContainer.mm`, Android
    `UIListContainer.java` + `UIList.java`): the list cell APIs
    are strongly typed on `LynxUIComponent` (iOS) /
    `UIComponent` (Android) — that's what realises layout,
    sticky, recycling, item-key tracking. `<list-item>`'s
    platform behavior (`LynxUIListItem` / `UIListItem`) inherits
    from `LynxUIComponent` / `UIComponent`; a plain `<view>`
    produces a `LynxUI` / `UIView` and the
    `instanceof UIComponent` check in Android's `UIList.java`
    (or the iOS analogue) fails. The empirical proof: setting
    `children: |s| render! { story_row(story: s) }` *without*
    the wrap broke the entire view hierarchy (not just the
    items — even the header bar disappeared) on iOS Simulator.

So `<list-item>` must remain in the rendered tree as the cell
anchor. The auto-wrap simply hides it from user code.

### Implementation

In the list builder's `__h()` effect, per new keyed item:

```rust
let li = with_owner(item_owner, || {
    let li = create_element_by_name("list-item");
    let content = children.call(item);
    append_child(li, content);
    append_child(handle, li);  // li is the list's direct child
    li                          // sign + item-key get applied to li
});
```

The list's items Vec stores `(li, sign)` — not `(content, sign)` —
so `componentAtIndex` returns the list-item's sign, which is what
Lynx's mediator expects.

### Trade-off

The user loses the ability to set `<list-item>`-specific
attributes (`sticky-top`, `sticky-bottom`, `full-span`,
`estimated-main-axis-size-px`) from the `children:` closure.
Whisker addresses this on demand via opt-in props on the `list`
builder itself when those features are needed:

  - `sticky_top_keys: &[K]` / `sticky_bottom_keys: &[K]` — the
    list builder reads these and sets the corresponding attributes
    on the wrapping `<list-item>` before broadcasting count.
  - `full_span_keys: &[K]` for waterfall / flow layouts.
  - `estimated_main_axis_size: |item: &T| -> f32` for upfront
    layout hints.

None of these are implemented in v1 — when a real use case
materialises we add the prop. The auto-wrap fork-side comment
`TODO(hujing.1): separate UIListItem with UIComponent` hints that
upstream is also reconsidering the cell-wrapper coupling; if
that lands, Whisker can pass the bare content through and skip
the wrap.

## 5. `<list>` as a render-props builder

`<list>` virtualisation requires Whisker to broadcast item count
and serve `componentAtIndex(i)` from a sign cache (see Lynx fork's
`list_element.cc` + `NativeItemProvider`). The decoupled native
list path Whisker uses **does not** auto-iterate tree children;
it relies on the provider + count metadata.

To keep `<list>` API-consistent with the rest of the design and
avoid magic observer hooks, the `list` builder takes the items
source as render-props (`each` / `key` / `children` setters) and
the macro rejects a body. The list builder owns its own reactive
effect + items Vec — no `attach_to_list` contract with external
control flow, no `View::ControlFlow` enum-dispatch.

### 5.1 Type-stated builder

The `list` struct is generic over three setter slots that start
as `()` and advance to the function-typed newtypes as the user
calls `.each` / `.key` / `.children` (in any order, because the
macro emits setters in source order). `__h()` is **only impl'd
on the fully-populated state**:

```rust
pub struct list<EachF = (), KeyF = (), ChildF = ()> {
    handle: Element,
    each: EachF,
    key: KeyF,
    children: ChildF,
}

impl<KeyF, ChildF> list<(), KeyF, ChildF> {
    pub fn each<T: 'static, F: Into<EachFn<T>>>(self, f: F)
        -> list<EachFn<T>, KeyF, ChildF> { … }
}
// … and `.key`, `.children` advance their slot similarly.

impl<T, K> list<EachFn<T>, KeyFn<T, K>, ItemFn<T>>
where T: 'static, K: Eq + Hash + Clone + 'static
{
    pub fn __h(self) -> Element {
        // install reactive items effect + native-item provider +
        // initial set_update_list_info(count).
    }
}
```

Missing any of the three setters surfaces as a compile-time
error at the close of the builder chain (rather than a runtime
panic).

### 5.2 Macro side: `list` body forbidden

The render! macro recognises `each` / `key` / `children` as
list-typed setters (alongside the existing `list_type` /
`column_count` / `vertical_orientation` typed kwargs) so closure
literals flow through the right `Into` path instead of being
mistaken for `apply_attr` calls. The `is_known_attr_method` table
in `crates/whisker-macros/src/render.rs` carries this list.

## 6. Phases (implementation roadmap)

  1. ✅ Phantom infrastructure in renderer
  2. ✅ `fragment` builtin element
  3. ✅ `EachFn` / `KeyFn` / `ItemFn` / `WhenFn` / `Fallback`
     newtypes
  4. ✅ `for_each` / `show` as `#[component]` functions
  5. ✅ `ElementBuilder::child` stays `Element` (no Box dispatch,
     no `View::ControlFlow` variant)
  6. ✅ render! macro — drop For/Show special case
  7. ✅ list render-props builder
  8. ✅ hn-reader migration
  9. ✅ tests (For→ForEach, body wrap)
  10. ✅ iOS Simulator acceptance — hn-reader renders + scrolls
       through native virtualisation
  11. ✅ PR + design-doc update (this doc)

## 7. Build verification

- `cargo check --workspace`: clean.
- `cargo test --workspace --no-fail-fast`: all targets pass.
- `whisker build --target android` on hn-reader: BUILD SUCCESSFUL.
- `whisker build --target ios-sim` on hn-reader: `.app` builds.

## 8. Comparison with the closed PR #91 design

PR #91 shipped a slightly different shape: `View::ControlFlow`
enum variant, `ControlFlow` trait with `attach_generic` /
`attach_to_list`, builder `child(impl IntoView)` dispatch. Items
inside `<list>` reached the list's items Vec through an explicit
`attach_to_list(list_handle, items_handle)` contract; outside,
control flow used a *real Lynx-side anchor element*.

The current design (this doc) trades the trait + enum dispatch for
phantom hoisting in the renderer + a list render-props builder.
That reorganises the complexity (more in the renderer, less in the
control-flow surface) and — crucially — gives user-defined control
flow the same author shape as the built-ins (a regular
`#[component]` function returning a fragment), which PR #91 didn't.

## 9. Caveat: macro-side `key:` handling for the `list` builder

The wrapper-less control flow shipped initially with one subtle
bug that hid the entire list-items code path: hypothesis 1 from
the earlier draft of this doc turned out to be right.

**Symptom.** With the type-stated `list` builder, items wouldn't
render: the list element + its background-colour styled area
appeared on screen, but the items inside stayed invisible even
under a synchronous hardcoded items source. A `panic!()` planted
at the top of the inherent `__h` (where the provider install +
count broadcast live) didn't fire on launch — proof the inherent
was *not* being dispatched.

**Root cause.** The `render!` macro silently dropped a `key:`
kwarg on **every** element via this guard inherited from the
old For/Show special-case:

```rust
fn kwarg_to_setter(&self, kw: &Kwarg) -> Option<TokenStream2> {
    …
    if name_str == "key" {
        return None;  // ← swallows `key:` on every builder
    }
    …
}
```

That historical "silently ignore `key` on direct elements"
behaviour meant `list(each: …, key: …, children: …)` expanded to:

```rust
__list_ctor()
    .each(closure)
    .children(closure)   // ← `.key()` missing!
    .style(value)
    .__h()
```

The receiver of `.__h()` then was
`list<EachFn<Story>, (), ItemFn<Story>>` — `KeyF` stuck at `()`
because the setter was never called. The inherent `__h` impl is
gated on `list<EachFn<T>, KeyFn<T, K>, ItemFn<T>>`, doesn't match,
and **method resolution falls back to the trait default**
`fn __h(self) -> Element { self.__element() }`. Hence no
provider, no count, no items.

**Fix** (`crates/whisker-macros/src/render.rs`):

```rust
if name_str == "key" && tag_name != "list" {
    return None;  // keep skipping on direct elements; let `list` see it
}
```

`list` is the only built-in with a typed `key:` setter today;
user-defined keyed-list control flow uses the `UserComponent`
path which already routes `key:` through the Props builder.

**Lesson worth recording.** The type-state builder pattern shipped
its own diagnostic — the "fall through to trait default" failure
mode silently produces an empty native list rather than a
compile-time error, because the trait method's signature is
satisfied. Future built-ins that rely on type-state finalisation
should add a debug-mode assertion (e.g. an attribute the test
renderer can look for) inside the inherent finaliser so the same
class of bug surfaces from a unit test rather than a screen
inspection.

## 10. Out of scope

- LIS-based minimal reorder (still O(N) detach-all + re-attach).
- `<list-header>` / `<list-footer>` (deferred; would land as
  optional props on the `list` builder).
- Removing `<list>` from the macro's snake_case-builtin
  whitelist (kept so existing `list_type` / `column_count` /
  `vertical_orientation` setters and the new `each` / `key` /
  `children` setters resolve through the typed path; without the
  whitelist the macro would route every kwarg through
  `attr(name, value)`, which expects `Into<Signal<String>>`).
- Observer-based `<list>` integration (was considered in the
  design discussion that produced this doc; rejected in favour of
  list-as-render-props because the observer machinery would
  silently route a fragment's items into the list's metadata,
  blurring the separation between "fragment hoists to nearest
  real ancestor" and "list collects items").
