# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.11.1](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.11.0...whisker-cli-v0.11.1) - 2026-08-12

### Other

- Point apps at Lynx 4.0.1-whisker.2, Android SDK 0.1.19, iOS SwiftPM 0.1.9 ([#393](https://github.com/whiskerrs/whisker/pull/393))

## [0.11.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.10.12...whisker-cli-v0.11.0) - 2026-08-11

### Other

- Sweep Kotlin/Swift comments and clear the Rust sweep's follow-ups ([#382](https://github.com/whiskerrs/whisker/pull/382))
- Sweep dev-server, cli, build, plugin, config, credentials comments ([#380](https://github.com/whiskerrs/whisker/pull/380))

## [0.10.7](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.10.5...whisker-cli-v0.10.7) - 2026-08-07

### Other

- release v0.10.6 ([#358](https://github.com/whiskerrs/whisker/pull/358))

## [0.10.6](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.10.4...whisker-cli-v0.10.6) - 2026-08-06

### Other

- release v0.10.5 ([#356](https://github.com/whiskerrs/whisker/pull/356))
- Point the CLI at SDK 0.1.17 ([#357](https://github.com/whiskerrs/whisker/pull/357))

## [0.10.5](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.10.4...whisker-cli-v0.10.5) - 2026-08-06

### Other

- Point the CLI at SDK 0.1.17 ([#357](https://github.com/whiskerrs/whisker/pull/357))

## [0.10.3](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.10.2...whisker-cli-v0.10.3) - 2026-08-05

### Other

- whisker SDK 0.1.16 / iOS SwiftPM v0.1.5 for Lynx 4.0.1

## [0.10.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.9.2...whisker-cli-v0.10.0) - 2026-07-28

### Fixed

- *(cli)* build discovered CNG plugins one cargo invocation per plugin
- *(cli)* propagate the app's [patch.crates-io] into the config-probe

### Other

- *(release)* SDK pins for AsyncFunction + module-driven iOS floor

## [0.9.1](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.9.0...whisker-cli-v0.9.1) - 2026-07-22

### Other

- update Cargo.lock dependencies

## [0.9.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.8.2...whisker-cli-v0.9.0) - 2026-07-21

### Added

- *(renderer)* require Lynx insert_before; pin v3.8.0-whisker.13 (Phase C)

### Fixed

- *(android)* actually apply tapSlop, bypassing the page-config path ([#315](https://github.com/whiskerrs/whisker/pull/315))
- *(android)* convert touch coordinates to dip in event reporter ([#311](https://github.com/whiskerrs/whisker/pull/311))
- *(runtime-android)* populate touch coordinates in event reporter ([#310](https://github.com/whiskerrs/whisker/pull/310))
- *(runtime-android)* align Lynx tap-cancel slop with scroll threshold ([#309](https://github.com/whiskerrs/whisker/pull/309))

### Other

- *(android)* read live tapSlop after ACTION_UP, not a fixed delay ([#314](https://github.com/whiskerrs/whisker/pull/314))
- *(android)* log live tapSlop value on WhiskerView init ([#313](https://github.com/whiskerrs/whisker/pull/313))

## [0.8.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.7.0...whisker-cli-v0.8.0) - 2026-07-06

### Added

- *(build)* whisker build appbundle/apk/ipa + age-encrypted credential store ([#288](https://github.com/whiskerrs/whisker/pull/288))
- *(hot-reload)* [**breaking**] saves only hot-reload — manual Full Reload (r/R), full-remount escalation, props-layout gate ([#287](https://github.com/whiskerrs/whisker/pull/287))
- *(list)* [**breaking**] ItemMeta — identity + per-item metadata unified; list_item removed ([#284](https://github.com/whiskerrs/whisker/pull/284))
- *(list)* minimal-diff data-source updates — scroll position holds across appends ([#281](https://github.com/whiskerrs/whisker/pull/281))
- *(list)* core-originated <list> events (scroll / scrolltolower / snap / layoutcomplete) now reach whisker ([#279](https://github.com/whiskerrs/whisker/pull/279))
- *(whisker-cli)* warn when the running CLI is older than crates/whisker-cng ([#260](https://github.com/whiskerrs/whisker/pull/260)) ([#269](https://github.com/whiskerrs/whisker/pull/269))
- *(whisker-run)* surface build staleness — compile relinked/up-to-date + gen reused/regenerated ([#260](https://github.com/whiskerrs/whisker/pull/260)) ([#268](https://github.com/whiskerrs/whisker/pull/268))

### Other

- *(android)* SDK v0.1.2 — roll Lynx fork .7 → .8 (capi ABI v2) ([#277](https://github.com/whiskerrs/whisker/pull/277))
- *(cli)* pin gradle-plugin 0.4.1 in generated projects (closes #159) ([#271](https://github.com/whiskerrs/whisker/pull/271))

## [0.7.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.6.0...whisker-cli-v0.7.0) - 2026-06-26

### Other

- migrate to Rust 2024 edition ([#248](https://github.com/whiskerrs/whisker/pull/248))

## [0.6.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.5.1...whisker-cli-v0.6.0) - 2026-06-18

### Added

- [**breaking**] signal() returns a single RwSignal instead of a (Read, Write) tuple ([#244](https://github.com/whiskerrs/whisker/pull/244))

## [0.5.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.4.3...whisker-cli-v0.5.0) - 2026-06-17

### Other

- [**breaking**] whisker owns the root page (remove user-facing `page`) ([#238](https://github.com/whiskerrs/whisker/pull/238))

## [0.4.3](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.4.2...whisker-cli-v0.4.3) - 2026-06-17

### Other

- *(cli)* scaffold a thin #[whisker::main] + Root component, styled with css! ([#234](https://github.com/whiskerrs/whisker/pull/234))

## [0.4.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.3.1...whisker-cli-v0.4.0) - 2026-06-16

### Added

- *(whisker-asset)* build plugin bundles declared assets (Phase 2) ([#225](https://github.com/whiskerrs/whisker/pull/225))

### Fixed

- *(whisker-fmt)* resolve edition like cargo fmt + fix --config-path ([#222](https://github.com/whiskerrs/whisker/pull/222))

## [0.3.1](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.3.0...whisker-cli-v0.3.1) - 2026-06-16

### Added

- *(cli)* scaffold a rust-analyzer.toml routing format-on-save to whisker fmt ([#220](https://github.com/whiskerrs/whisker/pull/220))
- *(cli)* `whisker fmt` — rustfmt drop-in that formats render!/css! macros ([#218](https://github.com/whiskerrs/whisker/pull/218))

## [0.3.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.2.5...whisker-cli-v0.3.0) - 2026-06-15

### Fixed

- *(module)* scaffold builds out of the box + reject reserved Lynx event names ([#211](https://github.com/whiskerrs/whisker/pull/211))

### Other

- *(lynx)* bump Lynx fork pin to v3.8.0-whisker.7 ([#215](https://github.com/whiskerrs/whisker/pull/215))

## [0.2.1](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.2.0...whisker-cli-v0.2.1) - 2026-06-11

### Fixed

- router hit-test, render! alias ergonomics, safe-area owner crash ([#195](https://github.com/whiskerrs/whisker/pull/195))

## [0.2.0](https://github.com/whiskerrs/whisker/compare/whisker-cli-v0.1.0...whisker-cli-v0.2.0) - 2026-06-10

### Added

- *(ios)* standalone builds via remote SwiftPM (no platforms/ios local path)

### Fixed

- generated starter compiles; drop dangling Suspense doc-link

### Other

- *(cli)* fold whisker-build binary into `whisker`; make whisker-build lib-only
