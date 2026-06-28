# Whisker internal documentation

Design notes, architecture, and conventions for people **working on
Whisker itself**.

> **Looking for how to *use* Whisker?** The user-facing documentation —
> getting started, guides, and the API reference — lives on the website:
> [whisker.rs/docs](https://whisker.rs/docs). This folder is for
> contributors and maintainers only.

## Contents

- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) — the practical "how do I build
  from source, run an example on a device, and submit a change" guide.
  **Read this first if you're new.**
- [`architecture.md`](architecture.md) — how the workspace is sliced
  into crates, the runtime layers, the Lynx bridge, and how the
  `whisker run` dev loop wires them together. **Start here.**
- [`reactivity-design.md`](reactivity-design.md) — the design and
  rationale of the fine-grained reactive runtime (signals, effects,
  the owner/scope tree, batching).
- [`hot-reload-internals.md`](hot-reload-internals.md) — how the Tier 1
  (subsecond) and Tier 2 (cold rebuild) hot-reload pipelines actually
  work, end to end.
- [`module-api-design.md`](module-api-design.md) — how to choose the
  user-facing surface shape for a new `whisker-*` module crate. Read
  before writing a new module.
- [`router-design.md`](router-design.md) — the router model: the static
  `RouteTree` (`routes!`) and the dynamic `NavState`, URL derivation,
  relative resolution, and the `navigate`/`back`/`replace`/`popTo`/`reset`
  operations.
- [`animation-design.md`](animation-design.md) — the continuous,
  signal-based animation engine (`AnimationController` + `Tween`), how it
  differs from Lynx's discrete CSS-keyframe animator, and how it backs the
  router's transitions.
- [`lynx-integration.md`](lynx-integration.md) — how Whisker integrates
  and distributes its Lynx fork (iOS SwiftPM binary targets, Android
  Maven AARs) and the fork's release process.
- [`ios-spm-distribution.md`](ios-spm-distribution.md) — how iOS apps
  resolve the runtime from the remote SwiftPM package, version lockstep,
  and the monorepo-dev caveat.
- [`comment-style.md`](comment-style.md) — the comment/doc convention.
  Cite it in code review.

## Conventions

- These docs describe the **current** design, not historical plans.
  When you change a system, update its doc in the same PR (or delete the
  doc if it no longer applies). Git history keeps the past.
- User-facing material belongs on the website, not here.
