---
name: whisker
description: Build, run, debug and ship Whisker apps (Rust UI rendered with native views on iOS, Android, Web and desktop). Use when the project has a `whisker.rs` at the crate root, a `whisker` dependency in `Cargo.toml`, a `#[whisker::main]` function, or uses `render!`, `css!` or `#[component]` from whisker; or when the user asks to create a Whisker app. Do NOT use for Dioxus, Leptos, Yew, Tauri or plain Lynx projects even though the syntax looks similar.
---

# Whisker

Whisker is a Rust UI framework: an app is an ordinary Rust crate with a
`#[whisker::main]` function, `#[component]` functions built with `render!`,
fine-grained signals, and a `whisker.rs` configuration file. The `whisker`
CLI generates the native projects and runs the dev loop.

## Before writing code

1. **Find the version.** Read the `whisker` version from `Cargo.lock` (or
   `Cargo.toml`). The public API is still pre-1.0 and changes between
   releases; when this skill or the website disagrees with that version's
   Rustdoc at `https://docs.rs/whisker/<version>`, the Rustdoc wins.
2. **Do not guess the API from other frameworks.** `render!` looks like
   Dioxus/Leptos/React but its rules differ. Look names up in the Rustdoc or
   the pages linked below instead of relying on training data.
3. **In an existing project, follow its structure.** Check `src/lib.rs`,
   `whisker.rs` and `Cargo.toml` before adding files.

## The model in four rules

- **A component function runs once, when it mounts.** It is not re-run on
  state changes; only the bindings that read a changed signal update.
- **Pass a signal handle to stay reactive; `.get()` is a one-time snapshot.**
  `Text(value: computed(move || format!("{}", count.get())))` updates;
  `Text(value: format!("{}", count.get()))` never does.
- **Signals are read and written through methods** (`.get()`, `.with()`,
  `.set()`, `.update()`), never called like functions. Handles are `Copy`
  and move freely into `move` closures.
- **Hot reload remounts the components you edited**, resetting their own
  signals. Keep state in a component above the ones being edited, and keep
  `app()` and the root component small: editing them rebuilds the whole UI.

## Starting a new app

```bash
cargo install whisker-cli
whisker new my-app && cd my-app
whisker doctor            # reports missing Rust targets, SDKs and Xcode
whisker run ios           # or: android, web, desktop (positional, not --ios)
```

Start from the generated counter in `src/lib.rs` rather than writing the
entry point from scratch.

## Where to read for each task

Every docs page is served as Markdown at its URL plus `.md`; https://whisker.rs/llms.txt
lists all of them.

| Task | Read |
| --- | --- |
| Components, props, children, `render!` syntax | https://whisker.rs/docs/components.md |
| Styles and layout with `css!` | https://whisker.rs/docs/styling.md |
| Signals, `computed`, effects | https://whisker.rs/docs/state-management.md, https://whisker.rs/docs/reactivity.md |
| `Show`, `ForEach`, virtualized `List` | https://whisker.rs/docs/lists-and-conditionals.md |
| Events and handlers | https://whisker.rs/docs/handling-events.md |
| Context (`provide_context` / `use_context`) | https://whisker.rs/docs/context.md |
| Async work and `resource` | https://whisker.rs/docs/async-and-data.md |
| Navigation with `whisker-router` | https://whisker.rs/docs/routing.md |
| Using platform modules (image, input, keyboard, ...) | https://whisker.rs/docs/using-modules.md |
| App configuration in `whisker.rs` | https://whisker.rs/docs/app-configuration.md |
| Hot reload behavior and its limits | https://whisker.rs/docs/hot-reload.md |
| Logs, panics, environment problems | https://whisker.rs/docs/debugging.md |
| Release builds and store submission | https://whisker.rs/docs/production-builds.md, https://whisker.rs/docs/store-submission.md |
| Every CLI command and flag | https://whisker.rs/docs/cli-reference.md |

## Verify your work

There is no headless UI test runner yet, so verify in this order and report
any step the environment could not run:

1. `cargo check` for compile errors.
2. `whisker fmt` to format Rust and the bodies of `render!` / `css!`, which
   plain `rustfmt` leaves untouched.
3. `whisker run <target>` on a simulator, emulator or browser, and check the
   screen you changed. Read the run terminal: a panic in an event handler is
   logged there and the app keeps running, so a tap that does nothing is
   often a logged panic.

Changes to `Cargo.toml` or `whisker.rs` are not hot-patched; they trigger a
cold rebuild, and `whisker.rs` changes need a `whisker run` restart.
