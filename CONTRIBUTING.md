# Contributing to Whisker

Thanks for hacking on Whisker! This guide covers how to build the project from
source, run an example on a device, and get a change merged.

- **Using** Whisker (getting started, guides, API)? → [whisker.rs/docs](https://whisker.rs/docs).
- **Designing/understanding** the internals? → [`docs/`](docs/README.md) (start
  with [`docs/architecture.md`](docs/architecture.md)).
- This file is the **practical** "how do I build, run, and contribute" guide.

## Prerequisites

| What | Notes |
|------|-------|
| **Rust** | Pinned by [`rust-toolchain.toml`](rust-toolchain.toml) (stable + `rustfmt` + `clippy`); `rustup` installs it automatically on first build. MSRV is **1.85** (edition 2024). |
| **Targets** | `rustup target add aarch64-apple-ios-sim aarch64-apple-ios` (iOS) and `rustup target add aarch64-linux-android` (Android — only `arm64-v8a` is built). |
| **iOS** | Xcode + an iOS Simulator. |
| **Android** | Android SDK + an **NDK** (one of: `23.1.7779620`, `25.1.8937393`, `26.1.10909125`, `26.3.11579264`, `27.0.12077973`, `27.1.12297006` — `sdkmanager --install "ndk;<version>"`). Set `ANDROID_HOME` (or have `~/Library/Android/sdk`). An **arm64 emulator, API 30+** (Pixel 5 or newer) — the Android build uses `-Ctarget-feature=+lse` and 16 KB page alignment, which need Armv8.2+. |

You only need the toolchain for the platform(s) you're touching. Pure-Rust
work (runtime, router, macros, …) needs no device tooling at all.

## Build, test, and lint

Whisker is one Cargo workspace. The checks below are exactly what CI
([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) enforces — run them
before pushing:

```sh
cargo build --workspace
cargo test  --workspace --all-targets
cargo test  --workspace --doc
cargo fmt   --all --check
cargo clippy --workspace --all-targets -- -D warnings   # warnings are errors
```

`cargo fmt --all` (no `--check`) applies formatting. Coverage is wired up:
`cargo coverage` (HTML) / `cargo coverage-text` (see [`.cargo/config.toml`](.cargo/config.toml)).

## Running an example on a device

The dev loop is the `whisker` CLI's `run` subcommand. **Always run it through
the workspace build** (`cargo run -p whisker-cli`), not a globally-installed
`whisker` — that guarantees your local changes (including the cng templates
baked into the binary) are actually used:

```sh
# From the repo root, pick any example:
cargo run -p whisker-cli --bin whisker -- run ios     --manifest-path examples/asset-demo/Cargo.toml
cargo run -p whisker-cli --bin whisker -- run android --manifest-path examples/asset-demo/Cargo.toml

# …or from inside the example crate (manifest is auto-resolved from cwd):
cd examples/asset-demo
cargo run -p whisker-cli -- run ios
```

`whisker run`:

1. Generates the native project tree under `examples/<app>/gen/{ios,android}/`
   (see the native pipeline below).
2. Cross-compiles the user crate to a native lib, builds the app
   (xcodebuild / Gradle), installs, and launches it on the booted
   simulator/emulator.
3. Watches your sources and **hot-reloads** edits in under a second
   (subsecond Tier 1 patch; falls back to a Tier 2 cold rebuild). See
   [`docs/hot-reload-internals.md`](docs/hot-reload-internals.md).

Examples live in [`examples/`](examples) (and one under each `packages/<mod>/example`).
`tokio-smoke` / `asset-demo` are small and fast to iterate on.

## The native build pipeline (and testing changes to it)

`whisker run` doesn't hand-write Xcode/Gradle projects — it **generates** them:

- **`whisker-cng`** renders the `gen/{ios,android}/` tree from templates
  `include_str!`-baked into the CLI binary. Regeneration is fingerprint-gated;
  the run log shows `regenerated`/`reused cached gen/`.
- **iOS**: the generated `.xcodeproj` runs `whisker build-ios` from a build
  phase (cargo → `WhiskerDriver.framework`).
- **Android**: the generated project applies the **Gradle plugin**
  (`platforms/android/gradle-plugin`, published to a Maven repo), whose
  `WhiskerBuildTask` runs `whisker build-android` (cargo → staged `.so`).

Two things follow from this that trip people up:

- **Editing a `whisker-cng` template?** The template is baked into the binary,
  and `gen/` regeneration is keyed on a fingerprint that includes a manual
  `template_version` (in `crates/whisker-cng/src/{android,ios}.rs`). **Bump it**
  when you change a template's shape, or the old `gen/` is silently reused.
  Running the workspace CLI (`cargo run -p whisker-cli`) rebuilds the binary so
  your edit is present; `whisker run` warns if the running CLI is older than
  `crates/whisker-cng/src`.

- **Editing the Gradle plugin?** It's resolved from Maven, so a source edit
  isn't picked up by an example build until republished. To test locally,
  override it with a composite build — add to the generated
  `examples/<app>/gen/android/settings.gradle.kts` (regenerated, so this is
  temporary):
  ```kotlin
  pluginManagement {
      includeBuild("/abs/path/to/whisker/platforms/android/gradle-plugin")
      // …existing repositories { } …
  }
  ```

## Troubleshooting: "my change had no effect"

The dev loop tells you whether your code actually reached the device — watch
the run log:

- `✓ compile <app> (…-android) relinked` vs `up-to-date` — did cargo produce a
  fresh `.so`, or no-op?
- `[whisker run] reused cached gen/ … (template_version N)` vs `regenerated` —
  did the native project tree refresh?

If an Android change still doesn't show up, the safest reset is to nuke the
generated build tree and force a fresh `.so`:

```sh
rm -rf examples/<app>/gen/android/app/build && touch examples/<app>/src/lib.rs
```

Background and the underlying fixes live in issue #260.

## Submitting changes

- **Branch** off `main`; open a PR against `main`.
- **Commit messages** follow Conventional Commits with a crate scope, e.g.
  `feat(whisker-router): …`, `fix(gradle-plugin): …`, `docs(router): …`.
- **Keep docs in lockstep.** When you change a system, update its
  [`docs/`](docs/README.md) note in the same PR (and follow
  [`docs/comment-style.md`](docs/comment-style.md) for code comments).
- **CI must be green** — run the build/test/fmt/clippy block above locally
  first. Clippy warnings are denied.
- Touching a new `whisker-*` module's public surface? Read
  [`docs/module-api-design.md`](docs/module-api-design.md) first.

By contributing, you agree your work is licensed under the project's MIT OR
Apache-2.0 terms.
