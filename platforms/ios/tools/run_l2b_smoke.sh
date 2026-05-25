#!/usr/bin/env bash
# Phase L-2b runtime smoke test runner.
#
# Compiles the WhiskerModuleApi DSL + registrar sources together
# with the in-tree smoke driver, runs the binary, and reports
# the result. No iOS simulator / Lynx framework needed — the
# smoke test stubs the LynxUI surface and exercises only the
# Obj-C-runtime install path.
#
# Used by:
#   - Local verification (`platforms/ios/tools/run_l2b_smoke.sh`)
#   - CI follow-up once we add a macOS Swift job (deferred).

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
tmp_bin="$(mktemp -d)/l2b_smoke"

# `-parse-as-library` makes `runL2bSmoke()` at the bottom of
# `l2b_lynx_installer_smoke.swift` legal — without it, swiftc
# treats one of the inputs as `main.swift` and rejects top-level
# expressions in the other files. Combined with a `@main`
# attribute on the entry struct below.
swiftc -o "${tmp_bin}" -parse-as-library \
  "${repo_root}/platforms/ios/Sources/WhiskerModuleApi/ModuleDefinition.swift" \
  "${repo_root}/platforms/ios/Sources/WhiskerModuleApi/Module.swift" \
  "${repo_root}/platforms/ios/Sources/WhiskerModuleApi/WhiskerModuleRegistrar.swift" \
  "${repo_root}/platforms/ios/tools/l2b_lynx_installer_smoke.swift"

"${tmp_bin}"
