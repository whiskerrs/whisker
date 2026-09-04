# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.1](https://github.com/whiskerrs/whisker/compare/whisker-css-v0.13.0...whisker-css-v0.13.1) - 2026-09-04

### Fixed

- *(android)* preserve text and disabled-motion rendering ([#664](https://github.com/whiskerrs/whisker/pull/664))

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-css-v0.12.0...whisker-css-v0.13.0) - 2026-09-03

### Added

- *(macros)* unify composition around public builders
- *(style)* complete typed custom properties
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
- *(transform)* lower typed 2d transforms after layout
- *(paint)* render backdrop blur across hosts
- *(background)* implement border-area clipping
- *(background)* lower typed gradients to frames
- *(background)* preserve per-layer URL geometry
- *(paint)* resolve logical borders in Rust
- *(layout)* support block floats and clearance
- *(css)* connect typed Grid styles to Taffy
- *(runtime)* lower intrinsic background size modes
- *(style)* lower supported background geometry
- *(protocol)* define complete CSS paint semantics
- add computed layout styles
- resolve inherited text styles
- add semantic style declaration values
- add stable style property registry

### Fixed

- *(css)* diagnose invalid background positions
- *(style)* resolve logical margin and padding

### Other

- refresh architecture and Rust API guidance
- remove legacy Lynx runtime dependencies
- *(css)* require structured style values
- Merge next-architecture into typed custom properties
- Merge next-architecture into custom properties
- *(background)* define typed gradient authoring
- *(background)* define typed multi-layer shorthand
- *(runtime)* define typed background URL lowering
- Complete retained Rust rendering and Host-driven runtime loop ([#418](https://github.com/whiskerrs/whisker/pull/418))
- separate typed style semantics
- close whisker-css line coverage gaps

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-css-v0.10.12...whisker-css-v0.11.0) - 2026-08-11

### Other

- Sweep whisker, css, macros, macro-syntax, fmt comments ([#377](https://github.com/whiskerrs/whisker/pull/377))

## [0.10.3](https://github.com/whiskerrs/whisker/compare/whisker-css-v0.10.2...whisker-css-v0.10.3) - 2026-08-05

### Other

- *(lynx)* bump the Lynx fork to v4.0.1-whisker.1

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-css-v0.6.0...whisker-css-v0.7.0) - 2026-06-26

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-css-v0.1.0...whisker-css-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)
