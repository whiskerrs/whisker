# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-v0.7.0...whisker-driver-v0.8.0) - 2026-07-02

### Added

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
