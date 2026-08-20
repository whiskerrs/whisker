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
