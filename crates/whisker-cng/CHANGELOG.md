# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.5](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.2.4...whisker-cng-v0.2.5) - 2026-06-13

### Other

- update Cargo.lock dependencies

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.1.0...whisker-cng-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)

### Other

- *(cli)* fold whisker-build binary into `whisker`; make whisker-build lib-only
- delete root Package.swift (unused remote-SPM entry point) ([#192](https://github.com/whiskerrs/whisker/pull/192))
