---
name: release-whisker
description: Cut a Whisker release — Rust crates to crates.io, the Android SDK AARs, the Gradle plugin, and the iOS Swift package. Use when publishing a change, deciding which release streams a change needs, or recovering a release that stalled on a crates.io rate limit, a stuck workflow, or a version pin left behind.
---

# Release Whisker

Whisker ships on **four independent streams**. Most changes need one.
Picking the wrong set is the usual way a release "succeeds" while the
change never reaches an app.

## Which streams does this change need?

| Changed | Streams | Why |
|---|---|---|
| Rust only | crates | Apps build the Rust half from crates.io. |
| `crates/whisker-driver-sys/bridge/**` (`.mm` / `.cc`) | crates | The bridge is compiled by `whisker-driver-sys`'s build script, not shipped in the Swift package. |
| `platforms/android/**` (Kotlin) | Android SDK → then crates | The AAR carries it; the CLI's `WHISKER_SDK_VERSION` has to name the new AAR, and that constant ships in a crate. |
| `platforms/ios/**` (Swift) | iOS SwiftPM → then crates | The tag carries it; `WHISKER_IOS_SPM_VERSION` and every module's `Package.swift` have to name the new tag, and those ship in crates. |
| `platforms/android/gradle-plugin/**` | Gradle plugin → then crates | Same shape as the SDK, via `WHISKER_GRADLE_PLUGIN_VERSION`. |

Read the table as: **the platform artifact goes out first, the crate
release that points at it goes out second.** An app can't reach a new
AAR or tag until a CLI carrying the new pin is published.

Before assuming iOS needs a tag, check whether the change lives entirely in
`whisker-driver-sys`'s compiled bridge. Bridge-only changes reach apps through
the crates stream; Swift Host changes require the iOS SwiftPM stream.

## Crates (crates.io)

release-plz drives it. `release_always = false`, so **publishing only
happens on the merge commit of a release PR**.

1. Merge the work.
2. release-plz opens (or refreshes) a `chore: release vX.Y.Z` PR.
   Confirm its changelog mentions your change.
3. Merge that PR. The push to `main` publishes and tags.

Expect **HTTP 429** partway through: crates.io limits how many existing
crates you may update in a burst, and the workspace is large. It is not
a failure of the release — re-run the failed job and it resumes from the
first unpublished crate.

```sh
gh run rerun <run-id> --failed   # wait ~2-3 min between attempts
```

Watch a specific crate rather than the run alone, since the run can be
green while later crates are still going out:

```sh
curl -sS https://crates.io/api/v1/crates/whisker-paths \
  -H 'User-Agent: release-check' | jq -r .crate.max_version
```

After publishing, a fresh app build can still fail to resolve
`whisker-dev-runtime` — the local registry index lags. Warm it:

```sh
cargo new --lib /tmp/warm && cd /tmp/warm
cargo add whisker-dev-runtime@X.Y.Z && cargo fetch
```

## Android SDK (AAR)

Tag-triggered: `sdk-v<version>` → `publish-sdk` → gh-pages Maven.

1. `git tag sdk-vX.Y.Z && git push origin sdk-vX.Y.Z`
2. Wait for `publish-sdk`, then confirm the artifact is actually
   reachable — the workflow can succeed while GitHub Pages has not
   deployed, and the deploy has timed out before:

   ```sh
   curl -sSI https://whiskerrs.github.io/whisker/maven/rs/whisker/\
   whisker-runtime-android/X.Y.Z/whisker-runtime-android-X.Y.Z.aar | head -1
   ```

   On a stuck deploy, re-run `pages build and deployment`.
3. Bump `WHISKER_SDK_VERSION` in `crates/whisker-cli/src/platforms.rs`,
   with a comment saying what forces the move, and release the crates.

## iOS Swift package

**Run the `publish-ios` workflow** — do not tag by hand. SwiftPM
resolves the git tag directly, so the tag *is* the release: a bad tag is
public the moment it exists, and moving it reaches consumers as a stale
checkout because SwiftPM caches by revision.

1. In the same PR as the Swift change, bump `WHISKER_IOS_SPM_VERSION`
   (`crates/whisker-build/src/ios.rs`) **and every**
   `packages/*/Package.swift` `exact:` pin. They resolve by path
   alongside the app, so one left behind makes the graph unresolvable
   for every consumer. A unit test in `whisker-build` fails on a
   mismatch.
2. Merge, then run `publish-ios` with the version (no leading `v`). It
   rejects an existing tag, checks the pins name that version, builds
   `WhiskerRuntime`, and only then tags. `dry_run` verifies without
   tagging.
3. Release the crates so apps pick up the new pin.

Nothing in the Rust jobs compiles Swift — `cross build` builds the Rust
half for an iOS target and stops. `ci` runs `swift build
(WhiskerRuntime)` for exactly this reason. Locally:

```sh
cd platforms/ios
xcodebuild -scheme WhiskerRuntime -destination 'generic/platform=iOS Simulator' build
```

The compiler is the reliable compatibility check for Swift Host APIs; source
inspection alone does not validate target availability or Objective-C import
behavior.

## Gradle plugin

`gradle-plugin-v<version>` → `publish-gradle-plugin` → gh-pages Maven,
then bump `WHISKER_GRADLE_PLUGIN_VERSION` and release the crates. Same
shape as the SDK.

## When a release stalls

**A queued run that never starts** (GitHub incident, or two pushes
racing). `release-plz` uses a concurrency group with
`cancel-in-progress: false`, so a stranded run blocks every later one,
and both `gh run cancel` and `gh run rerun` can refuse it
("cannot cancel a workflow re-run that has not yet queued"). Pushing a
new commit to `main` admits a newer run into the group and releases the
old one.

That new commit does **not** publish anything — `release_always = false`
means only a release-PR merge commit does. Expect release-plz to open a
fresh release PR at the next version instead; merge that.

**A version that never reached crates.io** stays a gap in the sequence.
That is fine — versions are cheap. Do not try to reuse the number.

## Verify against an app

A release is not done until an app resolves it. From a Whisker app:

```sh
cargo install whisker-cli --version X.Y.Z --locked
# lift the app's dependency versions, then
whisker run android   # or ios
```

Confirm the generated tree points at the new platform artifacts:
`gen/android/app/build.gradle.kts` names the AAR version, and
`gen/ios/whisker_modules/Package.swift` names the SwiftPM version. `gen/`
is regenerated from the CLI; delete it if it looks stale.
