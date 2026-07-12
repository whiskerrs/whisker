# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.2](https://github.com/whiskerrs/whisker/compare/whisker-fmt-v0.8.1...whisker-fmt-v0.8.2) - 2026-07-12

### Fixed

- *(fmt)* reformat css!/routes! nested inside render! kwargs ([#298](https://github.com/whiskerrs/whisker/pull/298))
- *(fmt)* use `?` instead of match for clippy::question-mark ([#296](https://github.com/whiskerrs/whisker/pull/296))

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-fmt-v0.6.0...whisker-fmt-v0.7.0) - 2026-06-26

### Added

- *(whisker-router)* reactive rendering — Outlet/Stack/Switch, transitions, swipe-back (phase 2) ([#258](https://github.com/whiskerrs/whisker/pull/258))

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.5.1](https://github.com/whiskerrs/whisker/compare/whisker-fmt-v0.5.0...whisker-fmt-v0.5.1) - 2026-06-18

### Added

- *(whisker-fmt)* preserve comments when formatting render!/css! bodies ([#241](https://github.com/whiskerrs/whisker/pull/241))

## [0.4.0](https://github.com/whiskerrs/whisker/compare/whisker-fmt-v0.3.1...whisker-fmt-v0.4.0) - 2026-06-16

### Fixed

- *(whisker-fmt)* resolve edition like cargo fmt + fix --config-path ([#222](https://github.com/whiskerrs/whisker/pull/222))

## [0.3.1](https://github.com/whiskerrs/whisker/compare/whisker-fmt-v0.3.0...whisker-fmt-v0.3.1) - 2026-06-16

### Added

- *(whisker-fmt)* format embedded Rust exprs in macro bodies via rustfmt ([#219](https://github.com/whiskerrs/whisker/pull/219))
- *(cli)* `whisker fmt` — rustfmt drop-in that formats render!/css! macros ([#218](https://github.com/whiskerrs/whisker/pull/218))
