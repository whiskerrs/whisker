# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/whiskerrs/whisker/compare/whisker-macos-v0.12.0...whisker-macos-v0.13.0) - 2026-09-03

### Added

- connect native hosts to hot reload
- *(list)* finalize virtualization grid and scrolling
- align module APIs across hosts
- *(host)* normalize pointer kinds across hosts
- *(interaction)* implement cursor and pointer events
- *(desktop)* manage raster resource lifecycle
- *(host)* unify retained module runtime across platforms
- *(elements)* implement RFC0004 module registry
- *(desktop)* add shared box conformance host

### Fixed

- *(desktop)* connect scrolling and pointer targets

### Other

- *(desktop)* share native application shell
- *(desktop)* move lifecycle into OS shells
- Render rounded borders as a single ring
- Render rounded boxes on macOS
- Implement native macOS box and text rendering
- Apply Host viewport metrics before layout ([#420](https://github.com/whiskerrs/whisker/pull/420))
- Add cross-platform Host bootstraps ([#419](https://github.com/whiskerrs/whisker/pull/419))
