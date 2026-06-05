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
//! ## Likely cause (hypothesis to verify)
//!
//! `safe_area_insets()` allocates a process-global signal lazily on
//! first call (`OnceLock::get_or_init`). When that first call lands
//! inside `StackLayout`'s `Owner::new(None).with(|| …)`
//! body, the signal becomes owned by the per-route owner. The
//! Android side of the module fires `OnStartObserving` synchronously
//! and `sendEvent("insetsChanged", …)` lands during the same tick
//! — the resulting `WriteSignal::set` interacts badly with the
//! still-mounting `computed` whose `f()` is about to register a
//! subscription on the same signal under a different owner.
//!
//! Two reasonable fixes to try (work for both):
//!
//! 1. Allocate the safe-area signal at app startup (in the engine's
//!    bootstrap before the first `tick_callback`), so the signal's
//!    owner is the root and the per-route owner never gets a fresh
//!    allocation tied to a soon-to-be-disposed scope.
//! 2. Have `computed(...)` defer its first `f()` call to the
//!    scheduler's next batch instead of running it inline at
//!    construction time, so the initial read happens under
//!    consistent owner / tracker state.
//!
//! ## Switching the repro
//!
//! Edit `MOUNT_VIA_STACK_LAYOUT` below to flip between the crashing
//! `StackLayout` mount and the working direct mount. The body of
//! `HomeScreen` in `crates/router-example-feature-home/` is the
//! safe-area + computed pattern that triggers the bug; reducing
//! that body to plain text makes both modes work.

use router_example_feature_detail::{DetailScreen, DetailScreenProps};
use router_example_feature_home::{HomeScreen, HomeScreenProps};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_router::{
    route, route_stack, AndroidPredictiveBack, AndroidPredictiveBackProps, IosSwipeBack,
    IosSwipeBackProps, RouteProvider, RouteProviderProps, RouteRenderFn, StackLayout,
    StackLayoutProps,
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
        page(
            style: "width: 100vw; height: 100vh; background-color: white; \
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
        page(
            style: "width: 100vw; height: 100vh; background-color: white; \
                    display: flex; flex-direction: column;",
        ) {
            HomeScreen()
        }
    }
}
