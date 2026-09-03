# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-protocol-v0.12.0...whisker-protocol-v0.13.0) - 2026-09-03

### Added

- negotiate host rendering capabilities
- align module APIs across hosts
- *(host)* render resolved linear gradients
- *(protocol)* define complete CSS paint semantics
- *(host)* unify retained module runtime across platforms
- *(elements)* implement RFC0004 module registry
- add semantic frame protocol model

### Fixed

- *(engine)* emit monotonic snapshot allocations
- *(protocol)* honor complete background capability
- *(protocol)* reject error values in frame data
- *(paint)* canonicalize radial gradients in Rust
- *(ci)* restore module parity quality gates

### Other

- Merge pull request #651 from whiskerrs/codex/review-rollup-runtime-abi
- *(protocol)* bound retired node tracking
- *(protocol)* cover bounded node allocation tracking
- *(protocol)* preserve full projection coverage
- *(protocol)* keep projection coverage stable
- *(protocol)* avoid full scene clones for deltas
- *(protocol)* close host operation reachability gaps
- cover capability negotiation diagnostics
- *(renderer)* cover accessibility contract
- *(ui)* align builtin public contracts
- Merge pull request #488 from whiskerrs/codex/paint-text-font-features
- Merge pull request #487 from whiskerrs/codex/paint-text-wrapping
- Merge pull request #486 from whiskerrs/codex/paint-text-indent
- Merge pull request #485 from whiskerrs/codex/paint-text-alignment
- Merge pull request #472 from whiskerrs/codex/visual-effects-filter
- Merge pull request #462 from whiskerrs/codex/background-clip-border-area
- Merge pull request #453 from whiskerrs/codex/host-background-intrinsic-sizing
- Merge pull request #448 from whiskerrs/codex/host-paint-background-resource-image
- Merge pull request #447 from whiskerrs/codex/host-paint-background-layer-stacking
- Merge pull request #446 from whiskerrs/codex/host-paint-background-content-box
- Merge pull request #445 from whiskerrs/codex/host-paint-background-repeat-round
- Merge pull request #444 from whiskerrs/codex/host-paint-background-repeat-space
- Merge pull request #443 from whiskerrs/codex/host-paint-background-repeat
- Merge pull request #442 from whiskerrs/codex/host-paint-background-boxes
- Merge pull request #441 from whiskerrs/codex/host-paint-background-position
- Merge pull request #440 from whiskerrs/codex/host-paint-background-geometry
- Merge remote-tracking branch 'origin/next-architecture' into codex/host-paint-conic-gradient
- Merge pull request #438 from whiskerrs/codex/host-paint-radial-gradient
- *(protocol)* cover duplicate gradient capability
- *(protocol)* restore complete coverage
- *(protocol)* cover element contract invariants
- Implement native macOS box and text rendering
- Complete retained Rust rendering and Host-driven runtime loop ([#418](https://github.com/whiskerrs/whisker/pull/418))
- enforce complete protocol coverage
