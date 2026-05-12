#!/usr/bin/env bash
#
# Build the LyraMobile.xcframework from the lyra-mobile Rust crate.
#
# Outputs: target/lyra-mobile/LyraMobile.xcframework
#
# Slices produced:
#   - ios-arm64                       (real device)
#   - ios-arm64_x86_64-simulator      (lipo of arm64-sim + x86_64-sim)
#
# This is the manual-script form. We plan to migrate it into `cargo xtask
# build-xcframework` once the build matures.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# The cdylib that ships to iOS is the *user's* crate (the one annotated
# with #[lyra::main]). For the in-repo demo that's examples/hello-world;
# the FFI exports `lyra_mobile_app_main` / `lyra_mobile_tick` come from
# the macro expansion in that crate. Header file lives next to the
# bootstrap helpers in `lyra-mobile/include/`.
USER_CRATE="hello-world"
USER_LIB="hello_world"
HEADERS_SRC="$ROOT/crates/lyra-mobile/include"
OUT="$ROOT/target/lyra-mobile"
XCF="$OUT/LyraMobile.xcframework"
PROFILE="release"

echo "==> Cleaning $OUT"
rm -rf "$OUT"
mkdir -p "$OUT"

echo "==> Building Rust static libs (user crate: $USER_CRATE)"
for triple in aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios; do
    echo "    -- $triple"
    (cd "$ROOT" && cargo build --release -p "$USER_CRATE" --target "$triple")
done

DEVICE_LIB="$ROOT/target/aarch64-apple-ios/$PROFILE/lib${USER_LIB}.a"
SIM_ARM64_LIB="$ROOT/target/aarch64-apple-ios-sim/$PROFILE/lib${USER_LIB}.a"
SIM_X86_LIB="$ROOT/target/x86_64-apple-ios/$PROFILE/lib${USER_LIB}.a"

SIM_FAT="$OUT/sim/lib${USER_LIB}.a"
mkdir -p "$(dirname "$SIM_FAT")"
echo "==> Lipo simulator slices"
lipo -create "$SIM_ARM64_LIB" "$SIM_X86_LIB" -output "$SIM_FAT"

echo "==> Staging headers"
HDR_DIR="$OUT/Headers"
mkdir -p "$HDR_DIR"
cp "$HEADERS_SRC/lyra_mobile.h" "$HDR_DIR/"
cp "$HEADERS_SRC/module.modulemap" "$HDR_DIR/"

echo "==> Creating xcframework"
xcodebuild -create-xcframework \
    -library "$DEVICE_LIB" -headers "$HDR_DIR" \
    -library "$SIM_FAT"    -headers "$HDR_DIR" \
    -output "$XCF"

echo
echo "✅ Created $XCF"
ls -la "$XCF"
