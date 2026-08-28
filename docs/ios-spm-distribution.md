# iOS distribution and the remote Swift package

How an iOS app receives the Swift Host SDK and links its Rust application.

## Two artifacts with different ownership

The iOS application combines two independently built artifacts:

| Artifact | Contents | Resolution |
|---|---|---|
| `WhiskerRuntime` / `WhiskerModule` / `WhiskerCBridge` | Checked-in Swift Host implementation, module API, and C declarations | SwiftPM package at the Whisker git tag |
| `WhiskerDriver.framework` | The consuming application's Rust crate, including `whisker-runtime` and the iOS-only `whisker-driver` adapter | Built per application by the generated Xcode Run Script phase |

The Swift package contains no copy of the user application and no generated
Rust binding. Conversely, `WhiskerDriver.framework` contains no UIKit Host
implementation. Their contract is the typed retained-runtime ABI declared by
`WhiskerCBridge`.

The name `WhiskerDriver.framework` describes the link product, while the Rust
`whisker-driver` crate is specifically the safe FFI adapter inside that product.
Platform-independent runtime behavior lives in `whisker-runtime`. Web and
Desktop link that runtime directly and do not link `whisker-driver`.

## Build flow

The generated Xcode project has a Run Script phase that invokes
`whisker build-ios`. That command cross-compiles the user's Cargo package for
the active iOS target, wraps the result as `WhiskerDriver.framework`, and puts
it where the app target's framework search path expects it. Xcode then links
the framework together with the SwiftPM Host libraries.

This keeps Xcode authoritative for application compilation: a developer can
build from Xcode without first running `whisker run` or `whisker build` in a
separate terminal. CNG is still responsible for materializing the Xcode project
and its dependency graph after a clean checkout or Cargo dependency change.

## Module resolution

Whisker modules remain loosely coupled across the language boundary. CNG walks
the app's Cargo dependency graph and adds each module's checked-in Swift source
package to the generated native dependency graph. Rust and Swift/Kotlin match
module, function, event, and element names at runtime. `WhiskerValue` is the
only argument, result, property, and event-payload value model crossing the
boundary; no generated per-module contract is required for compilation.

## Version source of truth

The generated project's remote package reference is driven by
`WHISKER_IOS_SPM_URL` and `WHISKER_IOS_SPM_VERSION` in
`crates/whisker-build/src/ios.rs`. First-party module manifests pin the same
version, and the build tests reject mismatches. Use the release workflow rather
than creating the tag manually; see the release skill linked from
`docs/README.md`.

## Developing the Swift Host locally

A generated external app normally resolves the published SwiftPM tag, so local
edits under `platforms/ios` are not picked up automatically. For framework
development, run the package tests in this repository or temporarily redirect
the package URL to the working copy. Do not commit a local path override to a
generated application.
