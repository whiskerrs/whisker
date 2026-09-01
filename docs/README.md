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
  into crates, the runtime layers, the mobile FFI Driver, and how the
  `whisker run` dev loop wires them together. **Start here.**
- [`reactivity-design.md`](reactivity-design.md) — the design and
  rationale of the fine-grained reactive runtime (signals, effects,
  the owner/scope tree, batching).
- [`hot-reload-internals.md`](hot-reload-internals.md) — how Hot Reload
  (subsecond patching) and Full Reload (cold rebuild) actually
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
  backs CSS animation/transition and the router's imperative transitions.
- [`ios-spm-distribution.md`](ios-spm-distribution.md) — how iOS apps
  resolve the runtime from the remote SwiftPM package, version lockstep,
  and the monorepo-dev caveat.
- [`comment-style.md`](comment-style.md) — the comment/doc convention.
  Cite it in code review.
- [`documentation.md`](documentation.md) — which facts belong in internal docs,
  the website, Rustdoc, or RFCs, plus the validation commands and update
  checklist.
- [`rfcs/`](rfcs/README.md) — proposed and accepted architectural changes.
  An accepted RFC records a decision; it does not describe shipped behavior
  until its status is `Implemented` and the current-design docs are updated.
- [`../.agents/skills/release-whisker/SKILL.md`](../.agents/skills/release-whisker/SKILL.md)
  — cutting a release: which of the four artifact streams a change
  needs, in what order, and how to recover one that stalled.

## Conventions

- Except for [`rfcs/`](rfcs/README.md), these docs describe the **current**
  design, not historical plans.
  When you change a system, update its doc in the same PR (or delete the
  doc if it no longer applies). Git history keeps the past.
- User-facing material belongs on the website, not here.
- Public Rust API contracts belong in Rustdoc. Internal docs may explain how
  those contracts compose, but should link to rather than duplicate signatures.
