# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.11.1](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.11.0...whisker-image-v0.11.1) - 2026-08-12

### Other

- Repair the release pins the batch E2E caught ([#394](https://github.com/whiskerrs/whisker/pull/394))
- Point apps at Lynx 4.0.1-whisker.2, Android SDK 0.1.19, iOS SwiftPM 0.1.9 ([#393](https://github.com/whiskerrs/whisker/pull/393))

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.10.12...whisker-image-v0.11.0) - 2026-08-11

### Other

- Sweep Kotlin/Swift comments and clear the Rust sweep's follow-ups ([#382](https://github.com/whiskerrs/whisker/pull/382))
- Sweep packages/* comments ([#379](https://github.com/whiskerrs/whisker/pull/379))

## [0.10.8](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.10.7...whisker-image-v0.10.8) - 2026-08-07

### Other

- Reach Lynx's touch handler without its private headers ([#362](https://github.com/whiskerrs/whisker/pull/362))

## [0.10.7](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.10.5...whisker-image-v0.10.7) - 2026-08-07

### Other

- Move the iOS SwiftPM pin to 0.1.6 ([#359](https://github.com/whiskerrs/whisker/pull/359))

## [0.10.4](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.10.3...whisker-image-v0.10.4) - 2026-08-06

### Other

- Make whisker-image's load events optional

## [0.10.3](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.10.2...whisker-image-v0.10.3) - 2026-08-05

### Other

- whisker SDK 0.1.16 / iOS SwiftPM v0.1.5 for Lynx 4.0.1

## [0.10.2](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.10.1...whisker-image-v0.10.2) - 2026-08-04

### Added

- *(whisker-image)* send request headers, report outcomes, prefetch

## [0.10.0](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.9.2...whisker-image-v0.10.0) - 2026-07-28

### Other

- *(release)* SDK pins for AsyncFunction + module-driven iOS floor

## [0.9.0](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.8.2...whisker-image-v0.9.0) - 2026-07-21

### Other

- *(ios)* bump module SwiftPM whisker pins 0.1.2 → 0.1.3

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.7.0...whisker-image-v0.8.0) - 2026-07-06

### Added

- *(list)* exhaustive Lynx <list> binding + on-demand virtualization ([#276](https://github.com/whiskerrs/whisker/pull/276))

### Fixed

- *(ios)* bump module Package.swift whisker pins 0.1.1 -> 0.1.2 + lockstep guard ([#290](https://github.com/whiskerrs/whisker/pull/290))

## [0.4.0](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.3.1...whisker-image-v0.4.0) - 2026-06-16

### Added

- *(whisker-asset)* native base registration + image integration (Phase 3) ([#226](https://github.com/whiskerrs/whisker/pull/226))

## [0.2.4](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.2.3...whisker-image-v0.2.4) - 2026-06-13

### Added

- *(macros)* module-component `style:` accepts `Css` directly (no to_css_string) ([#203](https://github.com/whiskerrs/whisker/pull/203))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-image-v0.1.0...whisker-image-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)
