# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
