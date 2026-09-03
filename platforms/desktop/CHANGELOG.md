# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-desktop-v0.12.0...whisker-desktop-v0.13.0) - 2026-09-03

### Added

- *(input)* support web and desktop hosts
- *(macros)* unify composition around public builders
- *(desktop)* add shared accessibility adapter
- connect native hosts to hot reload
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
- *(shadow)* render inset spread and blur
- *(shadow)* render hard inset shadows
- *(shadow)* render Gaussian outer blur
- *(shadow)* render hard outer box shadows
- *(background)* implement border-area clipping
- *(desktop)* preserve round background aspect ratio
- *(desktop)* paint intrinsic background sizes
- *(hosts)* wire typed resource runtime channel
- *(desktop)* manage raster resource lifecycle
- *(desktop)* paint raster background resources
- *(desktop)* stack multiple background layers
- *(desktop)* paint content-box backgrounds
- *(desktop)* round repeated background tiles
- *(desktop)* space repeated background tiles
- *(desktop)* repeat background tiles in shader
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
- *(host)* render dashed and dotted borders
- *(host)* package mobile runtimes as platform SDKs
- *(protocol)* define complete CSS paint semantics
- *(host)* unify retained module runtime across platforms
- *(elements)* implement RFC0004 module registry
- *(desktop)* add shared box conformance host

### Fixed

- *(desktop)* reuse Rust-authoritative input targets
- *(desktop)* normalize vertex depth for 2d rendering
- *(desktop)* bound remote resource requests
- *(desktop)* resolve assets from executable bundle
- *(desktop)* pool only reusable presentation content
- *(desktop)* reset scroll animations on snapshot
- *(desktop)* restore blocking event-loop wait
- *(desktop)* reclaim stale prepared text buffers
- *(runtime)* recover from host transaction failures
- *(input)* include content padding in IME anchor
- *(input)* anchor desktop IME to the caret
- *(input)* align desktop IME presentation
- *(input)* forward desktop printable key text
- *(router)* complete cross-platform navigation
- *(ci)* restore host conformance fixture identifiers
- *(desktop)* connect scrolling and pointer targets
- *(host)* preserve visible descendants of hidden nodes
- *(desktop)* composite opacity over element groups
- *(desktop)* gate conformance primitive alias
- *(desktop)* keep transform helpers lint-clean

### Other

- *(desktop)* keep target routing internal
- *(desktop)* absorb shared Host contract changes
- *(desktop)* keep frame validation side-effect free
- *(desktop)* borrow element bindings on lookup
- *(desktop)* remove duplicate pointer capture state
- *(desktop)* skip redraw for unhandled pointer motion
- *(protocol)* close host operation reachability gaps
- *(module)* unify host installation kernels
- Isolate runtime state and module events per surface
- remove legacy Lynx runtime dependencies
- split large modules by responsibility
- *(desktop)* share native application shell
- *(ui)* align builtin public contracts
- *(desktop)* keep virtual scrolling interactive
- run all desktop host conformance tests
- *(host)* cover text measurement flow
- *(host)* cover advanced text measurement
- *(clip)* define path fill-rule clipping
- *(clip)* define circle and ellipse clipping
- *(clip)* define rounded inset descendant clipping
- *(shadow)* define positive offset geometry
- *(background)* define border-area clipping
- *(hosts)* define intrinsic background sizing
- *(host)* isolate desktop conformance check
- *(host)* add cross-platform conformance harness
- *(desktop)* move lifecycle into OS shells
- *(desktop)* compose Taffy with box host
