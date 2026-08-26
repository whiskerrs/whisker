# Host conformance scenarios

These fixtures replace the Rust runtime only at the Host boundary. A Host
runner decodes a scenario, then calls its production measurement, frame,
painting, and event paths. The fixture format is intentionally independent of
the production Rust, JNI, C, or WASM transport.

Files under `wpt/` are adaptations of selected Web Platform Tests. Every
scenario records its upstream path, reference path, pinned WPT revision,
license, assertion, and adaptation. Native Hosts do not claim to execute the
original HTML/CSS test; they execute equivalent semantic Host commands.

The first `paint/box` slice uses both semantic and offscreen GPU pixel
checkpoints. Core scenarios also exercise production Host measurement without
mounting `RuntimeInstance`. A recording input sink replaces the Rust runtime at
the opposite boundary so normalized Host input can be checked independently;
each Host runner connects its native-event adapter to the same sink contract.

## Contract and runners

`manifest.json` is the ordered coverage gate. A case is only mandatory for a
Host after that Host appears in `required_hosts`; every listed fixture is still
decoded and validated so pending coverage cannot silently rot. The language-
neutral formats are documented by `schema/manifest.schema.json` and
`schema/scenario.schema.json`. `whisker-host-conformance` is a test-only Rust
decoder, not a second source of fixture semantics.

Every runner injects commands at the Host boundary and uses production Host
code. Desktop creates real protocol packets and performs offscreen wgpu pixel
comparison. Web creates protocol packets and drives the real `DomFrameSink` in
headless Chrome; its current checkpoint is semantic DOM projection because the
DOM has no synchronous texture readback API. Android stages the generated
production `WhiskerView.kt`, injects ABI-equivalent operations, and compares
bitmaps on an emulator. iOS compiles the generated production
`WhiskerView.swift`, injects the mobile C ABI frame, and compares UIKit layer
captures in a Simulator.

Manual visual WPT adaptations may attach logical-pixel `samples` to a paint
checkpoint instead of inventing an exact reference image for behavior whose
fine geometry is intentionally implementation-defined by CSS. Desktop,
Android, and iOS assert those samples against production raster output. Web
asserts the equivalent CSS projection and leaves dash/dot distribution to the
browser, while sharing the same scenario and semantic values.

The same checkpoint may use relative luminance `relations` when CSS requires
a lighter or darker rendering but deliberately leaves the exact derived color
to the user agent. Native rasterizers compare the requested pixels while Web
continues to verify the corresponding semantic CSS value.

From the repository root, run one Host with:

```sh
cargo xtask host-conformance desktop
cargo xtask host-conformance web
cargo xtask host-conformance android
cargo xtask host-conformance ios
```

Desktop conformance is gated by the `whisker-desktop/host-conformance` feature,
which only the `xtask` entry point enables. Ordinary workspace unit tests do
not execute the offscreen GPU fixture suite; CI reports it as an independent
Host check.

Web requires `wasm-pack`, Chrome, and `curl`. `xtask` detects the installed
Chrome version and caches a compatible ChromeDriver under `target/xtask`; set
`GOOGLE_CHROME_BIN` for a non-standard Chrome location or `CHROMEDRIVER` to use
an explicit driver. Android and iOS require a locally installed platform SDK.
`xtask` uses the running Android emulator and boots an available iPhone
Simulator. The same entry points are used by CI.

`capabilities.json` is the cross-Host implementation checklist. Its property
lists describe semantic ownership rather than one Host function per CSS
spelling: shorthands are resolved in Rust, layout entries become `SetLayout`,
and motion is sampled by Rust before a frame. `protocol-only` means the common
protocol can carry the value but the Host must reject that operation until its
production implementation and conformance scenarios land. It must never be
treated as successful no-op support. `partial` means at least one value family
or required checkpoint is still missing, even when the common operation is
already consumed.

The target follows two explicit baselines. Layout semantics are the CSS subset
represented by Taffy 0.13. Non-layout properties follow the standard,
non-vendor Lynx 4.0 inventory at the recorded upstream revision. The resulting
target is 155 conformance features: 154 properties plus the CSS Custom
Properties mechanism. Of the 177 currently registered spellings, 154 remain
in the target and 23 are deliberately unsupported. The legacy aliases
`grid-column-gap`, `grid-row-gap`, and `word-wrap` remain absent. Stable IDs
assigned to excluded spellings are reserved and are never reused.
