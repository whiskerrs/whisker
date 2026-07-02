# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-build-v0.7.0...whisker-build-v0.8.0) - 2026-07-02

### Added

- *(list)* exhaustive Lynx <list> binding + on-demand virtualization ([#276](https://github.com/whiskerrs/whisker/pull/276))
- *(whisker-run)* surface build staleness — compile relinked/up-to-date + gen reused/regenerated ([#260](https://github.com/whiskerrs/whisker/pull/260)) ([#268](https://github.com/whiskerrs/whisker/pull/268))

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-build-v0.6.0...whisker-build-v0.7.0) - 2026-06-26

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.4.2](https://github.com/whiskerrs/whisker/compare/whisker-build-v0.4.1...whisker-build-v0.4.2) - 2026-06-17

### Fixed

- *(hot-reload)* dispatch pointer-sized component closures via call_it ([#232](https://github.com/whiskerrs/whisker/pull/232))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-build-v0.1.0...whisker-build-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)

### Other

- green up main — cargo fmt + cargo deny
- *(cli)* fold whisker-build binary into `whisker`; make whisker-build lib-only
