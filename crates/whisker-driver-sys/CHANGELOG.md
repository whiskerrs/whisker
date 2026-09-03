# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.12.0...whisker-driver-sys-v0.13.0) - 2026-09-03

### Added

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
- *(android)* connect typed resource channel
- *(runtime)* add typed resource channel ABI
- *(android)* paint raster background resources
- *(mobile)* transfer background resource ids
- *(mobile)* transfer background layer arrays
- *(android)* stack multiple background layers
- *(protocol)* negotiate rounded background geometry
- *(protocol)* negotiate spaced background geometry
- *(host)* size non-repeating background images
- *(host)* render resolved conic gradients
- *(host)* render explicit radial gradients
- *(host)* render resolved linear gradients
- *(host)* preserve elliptical border radii
- *(host)* package mobile runtimes as platform SDKs
- *(host)* unify retained module runtime across platforms

### Fixed

- *(mobile)* retain runtimes across temporary detaches
- *(driver)* reject partial Android bootstrap
- *(input)* make Rust authoritative for hit testing
- *(abi)* regenerate mobile headers

### Other

- *(protocol)* close host operation reachability gaps
- *(abi)* establish mobile source of truth
- *(ui)* align builtin public contracts
- *(android)* batch intrinsic measurement JNI calls
- *(android)* batch frame presentation over JNI
- rebuild driver as mobile FFI adapter
- *(host)* cover advanced text measurement
- Add cross-platform Host bootstraps ([#419](https://github.com/whiskerrs/whisker/pull/419))

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.10.12...whisker-driver-sys-v0.11.0) - 2026-08-11

### Other

- Sweep driver, driver-sys, cng, animation comments ([#378](https://github.com/whiskerrs/whisker/pull/378))

## [0.10.9](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.10.8...whisker-driver-sys-v0.10.9) - 2026-08-07

### Other

- Enable Lynx multi-touch from the bridge, not from Swift ([#364](https://github.com/whiskerrs/whisker/pull/364))

## [0.10.4](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.10.3...whisker-driver-sys-v0.10.4) - 2026-08-06

### Other

- Deliver multi-touch to Rust ([#355](https://github.com/whiskerrs/whisker/pull/355))

## [0.10.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.9.2...whisker-driver-sys-v0.10.0) - 2026-07-28

### Added

- *(modules)* real async module functions — AsyncFunction + Promise

### Fixed

- *(android)* keep arm64 codegen on the Armv8.0 baseline

## [0.9.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.8.2...whisker-driver-sys-v0.9.0) - 2026-07-21

### Added

- *(renderer)* require Lynx insert_before; pin v3.8.0-whisker.13 (Phase C)
- *(renderer)* positioned insert via Lynx insert_before (Phase A)

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.7.0...whisker-driver-sys-v0.8.0) - 2026-07-06

### Added

- *(list)* [**breaking**] ItemMeta — identity + per-item metadata unified; list_item removed ([#284](https://github.com/whiskerrs/whisker/pull/284))
- *(list)* minimal-diff data-source updates — scroll position holds across appends ([#281](https://github.com/whiskerrs/whisker/pull/281))
- *(list)* core-originated <list> events (scroll / scrolltolower / snap / layoutcomplete) now reach whisker ([#279](https://github.com/whiskerrs/whisker/pull/279))
- *(list)* exhaustive Lynx <list> binding + on-demand virtualization ([#276](https://github.com/whiskerrs/whisker/pull/276))

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.6.0...whisker-driver-sys-v0.7.0) - 2026-06-26

### Added

- *(whisker-router)* reactive rendering — Outlet/Stack/Switch, transitions, swipe-back (phase 2) ([#258](https://github.com/whiskerrs/whisker/pull/258))

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.2.3](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.2.2...whisker-driver-sys-v0.2.3) - 2026-06-13

### Added

- *(whisker-input)* native text-input component ([#200](https://github.com/whiskerrs/whisker/pull/200))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.1.0...whisker-driver-sys-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)
