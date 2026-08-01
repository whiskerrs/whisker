# Source Selection

Use the smallest set of sources that establishes the API for the app's resolved version.

## Authority order

1. The exact source selected by `Cargo.lock`, a path dependency, or a Git revision
2. The matching release tag in `whiskerrs/whisker`
3. Examples, tests, changelogs, and rustdoc from that source tree
4. The editable documentation in `whiskerrs/website`
5. The rendered documentation at `https://whisker.rs/`

The framework source determines compile-time API shape. Documentation from another release can still explain a concept, but it cannot prove that an item exists in the target version.

When working inside the Whisker framework repository, follow `CONTRIBUTING.md` and run the workspace CLI with `cargo run -p whisker-cli -- ...`. Do not test local framework changes with a globally installed CLI.

## Establish the target version

```sh
rg -n 'whisker|version|git|path' Cargo.toml Cargo.lock
whisker --version
cargo tree | rg '^whisker'
cargo metadata --format-version 1
```

- `Cargo.lock` and `cargo tree` describe the app build.
- `whisker --version` describes the installed CLI and may differ from the app crates.
- Path and Git dependencies must be inspected at their exact path or revision.
- Do not change versions as a side effect of an unrelated feature.

## Find the implementation surface

| Capability | Framework source |
| --- | --- |
| Components and macros | `crates/whisker`, `crates/whisker-macros` |
| CSS and layout | `crates/whisker-css`, examples |
| Signals, resources, and tasks | `crates/whisker-runtime` |
| CLI, scaffolding, and platform runs | `crates/whisker-cli`, `crates/whisker-cng` |
| Input | `packages/whisker-input` |
| Persistence | `packages/whisker-local-store`, `packages/whisker-secure-store` |
| Routing | `packages/whisker-router` |
| Native capability | the corresponding package and example |
| Modules and plugins | `crates/whisker-plugin`, first-party packages |

Prefer targeted searches:

```sh
rg -n 'Input\(|on_input|text:' packages/whisker-input examples
rg -n 'WhiskerLocalStore|fn save|fn load' packages/whisker-local-store examples
rg -n 'run_blocking|spawn_local|resource\(' crates examples
rg -n 'routes!|use_navigator|use_param' packages/whisker-router examples
rg -n 'package.metadata.whisker|plugin::<' packages examples
```

Read a public API and a current example before copying internal types. Use tests for edge cases.

## Diagnose platform failures

Capture the versions that participate in the failed path:

- Whisker crates and CLI
- Rust toolchain and target
- macOS and Xcode, including installed iOS SDK and simulator runtime
- Android SDK, NDK, ABI, and emulator API level

Run `whisker doctor`, then compare its result with the failing build or run command. A successful probe does not establish that the selected build destination, runtime, or signing configuration is usable.

Search existing issues before reporting a framework defect. Include the smallest reproduction, expected and actual behavior, environment versions, and any verified workaround.
