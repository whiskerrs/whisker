# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.10.12...whisker-cng-v0.11.0) - 2026-08-11

### Other

- Sweep Kotlin/Swift comments and clear the Rust sweep's follow-ups ([#382](https://github.com/whiskerrs/whisker/pull/382))
- Sweep driver, driver-sys, cng, animation comments ([#378](https://github.com/whiskerrs/whisker/pull/378))

## [0.10.7](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.10.5...whisker-cng-v0.10.7) - 2026-08-07

### Other

- release v0.10.6 ([#358](https://github.com/whiskerrs/whisker/pull/358))

## [0.10.6](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.10.5...whisker-cng-v0.10.6) - 2026-08-06

### Other

- update Cargo.lock dependencies

## [0.10.1](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.10.0...whisker-cng-v0.10.1) - 2026-07-29

### Fixed

- *(cng)* stop the system panning the window for the keyboard

## [0.10.0](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.9.2...whisker-cng-v0.10.0) - 2026-07-28

### Added

- *(config)* allow more than one custom URL scheme
- *(cng,plugin)* plugin-settable splash-screen enablers

### Fixed

- *(ios)* declare supported orientations in the generated Info.plist

### Other

- *(cng)* move seed_orientation_plist above the test module
- Merge pull request #329 from whiskerrs/feat/whisker-splash-screen

## [0.9.1](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.9.0...whisker-cng-v0.9.1) - 2026-07-22

### Other

- update Cargo.lock dependencies

## [0.9.0](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.8.2...whisker-cng-v0.9.0) - 2026-07-21

### Added

- *(web-browser)* add whisker-web-browser module for in-app OAuth ([#307](https://github.com/whiskerrs/whisker/pull/307))

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.7.0...whisker-cng-v0.8.0) - 2026-07-06

### Added

- *(cng)* whisker-app-icon builtin — source PNG + iOS .icon + Android adaptive ([#289](https://github.com/whiskerrs/whisker/pull/289))
- *(build)* whisker build appbundle/apk/ipa + age-encrypted credential store ([#288](https://github.com/whiskerrs/whisker/pull/288))

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.6.0...whisker-cng-v0.7.0) - 2026-06-26

### Added

- *(whisker-router)* reactive rendering — Outlet/Stack/Switch, transitions, swipe-back (phase 2) ([#258](https://github.com/whiskerrs/whisker/pull/258))

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.4.0](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.3.1...whisker-cng-v0.4.0) - 2026-06-16

### Added

- *(whisker-asset)* build plugin bundles declared assets (Phase 2) ([#225](https://github.com/whiskerrs/whisker/pull/225))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-cng-v0.1.0...whisker-cng-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)

### Other

- *(cli)* fold whisker-build binary into `whisker`; make whisker-build lib-only
- delete root Package.swift (unused remote-SPM entry point) ([#192](https://github.com/whiskerrs/whisker/pull/192))
