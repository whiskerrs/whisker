# Wrapper-less Control Flow — `For` / `Show` redesign

Tracks the design for [#84](https://github.com/whiskerrs/whisker/issues/84).
The end-to-end acceptance signal is **`hn-reader` switches from
`scroll_view { For(...) }` to `list { For(...) }` and renders all
30 stories through Lynx's native virtualisation, with no extra
wrapper element in the tree**.

---

## 1. Problem

Today `For` and `Show` each create a `<view>` wrapper element so the
runtime has a stable parent to mount their reactive contents into:

```text
view                   ←  user wrote `view { For(...) text(...) }`
  view  (For wrapper)  ←  inserted by `for_each(...)`
    list-item × N
  text
```

Two consequences fall out of that:

1. **Extra elements**. Every `For` and `Show` is one more node in the
   tree than the markup suggests. Compounded across a screen this
   adds non-trivial work to layout / hit-testing.

2. **`<list>` can't see the items**. The list builder's
   `.child(child: Element)` receives the `For` wrapper as a single
   child, not the N `<list-item>` elements that live inside it. The
   native-item provider then advertises a 1-element list, and
   `list { For(...) }` renders nothing useful. The wrapper hides the
   items from list's eager-attach + `update-list-info` path
   (`whisker_list_3_bugs_breakthrough` memory has the full trace of
   that discovery).

The hn-reader workaround today is to use `scroll_view { For(...) }`,
which gives up the list's recycling / large-list scrolling and pays a
full render cost for every story up-front.

## 2. Goal

Wrapper-less, React-Fragment-style control flow:

- `For` and `Show` insert zero or one extra element in the DOM
  (only an invisible **anchor** in the generic-parent case; nothing
  at all in the `<list>` parent case).
- Reactive updates re-render in place, preserving sibling order
  relative to non-control-flow siblings.
- The native `<list>` path sees each item individually so its
  `NativeItemProvider` cache + `update-list-info` count are right.

The `hn-reader` change closes the loop:

```rust
// Before
scroll_view(scroll_orientation: "vertical", style: list_style) {
    For(each: …, key: |s| s.object_id.clone(), children: |s| render! { story_row(story: s) })
}

// After
list(style: list_style) {
    For(each: …, key: |s| s.object_id.clone(), children: |s| render! { list_item { story_row(story: s) } })
}
```

The macro-emitted call sites stay nearly identical; the entire
behaviour shift lives behind `For`'s implementation + the list
builder's `.child()`.

## 3. Surface change in `View`

`View` grows a `ControlFlow` variant carrying a trait object:

```rust
// crates/whisker-runtime/src/view/into_view.rs

pub enum View {
    Element(Element),
    Text(String),
    Fragment(Vec<View>),
    ControlFlow(Box<dyn ControlFlow>),  // ← new
    Empty,
}

/// A view that participates in its own attachment instead of being
/// `append_child`-ed directly. `For` / `Show` are the only two
/// implementors today; other reactive-children primitives can
/// follow the same pattern.
pub trait ControlFlow {
    /// Attach into a generic container (`view`, `scroll_view`, …).
    /// The control flow inserts an invisible anchor at `parent`'s
    /// current end (or wherever `materialise_into` placed it) and
    /// mounts its reactive children as siblings *before* the anchor.
    fn attach_generic(self: Box<Self>, parent: Element);

    /// Attach into a `<list>` container. The control flow writes its
    /// items directly into the list's shared `items` Vec and
    /// re-broadcasts `set_update_list_info(count)` on every update.
    /// `list_handle` is the `<list>` element; `items` is the shared
    /// state the native-item provider closure reads from.
    fn attach_to_list(
        self: Box<Self>,
        list_handle: Element,
        items: ListItemsHandle,
    );
}

/// Shared list-item state, owned by the list builder, handed to
/// every `ControlFlow::attach_to_list` call (and to the provider
/// closure installed in `list::__h`).
pub type ListItemsHandle = ::std::rc::Rc<::std::cell::RefCell<Vec<(Element, i32)>>>;
```

`View::ControlFlow` participates in `attach_to` / `materialise_into`
via `attach_generic`. `View::ControlFlow` returns nothing from
`elements()` — its contents are owned by the control-flow's effect,
not the caller's `Vec<Element>` of "leaf handles" used for
detach-on-update of `{expr}` children.

## 4. Anchor strategy (generic parent)

Picking **one** anchor (not two) keeps the element count at +1 vs
today's +1 wrapper, and the runtime can locate "the range that
belongs to me" cheaply by remembering the handles it last attached.

### 4.1 Layout

```text
parent
  ...
  child_before_For
  [ items in order ]    ← the control flow's range
  anchor   ← invisible 0×0 <view>; stays in place across updates
  child_after_For
```

The anchor goes in at the position the parent had reached when the
control flow was being attached (= the natural source-order position
of the `For` / `Show`). Items are inserted **before** the anchor on
every (re)render.

### 4.2 Anchor element shape

A regular `<view>` with `width: 0; height: 0; flex-shrink: 0;` and no
content. Flex layout collapses it to zero footprint without
`display: none` (which would skip layout entirely and break sibling
ordering in some Lynx versions). We document this convention so a
future "anchor element type" can replace it if Lynx ever exposes a
`<template>` / `<fragment>` primitive.

### 4.3 Insertion sequence (generic path)

`attach_generic(parent)`:

1. Create the anchor view (one-shot), apply zero-size style, append
   to `parent`. The anchor's current child index in `parent` is
   `anchor_idx`.
2. Install an `effect(...)` that:
   - Reads the reactive source (`each()` / `when()`).
   - Diffs against `self.tracked_handles` — items kept by key reuse
     their owners; new items get fresh `create_owner(None)` +
     `with_owner(|| view.into_view().materialise_into(...))`; removed
     items get their owners disposed and their handles `remove_child`-ed.
   - Re-inserts in the new order via
     `insert_child_at(parent, item, anchor_idx)`. After each
     insertion the anchor moves right by 1; we recompute
     `anchor_idx = child_index(parent, anchor)?` before the next
     insertion so order stays correct.

The tracked-handles step is the only state the effect carries
across reruns — the anchor handle and the parent are captured by
the closure.

### 4.4 Sibling order correctness

For `view { text("hdr") For(...) text("ftr") }`:

- Mount: `text("hdr")` appended, anchor for `For` appended (`anchor_idx = 1`),
  items inserted before anchor at index 1, then `text("ftr")` appended.
  Tree: `[hdr, item0, item1, ..., anchor, ftr]`.
- Update: detach old items, re-insert new items before anchor at its
  current index. `hdr` and `ftr` stay put.

For nested `For` (a `For` whose `children` body contains another
`For`), the inner `For`'s anchor is inserted relative to the outer
items' parent — which is `parent` (the outer flow flattens its
items as siblings, not nested). So nesting works without further
machinery.

## 5. `<list>` parent path

`<list>` differs from generic parents in three ways:

1. Items must end up as direct `<list-item>` children of the list
   handle (Lynx's iteration assumes that shape).
2. The list maintains an eagerly-computed `Vec<(Element, i32)>` of
   `(handle, sign)` pairs — the native-item provider closure reads
   from it.
3. The list calls `set_update_list_info(handle, count)` so Lynx
   knows the slot count at layout.

### 5.1 List builder refactor

```rust
pub struct list {
    handle: Element,
    items: ListItemsHandle,  // ← was `Vec<(Element, i32)>`
}
```

`items` is `Rc<RefCell<Vec<(Element, i32)>>>` instead of a plain
`Vec` so the control flow's effect can mutate the same store the
provider closure already reads from. The closure captures
`items.clone()`; the control flow's effect captures another clone.

### 5.2 list's `.child()` becomes view-aware

```rust
fn child<V: IntoView>(mut self, child: V) -> Self {
    match child.into_view() {
        View::Element(handle) => self.attach_one_item(handle),
        View::ControlFlow(cf) => cf.attach_to_list(self.handle, self.items.clone()),
        View::Fragment(children) => {
            for c in children { self = self.child(c); }
        }
        View::Text(_) | View::Empty => {}
    }
    self
}
```

The old static-children path lives in `attach_one_item` (set
`item-key`, `append_child`, `element_sign`, push to items Vec).

### 5.3 For/Show's `attach_to_list`

For `For`:

1. Install an `effect(...)` that:
   - Reads `each()`, builds the new `(key, handle, sign)` list,
     reusing per-key owners across reruns.
   - For new items: create owner, materialise child via
     `view.into_view().attach_to(list_handle)`, set `item-key`,
     compute sign, push.
   - For removed items: `remove_child(list_handle, handle)` + dispose
     owner.
   - Write the new ordered `Vec<(handle, sign)>` into
     `*items.borrow_mut()`.
   - Call `set_update_list_info(list_handle, items.len())`.

For `Show`, the effect rebuilds one branch's contents on each `when`
flip — same general shape but the per-item map collapses to a single
"is-currently-showing-A-vs-B" piece of state.

### 5.4 No anchor needed inside `<list>`

The list owns its own item ordering through the items Vec; the
provider closure indexes into that Vec. We bypass the generic
anchor path entirely. No invisible element ends up inside the list.

## 6. `render!` macro: how user code hits the new path

The macro's `For` / `Show` lowering already emits a function call
into `whisker::for_each(...)` / `whisker::show(...)`. The only shift
is that those functions now return concrete `For<...>` / `Show`
structs (instead of `Element`), and those structs `impl IntoView`
returning `View::ControlFlow(Box<Self>)`.

Container builders' `.child()` becomes generic over `impl IntoView`
(see §5.2 for `list`; the same pattern applies to `view`,
`scroll_view`, etc.). The macro's `.child(#inner)` token site
doesn't change — the only thing different is that `#inner` may now
be a `For<...>` or `Show` rather than an `Element`, and the
builder's `.child` dispatches.

Pre-existing callers that compose with `.child(elem)` directly keep
working because `impl IntoView for Element` already exists.

## 7. Phasing

The roadmap mirrors §6 of the issue, slightly refined:

### Phase 1 — Plumbing (non-breaking)

- Add `View::ControlFlow(Box<dyn ControlFlow>)` + the `ControlFlow`
  trait + `ListItemsHandle` typedef.
- `View::attach_to` / `materialise_into` / `elements` handle the
  new variant.
- Tests: a `MockControlFlow` that records its `attach_generic` calls
  proves the dispatch wires through `view.attach_to(parent)` and via
  `IntoView`.

### Phase 2 — `For` struct

- Replace `for_each(...) -> Element` with
  `for_each(...) -> For<...>` and an `IntoView for For<...>` impl.
- Implement `For::attach_generic` (anchor + insert-before pattern).
- Re-run `examples/hello-world` and existing scroll_view-based
  `For` callers to verify no regression in the generic path.

### Phase 3 — `Show` struct

- Same shape: `show(...) -> Show` impls `IntoView` →
  `View::ControlFlow(Box::new(self))`.
- `Show::attach_generic` reuses the anchor pattern but the
  per-key map collapses to a single mounted-branch owner.

### Phase 4 — `<list>` plumbing

- Refactor `list` to `items: ListItemsHandle`.
- Generic `.child()` on the list builder, with the `View::Element`
  static path and the `View::ControlFlow` reactive path.
- `For::attach_to_list` + `Show::attach_to_list` (Show inside a
  list is rare but for symmetry; if cost outweighs value we can
  ship `attach_to_list` as a `compile_error!`-style runtime panic
  for `Show` and revisit later).

### Phase 5 — `hn-reader` switch

- Replace `scroll_view { For }` with `list { For }`. Wrap each
  story in a `list_item { story_row(...) }` inside `For`'s
  `children`.
- Verify on iOS Simulator (Lynx fork v3.8.0-whisker.1 already
  ships the native list pieces) and Android emulator (real
  devices preferred per the `whisker_adb_input_tap_no_handler`
  memory, but rendering doesn't need touches).

### Phase 6 — Cleanup

- Drop the now-obsolete "wrapper view" notes from
  `whisker_list_3_bugs_breakthrough` and any inline comments that
  refer to the wrapper as the reason a particular kludge exists.
- Document the anchor convention (§4.2) in the render-macro doc so a
  future contributor doesn't try to re-introduce a wrapper.

## 8. Open questions

### 8.1 `Show` inside `<list>`

`list { Show(when, children) }` is unusual — `<list>` is a flat
collection of items, and `Show` flips between two single subtrees,
which would mean "the list has one item that comes and goes." For
v1 we implement `Show::attach_to_list` as the obvious
single-or-zero-item branch (mount one item when `when` is true, zero
items when false, re-broadcasting `set_update_list_info(0|1)`). If
that turns out to surprise users we revisit.

### 8.2 Anchor element type

`<view>` works but is a noisy nine-character element name in any
DOM dump. Lynx has no `<fragment>` / `<template>` element today; a
future fork PR could add a zero-cost marker tag, at which point the
anchor switches over without changing the rest of the architecture.

### 8.3 Reordering perf

The current `for_each` does a "detach all kept, re-attach in new
order" per update. With the anchor pattern we do the same — every
reorder is O(N). LIS-based minimal-move reordering is a known
follow-up; the issue notes typical hn-reader churn (append,
prepend, single insert) stays cheap with the simple path. We defer
LIS until a real workload shows the cost.

### 8.4 `View::ControlFlow` inside `View::Fragment`

A fragment containing a control flow + plain elements
(`(text("hi"), For(...))`) needs the control flow's anchor to land
in source order. `Fragment::materialise_into` already iterates in
order; passing `parent` through and letting `ControlFlow` attach as
each child is visited preserves that. Edge case: control flow as the
only child of an empty fragment — works identically since there are
no surrounding siblings to anchor against.

## 9. Acceptance test plan

Per the user-facing goal:

1. `whisker build --target ios-sim` on `hn-reader` succeeds (no
   build regression from the API changes).
2. `whisker run --target ios-sim` launches hn-reader; the loading
   banner shows, then the list of 30 stories appears.
3. Scrolling the list reaches the bottom (Lynx's native
   virtualisation does its job — items 0..29 all renderable).
4. No "For wrapper view" appears in element inspector / DOM dump.
5. `whisker build --target android` on hn-reader succeeds and
   runs on an Android emulator with the same item count.

`hello-world` and `counter` continue to pass their existing build
checks — both use `scroll_view`-style containers so they exercise the
generic-parent path through the new ControlFlow surface.

## 10. Out of scope

- The general "tuple / iterator-flatten / arbitrary IntoView in
  every child slot" surface — `.child(impl IntoView)` is what this
  PR ships for the containers `For`/`Show` need to compose against,
  not a universal builder overhaul.
- Removing the `flex-direction: column` default `for_each` sets on
  the wrapper today. Since the wrapper is gone, the default
  vanishes; if user code relied on it implicitly, fix-ups land in
  the same PR with an explicit doc note.
- LIS-based minimal-move reordering (deferred, §8.3).
- Any change to the `<list>` Lynx fork — the fork already supports
  the native-item-provider model this design relies on.
