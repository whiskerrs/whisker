# Whisker List benchmark

This example is a repeatable, profiler-oriented workload for the Rust-owned
`List`. It renders 100,000 logical cells in a two-column CSS Grid while only
materializing the viewport window. List virtualizes 50,000 fixed-height Grid
rows; each mounted row uses Taffy Grid to place two cards, and each card owns
three text nodes. Scrolling therefore exercises virtualization, Grid layout,
text, paint, FramePacket generation, and Host application together. The rows
receive a keyed `ReadSignal<u32>`: while a key remains mounted, value updates
retain the Rust owner, element handles, and Host views.

Run it from this directory:

```sh
whisker run desktop
whisker run web
whisker run android
whisker run ios
```

Use a release build when comparing implementations. Record at least:

- time to first presented frame;
- p50, p95, and p99 frame duration during a top-to-bottom fling;
- peak resident memory and mounted native view count;
- FramePacket operation count and bytes per frame;
- blank or incorrect rows during fast and reverse-direction scrolling.

Keep the item count, viewport, device, build profile, and gesture identical
between runs. Debug builds and simulator-to-device comparisons are not valid
performance comparisons.

For a repeatable Rust-runtime-only measurement, run the ignored benchmark in
release mode:

```sh
cargo test -p list-benchmark --test scroll_benchmark --release -- --ignored --nocapture
```

This reports the cost of 1,000 public Host `scroll` events and complete runtime
frames over the 100,000-row source, split into input dispatch and frame work
with the average operation count. It deliberately excludes Host drawing; use
the app and platform profiler for end-to-end frame results.
