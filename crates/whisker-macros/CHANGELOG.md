# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/whiskerrs/whisker/compare/whisker-macros-v0.2.5...whisker-macros-v0.3.0) - 2026-06-15

### Added

- *(reactive)* make Signal<T> Copy ([#213](https://github.com/whiskerrs/whisker/pull/213))

### Fixed

- *(module)* scaffold builds out of the box + reject reserved Lynx event names ([#211](https://github.com/whiskerrs/whisker/pull/211))

## [0.2.4](https://github.com/whiskerrs/whisker/compare/whisker-macros-v0.2.3...whisker-macros-v0.2.4) - 2026-06-13

### Added

- *(macros)* module-component `style:` accepts `Css` directly (no to_css_string) ([#203](https://github.com/whiskerrs/whisker/pull/203))

## [0.2.1](https://github.com/whiskerrs/whisker/compare/whisker-macros-v0.2.0...whisker-macros-v0.2.1) - 2026-06-11

### Fixed

- router hit-test, render! alias ergonomics, safe-area owner crash ([#195](https://github.com/whiskerrs/whisker/pull/195))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-macros-v0.1.0...whisker-macros-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)
