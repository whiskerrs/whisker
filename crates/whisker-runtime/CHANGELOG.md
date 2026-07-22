# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
