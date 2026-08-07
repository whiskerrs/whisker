# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
