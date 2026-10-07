set -euo pipefail
WORKSPACE="{{whisker_workspace_root}}"
WHISKER_CLI="${WHISKER_CLI:-whisker}"
if ! command -v "$WHISKER_CLI" >/dev/null 2>&1; then
  echo "error: Whisker CLI not found: $WHISKER_CLI. Install with: cargo install whisker-cli" >&2
  exit 1
fi
if [ ! -d "$WORKSPACE" ]; then
  echo "error: Whisker workspace no longer exists: $WORKSPACE" >&2
  echo "Re-run CNG generation after moving the project." >&2
  exit 1
fi
BUILD_COMMAND=(
  "$WHISKER_CLI" build-ios
  --workspace="$WORKSPACE"
  --package="{{whisker_user_package}}"
  --configuration="$CONFIGURATION"
  --platform="$PLATFORM_NAME"
  --archs="$ARCHS"
  --built-products-dir="$BUILT_PRODUCTS_DIR"
)
if [ -n "${WHISKER_FEATURES:-}" ]; then
  for feat in $WHISKER_FEATURES; do
    BUILD_COMMAND+=(--features "$feat")
  done
fi
"${BUILD_COMMAND[@]}"
