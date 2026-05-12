#!/usr/bin/env bash
#
# Build the hello-world Android example end-to-end:
#   1. Compile the Rust cdylib (libhello_world.so) for the target ABI
#   2. Drop it into the app's jniLibs
#   3. Run gradle :app:assembleDebug
#
# Pre-reqs:
#   - scripts/unpack-lynx-android.sh has been run (Lynx AARs unpacked)
#   - target/lynx-android/*.aar present
#   - Android NDK 21.1.6352462 installed in $ANDROID_HOME
#   - Java 17 available (Android Studio's JBR is fine)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ABI="${ABI:-arm64-v8a}"
RUST_TARGET="aarch64-linux-android"
EXAMPLE_DIR="$ROOT/examples/hello-world/android"

# --- Resolve toolchains ---
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
# Do NOT pin ANDROID_NDK_HOME here: cargo xtask picks a Rust-compatible
# NDK on its own (NDK 23+ for `-mno-outline-atomics` support), and
# Gradle resolves the Android NDK via ndk.dir / ndkVersion. Pinning to
# NDK 21 globally would feed it back to xtask and break the Rust build.

# Android Studio's bundled JBR (Java 17/21) is the most reliable choice.
if [ -z "${JAVA_HOME:-}" ]; then
    if [ -d "/Applications/Android Studio.app/Contents/jbr/Contents/Home" ]; then
        export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
    else
        echo "error: JAVA_HOME not set and Android Studio JBR not found"
        exit 1
    fi
fi
export PATH="$JAVA_HOME/bin:$PATH"

# --- 1. Build the Rust cdylib ---
# `xtask android cargo` (xtask/src/android/cargo_build.rs) does the
# cargo-ndk's job: pick a Rust-compatible NDK, set CC/CXX/AR/LINKER for
# the target triple, then invoke plain `cargo build`. Bundling
# libc++_shared.so is done below in step 2c (cargo-ndk's
# `--link-libcxx-shared` shipped that for us; we now do it manually).
echo "==> Building Rust cdylib for $ABI"
(cd "$ROOT" && cargo xtask android cargo --abi "$ABI" --api 24 -p hello-world)

SO_SRC="$ROOT/target/$RUST_TARGET/release/libhello_world.so"
if [ ! -f "$SO_SRC" ]; then
    echo "error: cargo did not produce $SO_SRC"
    exit 1
fi

# --- 2. Drop into app jniLibs ---
echo "==> Copying libhello_world.so → app jniLibs"
SO_DST="$EXAMPLE_DIR/app/src/main/jniLibs/$ABI"
mkdir -p "$SO_DST"
cp "$SO_SRC" "$SO_DST/libhello_world.so"

# libhello_world.so (and Lynx's .so files) DT_NEEDED libc++_shared.so.
# Lynx's AAR no longer ships it under the new build, and our Rust cdylib
# doesn't bundle it either — so we have to copy it from the NDK sysroot.
# libc++_shared.so is ABI-compatible across recent NDK versions on
# aarch64, so the first one we find under $ANDROID_HOME/ndk is fine.
NDK_LIBCPP=$(find "$ANDROID_HOME/ndk" \
    -path "*toolchains/llvm/prebuilt/*/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" \
    -print -quit 2>/dev/null)
if [ -n "$NDK_LIBCPP" ] && [ -f "$NDK_LIBCPP" ]; then
    echo "==> Bundling libc++_shared.so from NDK"
    cp "$NDK_LIBCPP" "$SO_DST/libc++_shared.so"
else
    echo "error: libc++_shared.so not found under any NDK in $ANDROID_HOME/ndk" >&2
    exit 1
fi

# --- 3. Make sure Lynx AARs are unpacked for the C++ bridge link step ---
if [ ! -d "$ROOT/target/lynx-android-unpacked/jni/$ABI" ]; then
    if [ ! -d "$ROOT/target/lynx-android" ] || [ -z "$(ls -A "$ROOT/target/lynx-android"/*.aar 2>/dev/null)" ]; then
        echo "error: no Lynx AARs in $ROOT/target/lynx-android/" >&2
        echo "       run scripts/build-lynx-android.sh first (see patches/lynx-android/README.md)" >&2
        exit 1
    fi
    echo "==> Unpacking Lynx AARs"
    "$ROOT/scripts/unpack-lynx-android.sh"
fi

# --- 4. Gradle assemble ---
echo "==> Running gradle :app:assembleDebug"
(cd "$EXAMPLE_DIR" && ./gradlew :app:assembleDebug --no-daemon)

APK="$EXAMPLE_DIR/app/build/outputs/apk/debug/app-debug.apk"
if [ -f "$APK" ]; then
    echo
    echo "✅ APK: $APK"
    ls -la "$APK"
fi
