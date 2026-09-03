# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.12.0...whisker-svg-v0.13.0) - 2026-09-03

### Added

- *(macros)* unify composition around public builders
- align module APIs across hosts
- *(svg)* support web and desktop hosts
- *(host)* package mobile runtimes as platform SDKs
- *(host)* unify retained module runtime across platforms

### Fixed

- *(android)* unblock SDK publication
- *(modules)* align host registration names
- *(android)* complete Aurora Wallet host integration

### Other

- *(svg)* adopt explicit platform manifest
- refresh architecture and Rust API guidance
- remove legacy Lynx runtime dependencies
- *(css)* require structured style values

### Added

- Add Web SVG-DOM and Desktop raster Host implementations.

## [0.11.1](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.11.0...whisker-svg-v0.11.1) - 2026-08-12

### Other

- Repair the release pins the batch E2E caught ([#394](https://github.com/whiskerrs/whisker/pull/394))
- Point apps at Lynx 4.0.1-whisker.2, Android SDK 0.1.19, iOS SwiftPM 0.1.9 ([#393](https://github.com/whiskerrs/whisker/pull/393))

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.10.12...whisker-svg-v0.11.0) - 2026-08-11

### Other

- Fix three native-side issues the comment sweep surfaced ([#384](https://github.com/whiskerrs/whisker/pull/384))
- Sweep Kotlin/Swift comments and clear the Rust sweep's follow-ups ([#382](https://github.com/whiskerrs/whisker/pull/382))
- Sweep packages/* comments ([#379](https://github.com/whiskerrs/whisker/pull/379))

## [0.10.8](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.10.7...whisker-svg-v0.10.8) - 2026-08-07

### Other

- Reach Lynx's touch handler without its private headers ([#362](https://github.com/whiskerrs/whisker/pull/362))

## [0.10.7](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.10.5...whisker-svg-v0.10.7) - 2026-08-07

### Other

- Move the iOS SwiftPM pin to 0.1.6 ([#359](https://github.com/whiskerrs/whisker/pull/359))

## [0.10.3](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.10.2...whisker-svg-v0.10.3) - 2026-08-05

### Other

- whisker SDK 0.1.16 / iOS SwiftPM v0.1.5 for Lynx 4.0.1

## [0.10.0](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.9.2...whisker-svg-v0.10.0) - 2026-07-28

### Fixed

- *(svg)* repaint on bounds change so hidden-branch Icons aren't blank ([#306](https://github.com/whiskerrs/whisker/pull/306))

### Other

- *(release)* SDK pins for AsyncFunction + module-driven iOS floor

## [0.9.0](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.8.2...whisker-svg-v0.9.0) - 2026-07-21

### Fixed

- *(svg)* parse rgb()/rgba() colors in the native tint parser ([#301](https://github.com/whiskerrs/whisker/pull/301))

### Other

- *(ios)* bump module SwiftPM whisker pins 0.1.2 → 0.1.3

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.7.0...whisker-svg-v0.8.0) - 2026-07-06

### Added

- *(list)* exhaustive Lynx <list> binding + on-demand virtualization ([#276](https://github.com/whiskerrs/whisker/pull/276))

### Fixed

- *(ios)* bump module Package.swift whisker pins 0.1.1 -> 0.1.2 + lockstep guard ([#290](https://github.com/whiskerrs/whisker/pull/290))

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.6.0...whisker-svg-v0.7.0) - 2026-06-26

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.3.0](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.2.5...whisker-svg-v0.3.0) - 2026-06-15

### Added

- *(reactive)* make Signal<T> Copy ([#213](https://github.com/whiskerrs/whisker/pull/213))

## [0.2.4](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.2.3...whisker-svg-v0.2.4) - 2026-06-13

### Added

- *(macros)* module-component `style:` accepts `Css` directly (no to_css_string) ([#203](https://github.com/whiskerrs/whisker/pull/203))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-svg-v0.1.0...whisker-svg-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)
