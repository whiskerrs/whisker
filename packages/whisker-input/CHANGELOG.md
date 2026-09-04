# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.3](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.13.2...whisker-input-v0.13.3) - 2026-09-04

### Fixed

- *(android)* preserve multiline input mode ([#670](https://github.com/whiskerrs/whisker/pull/670))

## [0.13.1](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.13.0...whisker-input-v0.13.1) - 2026-09-04

### Fixed

- *(android)* preserve text and disabled-motion rendering ([#664](https://github.com/whiskerrs/whisker/pull/664))
- fix ios host builds for clean consumers ([#661](https://github.com/whiskerrs/whisker/pull/661))

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.12.0...whisker-input-v0.13.0) - 2026-09-03

### Added

- *(input)* support web and desktop hosts
- *(macros)* unify composition around public builders
- align module APIs across hosts
- *(host)* package mobile runtimes as platform SDKs
- *(host)* unify retained module runtime across platforms

### Fixed

- *(android)* unblock SDK publication
- *(input)* include content padding in IME anchor
- *(input)* anchor desktop IME to the caret
- *(input)* align desktop IME presentation
- *(input)* keep multiline caret on trailing line
- *(input)* forward desktop printable key text

### Other

- remove legacy Lynx runtime dependencies
- *(css)* require structured style values
- make component commands one-way

## [0.11.1](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.11.0...whisker-input-v0.11.1) - 2026-08-12

### Other

- Repair the release pins the batch E2E caught ([#394](https://github.com/whiskerrs/whisker/pull/394))
- Point apps at Lynx 4.0.1-whisker.2, Android SDK 0.1.19, iOS SwiftPM 0.1.9 ([#393](https://github.com/whiskerrs/whisker/pull/393))

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.10.12...whisker-input-v0.11.0) - 2026-08-11

### Other

- Fix three native-side issues the comment sweep surfaced ([#384](https://github.com/whiskerrs/whisker/pull/384))
- Sweep Kotlin/Swift comments and clear the Rust sweep's follow-ups ([#382](https://github.com/whiskerrs/whisker/pull/382))
- Sweep packages/* comments ([#379](https://github.com/whiskerrs/whisker/pull/379))

## [0.10.8](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.10.7...whisker-input-v0.10.8) - 2026-08-07

### Other

- Reach Lynx's touch handler without its private headers ([#362](https://github.com/whiskerrs/whisker/pull/362))

## [0.10.7](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.10.5...whisker-input-v0.10.7) - 2026-08-07

### Other

- Move the iOS SwiftPM pin to 0.1.6 ([#359](https://github.com/whiskerrs/whisker/pull/359))

## [0.10.3](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.10.2...whisker-input-v0.10.3) - 2026-08-05

### Other

- whisker SDK 0.1.16 / iOS SwiftPM v0.1.5 for Lynx 4.0.1

## [0.10.1](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.10.0...whisker-input-v0.10.1) - 2026-07-29

### Fixed

- *(whisker-input)* report soft-keyboard and IME edits on Android

## [0.10.0](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.9.2...whisker-input-v0.10.0) - 2026-07-28

### Other

- *(release)* SDK pins for AsyncFunction + module-driven iOS floor

## [0.9.0](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.8.2...whisker-input-v0.9.0) - 2026-07-21

### Added

- *(router)* mirror React Navigation's Stack Navigator keyboard handling ([#316](https://github.com/whiskerrs/whisker/pull/316))

### Other

- *(ios)* bump module SwiftPM whisker pins 0.1.2 → 0.1.3

## [0.8.1](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.8.0...whisker-input-v0.8.1) - 2026-07-08

### Added

- *(keyboard)* whisker-keyboard — keyboard avoidance + dismiss-on-navigation ([#293](https://github.com/whiskerrs/whisker/pull/293))

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.7.0...whisker-input-v0.8.0) - 2026-07-06

### Added

- *(list)* exhaustive Lynx <list> binding + on-demand virtualization ([#276](https://github.com/whiskerrs/whisker/pull/276))
- *(whisker-input)* add auto_capitalize / autocorrect / spell_check text-input traits ([#274](https://github.com/whiskerrs/whisker/pull/274))

### Fixed

- *(ios)* bump module Package.swift whisker pins 0.1.1 -> 0.1.2 + lockstep guard ([#290](https://github.com/whiskerrs/whisker/pull/290))

## [0.6.0](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.5.1...whisker-input-v0.6.0) - 2026-06-18

### Added

- [**breaking**] signal() returns a single RwSignal instead of a (Read, Write) tuple ([#244](https://github.com/whiskerrs/whisker/pull/244))

## [0.3.0](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.2.5...whisker-input-v0.3.0) - 2026-06-15

### Added

- *(reactive)* make Signal<T> Copy ([#213](https://github.com/whiskerrs/whisker/pull/213))

### Fixed

- *(android)* dispatch module events synchronously (#3 follow-up) ([#216](https://github.com/whiskerrs/whisker/pull/216))
- *(view)* make renderer dispatch re-entrancy-safe ([#214](https://github.com/whiskerrs/whisker/pull/214))

## [0.2.4](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.2.3...whisker-input-v0.2.4) - 2026-06-13

### Added

- *(macros)* module-component `style:` accepts `Css` directly (no to_css_string) ([#203](https://github.com/whiskerrs/whisker/pull/203))

## [0.2.3](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.2.2...whisker-input-v0.2.3) - 2026-06-13

### Added

- *(whisker-input)* native text-input component ([#200](https://github.com/whiskerrs/whisker/pull/200))
