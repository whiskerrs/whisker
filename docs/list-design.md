# Rust-owned List design

Status: **implemented foundation**

## Decision

`list` is a runtime control primitive in the same family as `ForEach`. It is
not a fourth built-in Host element and it is not a wrapper around
`UICollectionView`, `RecyclerView`, or Lynx `<list>`.

User code remains declarative:

```rust
render! {
    list(
        style: css!(height: px(480)),
        each: move || rows.get(),
        key: |row: &Row| row.id,
        children: |row: Row| render! { row_view(row: row) },
    )
}
```

For non-uniform rows, `meta:` replaces `key:` and returns
`ItemMeta::key(row.id).estimated_size(estimated_height)`.

The lowering is deliberately ordinary:

```text
list control state (Rust only)
  └─ ScrollView (standard whisker.ui element)
       ├─ leading spacer View
       ├─ visible item subtrees
       └─ trailing spacer View
```

FramePacket never contains a List element type. Hosts receive the same
ScrollView/View/Text/custom-element operations they already implement.

## Ownership

Rust owns:

- keyed data diffing and item identity;
- the visible/overscan range;
- item owners and lifecycle;
- estimated and later measured item extents;
- layout policy (linear, horizontal, and Grid);
- scroll anchoring when estimates are corrected;
- snap target selection when CSS scroll snap is enabled.

The Host owns:

- native scrolling physics and transient offset;
- clipping and presentation;
- reporting logical scroll geometry through the standard node-event path.

The ScrollView `scroll` event detail is one `WhiskerValue::Map`:

- `scrollLeft`, `scrollTop`
- `scrollWidth`, `scrollHeight`
- `viewportWidth`, `viewportHeight`

All values are logical pixels/points. This is a generic ScrollView contract,
not a List ABI.

## Identity model

Three identities must remain separate:

1. Item key — stable application identity from `ItemMeta::key`.
2. Mounted owner — reactive state belonging to one visible keyed item.
3. Future presentation slot — recyclable Host-independent storage selected by
   item type/reuse identifier.

The foundation implements the first two. It mounts only a bounded keyed
window and preserves surviving owners. Plain `children:` disposes items that
leave the window. The opt-in `recycled_children:` path gives each presentation
slot a stable `ReadSignal<T>`; a leaving slot can be rebound to an entering item
with the same interned `reuse_identifier`. Reactive props update from that
signal while the Rust owner, element handles, and Host views retain identity.
`recyclable(false)` forces disposal instead. Unused slots are disposed before
the frame is presented, so detached nodes never accumulate in the retained
scene. A recycled builder must produce one stable subtree shape per
`reuse_identifier`, and every item-derived prop must read the slot signal (or a
`computed` derived from it). The old Lynx native item provider, numeric signs,
list action stream, and list-specific FFI are removed.

The source snapshot, stable keys, and prefix offsets are rebuilt only when the
reactive item source changes. A Host scroll event finds the visible window with
binary searches over that index and performs only the entering/leaving edge
mutations. Within one source generation it visits the range difference rather
than rescanning retained rows, inserts entering children directly at their
final positions, and clones only entering items. Scroll work is therefore
`O(log n + delta)` for `n` logical items and `delta` entering/leaving items; it
does not clone or rescan the complete source.

## Layout feedback

The initial window uses `estimated_size` (44 logical pixels when omitted).
Leading and trailing spacer Views preserve the complete estimated scroll
extent. A Host scroll event immediately reconciles the range using the real
viewport and offset.

The next layout slice replaces estimates with Taffy results after a mounted
item is laid out. Corrections must be applied before presentation and preserve
the first visible key as an anchor. This feedback belongs in
`SurfaceRuntime`; it must not introduce a Host List element.

## Horizontal, Grid, and snap

- Horizontal List is the same virtualizer using the inline axis and a generic
  horizontal ScrollView.
- Grid uses Taffy's Grid layout. The virtualizer groups item metadata into
  main-axis rows/tracks; it does not independently reimplement CSS Grid.
- Scroll snap is a ScrollView capability. A List inherits it because its Host
  container is a ScrollView. Pager and short-video feeds are therefore
  horizontal/vertical List plus mandatory snap, not separate Host widgets.

These policies are intentionally downstream of the vertical linear tracer
bullet. Their tests must use the same public `render! -> SurfaceRuntime ->
FramePacket/input event` seam before implementation.

## Complexity target

- Source/index rebuild after data changes: `O(n)`.
- Steady-state scroll reconciliation: `O(log n + entering + leaving)`.
- Mounted element subtrees: `O(visible + overscan)`.
- Host-specific List code and List-specific ABI: zero.
