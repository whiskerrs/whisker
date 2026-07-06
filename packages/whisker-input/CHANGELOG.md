# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.7.0...whisker-input-v0.8.0) - 2026-07-06

### Added

- *(list)* exhaustive Lynx <list> binding + on-demand virtualization ([#276](https://github.com/whiskerrs/whisker/pull/276))
- *(whisker-input)* add auto_capitalize / autocorrect / spell_check text-input traits ([#274](https://github.com/whiskerrs/whisker/pull/274))

### Fixed

- *(ios)* bump module Package.swift whisker pins 0.1.1 -> 0.1.2 + lockstep guard ([#290](https://github.com/whiskerrs/whisker/pull/290))

## [0.6.0](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.5.1...whisker-input-v0.6.0) - 2026-06-18

### Added

- [**breaking**] signal() returns a single RwSignal instead of a (Read, Write) tuple ([#244](https://github.com/whiskerrs/whisker/pull/244))

## [0.3.0](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.2.5...whisker-input-v0.3.0) - 2026-06-15

### Added

- *(reactive)* make Signal<T> Copy ([#213](https://github.com/whiskerrs/whisker/pull/213))

### Fixed

- *(android)* dispatch module events synchronously (#3 follow-up) ([#216](https://github.com/whiskerrs/whisker/pull/216))
- *(view)* make renderer dispatch re-entrancy-safe ([#214](https://github.com/whiskerrs/whisker/pull/214))

## [0.2.4](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.2.3...whisker-input-v0.2.4) - 2026-06-13

### Added

- *(macros)* module-component `style:` accepts `Css` directly (no to_css_string) ([#203](https://github.com/whiskerrs/whisker/pull/203))

## [0.2.3](https://github.com/whiskerrs/whisker/compare/whisker-input-v0.2.2...whisker-input-v0.2.3) - 2026-06-13

### Added

- *(whisker-input)* native text-input component ([#200](https://github.com/whiskerrs/whisker/pull/200))
