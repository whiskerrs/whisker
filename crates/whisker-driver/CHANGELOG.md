# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.12.0...whisker-driver-v0.13.0) - 2026-09-03

### Added

- connect native hosts to hot reload
- negotiate host rendering capabilities
- align module APIs across hosts
- *(host)* preserve text direction through measurement
- *(host)* normalize pointer input across hosts
- *(host)* align text measurement typography
- *(host)* preserve basic text styling across hosts
- *(interaction)* implement cursor and pointer events
- *(text)* implement extended font settings
- *(text)* implement Lynx wrapping and overflow
- *(text)* implement Lynx text indentation
- *(text)* implement Lynx text alignment
- *(paint)* implement Lynx text decoration
- *(paint)* implement single text shadow
- *(paint)* implement image rendering sampling
- *(paint)* render backdrop blur across hosts
- *(clip)* render path commands across hosts
- *(clip)* render circle and ellipse paths across hosts
- *(clip)* render rounded inset paths across hosts
- *(shadow)* render hard outer box shadows
- *(background)* implement border-area clipping
- *(runtime)* lower intrinsic background size modes
- *(runtime)* add typed resource channel ABI
- *(mobile)* transfer background resource ids
- *(mobile)* transfer background layer arrays
- *(protocol)* negotiate rounded background geometry
- *(protocol)* negotiate spaced background geometry
- *(host)* size non-repeating background images
- *(host)* render resolved conic gradients
- *(host)* render explicit radial gradients
- *(host)* render resolved linear gradients
- *(host)* preserve elliptical border radii
- *(host)* unify retained module runtime across platforms
- *(elements)* implement RFC0004 module registry

### Fixed

- *(mobile)* retain runtimes across temporary detaches
- *(driver)* null empty ABI slices
- *(driver)* preserve length-prefixed string bytes
- *(input)* make Rust authoritative for hit testing
- *(runtime)* recover from host transaction failures
- *(ci)* restore module parity quality gates

### Other

- *(driver)* match measurement responses positionally
- *(protocol)* close host operation reachability gaps
- Merge remote-tracking branch 'origin/next-architecture' into codex/mobile-element-schema-freeze
- Isolate runtime state and module events per surface
- split large modules by responsibility
- *(abi)* establish mobile source of truth
- *(ui)* align builtin public contracts
- rebuild driver as mobile FFI adapter

## [0.11.1](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.11.0...whisker-driver-v0.11.1) - 2026-08-12

### Other

- Tear down the previous app tree when run() is called again ([#396](https://github.com/whiskerrs/whisker/pull/396)) ([#397](https://github.com/whiskerrs/whisker/pull/397))
- Give apps the Android back button, and give it back to Android ([#392](https://github.com/whiskerrs/whisker/pull/392))

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.10.12...whisker-driver-v0.11.0) - 2026-08-11

### Other

- Sweep driver, driver-sys, cng, animation comments ([#378](https://github.com/whiskerrs/whisker/pull/378))

## [0.10.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.9.2...whisker-driver-v0.10.0) - 2026-07-28

### Added

- *(modules)* real async module functions — AsyncFunction + Promise

## [0.9.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.8.2...whisker-driver-v0.9.0) - 2026-07-21

### Added

- *(renderer)* require Lynx insert_before; pin v3.8.0-whisker.13 (Phase C)
- *(renderer)* positioned insert via Lynx insert_before (Phase A)

### Other

- Merge remote-tracking branch 'origin/main' into lynx-insert-before-phase-a

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.7.0...whisker-driver-v0.8.0) - 2026-07-06

### Added

- *(hot-reload)* [**breaking**] saves only hot-reload — manual Full Reload (r/R), full-remount escalation, props-layout gate ([#287](https://github.com/whiskerrs/whisker/pull/287))
- *(list)* [**breaking**] ItemMeta — identity + per-item metadata unified; list_item removed ([#284](https://github.com/whiskerrs/whisker/pull/284))
- *(list)* minimal-diff data-source updates — scroll position holds across appends ([#281](https://github.com/whiskerrs/whisker/pull/281))
- *(list)* core-originated <list> events (scroll / scrolltolower / snap / layoutcomplete) now reach whisker ([#279](https://github.com/whiskerrs/whisker/pull/279))
- *(list)* exhaustive Lynx <list> binding + on-demand virtualization ([#276](https://github.com/whiskerrs/whisker/pull/276))

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.6.0...whisker-driver-v0.7.0) - 2026-06-26

### Added

- *(whisker-driver)* tokio feature — host a multi-thread runtime so reqwest/spawn_blocking just work ([#262](https://github.com/whiskerrs/whisker/pull/262))
- *(whisker-animation)* continuous signal-based animation engine ([#251](https://github.com/whiskerrs/whisker/pull/251))

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.5.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.4.3...whisker-driver-v0.5.0) - 2026-06-17

### Other

- [**breaking**] whisker owns the root page (remove user-facing `page`) ([#238](https://github.com/whiskerrs/whisker/pull/238))

## [0.4.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.3.1...whisker-driver-v0.4.0) - 2026-06-16

### Fixed

- *(reactive)* close edge-triggered lost-wakeup that wedged the render loop ([#228](https://github.com/whiskerrs/whisker/pull/228))

## [0.3.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.2.5...whisker-driver-v0.3.0) - 2026-06-15

### Fixed

- *(view)* make renderer dispatch re-entrancy-safe ([#214](https://github.com/whiskerrs/whisker/pull/214))
- *(driver)* run app() under a persistent root owner so app-level provide_context works ([#210](https://github.com/whiskerrs/whisker/pull/210))

## [0.2.5](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.2.4...whisker-driver-v0.2.5) - 2026-06-14

### Fixed

- *(driver)* drive async tasks off the native main loop (proper resource hang fix; supersedes #206) ([#207](https://github.com/whiskerrs/whisker/pull/207))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.1.0...whisker-driver-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)
