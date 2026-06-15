# Lynx integration

How Whisker depends on its Lynx fork ([`whiskerrs/lynx`][fork]), and
what must move in lockstep when bumping the pinned Lynx version.
Audience: contributors bumping Lynx or debugging the C++ bridge build.

> A note for anyone who remembers the old setup: Whisker used to fetch
> Lynx as tarballs into `~/.cache/whisker/lynx/` via
> `whisker-build::lynx::ensure_lynx_*`. That code path is **gone** —
> Lynx binaries now come from SwiftPM `binaryTarget(url:checksum:)`
> (iOS) and Maven AARs (Android), resolved by the platform toolchains
> directly.

[fork]: https://github.com/whiskerrs/lynx

## How the dependency is wired

The fork publishes pre-built Lynx artifacts per `v<ver>` tag (currently
`v3.8.0-whisker.7`). Whisker consumes them per platform:

### iOS — SwiftPM binary targets

Both `Package.swift` files declare four `binaryTarget`s pointing at the
fork's GitHub Release xcframework zips:

- `Lynx`, `LynxBase`, `LynxServiceAPI`, `PrimJS`, each as
  `binaryTarget(url: ".../releases/download/v<ver>/<Name>-<ver>.xcframework.zip",
  checksum: "<sha>")`.

SPM resolves and unpacks these during xcodebuild's package-resolution
step (before any Build Phase runs) and caches them under the per-Xcode
SourcePackages dir, shared across every `WhiskerRuntime` consumer. No
local `target/lynx-*` pre-population is required for the binaries.

`WhiskerDriver` is deliberately **not** a binary target — it wraps the
user's `#[whisker::main]` crate and so must be compiled per-app. It's
produced as `WhiskerDriver.framework` by an Xcode Run Script Build Phase
(`whisker build-ios`) that whisker-cng injects into the generated
pbxproj. The Swift sources `@_exported import WhiskerCBridge` (a
header-only `systemLibrary` mirroring the C ABI); the undefined refs
resolve against that framework at the host app's link step.

The checksums come from the `swiftpm-manifest-<ver>.txt` published
alongside each release.

### Android — Maven AARs

`platforms/android/whisker-runtime/build.gradle.kts` has two dependency
modes toggled by the `whiskerSdkRelease` Gradle property:

- **Release / Maven-driven** (`-PwhiskerSdkRelease=true
  -PlynxFork=v<ver>`): pulls the fork's AARs by Maven coordinate from
  `whiskerrs.github.io/lynx/maven` —
  `rs.whisker:lynx-android`, `:lynx-base-android`, `:lynx-trace-android`,
  `:lynx-service-api-android`, all at `$lynxFork`. The consuming app's
  `settings.gradle.kts` must list that repo in
  `dependencyResolutionManagement`.
- **Local CLI / dev** (default): flatDir refs (`:LynxAndroid@aar` etc.)
  against `target/lynx-android/` — harmless when empty.

PrimJS comes from Maven Central in both modes:
`org.lynxsdk.lynx:primjs:3.7.0`.

## Version pinning points — move these in lockstep

When bumping the Lynx fork tag, **every** one of these must change
together (there is no cross-check that enforces it):

1. **Root `Package.swift`** — the four `binaryTarget` `url:` `<ver>`
   segments *and* their `checksum:` values.
2. **`platforms/ios/Package.swift`** — same four `url:` + `checksum:`
   (the monorepo-local mirror; both packages must agree).
3. **`platforms/android/whisker-runtime/build.gradle.kts`** — the
   `lynxFork` default (`getOrElse("v<ver>")`), and the CI publish
   workflow's `-PlynxFork=v<ver>`.
4. **PrimJS** — the Central coordinate
   (`org.lynxsdk.lynx:primjs:<ver>`). PrimJS versions independently of
   the Lynx fork tag; bump only when the fork's PrimJS moves.

The iOS checksums are SwiftPM-format (the value SPM prints on a checksum
mismatch); take them from the release's `swiftpm-manifest-<ver>.txt`.

> Stale-cache gotcha: after bumping the tag, `rm -rf target/lynx-ios`
> if a fresh crate fails with undefined `_lynx_*` symbols — an old
> simulator slice can linger and lack the new bridge symbols.

## The Whisker ↔ Lynx C++ bridge

The bridge lives at `crates/whisker-driver-sys/bridge/` and is compiled
**per-app** by `crates/whisker-driver-sys/build.rs` into a static
archive (`whisker_bridge_static`) that gets linked into the user crate's
final dylib. Single source of truth for both platforms.

Key property: **Lynx symbols are resolved at runtime via `dlopen` /
`dlsym`, not linked at build time.** The bridge calls Lynx through a
function-pointer table that `whisker_bridge_lynx_loader.cc` populates at
engine-attach time. Consequently:

- The bridge `.o` files carry **zero `lynx_*` undefined references**, so
  the build needs **no Lynx headers and no Lynx link line** (`-llynx` /
  `-framework Lynx*` are gone). A cold `cargo build --target
  aarch64-{linux-android,apple-ios}` succeeds against a fresh checkout
  with no prior tooling. The bridge sees only the vendored
  `lynx_capi.h` (pointer typedefs) and, on iOS, `lynx_objc_stubs.h`
  (minimal `@interface` decls); actual classes resolve at runtime via
  `objc_getClass`.

What `build.rs` still does:

- Compile the bridge sources per target OS (`whisker_bridge_common.cc` +
  platform glue `whisker_bridge_{android.cc,ios.mm}` + the loader; a
  host stub on other targets).
- Emit `cargo:rustc-link-lib=static:+whole-archive=whisker_bridge_static`
  so the `whisker_bridge_*` entry points (called from Swift / by the
  JNI runtime, not from Rust) survive the parent dylib's dead-strip.
- Declare the system frameworks/libs the bridge actually uses —
  Android: `log`, `c++_shared`, `c` (libdl is part of bionic's libc);
  iOS: `Foundation`, `UIKit`, `CoreFoundation`, `QuartzCore`, `c++`,
  `objc`.

On Android the bridge forces inline LSE atomics (`-march=armv8.1-a`,
`-mno-outline-atomics`) so it never reaches for compiler-rt's
outline-atomics dispatcher. Android support is arm64-v8a only.

Forcing the bridge entry points into the iOS dylib's `.dynsym` is **not**
done here — `cargo:rustc-link-arg` from an rlib's build.rs doesn't reach
the parent dylib's link. `whisker-build/src/ios.rs` appends the
`-Wl,-exported_symbol` flags directly to the `cargo rustc` invocation
that produces the user-crate dylib.

## Producing the artifacts (outside this repo)

The release process lives on `whiskerrs/lynx`, not here. Each `v<ver>`
tag's CI builds and publishes, as GitHub Release assets:

- the four iOS xcframework zips (`Lynx`, `LynxBase`, `LynxServiceAPI`,
  `PrimJS`),
- the `swiftpm-manifest-<ver>.txt` listing each zip's SwiftPM checksum,
- the Android AARs, published to the fork's gh-pages Maven repo
  (`whiskerrs.github.io/lynx/maven`) under the `rs.whisker:lynx-*`
  coordinates.

Bumping Whisker to a new Lynx then means: cut the fork release, copy the
checksums from the manifest into both `Package.swift` files, and update
the Android `lynxFork` pin (see the lockstep list above). The
xcframework/AAR build mechanics on the fork are out of scope for this
repo.
