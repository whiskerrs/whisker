# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.3](https://github.com/whiskerrs/whisker/compare/whisker-layout-v0.13.2...whisker-layout-v0.13.3) - 2026-09-04

### Other

- *(list)* optimize retained virtual layout ([#671](https://github.com/whiskerrs/whisker/pull/671))

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-layout-v0.12.0...whisker-layout-v0.13.0) - 2026-09-03

### Added

- *(layout)* complete Taffy intrinsic core semantics
- *(layout)* support block floats and clearance
- *(css)* connect typed Grid styles to Taffy
- *(layout)* lower computed grid styles to Taffy

### Fixed

- *(layout)* scope order to flex and grid containers
- *(layout)* bound measurement cache lifetime
- *(android)* complete Aurora Wallet host integration

### Other

- remove legacy Lynx runtime dependencies
- *(motion)* restore renderer coverage
- *(layout)* define Taffy intrinsic overflow behavior
- *(layout)* cover float lowering branches
- *(layout)* define float and clear behavior
- *(layout)* cover grid lowering branches
- *(layout)* define grid track and placement behavior
- Constrain application layout with a surface root
- Implement native macOS box and text rendering
- Complete retained Rust rendering and Host-driven runtime loop ([#418](https://github.com/whiskerrs/whisker/pull/418))
- Add retained Rust layout engine
