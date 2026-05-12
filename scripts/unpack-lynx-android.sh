#!/usr/bin/env bash
#
# Unpack the locally-built Lynx Android AARs so the Rust bridge build
# (examples/hello-world/build.rs → cc::Build) can link against
# liblynx.so / liblynxbase.so at compile time, and so example apps can
# bundle the .so files into their jniLibs.
#
# Inputs:  target/lynx-android/*.aar
# Outputs: target/lynx-android-unpacked/jni/<abi>/*.so
#
# Run after building Lynx from source (see docs) or after manually
# copying the AARs in from the upstream Lynx checkout.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/target/lynx-android"
DST="$ROOT/target/lynx-android-unpacked"

if [ ! -d "$SRC" ]; then
    echo "error: $SRC not found"
    echo "       expected LynxAndroid.aar, LynxBase.aar, ServiceAPI.aar"
    exit 1
fi

rm -rf "$DST"
mkdir -p "$DST"

for aar in "$SRC"/*.aar; do
    name="$(basename "$aar" .aar)"
    echo "==> unpacking $name"
    tmp=$(mktemp -d)
    unzip -q -o "$aar" -d "$tmp"
    if [ -d "$tmp/jni" ]; then
        mkdir -p "$DST/jni"
        cp -R "$tmp/jni/." "$DST/jni/"
    fi
    rm -rf "$tmp"
done

echo
echo "✅ unpacked to $DST"
ls -la "$DST/jni" 2>/dev/null || true
