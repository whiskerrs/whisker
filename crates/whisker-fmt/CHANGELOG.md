# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/whiskerrs/whisker/compare/whisker-fmt-v0.3.1...whisker-fmt-v0.4.0) - 2026-06-16

### Fixed

- *(whisker-fmt)* resolve edition like cargo fmt + fix --config-path ([#222](https://github.com/whiskerrs/whisker/pull/222))

## [0.3.1](https://github.com/whiskerrs/whisker/compare/whisker-fmt-v0.3.0...whisker-fmt-v0.3.1) - 2026-06-16

### Added

- *(whisker-fmt)* format embedded Rust exprs in macro bodies via rustfmt ([#219](https://github.com/whiskerrs/whisker/pull/219))
- *(cli)* `whisker fmt` — rustfmt drop-in that formats render!/css! macros ([#218](https://github.com/whiskerrs/whisker/pull/218))
