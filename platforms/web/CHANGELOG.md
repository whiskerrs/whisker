# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-web-v0.12.0...whisker-web-v0.13.0) - 2026-09-03

### Added

- *(macros)* unify composition around public builders
- *(web)* replace Trunk with Whisker dev server
- negotiate host rendering capabilities
- *(list)* finalize virtualization grid and scrolling
- *(list)* add Rust-owned virtualized list foundation
- align module APIs across hosts
- *(svg)* support web and desktop hosts
- *(host)* preserve text direction through measurement
- *(host)* normalize pointer kinds across hosts
- *(host)* normalize pointer input across hosts
- *(host)* implement crisp edge image sampling
- *(host)* align text measurement typography
- *(host)* preserve basic text styling across hosts
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
- *(paint)* render backdrop blur across hosts
- *(clip)* render path commands across hosts
- *(clip)* render circle and ellipse paths across hosts
- *(clip)* render rounded inset paths across hosts
- *(shadow)* render hard outer box shadows
- *(background)* implement border-area clipping
- *(web)* project intrinsic background sizes
- *(hosts)* wire typed resource runtime channel
- *(web)* manage raster resource lifecycle
- *(web)* paint background image resources
- *(web)* stack multiple background layers
- *(web)* support content-box backgrounds
- *(web)* support rounded background repeat
- *(web)* support spaced background repeat
- *(web)* project per-axis background repeat
- *(host)* resolve background origin and clip boxes
- *(host)* position explicit background images
- *(host)* size non-repeating background images
- *(host)* render resolved conic gradients
- *(host)* render explicit radial gradients
- *(host)* render resolved linear gradients
- *(host)* clip rounded descendant paint
- *(host)* conform compositing state
- *(host)* apply resolved transforms
- *(host)* clip overflow per axis
- *(host)* preserve elliptical border radii
- *(host)* render relief borders
- *(host)* render double borders
- *(host)* render dashed and dotted borders
- *(host)* package mobile runtimes as platform SDKs
- *(protocol)* define complete CSS paint semantics
- *(host)* unify retained module runtime across platforms
- *(elements)* implement RFC0004 module registry

### Fixed

- reconcile shared Host contracts after audit
- *(host)* honor min-content text measurement
- isolate host element failures
- *(input)* make Rust authoritative for hit testing
- *(runtime)* recover from host transaction failures
- *(ci)* restore host conformance fixture identifiers
- *(web)* keep virtual rows ahead of scrolling
- *(web)* preserve scroll containers and desktop spacing
- *(desktop)* composite opacity over element groups

### Other

- *(web)* remove superseded failure helper
- Merge branch 'codex/review-rollup-runtime-abi' into codex/review-rollup-web
- Revert "refactor: keep runtime rollup platform-neutral"
- keep runtime rollup platform-neutral
- *(protocol)* close host operation reachability gaps
- *(module)* unify host installation kernels
- Isolate runtime state and module events per surface
- split large modules by responsibility
- *(ui)* align builtin public contracts
- *(host)* cover rounded border styles
- *(host)* cover proportional radius normalization
- *(host)* expand z-index stacking conformance
- *(host)* cover rounded asymmetric border edges
- *(host)* cover visibility descendant overrides
- *(host)* cover element group opacity
- *(host)* cover text measurement flow
- *(host)* cover advanced text measurement
- *(clip)* define path fill-rule clipping
- *(clip)* define circle and ellipse clipping
- *(clip)* define rounded inset descendant clipping
- *(shadow)* define multiple shadow paint order
- *(shadow)* define inset spread and blur
- *(shadow)* define inset padding-edge geometry
- *(shadow)* define Gaussian blur falloff
- *(shadow)* define positive spread geometry
- *(shadow)* define positive offset geometry
- *(background)* define border-area clipping
- *(web)* model round auto aspect coupling
- *(hosts)* define intrinsic background sizing
- *(web)* model background repeat space gaps
- *(hosts)* add background geometry symmetry fixture
- *(host)* cover solid borders on every edge
- *(host)* add cross-platform conformance harness
- Implement native macOS box and text rendering
- Apply Host viewport metrics before layout ([#420](https://github.com/whiskerrs/whisker/pull/420))
- Add cross-platform Host bootstraps ([#419](https://github.com/whiskerrs/whisker/pull/419))
