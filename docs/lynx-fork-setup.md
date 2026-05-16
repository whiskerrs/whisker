# whiskerrs/lynx fork setup

`whisker-build::lynx` downloads pre-built Lynx artifacts from a
GitHub release on the [`whiskerrs/lynx`][fork] fork. This document
covers the one-time setup of that fork's repo + CI workflow + first
release.

Outside-this-repo work — execute on `whiskerrs/lynx`, not on
`whiskerrs/whisker`.

[fork]: https://github.com/whiskerrs/lynx

## Goal

Each Whisker-pinned Lynx version produces two tarballs that
`whisker-build::lynx::ensure_lynx_{android,ios}` consumes:

```
whisker-lynx-android-<version>.tar.gz
├── LynxAndroid.aar
├── LynxBase.aar
├── LynxTrace.aar
├── ServiceAPI.aar
├── unpacked/jni/<abi>/*.so       (extracted from the AARs at CI time)
└── headers/{Lynx,LynxBase,LynxServiceAPI,PrimJS/src}/

whisker-lynx-ios-<version>.tar.gz
├── Lynx.xcframework/
├── LynxBase.xcframework/
├── LynxServiceAPI.xcframework/
├── PrimJS.xcframework/
└── headers/{Lynx,LynxBase,LynxServiceAPI,PrimJS/src}/
```

The Whisker repo pins one Lynx fork version + SHA-256 per tarball in
`crates/whisker-build/src/lynx.rs`. The CI workflow on the fork
publishes these tarballs as release assets when a tag is pushed.

## 1. Apply Whisker patches as commits

The patches in this repo at `patches/lynx-android/*.patch` need to
become commits on a branch of the fork. The branch name should
encode the upstream Lynx version + Whisker patch iteration, e.g.
`whisker/3.7.0`.

From a clone of the fork:

```bash
# Bootstrap: clone upstream Lynx into the fork
git remote add upstream git@github.com:lynx-family/lynx.git
git fetch upstream
git checkout -b whisker/3.7.0 v3.7.0   # upstream tag

# Apply Whisker patches as real commits (not git-apply chain)
git apply --check /path/to/whisker/patches/lynx-android/buildroot.patch
git apply         /path/to/whisker/patches/lynx-android/buildroot.patch
# build/ is a submodule; commit in buildroot then propagate
cd build && git add -A && git commit -m "feat(whisker): expose Lynx internal symbols for C++ bridge" && cd ..

git apply --check /path/to/whisker/patches/lynx-android/lynx.patch
git apply         /path/to/whisker/patches/lynx-android/lynx.patch
git add -A && git commit -m "feat(whisker): expose Lynx internal symbols for C++ bridge"

git push -u origin whisker/3.7.0
```

Future Lynx upgrades follow the same pattern: rebase
`whisker/<new-version>` against the new upstream tag.

## 2. CI workflow

Add `.github/workflows/build-whisker-tarballs.yml` to the fork:

```yaml
name: build-whisker-tarballs

on:
  push:
    tags:
      - 'v*-whisker.*'   # matches v3.7.0-whisker.0 etc.
  workflow_dispatch:

jobs:
  android:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - name: Set up JDK 11 (Lynx gradle 6.7.1 requires it)
        uses: actions/setup-java@v4
        with:
          distribution: temurin
          java-version: 11

      - name: Set up Android NDK 21.1.6352462 (Lynx gn/ninja toolchain pin)
        uses: nttld/setup-ndk@v1
        with:
          ndk-version: r21e

      - name: Bootstrap Lynx (envsetup + hab sync if upstream needs it)
        run: |
          # Adjust per Lynx's actual bootstrap. If `tools/envsetup.sh`
          # and `tools/hab sync` aren't needed at HEAD, drop this step.
          if [ -f tools/envsetup.sh ]; then source tools/envsetup.sh; fi
          if [ -x tools/hab ]; then tools/hab sync; fi

      - name: Build AARs
        run: |
          cd platform/android
          ./gradlew --no-daemon \
            :LynxBase:assembleNoasanRelease \
            :LynxTrace:assembleNoasanRelease \
            :LynxAndroid:assembleNoasanRelease \
            :ServiceAPI:assembleNoasanRelease

      - name: Stage tarball contents
        run: |
          VERSION="${GITHUB_REF_NAME#v}"
          STAGE="whisker-lynx-android-$VERSION"
          mkdir -p "$STAGE/unpacked/jni"
          for aar in \
            base/platform/android/build/outputs/aar/LynxBase-noasan-release.aar:LynxBase.aar \
            base/trace/android/build/outputs/aar/LynxTrace-noasan-release.aar:LynxTrace.aar \
            platform/android/lynx_android/build/outputs/aar/LynxAndroid-noasan-release.aar:LynxAndroid.aar \
            platform/android/service_api/build/outputs/aar/ServiceAPI-noasan-release.aar:ServiceAPI.aar; do
            src="${aar%%:*}"; dst="${aar##*:}"
            cp "$src" "$STAGE/$dst"
            # Extract jni/<abi>/*.so from each AAR (unzip is sufficient — AARs are zips)
            unzip -o "$src" "jni/*" -d "$STAGE/unpacked/" >/dev/null || true
          done

      - name: Stage C++ headers
        run: |
          STAGE="whisker-lynx-android-${GITHUB_REF_NAME#v}"
          mkdir -p "$STAGE/headers"
          # Copy the same header tree that whisker-driver-sys/build.rs
          # references — paths inside Lynx tree may vary by version.
          # See `whisker/crates/whisker-driver-sys/build.rs` for the
          # `.include(...)` calls and match them.
          rsync -a core/ "$STAGE/headers/Lynx/" --include='**/*.h' --include='*/' --exclude='*'
          rsync -a base/ "$STAGE/headers/LynxBase/" --include='**/*.h' --include='*/' --exclude='*'
          rsync -a service_api/ "$STAGE/headers/LynxServiceAPI/" --include='**/*.h' --include='*/' --exclude='*'
          rsync -a third_party/primjs/ "$STAGE/headers/PrimJS/src/" --include='**/*.h' --include='*/' --exclude='*'

      - name: Pack tarball
        run: |
          NAME="whisker-lynx-android-${GITHUB_REF_NAME#v}.tar.gz"
          tar czf "$NAME" "${NAME%.tar.gz}"
          sha256sum "$NAME" | tee "$NAME.sha256"

      - name: Upload to release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            whisker-lynx-android-*.tar.gz
            whisker-lynx-android-*.tar.gz.sha256

  ios:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - name: Bootstrap Lynx
        run: |
          if [ -f tools/envsetup.sh ]; then source tools/envsetup.sh; fi
          if [ -x tools/hab ]; then tools/hab sync; fi

      - name: Install xcodegen + CocoaPods
        run: |
          brew install xcodegen
          gem install cocoapods

      - name: Build Lynx iOS xcframeworks
        run: .whisker/ios/build-xcframeworks.sh
        # The fork's own script. Does pod-install of upstream Lynx
        # source pods into a tiny carrier Xcode project, then
        # xcodebuilds device + simulator and wraps each pod's
        # framework into an xcframework. Outputs land under
        # `$OUT_DIR/<Pod>.xcframework/` + `$OUT_DIR/headers/<Pod>/`.

      - name: Stage + pack
        run: |
          VERSION="${GITHUB_REF_NAME#v}"
          STAGE="whisker-lynx-ios-$VERSION"
          mkdir -p "$STAGE/headers"
          cp -R out/Lynx.xcframework            "$STAGE/"
          cp -R out/LynxBase.xcframework        "$STAGE/"
          cp -R out/LynxServiceAPI.xcframework  "$STAGE/"
          cp -R out/PrimJS.xcframework          "$STAGE/"
          rsync -a core/             "$STAGE/headers/Lynx/"           --include='**/*.h' --include='*/' --exclude='*'
          rsync -a base/             "$STAGE/headers/LynxBase/"       --include='**/*.h' --include='*/' --exclude='*'
          rsync -a service_api/      "$STAGE/headers/LynxServiceAPI/" --include='**/*.h' --include='*/' --exclude='*'
          rsync -a third_party/primjs/ "$STAGE/headers/PrimJS/src/"   --include='**/*.h' --include='*/' --exclude='*'
          NAME="$STAGE.tar.gz"
          tar czf "$NAME" "$STAGE"
          shasum -a 256 "$NAME" | tee "$NAME.sha256"

      - name: Upload to release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            whisker-lynx-ios-*.tar.gz
            whisker-lynx-ios-*.tar.gz.sha256
```

Notes:
- The Android header staging globs are sketched but need
  adjustment to match Lynx's actual on-disk layout. Compare with
  `crates/whisker-driver-sys/build.rs`'s `.include(...)` calls.
- The iOS pod-install carrier project lives in the fork at
  `.whisker/ios/build-xcframeworks.sh` — the same logic that used
  to live in `xtask/src/ios/build_lynx_frameworks.rs` in Whisker
  pre-Phase-4.

## 3. Cut the first release

```bash
git checkout whisker/3.7.0
git tag v3.7.0-whisker.0
git push origin v3.7.0-whisker.0
```

Wait for the workflow to complete, then on the resulting release
page, take the SHA-256 of each tarball (from the `.sha256` file
attachment, or `shasum -a 256 whisker-lynx-android-*.tar.gz`).

## 4. Pin the SHA-256 in Whisker

Back in this repo, edit `crates/whisker-build/src/lynx.rs`:

```rust
pub const LYNX_VERSION: &str = "3.7.0-whisker.0";
pub const LYNX_ANDROID_SHA256: &str = "abc123…";   // from release
pub const LYNX_IOS_SHA256: &str = "def456…";       // from release
```

Commit and push. Now `whisker run` / `whisker build` will fetch on
first invocation, verify the SHA-256, and unpack to
`~/.cache/whisker/lynx/3.7.0-whisker.0/`.

## Migrating existing Whisker contributors

Contributors who built Lynx locally before the GitHub Releases
fetcher landed will have `target/lynx-{android,android-unpacked,
headers,ios}` as real directories. `whisker-build::lynx::
link_into_workspace` refuses to clobber them. One-time fix:

```bash
rm -rf target/lynx-android target/lynx-android-unpacked \
       target/lynx-headers target/lynx-ios
```

Next `whisker run` / `whisker build` will populate the cache + create
the symlinks.

If a contributor wants to use a locally-built Lynx (e.g. testing a
new patch before pushing to the fork), set `WHISKER_LYNX_DIR=/path`
where `/path` contains `android/` and/or `ios/` subdirs matching the
tarball layout.
