# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.7.0...whisker-driver-sys-v0.8.0) - 2026-07-06

### Added

- *(list)* [**breaking**] ItemMeta — identity + per-item metadata unified; list_item removed ([#284](https://github.com/whiskerrs/whisker/pull/284))
- *(list)* minimal-diff data-source updates — scroll position holds across appends ([#281](https://github.com/whiskerrs/whisker/pull/281))
- *(list)* core-originated <list> events (scroll / scrolltolower / snap / layoutcomplete) now reach whisker ([#279](https://github.com/whiskerrs/whisker/pull/279))
- *(list)* exhaustive Lynx <list> binding + on-demand virtualization ([#276](https://github.com/whiskerrs/whisker/pull/276))

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.6.0...whisker-driver-sys-v0.7.0) - 2026-06-26

### Added

- *(whisker-router)* reactive rendering — Outlet/Stack/Switch, transitions, swipe-back (phase 2) ([#258](https://github.com/whiskerrs/whisker/pull/258))

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.2.3](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.2.2...whisker-driver-sys-v0.2.3) - 2026-06-13

### Added

- *(whisker-input)* native text-input component ([#200](https://github.com/whiskerrs/whisker/pull/200))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-driver-sys-v0.1.0...whisker-driver-sys-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)
