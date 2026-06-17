//! `whisker-router` `StackLayout` cross-crate crash repro.
//!
//! ## Findings (May 2026 bisection)
//!
//! Wiring a cross-crate `#[component]` through `StackLayout`'s
//! `RouteRenderFn` panics inside the first `tick_callback` IFF that
//! component creates a [`computed`] whose body reads a signal
//! returned by [`whisker_safe_area::safe_area_insets`]. The same
//! component mounted directly under `page` (no `StackLayout`)
//! renders without issue.
//!
//! Cross-product (Android emulator, `v3.8.0-whisker.1`):
//!
//! | screen body                                         | StackLayout | direct mount |
//! |-----------------------------------------------------|-------------|--------------|
//! | text only                                           | ✓ renders   | ✓ renders    |
//! | nested `#[component]` only                          | ✓ renders   | ✓ renders    |
//! | nested + `use_context::<Rc<dyn Fn>>` + ctx provider | ✓ renders   | n/a          |
//! | + `safe_area_insets()` (no `.get()`)                | ✓ renders   | ✓ renders    |
//! | + `safe_area_insets()` + `computed(insets.get())`   | ✗ aborts    | ✓ renders    |
//!
//! Panic stack (Android, abridged):
//! ```text
//!   panic_cannot_unwind
//!     whisker_driver::lynx::bootstrap::tick_callback
//!     lynx_shell_run_on_tasm_thread
//!     whisker_bridge_dispatch
//!     whisker_tick
//!     Java_rs_whisker_runtime_WhiskerView_nativeTick
//! ```
//!
//! `panic_cannot_unwind` means a Rust panic walked into an
//! `extern "C"` boundary (`tick_callback`) and was forced to abort.
//! The underlying Rust panic message doesn't reach `logcat`
//! (`std::panic::set_hook` apparently isn't wired to the Android
//! log) — we narrowed the trigger by bisection rather than by
//! reading the panic text.
//!
//! ## Root cause (confirmed June 2026) + fix
//!
//! Not the `computed` timing — the **handle ownership**.
//! `safe_area_insets()` used to convert its process-global
//! `ArcReadSignal` into an arena `ReadSignal` (`.into()`) **on every
//! call**, minting a fresh arena entry in *whichever owner was current
//! at call time*. Under `StackLayout` that owner is the per-route /
//! per-component scope. When a later read of that handle outlives the
//! scope — a surviving `computed`/effect, or a native `insetsChanged`
//! event after the scope disposed — `ReadSignal`'s `fetch_value` hits
//! `expect("signal disposed …")`, and the panic aborts as
//! `panic_cannot_unwind` at the `tick_callback` FFI boundary. The
//! `computed(insets.get())` row in the table is just the smallest body
//! that retains a handle long enough to be read after disposal; plain
//! `safe_area_insets()` with no read never trips it. The direct mount
//! never disposes its owner, so it never trips it either.
//!
//! The mechanism is pinned by two whisker-runtime unit tests:
//! `reading_arc_backed_arena_signal_after_owner_dispose_panics`
//! (reproduces the abort) and
//! `arc_backed_arena_signal_under_detached_root_survives_sibling_dispose`
//! (proves the fix).
//!
//! **Fix (shipped).** `safe_area_insets()` now mints its arena handle
//! **once**, under a never-disposed [`whisker::Owner::detached_root`],
//! and caches it — every call returns the same `Copy` handle, pinned
//! to a process-lifetime root instead of a transient scope. See
//! `packages/whisker-safe-area/src/lib.rs`. This repro is kept as a
//! living regression: with the fix, `MOUNT_VIA_STACK_LAYOUT = true`
//! renders without aborting.
//!
//! ## Switching the repro
//!
//! Edit `MOUNT_VIA_STACK_LAYOUT` below to flip between the crashing
//! `StackLayout` mount and the working direct mount. The body of
//! `HomeScreen` in `crates/router-example-feature-home/` is the
//! safe-area + computed pattern that triggers the bug; reducing
//! that body to plain text makes both modes work.

use router_example_feature_detail::DetailScreen;
use router_example_feature_home::HomeScreen;
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_router::{
    route, route_stack, AndroidPredictiveBack, IosSwipeBack, RouteProvider, RouteRenderFn,
    StackLayout,
};

/// Flip to `false` to mount `HomeScreen` directly under `page`
/// (the working control case). Keep `true` to reproduce the crash.
const MOUNT_VIA_STACK_LAYOUT: bool = true;

#[route]
#[derive(Clone, Debug, PartialEq)]
pub enum AppRoute {
    #[at("/")]
    Home,
    #[at("/detail")]
    Detail,
}

#[whisker::main]
pub fn render_app() -> Element {
    if MOUNT_VIA_STACK_LAYOUT {
        render_stacked()
    } else {
        render_direct()
    }
}

fn render_stacked() -> Element {
    let stack = route_stack(AppRoute::Home);
    let render: RouteRenderFn<AppRoute> = (|r: AppRoute| match r {
        AppRoute::Home => render! { HomeScreen() },
        AppRoute::Detail => render! { DetailScreen() },
    })
    .into();
    render! {
        view(
            style: "flex-grow: 1; width: 100vw; height: 100vh; background-color: white; \
                    display: flex; flex-direction: column;",
        ) {
            RouteProvider(stack: stack) {
                StackLayout(render: render.clone()) {
                    IosSwipeBack()
                    AndroidPredictiveBack()
                }
            }
        }
    }
}

fn render_direct() -> Element {
    render! {
        view(
            style: "flex-grow: 1; width: 100vw; height: 100vh; background-color: white; \
                    display: flex; flex-direction: column;",
        ) {
            HomeScreen()
        }
    }
}
