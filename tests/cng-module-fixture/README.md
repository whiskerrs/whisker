# CNG module fixture

This standalone workspace supplies Cargo metadata and Host manifests to the
CLI/CNG module-discovery and project-generation tests. It has no dependencies
on the repository's examples, packages, or published crates.

- `widget` declares Desktop and Web Rust hosts and an iOS SwiftPM host.
- `service` declares a Web Rust host and common-only Desktop/iOS support.
- The root package consumes both modules, with no CNG plugins.

The empty Rust libraries and Swift registration stub are inputs for discovery
and generated-source assertions, not runnable Whisker modules. Actual consumer
linking is covered separately by `tests/mobile-link-test` and
`tests/rust-host-link-test`.

Run the relevant tests from the repository root:

```sh
cargo test -p whisker-cng --lib
cargo test -p whisker-cli --lib platforms::tests
```
