#!/usr/bin/env bash
#
# Build Lynx + PrimJS + LynxBase + LynxServiceAPI xcframeworks from the
# upstream CocoaPods source pods. Used to feed our SPM binaryTarget chain.
#
# Lynx ships only as source pods on CocoaPods; there are no prebuilt iOS
# binaries on GitHub Releases. So we set up a tiny "carrier" Xcode project,
# `pod install` the source pods into it, build for iOS device + Simulator
# in static-framework form, and lift the resulting frameworks into
# xcframeworks.
#
# Outputs (under target/lynx-ios/):
#   Lynx.xcframework
#   PrimJS.xcframework
#   LynxBase.xcframework
#   LynxServiceAPI.xcframework

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LYNX_VERSION="3.7.0"
PRIMJS_VERSION="3.7.0"
BUILD="$ROOT/target/lynx-build"
OUT="$ROOT/target/lynx-ios"

echo "==> Clean"
rm -rf "$BUILD" "$OUT"
mkdir -p "$BUILD" "$OUT"
cd "$BUILD"

echo "==> Generate carrier Xcode project"
mkdir -p Sources
cat > Sources/AppDelegate.swift <<'EOF'
import UIKit
@UIApplicationMain
class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
}
EOF

cat > project.yml <<EOF
name: LynxCarrier
options:
  bundleIdPrefix: dev.lyra.carrier
  deploymentTarget:
    iOS: '13.0'
targets:
  LynxCarrier:
    type: application
    platform: iOS
    sources: [Sources]
    info:
      path: Info.plist
      properties:
        UILaunchScreen: {}
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: dev.lyra.carrier.LynxCarrier
EOF

xcodegen generate

cat > Podfile <<EOF
platform :ios, '13.0'
use_frameworks! :linkage => :static
target 'LynxCarrier' do
  pod 'Lynx', '$LYNX_VERSION'
  pod 'PrimJS', '$PRIMJS_VERSION', :subspecs => ['quickjs', 'napi']
end
EOF

echo "==> pod install"
pod install --repo-update

echo "==> Patch upstream podspec bug (HEADER_SEARCH_PATHS contains a CI-only path)"
# Lynx 3.7.0's xcconfigs ship with HEADER_SEARCH_PATHS pointing at
# `/Users/runner/work/lynx/lynx/lynx`, the GitHub Actions runner path used
# during release. For LynxServiceAPI it's the *only* search path, so the
# build fails outright. Rewrite to PODS_TARGET_SRCROOT so headers resolve
# against the locally extracted pod sources.
find Pods -name "*.xcconfig" -exec sed -i '' \
    's|/Users/runner/work/lynx/lynx/lynx|${PODS_TARGET_SRCROOT}|g' {} \;

WORKSPACE="LynxCarrier.xcworkspace"
COMMON_FLAGS=(
    -workspace "$WORKSPACE"
    -scheme LynxCarrier
    -configuration Release
    SKIP_INSTALL=NO
    ONLY_ACTIVE_ARCH=NO
    CODE_SIGNING_ALLOWED=NO
    CODE_SIGNING_REQUIRED=NO
    CODE_SIGN_IDENTITY=""
)

echo "==> Build for iOS device"
xcodebuild build "${COMMON_FLAGS[@]}" \
    -destination 'generic/platform=iOS' \
    -derivedDataPath build/device

echo "==> Build for iOS Simulator"
xcodebuild build "${COMMON_FLAGS[@]}" \
    -destination 'generic/platform=iOS Simulator' \
    -derivedDataPath build/sim

DEVICE_DIR="build/device/Build/Products/Release-iphoneos"
SIM_DIR="build/sim/Build/Products/Release-iphonesimulator"

echo "==> Inventory of framework outputs"
echo "Device: $DEVICE_DIR"
ls "$DEVICE_DIR" 2>/dev/null | grep -E '\.(framework|a)$' || true
echo "Sim:    $SIM_DIR"
ls "$SIM_DIR" 2>/dev/null | grep -E '\.(framework|a)$' || true

echo "==> Create xcframeworks"
for fw in Lynx PrimJS LynxBase LynxServiceAPI; do
    DEV_FW="$DEVICE_DIR/$fw/$fw.framework"
    SIM_FW="$SIM_DIR/$fw/$fw.framework"
    if [ -d "$DEV_FW" ] && [ -d "$SIM_FW" ]; then
        rm -rf "$OUT/$fw.xcframework"
        xcodebuild -create-xcframework \
            -framework "$DEV_FW" \
            -framework "$SIM_FW" \
            -output "$OUT/$fw.xcframework"
        echo "✅ $OUT/$fw.xcframework"
    else
        echo "⚠️  Missing $fw framework"
        echo "    expected device: $DEV_FW"
        echo "    expected sim:    $SIM_FW"
    fi
done

echo "==> Stage Lynx C++ headers for the bridge target"
# Lynx's framework PrivateHeaders are flattened, but the headers themselves
# include each other via tree-relative paths like `core/...`, `base/...`.
# Copy every pod's source tree (headers only) into a stable location with
# directory structure preserved, so the bridge target's header search paths
# can use them directly.
SRC_OUT="$OUT/sources"
rm -rf "$SRC_OUT"
mkdir -p "$SRC_OUT"
for pod in Lynx LynxBase LynxServiceAPI PrimJS; do
    rsync -am --include='*/' --include='*.h' --exclude='*' \
        "Pods/$pod/" "$SRC_OUT/$pod/"
done
echo "    staged headers per pod:"
for pod in Lynx LynxBase LynxServiceAPI PrimJS; do
    echo "      $pod: $(find "$SRC_OUT/$pod" -name '*.h' | wc -l | tr -d ' ')"
done

echo
echo "==> Final outputs"
ls -la "$OUT"
