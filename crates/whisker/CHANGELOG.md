# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.3](https://github.com/whiskerrs/whisker/compare/whisker-v0.13.2...whisker-v0.13.3) - 2026-09-04

### Fixed

- preserve initial position for virtualized lists ([#669](https://github.com/whiskerrs/whisker/pull/669))

### Other

- *(list)* optimize retained virtual layout ([#671](https://github.com/whiskerrs/whisker/pull/671))

## [0.13.1](https://github.com/whiskerrs/whisker/compare/whisker-v0.13.0...whisker-v0.13.1) - 2026-09-04

### Fixed

- *(runtime)* retain touch sequence targets ([#665](https://github.com/whiskerrs/whisker/pull/665))

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-v0.12.0...whisker-v0.13.0) - 2026-09-03

### Added

- *(macros)* unify composition around public builders
- connect native hosts to hot reload
- *(list)* finalize virtualization grid and scrolling
- *(list)* add Rust-owned virtualized list foundation
- align module APIs across hosts
- *(style)* complete typed custom properties
- *(motion)* complete transform and lifecycle support
- *(motion)* run CSS transitions and keyframes in Rust
- *(style)* add typed custom properties
- *(interaction)* implement cursor and pointer events
- *(text)* implement extended font settings
- *(text)* implement Lynx wrapping and overflow
- *(text)* implement Lynx text indentation
- *(text)* implement Lynx text alignment
- *(paint)* implement Lynx text decoration
- *(paint)* implement single text shadow
- *(paint)* implement image rendering sampling
- *(transform)* lower SVG elliptical motion paths
- *(transform)* resolve rounded inset motion paths
- *(transform)* resolve circle and ellipse motion paths
- *(transform)* lower quadratic and cubic motion paths
- *(transform)* lower Lynx polyline motion paths
- *(transform)* lower Lynx perspective on current nodes
- *(transform)* project flat-plane 3d transforms
- *(transform)* lower typed 2d transforms after layout
- *(paint)* render backdrop blur across hosts
- *(clip)* render path commands across hosts
- *(clip)* render circle and ellipse paths across hosts
- *(clip)* render rounded inset paths across hosts
- *(shadow)* render hard outer box shadows
- *(background)* implement border-area clipping
- *(background)* lower typed gradients to frames
- *(background)* preserve per-layer URL geometry
- *(paint)* resolve logical borders in Rust
- *(layout)* support block floats and clearance
- *(css)* connect typed Grid styles to Taffy
- *(runtime)* lower intrinsic background size modes
- *(runtime)* project background geometry into frames
- *(runtime)* lower background URLs through resource channel
- *(runtime)* add typed resource channel ABI
- *(mobile)* transfer background resource ids
- *(mobile)* transfer background layer arrays
- *(protocol)* negotiate content-box backgrounds
- *(protocol)* negotiate rounded background geometry
- *(protocol)* negotiate spaced background geometry
- *(protocol)* negotiate repeated background geometry
- *(host)* resolve background origin and clip boxes
- *(host)* position explicit background images
- *(host)* size non-repeating background images
- *(host)* render resolved conic gradients
- *(host)* render explicit radial gradients
- *(host)* render resolved linear gradients
- *(host)* preserve elliptical border radii
- *(protocol)* define complete CSS paint semantics
- *(host)* unify retained module runtime across platforms
- *(elements)* implement RFC0004 module registry
- *(desktop)* add shared box conformance host

### Fixed

- *(mobile)* retain runtimes across temporary detaches
- *(runtime)* recover from host transaction failures
- *(router)* complete cross-platform navigation
- *(paint)* cover mobile text shadow paths
- *(protocol)* restrict background paint boxes

### Other

- Merge pull request #651 from whiskerrs/codex/review-rollup-runtime-abi
- refresh architecture and Rust API guidance
- Merge remote-tracking branch 'origin/next-architecture' into codex/mobile-element-schema-freeze
- Isolate runtime state and module events per surface
- remove legacy Lynx runtime dependencies
- split large modules by responsibility
- *(abi)* establish mobile source of truth
- *(ui)* align builtin public contracts
- *(css)* require structured style values
- *(list)* recycle compatible presentation slots
- *(list)* index large sources for scrolling
- make component commands one-way
- rebuild driver as mobile FFI adapter
- Merge pull request #514 from whiskerrs/codex/android-aurora-host-fixes
- Merge next-architecture into text direction lowering
- Merge pull request #504 from whiskerrs/codex/text-measure-direction
- Merge pull request #501 from whiskerrs/codex/pointer-input-all-hosts
- Merge pull request #498 from whiskerrs/codex/text-measure-typography
- Merge pull request #497 from whiskerrs/codex/text-basic-conformance
- Merge next-architecture into custom properties
- *(hosts)* define intrinsic background sizing
- *(runtime)* define background geometry lowering
- *(runtime)* cover background resource lifecycle
- *(runtime)* define typed background URL lowering
- *(runtime)* cover resource completion wake semantics
- Constrain application layout with a surface root
- Implement native macOS box and text rendering
- Apply Host viewport metrics before layout ([#420](https://github.com/whiskerrs/whisker/pull/420))
- Complete retained Rust rendering and Host-driven runtime loop ([#418](https://github.com/whiskerrs/whisker/pull/418))

## [0.11.1](https://github.com/whiskerrs/whisker/compare/whisker-v0.11.0...whisker-v0.11.1) - 2026-08-12

### Other

- Give apps the Android back button, and give it back to Android ([#392](https://github.com/whiskerrs/whisker/pull/392))

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-v0.10.12...whisker-v0.11.0) - 2026-08-11

### Other

- Sweep whisker, css, macros, macro-syntax, fmt comments ([#377](https://github.com/whiskerrs/whisker/pull/377))

## [0.9.0](https://github.com/whiskerrs/whisker/compare/whisker-v0.8.2...whisker-v0.9.0) - 2026-07-21

### Added

- *(router)* mirror React Navigation's Stack Navigator keyboard handling ([#316](https://github.com/whiskerrs/whisker/pull/316))

### Fixed

- *(pan-intercept)* encode direction/scope as wire ints, not strings ([#312](https://github.com/whiskerrs/whisker/pull/312))

### Other

- *(control-flow)* rebuild <Show> only when the condition changes ([#317](https://github.com/whiskerrs/whisker/pull/317))

## [0.8.2](https://github.com/whiskerrs/whisker/compare/whisker-v0.8.1...whisker-v0.8.2) - 2026-07-12

### Added

- *(reactive)* add Callback<In, Out> — Copy event-handler-prop wrapper ([#299](https://github.com/whiskerrs/whisker/pull/299))

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-v0.7.0...whisker-v0.8.0) - 2026-07-06

### Added

- *(hot-reload)* [**breaking**] saves only hot-reload — manual Full Reload (r/R), full-remount escalation, props-layout gate ([#287](https://github.com/whiskerrs/whisker/pull/287))
- *(list)* [**breaking**] ItemMeta — identity + per-item metadata unified; list_item removed ([#284](https://github.com/whiskerrs/whisker/pull/284))
- *(list)* exhaustive Lynx <list> binding + on-demand virtualization ([#276](https://github.com/whiskerrs/whisker/pull/276))

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-v0.6.0...whisker-v0.7.0) - 2026-06-26

### Added

- *(whisker-driver)* tokio feature — host a multi-thread runtime so reqwest/spawn_blocking just work ([#262](https://github.com/whiskerrs/whisker/pull/262))
- *(whisker-animation)* continuous signal-based animation engine ([#251](https://github.com/whiskerrs/whisker/pull/251))

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.6.0](https://github.com/whiskerrs/whisker/compare/whisker-v0.5.1...whisker-v0.6.0) - 2026-06-18

### Added

- [**breaking**] signal() returns a single RwSignal instead of a (Read, Write) tuple ([#244](https://github.com/whiskerrs/whisker/pull/244))

## [0.5.0](https://github.com/whiskerrs/whisker/compare/whisker-v0.4.3...whisker-v0.5.0) - 2026-06-17

### Other

- [**breaking**] whisker owns the root page (remove user-facing `page`) ([#238](https://github.com/whiskerrs/whisker/pull/238))

## [0.3.0](https://github.com/whiskerrs/whisker/compare/whisker-v0.2.5...whisker-v0.3.0) - 2026-06-15

### Added

- *(reactive)* make Signal<T> Copy ([#213](https://github.com/whiskerrs/whisker/pull/213))

### Fixed

- *(view)* make renderer dispatch re-entrancy-safe ([#214](https://github.com/whiskerrs/whisker/pull/214))
- *(module)* scaffold builds out of the box + reject reserved Lynx event names ([#211](https://github.com/whiskerrs/whisker/pull/211))

## [0.2.4](https://github.com/whiskerrs/whisker/compare/whisker-v0.2.3...whisker-v0.2.4) - 2026-06-13

### Added

- *(macros)* module-component `style:` accepts `Css` directly (no to_css_string) ([#203](https://github.com/whiskerrs/whisker/pull/203))

## [0.2.1](https://github.com/whiskerrs/whisker/compare/whisker-v0.2.0...whisker-v0.2.1) - 2026-06-11

### Fixed

- router hit-test, render! alias ergonomics, safe-area owner crash ([#195](https://github.com/whiskerrs/whisker/pull/195))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-v0.1.0...whisker-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)

### Other

- green up main — cargo fmt + cargo deny
