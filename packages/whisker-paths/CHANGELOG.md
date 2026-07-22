# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- *(whisker-paths)* `set_excluded_from_backup` — exclude a file/dir from iCloud backup (iOS `NSURLIsExcludedFromBackupKey`; no-op on Android). Required for re-downloadable content under `document_dir`.

## [0.9.0](https://github.com/whiskerrs/whisker/releases/tag/whisker-paths-v0.9.0) - 2026-07-21

### Added

- *(whisker-paths)* resolve per-app directories for std::fs

### Added

- *(whisker-paths)* resolve per-app directories (cache / document / support / temp) via a native module, for use with `std::fs`
