# podcast — pending router migration (excluded from the workspace)

This example is **temporarily excluded from the Cargo workspace** (see the
`[workspace] exclude` entry in the repo-root `Cargo.toml`). Its files are
kept intact; it is just not built/tested as part of `cargo build --workspace`.

## Why

`examples/podcast` (and its `crates/podcast-*` feature crates) depend
broadly on the **old `whisker-router` API** — `RouteStack` / `route_stack`,
`StackLayout` / `TabsLayout` / `ModalLayout` / `Pane`, `IosSwipeBack` /
`AndroidPredictiveBack`, `RouteProvider` / `Outlet`, the `StackTransition`
trait + built-ins, and the `#[route]` enum macro.

That entire signal-stack router was **removed in phase 4**. The crate now
exposes only:

- `whisker_router::core` — the `RouteTree` / `RouteState` graphs +
  `Navigator` (phase 1), and
- `whisker_router::render` — the reactive `Router` / `Outlet` / `Stack` /
  `Switch` / `Tabs` / `SwipeBack` render layer + `RouteRegistry` (phase 2).

## What migration needs

The podcast app should be re-expressed against the new router. The clean
path is to wait for the **`routes!` macro (phase 3)** — which will generate
the `RouteTree` + the id→component `RouteRegistry` + typed nav targets —
and then port each feature crate's screens onto `Router` + `Outlet` /
`Tabs`, replacing `RouteStack.push/back` calls with
`use_navigator().navigate(..)` / `.back()`.

See `examples/router-smoke` for a minimal, hand-wired (no-macro) example of
the new API, and `docs/router-design.md` for the model.

## To build it locally meanwhile

It still compiles against the *old* crate state, but not against `main`.
Re-including it in the workspace will fail until the migration is done.
