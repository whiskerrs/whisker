# Whisker Documentation

Design docs and architectural notes. See the workspace
[README](../README.md) for status.

## Current documents

- [`architecture.md`](architecture.md) — crate graph, hot-reload
  feature flow, end-to-end Tier 1 patch sequence
- [`hot-reload-plan.md`](hot-reload-plan.md) — design + implementation
  log for the subsecond-based hot-reload pipeline

## Planned documents

- `lynx-integration.md` — how we layer on top of Lynx Android/iOS SDK + Element PAPI
- `runtime.md` — reactive primitives, element tree, diffing
- `building.md` — how to build Whisker itself (workspace, native bridge, AAR, xcframework)
