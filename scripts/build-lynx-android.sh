#!/usr/bin/env bash
#
# Build Lynx's Android AARs from source with Lyra-required patches
# applied. Produces:
#   target/lynx-android/LynxBase.aar
#   target/lynx-android/LynxTrace.aar
#   target/lynx-android/LynxAndroid.aar
#   target/lynx-android/ServiceAPI.aar
#
# Why we can't use the maven-published AARs: the upstream build hides
# every C++ symbol (`-fvisibility=hidden` + `-Wl,--exclude-libs,ALL` +
# `-static-libstdc++`), so the Element PAPI / LynxShell entry points
# the Lyra Rust bridge calls aren't in `liblynx.so`'s `.dynsym`. The
# patches under `patches/lynx-android/` flip those defaults; see the
# patch headers for the full rationale.
#
# Pre-reqs (user-provided — we don't try to fetch sources ourselves):
#   - Lynx source tree checked out via Lynx's normal bootstrap
#     (`source tools/envsetup.sh` + `tools/hab sync -f` per Lynx docs)
#     at LYNX_SRC (defaults to $HOME/work/lynx-src).
#     Pinned commits we tested:
#       lynx          248765e76fb0f889efd0b168b8b892819c1c17e4
#       buildroot     917b38180c78da016b1023436d5b568ca5402bee
#   - JAVA_HOME pointing at a JDK 11 (Lynx's gradle wrapper rejects
#     newer JVMs). Override LYRA_JAVA11_HOME if not auto-detected.
#   - ANDROID_HOME / Android NDK 21.1.6352462 installed (Lynx's
#     gn/ninja toolchain assumes this exact version).
#
# Usage:
#   scripts/build-lynx-android.sh                     # build + copy
#   LYNX_SRC=/path/to/lynx scripts/build-lynx-android.sh

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LYNX_SRC="${LYNX_SRC:-$HOME/work/lynx-src}"
DEST="$ROOT/target/lynx-android"

# --- 0. Toolchain resolution ---
if [ -z "${JAVA_HOME:-}" ]; then
    for cand in "${LYRA_JAVA11_HOME:-}" \
                "$HOME/work/java11/jdk-11.0.25+9/Contents/Home" \
                "$HOME/work/java11/jdk-11.0.25+9" \
                "/Library/Java/JavaVirtualMachines/temurin-11.jdk/Contents/Home"; do
        [ -n "$cand" ] && [ -d "$cand" ] && export JAVA_HOME="$cand" && break
    done
fi
if [ -z "${JAVA_HOME:-}" ] || [ ! -d "$JAVA_HOME" ]; then
    echo "error: JAVA_HOME not set and JDK 11 not found." >&2
    echo "       Lynx's gradle (6.7.1) refuses anything newer; install" >&2
    echo "       Temurin 11 and re-run, or set LYRA_JAVA11_HOME." >&2
    exit 1
fi
export PATH="$JAVA_HOME/bin:$PATH"

export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/21.1.6352462}"
if [ ! -d "$ANDROID_NDK_HOME" ]; then
    echo "error: NDK 21.1.6352462 required for Lynx C++ build." >&2
    echo "       install via:  sdkmanager 'ndk;21.1.6352462'" >&2
    exit 1
fi

# --- 1. Lynx source sanity ---
if [ ! -d "$LYNX_SRC/platform/android/lynx_android" ] || \
   [ ! -d "$LYNX_SRC/build/config" ]; then
    echo "error: Lynx source not found at $LYNX_SRC" >&2
    echo "       follow Lynx's bootstrap (tools/envsetup.sh +" >&2
    echo "       tools/hab sync) then set LYNX_SRC." >&2
    exit 1
fi

# --- 2. Apply patches (idempotent) ---
apply_patch_if_needed() {
    local repo="$1" patch="$2"
    if git -C "$repo" apply --reverse --check "$patch" >/dev/null 2>&1; then
        echo "  already applied: $(basename "$patch")"
        return 0
    fi
    if ! git -C "$repo" apply --check "$patch" >/dev/null 2>&1; then
        echo "error: $patch doesn't apply cleanly to $repo" >&2
        echo "       Lynx source may have moved; re-record the patch" >&2
        echo "       (git diff > $patch) after porting the changes." >&2
        exit 1
    fi
    git -C "$repo" apply "$patch"
    echo "  applied: $(basename "$patch")"
}

echo "==> Applying Lyra patches to Lynx source"
apply_patch_if_needed "$LYNX_SRC/build" "$ROOT/patches/lynx-android/buildroot.patch"
apply_patch_if_needed "$LYNX_SRC"        "$ROOT/patches/lynx-android/lynx.patch"

# --- 3. Build ---
echo "==> Building AARs (this takes a few minutes the first time)"
cd "$LYNX_SRC/platform/android"
./gradlew --no-daemon \
    :LynxBase:assembleNoasanRelease \
    :LynxTrace:assembleNoasanRelease \
    :LynxAndroid:assembleNoasanRelease \
    :ServiceAPI:assembleNoasanRelease

# --- 4. Copy results ---
mkdir -p "$DEST"
copy_aar() {
    local src="$1" dst="$2"
    if [ ! -f "$src" ]; then
        echo "error: expected AAR not produced: $src" >&2
        exit 1
    fi
    cp "$src" "$DEST/$dst"
    echo "  $dst"
}
echo "==> Copying AARs to $DEST"
copy_aar "$LYNX_SRC/base/platform/android/build/outputs/aar/LynxBase-noasan-release.aar"   LynxBase.aar
copy_aar "$LYNX_SRC/base/trace/android/build/outputs/aar/LynxTrace-noasan-release.aar"     LynxTrace.aar
copy_aar "$LYNX_SRC/platform/android/lynx_android/build/outputs/aar/LynxAndroid-noasan-release.aar" LynxAndroid.aar
copy_aar "$LYNX_SRC/platform/android/service_api/build/outputs/aar/ServiceAPI-noasan-release.aar"   ServiceAPI.aar

echo
echo "✅ Lynx Android AARs ready at $DEST"
echo "   Next: scripts/unpack-lynx-android.sh && scripts/build-android-example.sh"
