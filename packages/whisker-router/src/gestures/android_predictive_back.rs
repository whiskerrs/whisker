//! [`AndroidPredictiveBack`] — Android system back gesture for
//! [`StackLayout`](crate::StackLayout).
//!
//! Mount as a child of the layout. Subscribes to the
//! `whisker-router:PredictiveBack` native module's `backInvoked`
//! event — the Kotlin side registers an
//! `OnBackPressedDispatcher.OnBackPressedCallback` against the host
//! Activity. On press, calls [`StackLayoutHandle::back`] to pop the
//! current entry; the layout's natural route-change effect plays the
//! backward slide.
//!
//! ```ignore
//! StackLayout(transition: IosSlide::default(), render: render.into()) {
//!     AndroidPredictiveBack()
//! }
//! ```
//!
//! Mounting takes over the system back press while the component is
//! alive. Unmounting (e.g. when the layout tears down) releases the
//! callback so the platform's normal back behaviour (finish the
//! Activity) resumes.
//!
//! Unlike [`IosSwipeBack`](crate::IosSwipeBack) this component does
//! **not** use `commit_preview_and_back`: a predictive-back press is
//! a single discrete event with no interactive drag, so there's no
//! preview wrapper to promote — the natural route-change effect's
//! built-in backward animation is the right path.
//!
//! ## What's not here yet
//!
//! Only the commit event is wired today — no interactive preview
//! during the drag. API 34+'s `BackEventCompat` progress / cancel
//! callbacks would let the layout pose the outgoing wrapper as the
//! user drags. Adding it means declaring more events
//! (`backProgressed`, `backCancelled`, `backStarted`) in the Kotlin
//! module and routing them through
//! [`StackLayoutHandle`]'s preview API the same way
//! [`IosSwipeBack`](crate::IosSwipeBack) does.
//!
//! [`StackLayoutHandle`]: crate::StackLayoutHandle

use std::rc::Rc;

use whisker::module;
use whisker::runtime::reactive::owner::on_cleanup;
use whisker::{component, render, use_context};

use crate::layouts::stack::StackLayoutHandle;
use whisker::runtime::view::Element;

/// Android predictive-back gesture component for
/// [`StackLayout`](crate::StackLayout).
///
/// Renders no DOM of its own; reads the
/// [`StackLayoutHandle`](crate::StackLayoutHandle) from context and
/// subscribes to the `whisker-router:PredictiveBack` module's
/// `backInvoked` event. See the [module docs](self) for the
/// user-facing summary.
#[component]
pub fn android_predictive_back() -> Element {
    let handle = use_context::<StackLayoutHandle>()
        .expect("AndroidPredictiveBack must be a child of StackLayout");
    let back: Rc<dyn Fn()> = handle.back.clone();

    // The bridge stores callbacks in a Send + Sync box (the C side
    // can fire from any thread that calls `module_send_event`). For
    // predictive back the Kotlin sender is always the
    // `OnBackPressedDispatcher` callback on the UI thread — same
    // thread as Whisker's main loop. `MainThreadOnly` papers over
    // the type bound without a per-callback marshal hop.
    let holder = MainThreadOnly { inner: back };

    let module = module!("PredictiveBack");
    let sub = module.on_event("backInvoked", move |_payload| {
        // Bind `holder` (not `holder.inner`) so Rust 2021 disjoint
        // closure captures move the wrapper as a whole, carrying its
        // Send + Sync impls. Capturing `.inner: Rc<...>` would
        // re-introduce the !Sync error.
        let h = &holder;
        (h.inner)();
    });

    if let Some(err) = sub.error() {
        eprintln!("[whisker-router] AndroidPredictiveBack failed to subscribe: {err}");
    }

    // Subscription drop runs `module_remove_event_listener` inside
    // its Drop impl → Kotlin OnStopObserving fires → host Activity
    // releases the back dispatcher callback.
    on_cleanup(move || drop(sub));

    render! { fragment() }
}

// Asserts main-thread-only access to `inner`. The unsafe Send + Sync
// is bounded by the gesture's sole event source — the Kotlin
// OnBackPressedCallback fires on the UI thread, same thread as the
// Whisker main loop. Lives here until the bridge gains a proper
// main-thread-only listener API; then this shim goes away.
struct MainThreadOnly<T> {
    inner: T,
}
// Safety: see the type-level comment. Never expose this beyond the
// gesture module — moving the inner value across threads breaks
// the Rc invariant.
unsafe impl<T> Send for MainThreadOnly<T> {}
unsafe impl<T> Sync for MainThreadOnly<T> {}
