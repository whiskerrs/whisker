# iOS distribution & the remote SwiftPM package

How an iOS app consumes Whisker's native runtime, and the one caveat
monorepo developers need to know.

## How it works

iOS apps resolve Whisker's Swift runtime from a **remote SwiftPM
package**, not from a local `platforms/ios` path. This is what lets an
app created with `whisker new` (outside this repo) build without cloning
the monorepo — the same "prebuilt, fetched by URL" model Android already
uses with its Maven AARs.

The pieces:

| Artifact | Source |
|---|---|
| `WhiskerRuntime` / `WhiskerModule` / `WhiskerCBridge` / `WhiskerModuleCodegenPlugin` | The **`whisker` SwiftPM package** — the [`Package.swift`](../Package.swift) at the repo root, resolved by tagged git URL |
| `Lynx*` / `PrimJS` xcframeworks | `binaryTarget(url:checksum:)` against `whiskerrs/lynx` GitHub Releases |
| `WhiskerDriver.framework` | Built **per-app** during the Xcode build (it wraps the user's `#[whisker::main]` crate) by the "Whisker Generate" Run Script phase |
| Each `whisker-*` module | Its own SwiftPM package; references the `whisker` package by the same URL so the build graph has one `WhiskerRuntime` identity |
| Bridge headers (`whisker.h`, `whisker_bridge.h`, …) | Resolved from the `whisker-driver` / `whisker-driver-sys` crates via `cargo metadata` (registry extraction for external users, `crates/` in-repo) |

The root `Package.swift` **must** live at the repo root — SwiftPM only
resolves a `Package.swift` at the root of a git URL, never a
subdirectory. It re-paths into `platforms/ios/Sources/...` and
`platforms/ios/macros/...` so the Swift sources stay in one place.

### Single source of truth for the version

The remote URL + version are defined once, in Rust:

```rust
// crates/whisker-build/src/ios.rs
pub const WHISKER_IOS_SPM_URL: &str = "https://github.com/whiskerrs/whisker.git";
pub const WHISKER_IOS_SPM_VERSION: &str = "0.1.0";
```

These drive the generated app's `XCRemoteSwiftPackageReference`
(`whisker-cng`) and the generated module aggregator (`whisker-build`).
**Every module's `Package.swift`** hardcodes the same URL + `exact:`
version as a Swift literal — keep all of these, the constants, the root
`Package.swift`, and the published git tag in lockstep.

## ⚠️ Caveat for monorepo developers

Because iOS resolves the runtime **only** from the published git tag (no
local path fallback anymore), **editing `platforms/ios/**` Swift sources
and rebuilding an app will NOT pick up your local changes** — SwiftPM
keeps using whatever is committed at the tag.

To iterate on the Swift runtime locally, use one of:

1. **Local URL redirect (recommended for quick iteration).** Point the
   production HTTPS URL at your working copy via git, build, then unset:

   ```sh
   git config --global \
     url."file://$(pwd)".insteadOf "https://github.com/whiskerrs/whisker.git"
   # commit your platforms/ios changes on a branch + tag it v0.1.0 locally,
   # then `whisker run ios` resolves against the local repo.
   git config --global --unset \
     url."file://$(pwd)".insteadOf
   ```

   > Note: Xcode's SwiftPM honours `insteadOf` for the `swift package`
   > CLI but **not always** for `xcodebuild`. If `xcodebuild` ignores it,
   > temporarily set `WHISKER_IOS_SPM_URL` to a `file://…` path in
   > `crates/whisker-build/src/ios.rs` (and the module manifests) for the
   > duration of your local testing — that is exactly how this feature
   > was verified end-to-end.

2. **Re-tag.** Commit your `platforms/ios` change and move the `v0.1.0`
   tag (`git tag -f v0.1.0 && git push -f origin v0.1.0`), then clear the
   SwiftPM cache so it re-fetches. Heavier; prefer (1) for tight loops.

A dedicated local-override mechanism for monorepo development (a
`.package(path:)` override that SwiftPM prefers over the same-identity
remote) can be added later if this friction becomes painful.

## Bumping the version

When you publish a new `whisker` package version:

1. Bump `WHISKER_IOS_SPM_VERSION` in `crates/whisker-build/src/ios.rs`.
2. Bump the `exact:` version literal in **every** `packages/*/Package.swift`.
3. Commit, then `git tag vX.Y.Z && git push origin vX.Y.Z`.

If the Lynx fork tag moves, also update the `binaryTarget` URLs +
checksums in both the root `Package.swift` and
`platforms/ios/Package.swift` (keep them identical).

## Compatibility matrix

Keep these aligned per release:

| crates.io | iOS SwiftPM tag | Android SDK (Maven) | Gradle plugin | Lynx fork |
|---|---|---|---|---|
| `0.1.0` | `v0.1.0` | `0.1.1` | `0.4.0` | `3.8.0-whisker.6` |
