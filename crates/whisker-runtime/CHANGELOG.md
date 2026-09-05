# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.5](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.13.3...whisker-runtime-v0.13.5) - 2026-09-05

### Fixed

- *(runtime)* recognize long presses and preserve List margins ([#674](https://github.com/whiskerrs/whisker/pull/674))
- *(list)* preserve routed state and Android overflow ([#673](https://github.com/whiskerrs/whisker/pull/673))

### Other

- release v0.13.4 ([#672](https://github.com/whiskerrs/whisker/pull/672))

## [0.13.4](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.13.3...whisker-runtime-v0.13.4) - 2026-09-05

### Fixed

- *(runtime)* recognize long presses and preserve List margins ([#674](https://github.com/whiskerrs/whisker/pull/674))
- *(list)* preserve routed state and Android overflow ([#673](https://github.com/whiskerrs/whisker/pull/673))

## [0.13.3](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.13.2...whisker-runtime-v0.13.3) - 2026-09-04

### Fixed

- preserve initial position for virtualized lists ([#669](https://github.com/whiskerrs/whisker/pull/669))

### Other

- *(list)* optimize retained virtual layout ([#671](https://github.com/whiskerrs/whisker/pull/671))

## [0.13.2](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.13.1...whisker-runtime-v0.13.2) - 2026-09-04

### Fixed

- *(runtime)* preserve list row owner context ([#667](https://github.com/whiskerrs/whisker/pull/667))

## [0.13.1](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.13.0...whisker-runtime-v0.13.1) - 2026-09-04

### Fixed

- *(runtime)* retain touch sequence targets ([#665](https://github.com/whiskerrs/whisker/pull/665))
- *(runtime)* allow tasks to spawn while polling ([#663](https://github.com/whiskerrs/whisker/pull/663))

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.12.0...whisker-runtime-v0.13.0) - 2026-09-03

### Added

- *(router)* define navigation semantics
- *(macros)* unify composition around public builders
- connect native hosts to hot reload
- negotiate host rendering capabilities
- *(list)* finalize virtualization grid and scrolling
- *(list)* add Rust-owned virtualized list foundation
- align module APIs across hosts
- *(motion)* complete transform and lifecycle support
- *(host)* unify retained module runtime across platforms
- *(elements)* implement RFC0004 module registry

### Fixed

- *(runtime)* recover from host transaction failures
- *(router)* complete cross-platform navigation
- *(web)* keep virtual rows ahead of scrolling
- *(ci)* restore module parity quality gates
- *(desktop)* connect scrolling and pointer targets

### Other

- Merge pull request #656 from whiskerrs/codex/review-rollup-tooling-ci
- *(ci)* satisfy current stable clippy
- refresh architecture and Rust API guidance
- Merge remote-tracking branch 'origin/next-architecture' into codex/mobile-element-schema-freeze
- Isolate runtime state and module events per surface
- remove legacy Lynx runtime dependencies
- split large modules by responsibility
- *(ui)* align builtin public contracts
- *(css)* require structured style values
- *(list)* recycle compatible presentation slots
- *(list)* index large sources for scrolling
- make component commands one-way
- rebuild driver as mobile FFI adapter
- Complete retained Rust rendering and Host-driven runtime loop ([#418](https://github.com/whiskerrs/whisker/pull/418))

## [0.11.1](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.11.0...whisker-runtime-v0.11.1) - 2026-08-12

### Other

- Give apps the Android back button, and give it back to Android ([#392](https://github.com/whiskerrs/whisker/pull/392))

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.10.12...whisker-runtime-v0.11.0) - 2026-08-11

### Other

- Sweep whisker-runtime comments ([#381](https://github.com/whiskerrs/whisker/pull/381))

## [0.10.12](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.10.11...whisker-runtime-v0.10.12) - 2026-08-10

### Other

- Keep app-root contexts resolvable after a hot-reload remount ([#373](https://github.com/whiskerrs/whisker/pull/373))

## [0.10.0](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.9.2...whisker-runtime-v0.10.0) - 2026-07-28

### Fixed

- *(reactive)* run an owner's cleanups before freeing its nodes
- *(animation)* a disposed controller must not crash the animation step

## [0.9.1](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.9.0...whisker-runtime-v0.9.1) - 2026-07-22

### Fixed

- *(renderer)* insert multi-child phantom hoist back-to-front

## [0.9.0](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.8.2...whisker-runtime-v0.9.0) - 2026-07-21

### Added

- *(renderer)* require Lynx insert_before; pin v3.8.0-whisker.13 (Phase C)
- *(renderer)* positioned insert via Lynx insert_before (Phase A)

### Fixed

- *(pan-intercept)* encode direction/scope as wire ints, not strings ([#312](https://github.com/whiskerrs/whisker/pull/312))

### Other

- *(renderer)* insert_child_at via positioned insert, no rotate
- Merge remote-tracking branch 'origin/main' into lynx-insert-before-phase-a

## [0.8.2](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.8.1...whisker-runtime-v0.8.2) - 2026-07-12

### Added

- *(reactive)* add Callback<In, Out> — Copy event-handler-prop wrapper ([#299](https://github.com/whiskerrs/whisker/pull/299))

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.7.0...whisker-runtime-v0.8.0) - 2026-07-06

### Added

- *(hot-reload)* [**breaking**] saves only hot-reload — manual Full Reload (r/R), full-remount escalation, props-layout gate ([#287](https://github.com/whiskerrs/whisker/pull/287))
- *(list)* [**breaking**] ItemMeta — identity + per-item metadata unified; list_item removed ([#284](https://github.com/whiskerrs/whisker/pull/284))
- *(list)* minimal-diff data-source updates — scroll position holds across appends ([#281](https://github.com/whiskerrs/whisker/pull/281))
- *(list)* exhaustive Lynx <list> binding + on-demand virtualization ([#276](https://github.com/whiskerrs/whisker/pull/276))

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.6.0...whisker-runtime-v0.7.0) - 2026-06-26

### Added

- *(whisker-router)* reactive rendering — Outlet/Stack/Switch, transitions, swipe-back (phase 2) ([#258](https://github.com/whiskerrs/whisker/pull/258))
- *(whisker-animation)* continuous signal-based animation engine ([#251](https://github.com/whiskerrs/whisker/pull/251))

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.6.0](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.5.1...whisker-runtime-v0.6.0) - 2026-06-18

### Added

- [**breaking**] signal() returns a single RwSignal instead of a (Read, Write) tuple ([#244](https://github.com/whiskerrs/whisker/pull/244))

## [0.4.0](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.3.1...whisker-runtime-v0.4.0) - 2026-06-16

### Fixed

- *(reactive)* close edge-triggered lost-wakeup that wedged the render loop ([#228](https://github.com/whiskerrs/whisker/pull/228))

## [0.3.0](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.2.5...whisker-runtime-v0.3.0) - 2026-06-15

### Added

- *(reactive)* make Signal<T> Copy ([#213](https://github.com/whiskerrs/whisker/pull/213))

### Fixed

- *(view)* make renderer dispatch re-entrancy-safe ([#214](https://github.com/whiskerrs/whisker/pull/214))
- *(runtime)* wake tasks driven from foreign threads ([#212](https://github.com/whiskerrs/whisker/pull/212))

## [0.2.5](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.2.4...whisker-runtime-v0.2.5) - 2026-06-14

### Fixed

- *(driver)* drive async tasks off the native main loop (proper resource hang fix; supersedes #206) ([#207](https://github.com/whiskerrs/whisker/pull/207))

## [0.2.4](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.2.3...whisker-runtime-v0.2.4) - 2026-06-13

### Fixed

- *(reactive)* make `resource` fetcher reactive to the signals it reads ([#204](https://github.com/whiskerrs/whisker/pull/204))

## [0.2.1](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.2.0...whisker-runtime-v0.2.1) - 2026-06-11

### Fixed

- router hit-test, render! alias ergonomics, safe-area owner crash ([#195](https://github.com/whiskerrs/whisker/pull/195))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-runtime-v0.1.0...whisker-runtime-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)

### Fixed

- generated starter compiles; drop dangling Suspense doc-link
