# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.0](https://github.com/whiskerrs/whisker/compare/whisker-webview-v0.8.2...whisker-webview-v0.9.0) - 2026-07-21

### Added

- *(web-browser)* add whisker-web-browser module for in-app OAuth ([#307](https://github.com/whiskerrs/whisker/pull/307))

### Other

- *(ios)* bump module SwiftPM whisker pins 0.1.2 → 0.1.3

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-webview-v0.7.0...whisker-webview-v0.8.0) - 2026-07-06

### Added

- *(list)* exhaustive Lynx <list> binding + on-demand virtualization ([#276](https://github.com/whiskerrs/whisker/pull/276))

### Fixed

- *(ios)* bump module Package.swift whisker pins 0.1.1 -> 0.1.2 + lockstep guard ([#290](https://github.com/whiskerrs/whisker/pull/290))

## [0.3.0](https://github.com/whiskerrs/whisker/compare/whisker-webview-v0.2.5...whisker-webview-v0.3.0) - 2026-06-15

### Added

- *(reactive)* make Signal<T> Copy ([#213](https://github.com/whiskerrs/whisker/pull/213))

### Fixed

- *(android)* dispatch module events synchronously (#3 follow-up) ([#216](https://github.com/whiskerrs/whisker/pull/216))
- *(view)* make renderer dispatch re-entrancy-safe ([#214](https://github.com/whiskerrs/whisker/pull/214))

## [0.2.4](https://github.com/whiskerrs/whisker/compare/whisker-webview-v0.2.3...whisker-webview-v0.2.4) - 2026-06-13

### Added

- *(macros)* module-component `style:` accepts `Css` directly (no to_css_string) ([#203](https://github.com/whiskerrs/whisker/pull/203))

## [0.2.3](https://github.com/whiskerrs/whisker/compare/whisker-webview-v0.2.2...whisker-webview-v0.2.3) - 2026-06-13

### Added

- *(whisker-webview)* native web view component ([#201](https://github.com/whiskerrs/whisker/pull/201))
