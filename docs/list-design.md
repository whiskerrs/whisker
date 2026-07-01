# `<list>` design — making Lynx's list correctly & exhaustively usable from Whisker

Status: **planning** · Owner: TBD · Supersedes the `feature/builtin-list` spike (obsolete).

## Goal & scope

Make Lynx's built-in [`<list>`](https://lynxjs.org/api/elements/built-in/list) **correctly and
exhaustively** usable from Whisker. The official Lynx `<list>` API surface (its documented props,
events, and methods) is the **exact boundary** of this work:

- Everything documented for `<list>` should be bindable from Whisker.
- Anything **not** in the Lynx docs is **out of scope** (no FlashList-style invented features, no
  non-standard attributes).

Concretely this means three tracks:

1. **Correctness** — fix the three known bugs (reorder, non-uniform-cell crush, cross-axis shrink),
   which are all *incorrect/missing bindings of documented Lynx features*, not novel work.
2. **Completeness** — bind the full documented prop/event/method surface (today only a handful are
   wired).
3. **Virtualization (pull path)** — drive items on demand via the native item provider rather than
   materializing every item up front, structured as a reusable `Virtualizer<T, K>` (see
   [#83](https://github.com/whiskerrs/whisker/issues/83)).

### Non-goals

- Generalizing the provider to arbitrary custom container modules (ViewPager/Carousel) — that is
  **Phase P / #83**, downstream of this work.
- Eager children for module elements — **Phase O / #82**.
- Any prop/event not in the Lynx `<list>` docs.

## Current state (authoritative: current `main`)

`feature/builtin-list` is **obsolete** (161 commits behind `main`, last touched 2026-05-29, references
fork `v3.7.0-whisker.20`). The list functionality re-landed on `main` via separate PRs and evolved
past it. Do **not** base work on that branch.

What exists on `main` today:

| Piece | File | State |
|---|---|---|
| `list` builder `__h()` | `crates/whisker/src/lib.rs` | **Eager-materialize**: builds every item up front, sets `item-key="w_{index}"` (positional), feeds Lynx only `count`. Provider returns cached signs by index. |
| `NativeItemProvider` (value) | `crates/whisker-runtime/src/view/list_provider.rs` | `component_at_index` / `enqueue_component` closures. |
| `list_mount` (true virtualization) | `crates/whisker-runtime/src/view/list_mount.rs` | On-demand item build inside `component_at_index`. **Built but unused.** `key` ignored (v1 limitations). |
| FFI trampolines | `crates/whisker-driver/src/lynx/list_provider.rs` | Wired & live. |
| Bridge | `crates/whisker-driver-sys/bridge/.../whisker_bridge_common.cc` | `list_set_native_item_provider` + `element_set_update_list_info(element, count)`. Fork `v3.7.0-whisker.21`. |
| Re-entrancy fix | `#214` | `with_renderer` uses a shared borrow — re-entrant element creation during a native callback is permitted. **This unblocks the pull path.** |

Builder props bound today: `list_type`, `column_count` (⚠️ non-doc), `span_count`, `vertical_orientation`
(⚠️ non-doc name) + render-props `each` / `key` / `children`. **Zero scroll events, no list handle.**

## The three bugs, reframed as binding gaps

| Bug (from `examples/bluesky/MEMO.md`) | Root cause | Fix (documented feature) |
|---|---|---|
| ① Reorder / prepend / insert corrupts order | `item-key="w_{index}"` is **positional**, so the native diff sees no key change → no move/remove. The user's stable `key` is computed but discarded for the attribute. | Use the stable `key` as `item-key`; add `layout-id` + `update-animation`. |
| ② Tall non-uniform first cell (header) crushed on recycle | No `reuse-identifier`, no `estimated-main-axis-size-px`, no `full-span` sent → recycler mis-measures/mis-reuses. | `full-span` + `estimated-main-axis-size-px` + `reuse-identifier` on the item. |
| ③ Cell shrinks to content width (cross-axis) | Item not marked to occupy the cross axis. | `full-span` (and/or verify cross-axis sizing default). |

All three are *“we aren’t feeding the native list documented information.”* None require inventing
anything.

## Per-item props — the key API decision

Lynx sets several attributes **on `<list-item>`**, which vary per item:
`item-key`, `sticky-top`, `sticky-bottom`, `full-span`, `reuse-identifier`,
`estimated-main-axis-size-px`, `recyclable`. But Whisker’s `children: |T| -> Element` returns only the
**content**; the `<list-item>` wrapper is created internally, so the user has no handle to set these.

### Options considered

- **A. Rich return type** — `children: |T| -> impl Into<ListItem>`, where `ListItem` is a bespoke
  builder over the content element + per-item attrs. **Rejected**: introduces a one-off type that
  appears *only* in list children, breaking Whisker’s uniform “return `render! { .. }`” model — poor,
  inconsistent DX.
- **B. Separate config closure** — `item_config: |&T| -> ItemConfig` alongside `children`. Rejected:
  two closures over the same item; rendering and config split apart.
- **C. `list_item` as a first-class element + auto-wrap** — user optionally returns
  `render! { list_item(full_span: ..) { .. } }`; the list auto-wraps when they don’t. Rejected: the
  auto-wrap requires a detect-or-wrap rule and a `tag_of` runtime primitive — i.e. whisker-runtime
  gains special handling. Superseded by E.
- **D. Attribute hoisting** — set attrs on the content root; whisker hoists the per-item ones.
  Rejected: implicit/magic, conflates content styling with list-item semantics.
- **E. `list_item` is a fully normal element; user always writes it; NO runtime special path.**
  **Chosen.**

### Decision: **Option E — `list_item` is just an element; no whisker-runtime special path**

`list_item` becomes a **public built-in element**, used in `render!` exactly like `view`. The user
**always writes it explicitly** in the `children` closure. The list does **not** auto-wrap, does
**not** detect, and **whisker-runtime has zero list-item knowledge** — no `tag_of`, no marker set, no
wrap helper.

```rust
list(
    each: move || items.get(),
    key:  |p: &Post| p.uri.clone(),
    children: |p: Post| render! {
        list_item(full_span: p.is_header, reuse_identifier: p.cell_kind()) {
            post_card(post: p)
        }
    },
)
```

Rationale:

- **No runtime special-casing** — this is the deciding criterion. `list_item` flows through the same
  `create_element_by_name` / `append_child` path as any element. The earlier "generic slot wrapper"
  question (`tag_of` + `wrap_child_in_slot`, and whether to generalize it) **dissolves**: there is no
  wrap mechanism to generalize.
- **One mental model** — `list_item` is an element builder like every other built-in; no foreign type,
  no auto-wrap rule.
- **Most Lynx-like** — mirrors `<list><list-item>…` exactly (Lynx also requires you to write
  `<list-item>`).
- **No identity duality** — `item-key` is **owned by the list**: the builder’s `__h` stamps it from
  the `key` extractor onto the returned child via a generic `set_attribute` (it never inspects the
  child’s tag). `list_item` does not take `item_key` in this path.

What the list builder’s `__h` does (all generic ops, no list-item awareness in the runtime): run the
`children` closure → `set_attribute(child, "item-key", key.call(item))` → `append_child(list, child)`
→ track the per-item owner by `key` → `set_update_list_info(count)`. For virtualization the provider
builds the `list_item` on demand the same way.

`list_item` kwargs (all optional): `full_span`, `sticky_top`, `sticky_bottom`, `reuse_identifier`,
`estimated_size` (→ `estimated-main-axis-size-px`), `recyclable`. **Not** taken in the render-props
path: `item_key` (list owns it).

`reuse-identifier` default: when unset, assign **one stable identifier per list** (all items reuse
each other — correct for homogeneous lists, mirrors ReactLynx’s compile-time “same shape → same id”).
Heterogeneous lists opt into groups via `list_item(reuse_identifier: ..)`.

**Trade-off (accepted):** the common homogeneous case is more verbose than auto-wrap — every item is
explicitly wrapped in `list_item`. This buys a generic runtime. If the boilerplate later proves
annoying, auto-wrap can be added as **thin sugar in the macro/builder layer** (`crates/whisker`,
e.g. the `render!`/`list` lowering detecting a non-`list_item` body) — **never** in whisker-runtime.
Option E keeps that door open without paying for it now.

Footgun: if the closure returns a non-`list_item`, the native list misbehaves (same failure mode as
Lynx itself). Documented as a rule.

## Full API-surface mapping

### Existing-binding corrections

| Action | Detail |
|---|---|
| Keep `column-count` as a deprecated alias | Non-doc (doc has only `span-count`), BUT the pinned fork's Android `<list>` reads `column-count` while iOS reads `span-count` (per the icons example). Removing it regresses the Android grid, so it stays — documented as deprecated. |
| Replace `vertical-orientation` → `scroll-orientation` | Doc name; enum `vertical`/`horizontal`. |
| `item-key`: positional → stable `key` | Bug ① core. |

### Container props → `list` builder methods (`apply_attr*`)

`scroll-orientation`, `enable-scroll`, `enable-nested-scroll`, `sticky`, `sticky-offset`, `bounces`,
`initial-scroll-index`, `need-visible-item-info`, `upper-threshold-item-count`,
`lower-threshold-item-count`, `scroll-event-throttle`, `item-snap` (`{factor, offset}`),
`update-animation`, `need-layout-complete-info`, `preload-buffer-count`,
`scroll-bar-enable`, `experimental-recycle-sticky-item`, `list-main-axis-gap`, `list-cross-axis-gap`,
`harmony-scroll-edge-effect` (low priority). Already bound: `list-type`, `span-count`.

`layout-id` is **owned by the list** (like `item-key`): `__h()` bumps it on every data update so the
native list registers a new version and `layoutcomplete` can be correlated. Not a user prop.

### Item props → `list_item` element kwargs (Option E)

`full-span`, `sticky-top`, `sticky-bottom`, `reuse-identifier`, `estimated-main-axis-size-px`,
`recyclable`. (`item-key` owned by the list, stamped from `key`; not a user kwarg.)

### Events → `list` builder (`bind_typed`) + typed detail structs

| Whisker | Lynx | Detail type |
|---|---|---|
| `on_scroll` | `bindscroll` | offset/position (+ visible items if `need-visible-item-info`) |
| `on_scrolltoupper` | `bindscrolltoupper` | — |
| `on_scrolltolower` | `bindscrolltolower` | — (infinite scroll) |
| `on_scrollstatechange` | `bindscrollstatechange` | state enum |
| `on_layoutcomplete` | `bindlayoutcomplete` | layout id (+ diff if `need-layout-complete-info`) |
| `on_snap` | `bindsnap` | target position |

### Methods → new `ListHandle` (via `ElementRef`, like `ScrollViewHandle`)

| Whisker | Lynx | Note |
|---|---|---|
| `scroll_to_position(index, align, offset, smooth)` | `scrollToPosition` | aligns to index (anchor model — one shot, no FlashList-style refinement needed) |
| `scroll_by(offset, smooth)` | `scrollBy` | |
| `auto_scroll(rate, start)` | `autoScroll` | |
| `get_visible_cells()` | `getVisibleCells` | **result-returning → async; Android may need a fork build (see [[whisker_element_method_results_need_async]])** |

## Virtualization / pull path

Today `__h()` materializes every item; this is the first place Lynx **pulls** elements from Whisker
(vs Whisker’s usual push). Infra is ready (provider live + re-entrancy fixed + `list_mount`
prototype).

Work items:

- **P-1** Wire on-demand creation into the builder (`list_mount`-style: build in `component_at_index`).
- **P-2** `reuse-identifier` assignment (default one-per-list; per-item override via `list_item`).
- **P-3** Per-item reactive re-bind on same-key data change.
- **P-4** **Verify re-entrant element creation is safe with the *real* bridge renderer** (not just the
  test renderer): does `create_element` hold a `CHILDREN_OF`/`PARENT_OF` borrow across the FFI call?
  This is the go/no-go gate for true virtualization. The `__h()` “must not call back into the
  renderer” comment predates `#214` — confirm whether it’s stale or a real remaining constraint.
- **P-5** Recycle pool on `enqueue_component` (currently `None`).

Per **#83**, extract the container-agnostic core as **`Virtualizer<T, K>`** from day one
(`ListMount = Virtualizer + <list> element`; future `PagerMount = Virtualizer + module element`). The
issue notes this costs ~50 lines up front vs hundreds retrofitted.

## On-device verifications

**Verified on iOS simulator** (bluesky timeline + the `list-smoke` example). Four bugs surfaced and
were fixed — all whisker-side, **no fork change**:

- **P-4 / re-entrancy** — safe (bridge renderer holds no field borrow across FFI, #214). ✓
- **Context inheritance** — slot owners built in the native callback were detached roots, severing the
  context chain (items panicked in `use_navigator()`). Fixed by parenting slot owners to the setup
  owner. ✓
- **Provider append contract** — Lynx `ListElement::ComponentAtIndex` *requires* the embedder to
  `append_child(list, item)`; returning the sign alone crashed the native list
  (`OnListItemWillAppear` null deref). Fixed: append on build. ✓
- **Scroll/recycle use-after-free** — `enqueue_component` destroyed the element, but Lynx's
  `EnqueueElement` calls the callback *before* it detaches the element itself, so rows blanked out
  after scrolling. Fixed: `enqueue_component` is a no-op on the element; `component_at_index` caches
  built items by stable key and reuses the same element on re-query (built once per logical item). ✓
- **Bugs ②③** — the `list-smoke` full-span header renders full-width + full-height and stays intact
  through scroll-away-and-back (no crush); variable-height rows are all full-width. ✓
- Result: on-demand `Virtualizer` renders + scrolls (rows build on demand, reuse on scroll-back) with
  no blanks / crashes; bluesky timeline renders too. ✓

**Trade-off:** lazy materialization, **not true recycling** — a visited item's element stays alive
until list teardown (memory grows with distinct items scrolled to, not total). A recycle pool that
safely releases scrolled-out elements (deferred past the native's detach) is a follow-up.

**Still open:**

1. Bug ① (reorder/insert): stable item-key + dedup-reuse is unit-tested, but the live tap can't be
   automated (synthetic taps don't reach Lynx's `on_tap` recognizer on the sim). Needs a real device
   or an auto-trigger to confirm on-device.
2. Event `detail` payload shapes (`bindscroll`, `bindsnap`, `bindlayoutcomplete`) — structs use
   defaulted fields; confirm on device.
3. **Android parity** — all verification is iOS-sim so far.
4. `getVisibleCells` result return on Android.
5. `item-snap` — Lynx reads it as an **object** (`value[@"factor"]`); whisker's attr path is
   scalar-only, so it needs a new object-attribute bridge capi (fork work). Deferred.
6. `reuse-identifier` per-list default (P-2) and per-item in-place re-bind on same-key update (P-3):
   not implemented (relies on Lynx default / rebuild-on-change); optimizations, deferred.

## What this adds to Whisker

Most of the work is **additive bindings following existing patterns** — not new concepts:

- `list_item` promoted to a **public built-in element** + kwargs (`full_span`, `sticky_top`,
  `sticky_bottom`, `reuse_identifier`, `estimated_size`, `recyclable`) — same shape as `view` /
  `scroll_view`.
- ~20 documented **container props** on `list` (`apply_attr*`, like `scroll_view`).
- **6 events** + typed detail structs (`bind_typed`, like `ScrollEvent`).
- **`ListHandle`** (`scroll_to_position` / `scroll_by` / `auto_scroll` / `get_visible_cells`) —
  mirrors `ScrollViewHandle`.

**Phases 1–3 introduce zero new whisker concepts.** The only genuinely new concepts arrive in
Phase 4 (virtualization):

1. **Pull / data-source rendering direction (lynx→whisker)** — whisker has been push-only; the
   virtualized list is the first place Lynx *requests* elements on demand via `componentAtIndex`.
2. **`Virtualizer<T, K>`** — a container-agnostic runtime abstraction (slot pool, key diff,
   sign↔index map, provider wiring).
3. **External-recycler-driven slot owner lifecycle** — a slot's reactive owner is disposed by Lynx's
   `enqueue_component`, not by the reactive graph.

One low-level capability to confirm: **object-valued attribute** for `item-snap` (`{factor, offset}`).
Whisker's attr path is scalar-only (`apply_attr` / `_int` / `_bool` / `_double`); `item-snap` may need
a small new object-attr path, or be expressed as two scalars / a JSON string. Decide on device.

Explicitly **not** added (by design): runtime special-casing for `list_item`, a `tag_of` /
`wrap_child_in_slot` mechanism, an auto-wrap rule, or a `SlotContainer` trait (deferred to #83).

## Removed / breaking changes

Removed whisker internals:

- **`children` auto-wrap** in `__h()` (`create_element_by_name("list-item")` + wrap) — Option E has the
  user write `list_item` explicitly.
- **Positional `item-key="w_{index}"`** — replaced by stamping the stable `key`.
- **`vertical_orientation` builder method** — replaced by `scroll_orientation` (the documented name).
  (`column_count` is **kept** as a deprecated alias — Android grid still needs it; see corrections.)
- **Eager-materialize body** in `__h()` (Phase 4) — replaced by `Virtualizer` on-demand.
- **`enqueue_component: None`** — replaced by real recycle.
- **`list_mount` v1** (`let _ = key`) — refactored into `Virtualizer<T, K>`.

**Breaking — call sites must migrate** (whisker is pre-release; done in this same PR):

- All `list` `children` closures returning **bare content** must wrap in `list_item`. On `main`:
  podcast (search `result_row`, browse `section_block`, browse `(rank, podcast)`, detail
  `episode_list`).
- whisker-icons example: `column_count: 3` → `span_count: 3`.
- **`examples/bluesky` is not on `main`** (lives on `feat/bluesky-example-oauth`); its ~3 list sites +
  width/header workarounds migrate **when bluesky lands**, not in this PR.

## Phasing

1. **Foundation (Whisker-only, high confidence)** — stable `item-key`; `scroll-orientation` fix; drop
   `column-count`; add the straightforward container props; add all six events with typed details;
   `ListHandle` with `scroll_to_position`/`scroll_by`/`auto_scroll`. → unblocks infinite scroll &
   imperative scrolling immediately.
2. **Per-item props (Option E)** — promote `list_item` to a public element + kwargs; wire `full-span`/`estimated-size`/
   `reuse-identifier`/`sticky-*`/`recyclable`; verify bug ② / ③ on device.
3. **Reorder correctness** — `layout-id` + `update-animation`; verify bug ① on device.
4. **Virtualization** — `Virtualizer<T, K>` extraction (P-1..P-5), gated by P-4.

## Testing

- Extend the `CapturingRenderer` (in `list_mount.rs` tests) to assert: stable item-keys, per-item
  attr emission, event binding, provider call sequence.
- Device smoke for each on-device verification above.
- Remember: impl agents must run `cargo fmt` before commit (see [[whisker_impl_subagent_run_fmt]]).
