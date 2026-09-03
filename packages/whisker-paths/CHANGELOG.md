# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-paths-v0.12.0...whisker-paths-v0.13.0) - 2026-09-03

### Added

- *(macros)* unify composition around public builders
- *(host)* package mobile runtimes as platform SDKs
- *(host)* unify retained module runtime across platforms

### Fixed

- *(android)* unblock SDK publication

### Other

- *(paths)* declare native platform support
- remove legacy Lynx runtime dependencies
- *(css)* require structured style values

## [0.11.1](https://github.com/whiskerrs/whisker/compare/whisker-paths-v0.11.0...whisker-paths-v0.11.1) - 2026-08-12

### Other

- Repair the release pins the batch E2E caught ([#394](https://github.com/whiskerrs/whisker/pull/394))
- Point apps at Lynx 4.0.1-whisker.2, Android SDK 0.1.19, iOS SwiftPM 0.1.9 ([#393](https://github.com/whiskerrs/whisker/pull/393))

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-paths-v0.10.12...whisker-paths-v0.11.0) - 2026-08-11

### Other

- Point apps at Android SDK 0.1.18 and iOS SwiftPM 0.1.8 ([#386](https://github.com/whiskerrs/whisker/pull/386))

## [0.10.8](https://github.com/whiskerrs/whisker/compare/whisker-paths-v0.10.7...whisker-paths-v0.10.8) - 2026-08-07

### Other

- Reach Lynx's touch handler without its private headers ([#362](https://github.com/whiskerrs/whisker/pull/362))

## [0.10.7](https://github.com/whiskerrs/whisker/compare/whisker-paths-v0.10.5...whisker-paths-v0.10.7) - 2026-08-07

### Other

- Move the iOS SwiftPM pin to 0.1.6 ([#359](https://github.com/whiskerrs/whisker/pull/359))

## [0.10.3](https://github.com/whiskerrs/whisker/compare/whisker-paths-v0.10.2...whisker-paths-v0.10.3) - 2026-08-05

### Other

- whisker SDK 0.1.16 / iOS SwiftPM v0.1.5 for Lynx 4.0.1

## [0.10.0](https://github.com/whiskerrs/whisker/compare/whisker-paths-v0.9.2...whisker-paths-v0.10.0) - 2026-07-28

### Other

- *(release)* SDK pins for AsyncFunction + module-driven iOS floor

## [0.9.2](https://github.com/whiskerrs/whisker/compare/whisker-paths-v0.9.1...whisker-paths-v0.9.2) - 2026-07-22

### Added

- *(whisker-paths)* set_excluded_from_backup for iCloud backup exclusion

### Added

- *(whisker-paths)* `set_excluded_from_backup` — exclude a file/dir from iCloud backup (iOS `NSURLIsExcludedFromBackupKey`; no-op on Android). Required for re-downloadable content under `document_dir`.

## [0.9.0](https://github.com/whiskerrs/whisker/releases/tag/whisker-paths-v0.9.0) - 2026-07-21

### Added

- *(whisker-paths)* resolve per-app directories for std::fs

### Added

- *(whisker-paths)* resolve per-app directories (cache / document / support / temp) via a native module, for use with `std::fs`
