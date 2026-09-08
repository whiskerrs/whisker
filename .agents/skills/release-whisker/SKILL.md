---
name: release-whisker
description: Prepare or publish a Whisker release across Rust crates, Android SDK, Gradle plugin, and SwiftPM; recover a partial release or diagnose a missing SDK pin.
---

# Release Whisker

Use `.github/workflows/release.yml`. Release logic lives in
`cargo xtask release`; no release-plz installation is needed.

## Prepare one release

Run the `release` workflow on **main** with these inputs:

| Input | Meaning |
| --- | --- |
| `release_id` | Required identifier independent of versions, such as `20260908.1`; increment the suffix for another release that day |
| `version` | New core version; leave blank for packages-only releases |
| `package_versions` | Optional JSON overrides such as `{"whisker-router":"minor","whisker-input":"0.15.0"}`; values accept `patch`, `minor`, `major`, or a stable version |
| `sdk_version`, `gradle_version`, `ios_version` | New native versions when those sources changed; blanks reuse current pins |
| `subsecond_version` | Explicit version for the independently versioned fork |

Core means publishable crates in `crates/` and `platforms/`, except
`whisker-subsecond`. Core releases update the group together and pin
core-to-core dependencies to the exact version. Each `packages/<directory>`
is a separate version group, including nested web/desktop crates. Override
keys use directory names. Every crate within a group must share its version.

Preparation compares packaged files and resolved manifest metadata with the
last completed release. Changed package groups receive a patch bump by
default. An override also selects an unchanged group. If a dependency's new
version falls outside an unselected group's requirement, that group is
selected too. Core or fork changes require their explicit version inputs.
Unchanged package groups keep their versions and dependency requirements;
selected packages receive compatible minimum versions of their local
dependencies. Review patch proposals for API compatibility, especially new
APIs or breaking changes that require a core release or a larger version bump.

Package versions and internal dependency requirements are detached from
workspace inheritance during preparation. Native pin updates participate in
change detection: updating CLI/CNG pins requires a core release, and updating
module SwiftPM manifests selects the affected packages. Excluded files and
root Cargo.lock-only changes do not automatically select crates; explicitly
request a version when a lockfile change needs a new CLI binary.

Preparation rejects omitted native streams with changes since their pinned
tag, incomplete previous releases, and manual crate version changes outside
release preparation. It records selected versions, reasons, content hashes,
and Cargo.lock's hash in `.github/release.json`, with notes in
`releases/<release_id>.md`. One `codex/release-<release_id>` PR includes all
selected Rust and native changes, then CI is explicitly dispatched on it.
The explicit dispatch is necessary because bot pushes do not start ordinary
push/PR workflows reliably.

Review the generated release notes, wait for CI, and merge the PR. Existing
main protection still requires an approving review. The pipeline does not
bypass that protection or push release commits directly to main.

## Preview selection locally

Fetch release tags, then run against the committed HEAD:

```sh
git fetch origin --tags
RELEASE_ID=20260908.1 RELEASE_VERSION=0.13.9 cargo xtask release preview
```

Omit `RELEASE_VERSION` for packages-only changes; `PACKAGE_VERSIONS` accepts
the same JSON as Actions. Native inputs use `SDK_VERSION`, `GRADLE_VERSION`,
and `IOS_VERSION`. Preview prepares a temporary worktree and prints the plan;
it does not modify the checkout, create a PR, or publish. Uncommitted edits
are not included. Actions preparation additionally verifies remote release
and native tags before creating the PR.

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
4. Verify every selected crate version in the registry, then create **one**
   GitHub Release, `Whisker <release_id>`, at `whisker-release-<release_id>`.

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
Changes to packaged Rust content or Cargo.lock after preparation are rejected
before publishing. Re-prepare the release if main acquired those changes.
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
Legacy plans without a selective inventory retain their original publication
set and `whisker-v<version>` tag, so their original failed jobs can still resume.

## Verify delivery to an app

A pipeline success must be followed by a consumer build when validating a
specific application fix. Install the released CLI and update the app's
crate versions, then run `whisker run android` or `whisker run ios`.

Confirm generated Android Gradle files reference the new SDK/plugin, and
the generated iOS SwiftPM aggregator references the new Swift tag. If a
freshly published crate does not resolve, refresh the consumer registry
index and retry. `gen/` is generated from the CLI, so changing only a local
Rust dependency does not upgrade an old CLI's SDK pins.
