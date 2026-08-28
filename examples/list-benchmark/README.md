# Whisker List benchmark

This example is a repeatable, profiler-oriented workload for the Rust-owned
`List`. It renders 100,000 logical rows while only materializing the viewport
window. Each mounted row contains a box and three text nodes so scrolling
exercises layout, text, paint, FramePacket generation, and Host application.
The rows use `recycled_children:`: their `ReadSignal<u32>` is rebound while
compatible Rust owners, element handles, and Host views retain identity.

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

This reports the cost of 1,000 public Host `scroll` events and transactional
FramePacket presentations over the 100,000 row source, split into runtime and
presentation time with the average operation count. It deliberately excludes
Host drawing; use the app and platform profiler for end-to-end frame results.
