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
implementation. Their contract is the typed retained-runtime ABI owned by
`whisker-driver-sys` and checked in as the generated `WhiskerCBridge` header.
The Swift package consumes that header directly; an application build does not
run Whisker's ABI generator.

The name `WhiskerDriver.framework` describes the link product, while the Rust
`whisker-driver` crate is specifically the safe FFI adapter inside that product.
Platform-independent runtime behavior lives in `whisker-runtime`. Web and
Linux link that runtime directly and do not link `whisker-driver`.

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

## Host sources from git and path dependencies

An app that depends on `whisker` from crates.io resolves the published SwiftPM
tag. When Cargo resolves `whisker` from git or a local path, the repository at
that revision already contains `platforms/`, and CNG builds the Hosts from it:

- iOS and macOS depend on the repository as a local Swift package. SwiftPM
  takes a path package's identity from its directory name, so a checkout that
  is not named `whisker` (Cargo names git checkouts after the revision) is
  linked from `gen/.whisker-host-sdk/whisker`. The local package then replaces
  the remote `whisker` package pinned by module manifests.
- Android includes `platforms/android` as the `:whisker-runtime` and
  `:whisker-module` projects and includes its Gradle plugin and KSP builds.

The Host therefore always matches the Rust revision in `Cargo.lock`, including
for forks and working copies patched in through `[patch]`. The framework
workspace itself takes the same path. Gradle writes its build outputs inside
the checkout.
