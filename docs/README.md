# Whisker Documentation

Design docs and architectural notes. See the workspace
[README](../README.md) for status.

## Current documents

- [`architecture.md`](architecture.md) — crate graph, hot-reload
  feature flow, end-to-end Tier 1 patch sequence
- [`hot-reload-plan.md`](hot-reload-plan.md) — design + implementation
  log for the subsecond-based hot-reload pipeline
- [`module-api-design.md`](module-api-design.md) — how to pick a
  user-facing surface shape for a new module crate (component
  vs. handle vs. signal-returning fn). Read this before writing a
  new `whisker-*` module
- [`module-author-guide.md`](module-author-guide.md) — mechanics of
  wiring Kotlin / Swift / Rust into one module crate
- [`comment-style.md`](comment-style.md) — rustdoc vs. internal
  comment standard. Cite in code review

## Planned documents

- `lynx-integration.md` — how we layer on top of Lynx Android/iOS SDK + Element PAPI
- `runtime.md` — reactive primitives, element tree, diffing
- `building.md` — how to build Whisker itself (workspace, native bridge, AAR, xcframework)
