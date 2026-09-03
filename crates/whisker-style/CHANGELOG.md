# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-style-v0.12.0...whisker-style-v0.13.0) - 2026-09-03

### Added

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
- *(transform)* lower Lynx perspective on current nodes
- *(transform)* project flat-plane 3d transforms
- *(transform)* lower typed 2d transforms after layout
- *(paint)* render backdrop blur across hosts
- *(background)* implement border-area clipping
- *(background)* lower typed gradients to frames
- *(background)* preserve per-layer URL geometry
- *(paint)* resolve logical borders in Rust
- *(layout)* complete Taffy intrinsic core semantics
- *(layout)* support block floats and clearance
- *(css)* connect typed Grid styles to Taffy
- *(layout)* lower computed grid styles to Taffy
- *(runtime)* lower intrinsic background size modes
- *(style)* lower supported background geometry
- *(protocol)* define complete CSS paint semantics
- add computed layout styles
- resolve inherited text styles
- add semantic style declaration values

### Fixed

- *(style)* follow CSS non-ASCII identifier syntax
- *(style)* reject unicode whitespace in custom properties
- *(style)* require length-only border widths
- *(style)* align calc length semantics
- *(style)* sample jump-start boundaries correctly
- *(style)* validate composite colors consistently
- *(style)* align inheritance metadata and RFC
- *(style)* collapse non-painting border widths
- *(style)* resolve logical margin and padding
- *(style)* invalidate failed variable declarations
- *(style)* align pointer inheritance metadata
- *(paint)* cover mobile text shadow paths

### Other

- *(style)* cover typed layout resolution branches
- cover typed paint lowering
- remove legacy Lynx runtime dependencies
- split large modules by responsibility
- *(motion)* restore renderer coverage
- Merge next-architecture into nested custom properties
- Merge next-architecture into typed custom properties
- Merge next-architecture into text direction lowering
- Merge next-architecture into custom properties
- *(style)* cover custom property resolution paths
- *(pointer)* cover every lowering path
- *(style)* cover invalid decoration color
- *(paint)* define logical border resolution
- *(layout)* define Taffy intrinsic overflow behavior
- *(layout)* cover float lowering branches
- *(style)* cover grid resolution branches
- *(style)* cover background geometry resolution
- *(style)* cover background geometry branches
- *(runtime)* define typed background URL lowering
- *(style)* cover elliptical radius resolution
- Complete retained Rust rendering and Host-driven runtime loop ([#418](https://github.com/whiskerrs/whisker/pull/418))
- separate typed style semantics
