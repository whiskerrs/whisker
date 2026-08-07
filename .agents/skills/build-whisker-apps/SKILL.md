---
name: build-whisker-apps
description: Build, extend, diagnose, and validate native iOS and Android apps written in Rust with Whisker. Use for Whisker app scaffolding, render! and css! components, signals, input, persistence, routing, native modules and plugins, hot reload, whisker run, whisker fmt, and whisker doctor. For changes to the Whisker framework itself, follow the repository's CONTRIBUTING.md instead.
---

# Build Whisker Apps

Use the API resolved by the target app. Whisker changes quickly, so confirm the app's dependency and CLI versions before copying current examples or documentation.

## Select the matching source

1. Inspect `Cargo.toml`, `Cargo.lock`, path or Git dependencies, and `whisker --version`.
2. Treat the matching release source or resolved crate source as authoritative for API shape.
3. Use examples, package rustdoc, tests, changelogs, and the editable website source to fill in details.
4. When sources disagree, compile against the target dependency instead of silently upgrading it.

Read [source selection](references/source-selection.md) when choosing documentation, resolving a version mismatch, or diagnosing the toolchain.

## Work within the app repository

- Read applicable repository instructions before editing.
- Inspect the worktree, workspace members, manifests, and lockfile without disturbing unrelated changes.
- Determine whether the app is already scaffolded or belongs to a larger Cargo workspace.
- Preserve the existing structure unless the task explicitly calls for scaffolding or migration.
- Keep `src/lib.rs` focused on the `#[whisker::main]` entry point and top-level composition as the app grows.
- Put domain models, commands, serialization, and repositories in ordinary Rust modules that can be tested without a simulator.
- Keep app identity and platform or plugin configuration in `whisker.rs`.
- Do not hand-edit or commit `gen/`; `whisker run` can regenerate it.

## Apply Whisker's reactive model

- Assume component bodies run once at mount. Put mutable UI state in signals.
- Pass signal handles or tracked closures for live attributes. A plain `.get()` passed during mount is a snapshot.
- Derive values with `computed` instead of synchronizing duplicate writable state.
- Mutate collections with `update`.
- Render short collections with `ForEach` and stable unique keys. A retained key preserves its child and does not rerun `children(item)`, so mutable rows should read current state reactively by ID.
- Use the virtualized `list` element for large scrolling collections.
- Use `Show` for reactive branch mounting and disposal.
- Keep signal reads and writes on Whisker's UI thread.
- Set flex direction explicitly; Lynx defaults it to row.
- Add accessibility labels and traits to interactive elements.

Read [application patterns](references/app-patterns.md) when implementing components, input, lists, persistence, async work, modules, or plugins.

## Handle native capability and IO

- Confirm that each first-party package exists in the app's target version before adding it.
- Use `whisker-local-store` for small string values. Serialize structured values explicitly and surface bridge or decoding errors.
- Do not block the UI thread. Use `run_blocking` for synchronous filesystem, database, CPU, or HTTP work.
- Use `spawn_local` or `resource` for async flows. When the `tokio` feature is enabled, follow the current Tokio example rather than adding a second runtime.
- Add native capability through a module and native project configuration through a plugin registered in `whisker.rs`.
- Register only the permissions and platform configuration the app needs.

## Validate the change

Run the narrowest relevant checks first:

```sh
cargo check --workspace --all-targets
cargo test --workspace --all-targets
whisker fmt --check <changed-rust-files>
whisker doctor
```

- Include every changed Rust file in `whisker fmt`; `cargo fmt` does not format Whisker macro bodies.
- Run `whisker run ios` or `whisker run android` for UI or native changes and inspect the app and logs.
- Restart the development loop after dependency, function signature, `whisker.rs`, or native configuration changes. Hot reload alone does not prove a clean launch.
- If platform execution is unavailable, complete pure Rust checks and state the exact SDK, simulator, emulator, signing, or device gap.
- Record framework defects in the upstream issue tracker only after separating them from application code and reducing them to a reproducible case.
