# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-router-v0.7.0...whisker-router-v0.8.0) - 2026-07-02

### Fixed

- *(whisker-router)* [**breaking**] reset/replace/swipe-back fixes + global reset + leaf URL resolution ([#272](https://github.com/whiskerrs/whisker/pull/272))

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-router-v0.6.0...whisker-router-v0.7.0) - 2026-06-26

### Added

- *(whisker-router)* Router accepts routes: RouteSet directly ([#261](https://github.com/whiskerrs/whisker/pull/261))
- *(whisker-router)* reactive rendering — Outlet/Stack/Switch, transitions, swipe-back (phase 2) ([#258](https://github.com/whiskerrs/whisker/pull/258))
- *(whisker-router)* RouteTree + RouteState core (new router, phase 1) ([#257](https://github.com/whiskerrs/whisker/pull/257))

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.4.1](https://github.com/whiskerrs/whisker/compare/whisker-router-v0.4.0...whisker-router-v0.4.1) - 2026-06-16

### Fixed

- *(whisker-router)* re-establish back-stack position invariant on every nav ([#230](https://github.com/whiskerrs/whisker/pull/230))

## [0.2.2](https://github.com/whiskerrs/whisker/compare/whisker-router-v0.2.1...whisker-router-v0.2.2) - 2026-06-11

### Fixed

- *(router)* own RouteStack signals in a detached root (#6 follow-up) ([#198](https://github.com/whiskerrs/whisker/pull/198))

## [0.2.1](https://github.com/whiskerrs/whisker/compare/whisker-router-v0.2.0...whisker-router-v0.2.1) - 2026-06-11

### Fixed

- router hit-test, render! alias ergonomics, safe-area owner crash ([#195](https://github.com/whiskerrs/whisker/pull/195))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-router-v0.1.0...whisker-router-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)
