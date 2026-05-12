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
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/21.1.6352462}"
export ANDROID_NDK="$ANDROID_NDK_HOME"

if [ ! -d "$ANDROID_NDK_HOME" ]; then
    echo "error: NDK not found at $ANDROID_NDK_HOME"
    echo "       install via sdkmanager: sdkmanager 'ndk;21.1.6352462'"
    exit 1
fi

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
# cargo-ndk requires NDK r23+. NDK 21 works for Lynx C++ but not for
# cargo-ndk, so we pick the newest NDK in $ANDROID_HOME/ndk for Rust.
RUST_NDK=""
# Prefer NDK 23 (oldest cargo-ndk-supported version). Newer NDKs add
# `init_have_lse_atomics` ELF init code that crashes inside a local
# `getauxval` stub on API 36 emulators.
for cand in 23.1.7779620 26.1.10909125 26.3.11579264 27.0.12077973 27.1.12297006 29.0.14206865; do
    if [ -d "$ANDROID_HOME/ndk/$cand" ]; then
        RUST_NDK="$ANDROID_HOME/ndk/$cand"
        break
    fi
done
if [ -z "$RUST_NDK" ]; then
    echo "error: no NDK r23+ installed (cargo-ndk requires it)"
    exit 1
fi
echo "==> Building Rust cdylib for $ABI (NDK $(basename "$RUST_NDK"))"
# `-P 24` matches the app's minSdk and avoids NDK r27's
# outline-atomics init-time crash that fires on the API 36 emulator
# when targeting aarch64-linux-android21. `--link-libcxx-shared` adds
# libc++_shared.so to DT_NEEDED and bundles it into the output dir.
ANDROID_NDK_HOME="$RUST_NDK" cargo ndk \
    -t "$ABI" -P 24 --link-libcxx-shared \
    build --release -p hello-world

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
# Use the Rust-side NDK (27.x); both NDK 27 and Lynx's NDK 21 use the
# same libc++ ABI on aarch64.
NDK_LIBCPP="$RUST_NDK/toolchains/llvm/prebuilt/darwin-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so"
if [ -f "$NDK_LIBCPP" ]; then
    echo "==> Bundling libc++_shared.so from NDK"
    cp "$NDK_LIBCPP" "$SO_DST/libc++_shared.so"
fi

# --- 3. Make sure Lynx AARs are unpacked for the C++ bridge link step ---
if [ ! -d "$ROOT/target/lynx-android-unpacked/jni/$ABI" ]; then
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
