---
name: release-whisker
description: Prepare or publish a Whisker release across Rust crates, Android SDK, Gradle plugin, and SwiftPM; recover a partial release or diagnose a missing SDK pin.
---

# Release Whisker

Use `.github/workflows/release.yml`. Release logic lives in
`cargo xtask release`; no release-plz installation is needed.

## Prepare one release

Run the `release` workflow on **main** with the Rust workspace `version`.
Supply `sdk_version`, `gradle_version`, or `ios_version` when those
sources changed. Blank native inputs reuse the current pins; preparation
rejects unselected streams with changes since their pinned tag.
`subsecond_version` is for the independently versioned fork crate.

The workflow updates workspace versions, local path dependency requirements,
Cargo.lock, CLI SDK pins, and all module SwiftPM pins together. It records
`.github/release.json` and `releases/<version>.md` in a single
`codex/release-v<version>` PR, then explicitly dispatches CI on that branch.
The explicit dispatch is necessary because bot pushes do not start ordinary
push/PR workflows reliably.

Review the generated release notes, wait for CI, and merge the PR. Existing
main protection still requires an approving review. The pipeline does not
bypass that protection or push release commits directly to main.

## Publication order

The release-plan merge triggers the same workflow's publishing jobs:

1. Build and publish selected Android SDK / Gradle plugin artifacts to Maven.
   Both workflows serialize writes to gh-pages. They explicitly request a
   Pages build, then verify public AAR, JAR, POM, and plugin marker URLs.
2. Build the public Swift package before creating its selected `v<version>`
   tag. SwiftPM consumes the tag directly; never move a published tag.
   SwiftPM can build in parallel with the Maven jobs.
3. After all selected native jobs succeed, verify reused SDKs too, then use
   Cargo workspace publishing for the unpublished Rust versions. Publishing
   uses Cargo 1.98.1; the framework's consumer MSRV is unchanged.
4. Verify every planned crate version in the registry, then create **one**
   GitHub Release, `Whisker <version>`, at `whisker-v<version>`.

`sdk-v*`, `gradle-plugin-v*`, and SwiftPM's `v*` tags remain as artifact
identifiers. New per-crate GitHub Releases are not created. Historical
per-crate CHANGELOGs and Releases are retained; new release notes are
consolidated under `releases/`.

The standalone `publish-sdk`, `publish-gradle-plugin`, and `publish-ios`
dispatches are verification/smoke builds. Real publishing is requested by
the unified workflow through `workflow_call`.

## Choose the streams

| Changed | Required stream before Rust |
| --- | --- |
| Android runtime/module/KSP sources or their shared build configuration | Android SDK |
| `platforms/android/gradle-plugin/**` | Gradle plugin |
| Root `Package.swift` or `platforms/ios/**` | SwiftPM |
| Only `crates/whisker-driver-sys/bridge/**` C/C++ sources | Rust (the bridge ships in the crate) |
| Rust framework sources | Rust |
| `crates/whisker-subsecond/**` | Explicit fork version, included in the same release |

The source pins are `WHISKER_SDK_VERSION` and
`WHISKER_GRADLE_PLUGIN_VERSION` in `crates/whisker-cli/src/platforms.rs`, and
`WHISKER_IOS_SPM_VERSION` in `crates/whisker-cng/src/ios_modules.rs` plus
`packages/*/Package.swift`. The preparation command keeps these consistent.

## Resume a partial release

Re-run the failed jobs of the **original release-plan merge run**:

```sh
gh run rerun <run-id> --failed
```

The checkout must remain the same commit. Native tags reject another commit.
Maven publication receipts prevent overwriting a successful upload when
Pages propagation is delayed. SwiftPM skips an already verified tag at the
same commit. Rust checks exact registry versions and publishes only those
still missing, using Cargo to order dependencies.

crates.io HTTP 429 waits until its advertised reset time plus a margin.
Missing/stale reset times use a fallback; attempts and duration are bounded.
Other publishing errors fail immediately. A registry read failure is an
error, never evidence that a crate is unpublished.

Re-running preparation with identical inputs on the original source commit
reuses its branch/PR. A closed or merged PR, a conflicting plan, or another
open unified release PR requires inspection rather than overwriting history.

## Verify delivery to an app

A pipeline success must be followed by a consumer build when validating a
specific application fix. Install the released CLI and update the app's
crate versions, then run `whisker run android` or `whisker run ios`.

Confirm generated Android Gradle files reference the new SDK/plugin, and
the generated iOS SwiftPM aggregator references the new Swift tag. If a
freshly published crate does not resolve, refresh the consumer registry
index and retry. `gen/` is generated from the CLI, so changing only a local
Rust dependency does not upgrade an old CLI's SDK pins.
