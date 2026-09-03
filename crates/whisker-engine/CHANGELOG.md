# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-engine-v0.12.0...whisker-engine-v0.13.0) - 2026-09-03

### Added

- negotiate host rendering capabilities
- align module APIs across hosts
- *(style)* complete typed custom properties
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
- *(background)* lower typed gradients to frames
- *(background)* preserve per-layer URL geometry
- *(style)* lower supported background geometry
- *(protocol)* define complete CSS paint semantics
- *(host)* unify retained module runtime across platforms
- add retained scene engine

### Fixed

- *(engine)* emit monotonic snapshot allocations
- *(engine)* lower zero-length motion paths
- *(style)* align calc length semantics
- *(layout)* bound measurement cache lifetime
- *(paint)* canonicalize radial gradients in Rust
- *(runtime)* recover from host transaction failures
- *(ci)* restore module parity quality gates
- *(style)* inherit direction into text shaping
- keep stable CI warning-free

### Other

- Merge pull request #651 from whiskerrs/codex/review-rollup-runtime-abi
- *(engine)* cover invalid motion path geometry
- cover ready measurement cache consumers
- cover radial gradient canonicalization
- cover typed paint lowering
- remove legacy Lynx runtime dependencies
- split large modules by responsibility
- *(renderer)* cover accessibility contract
- *(ui)* align builtin public contracts
- Merge next-architecture into text direction lowering
- *(engine)* cover pointer mutation regions
- *(pointer)* cover every lowering path
- *(engine)* cover decoration style lowering
- *(transform)* cover malformed arc commands
- *(transform)* cover invalid rotated arc geometry
- *(engine)* cover background layer retention
- *(runtime)* cover background resource lifecycle
- *(runtime)* define typed background URL lowering
- *(engine)* cover renderer capabilities
- *(engine)* cover element mutation wrappers
- Constrain application layout with a surface root
- Implement native macOS box and text rendering
- Complete retained Rust rendering and Host-driven runtime loop ([#418](https://github.com/whiskerrs/whisker/pull/418))
