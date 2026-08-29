# Rust-owned List design

Status: **implemented**

## Decision

`list` is keyed Rust control flow built on the ordinary
`whisker.ui/ScrollView`. It is not a Host element and does not wrap Lynx
`<list>`, `RecyclerView`, or `UICollectionView`.

```rust
let handle = ListHandle::<RowId>::new();

render! {
    list(
        ref: handle.r(),
        style: css!(height: px(480)),          // viewport
        content_style: css!(row_gap: px(8)),  // content layout
        each: move || rows.get(),
        key: |row: &Row| row.id,
        children: |row: ReadSignal<Row>| render! { row_view(row: row) },
    )
}
```

The Host-visible tree contains only ordinary primitives:

```text
ScrollView
  └─ content View
       ├─ optional header
       ├─ leading spacer View
       ├─ mounted keyed item subtrees
       ├─ trailing spacer View
       └─ optional footer
```

When the source is empty, `empty` replaces the spacers and item range between
the optional header and footer. `FramePacket` has no List operation, element
type, data source, or recycling protocol.

## Public contract

The required inputs are `each`, `key`, and `children`. The child receives a
`ReadSignal<T>`:

- replacing data with the same key updates that signal and preserves its
  reactive Owner and local state;
- leaving the mounted window disposes the Owner;
- re-entering later creates a fresh Owner;
- duplicate keys fail deterministically in Rust.

There is no public estimated/fixed size, reuse identifier, recyclable child,
or item metadata API. Mounted item extents are learned from Taffy's completed
Rust layout. A private 44 logical-pixel bootstrap extent exists only until a
row has produced layout feedback.

Optional configuration:

- `axis: ScrollAxis::{Vertical, Horizontal}`;
- `content_style`, applied to the internal content View while `style` applies
  to the ScrollView viewport;
- reactive `scroll_enabled`;
- `header`, `footer`, and `empty` render functions;
- logical-pixel `start_reached_threshold` / `end_reached_threshold` and their
  edge-entry callbacks;
- typed `initial_scroll: ListScrollTarget<K>`.

Linear layout is the default. A constrained Grid may be requested through a
static typed `content_style`; the virtualizer derives its private track model
from that same style and every mounted track is still laid out by Taffy.
Supported Grid configuration is deliberately small and deterministic:

- a fixed explicit cross-axis track count, including `repeat(<count>, ...)`;
- fixed, percentage, and `fr` cross-axis tracks;
- sparse source-order auto placement;
- row/column gaps (the main-axis gap is currently a non-negative logical-pixel
  value);
- variable item sizes and both vertical and horizontal Lists.

Dense placement, explicit item placement/span/order, named lines/areas,
`auto-fill`/`auto-fit`, intrinsic track sizing, and a main-axis explicit
template fail immediately as unsupported virtualized Grid. Those constraints
apply only to `list`; ordinary `view` Grid remains whatever Taffy supports.
Reactive changes that switch the List content between linear and Grid are also
rejected because the virtual track topology must be stable. Scroll snapping
remains a ScrollView/CSS capability rather than a List API.

## Imperative API

`ListHandle<K>` and `ListRef<K>` are typed by the same stable key used by the
List. They expose:

- `scroll_to(ListScrollTarget<K>, ScrollBehavior)` for start, end, logical
  offset, index, or key targets;
- `scroll_by(delta, ScrollBehavior)`;
- `snapshot()` with cached offset, viewport/content extents, visible indices,
  first visible key, and visible keys;
- a reactive `bound()` signal.

Key/index lookup and snapshots use the Rust prefix index. Only the final
`scrollTo` or `scrollBy` command crosses the Host boundary. Initial key targets
remain pending until that key exists in a source snapshot.

## Ownership and update model

Rust owns the source snapshot, key index, mounted range, item Owners, measured
extents, and first-visible-key anchoring. The Host owns native scroll physics,
clipping, and transient offset. It reports one standard ScrollView event map:

- `scrollLeft`, `scrollTop`;
- `scrollWidth`, `scrollHeight`;
- `viewportWidth`, `viewportHeight`.

All values are logical pixels/points. Scroll reconciliation is
`O(log n + delta)`: binary searches select the range and only entering/leaving
edges mutate during one source generation. Source changes rebuild the key and
prefix index in `O(n)`.

When layout feedback changes an extent or the source is prepended/reordered,
the runtime preserves the first visible key and its intra-item offset. Any
required correction is one instant standard ScrollView command before the
next presentation.

## Presentation reuse

Logical state is never recycled across keys. Independently, each Host may keep
a bounded, type-indexed pool of deleted built-in View/Text presentations. This
optimization lives in the normal `CreateNode`/`DeleteNode` path, so it applies
to List and non-List trees alike and adds no ABI.

Before reuse, protocol-owned properties, event masks, text/common
presentation, DOM attributes, and parentage are reset. Custom element
presentations are destroyed normally; module authors do not need a List-aware
reuse contract and custom native state cannot leak between keys.

Desktop uses the same general pool for its lightweight element-content
objects. Android and iOS reuse their built-in native View/Text content, and
Web reuses the corresponding DOM/native wrapper. Every pool is bounded.

## Complexity targets

- source/index rebuild: `O(n)`;
- steady-state reconciliation: `O(log n + entering + leaving)`;
- mounted logical subtrees: `O(visible + overscan)`;
- List-specific Host classes and ABI: zero.
