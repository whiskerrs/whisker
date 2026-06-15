# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.2.5...whisker-cli-v0.3.0) - 2026-06-15

### Fixed

- *(module)* scaffold builds out of the box + reject reserved Lynx event names ([#211](https://github.com/whiskerrs/whisker/pull/211))

### Other

- *(lynx)* bump Lynx fork pin to v3.8.0-whisker.7 ([#215](https://github.com/whiskerrs/whisker/pull/215))

## [0.2.1](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.2.0...whisker-cli-v0.2.1) - 2026-06-11

### Fixed

- router hit-test, render! alias ergonomics, safe-area owner crash ([#195](https://github.com/whiskerrs/whisker/pull/195))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.1.0...whisker-cli-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)

### Fixed

- generated starter compiles; drop dangling Suspense doc-link

### Other

- *(cli)* fold whisker-build binary into `whisker`; make whisker-build lib-only
