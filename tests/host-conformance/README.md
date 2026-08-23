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

`capabilities.json` is the cross-Host implementation checklist. Its property
lists describe semantic ownership rather than one Host function per CSS
spelling: shorthands are resolved in Rust, layout entries become `SetLayout`,
and motion is sampled by Rust before a frame. `protocol-only` means the common
protocol can carry the value but the Host must reject that operation until its
production implementation and conformance scenarios land. It must never be
treated as successful no-op support. `partial` means at least one value family
or required checkpoint is still missing, even when the common operation is
already consumed.

The target is pinned to 175 standard CSS conformance features: 174 registered
property spellings plus the CSS Custom Properties mechanism. It starts from
the 191-property Lynx inventory at the recorded upstream revision, excludes 32
Lynx-only properties and the three unprefixed non-standard `text-stroke*`
spellings, then adds 19 standard properties that were already part of
Whisker's registry. The legacy aliases `grid-column-gap`, `grid-row-gap`, and
`word-wrap` are intentionally absent; their canonical spellings are tracked
instead. Stable IDs previously assigned to removed compatibility spellings are
reserved and are never reused.
