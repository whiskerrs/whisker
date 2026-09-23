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
- [`architecture.md`](architecture.md) — how Rust and each platform Host
  share the work, how an input becomes a frame, who owns runtime state,
  and where to find the implementation. **Start here for the overall design.**
- [`reactivity-design.md`](reactivity-design.md) — the design and
  rationale of the fine-grained reactive runtime (signals, effects,
  the owner/scope tree, batching).
- [`text-design.md`](text-design.md) — paragraph compilation, atomic inline views,
  Host text layout, selection, and prepared-content lifetime.
- [`list-design.md`](list-design.md) — how Rust virtualizes keyed items
  over a ScrollView and reconciles their sizes and scroll position.
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
- [`cng-design.md`](cng-design.md) — current CNG composition model,
  platform configuration boundaries, and the division between generated
  app projects and settings owned by native packages.
- [`cng-android-ir.md`](cng-android-ir.md) — declarative Android IR coverage,
  Gradle module and variant contracts, and composition/validation boundaries.
- [`cng-plugin-composition.md`](cng-plugin-composition.md) — application policy,
  built-in plugin ownership, native scaffolding, declarative composition,
  and versioned subprocess compatibility.
- [`cng-apple-ir.md`](cng-apple-ir.md) — iOS/macOS target, configuration,
  scheme and bundle contracts, composition, and native validation boundaries.
- [`cng-web-ir.md`](cng-web-ir.md) — Web output ownership, static HTML,
  PWA registration, response headers, and composition contracts.
- [`cng-desktop-ir.md`](cng-desktop-ir.md) — Windows/Linux OS metadata,
  packaging, native XML edits, and platform-specific validation.
- [`comment-style.md`](comment-style.md) — the comment/doc convention.
  Cite it in code review.
- [`documentation.md`](documentation.md) — which facts belong in internal docs,
  the website, or Rustdoc, plus the validation commands and update
  checklist.
- [`../.agents/skills/release-whisker/SKILL.md`](../.agents/skills/release-whisker/SKILL.md)
  — cutting a release: which of the four artifact streams a change
  needs, in what order, and how to recover one that stalled.

## Conventions

- These docs describe the **current** design, not historical plans.
  When you change a system, update its doc in the same PR (or delete the
  doc if it no longer applies). Git history keeps the past.
- User-facing material belongs on the website, not here.
- Public Rust API contracts belong in Rustdoc. Internal docs may explain how
  those contracts compose, but should link to rather than duplicate signatures.
- Keep investigation logs, benchmark captures, migration handoffs, and temporary
  review inventories outside the repository. Move durable conclusions into the
  relevant design document instead of keeping a second, dated description.
