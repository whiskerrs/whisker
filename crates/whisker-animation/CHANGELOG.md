# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-animation-v0.12.0...whisker-animation-v0.13.0) - 2026-09-03

### Added

- *(host)* unify retained module runtime across platforms

### Other

- refresh architecture and Rust API guidance
- Isolate runtime state and module events per surface
- remove legacy Lynx runtime dependencies

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-animation-v0.10.12...whisker-animation-v0.11.0) - 2026-08-11

### Other

- Sweep driver, driver-sys, cng, animation comments ([#378](https://github.com/whiskerrs/whisker/pull/378))

## [0.10.0](https://github.com/whiskerrs/whisker/compare/whisker-animation-v0.9.2...whisker-animation-v0.10.0) - 2026-07-28

### Fixed

- *(animation)* a disposed controller must not crash the animation step

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-animation-v0.6.0...whisker-animation-v0.7.0) - 2026-06-26

### Added

- *(whisker-animation)* spring initial/hand-off velocity, overshoot clamping, cancel-aware on_finish ([#255](https://github.com/whiskerrs/whisker/pull/255))
- *(whisker-animation)* physics-based spring timing ([#254](https://github.com/whiskerrs/whisker/pull/254))
- *(whisker-animation)* continuous signal-based animation engine ([#251](https://github.com/whiskerrs/whisker/pull/251))

### Fixed

- *(whisker-animation)* anchor a run's start time on its first frame ([#253](https://github.com/whiskerrs/whisker/pull/253))
