//! [`AndroidPredictiveBack`] — Android system back gesture for
//! [`StackLayout`](crate::StackLayout).
//!
//! Mount as a child of the layout. Subscribes to the
//! `whisker-router:PredictiveBack` native module's `backInvoked`
//! event (Kotlin side: registers an
//! `OnBackPressedDispatcher.OnBackPressedCallback` against the host
//! Activity). On commit, calls [`StackLayoutHandle::commit_preview_and_back`]
//! to pop the current entry — same hook the iOS swipe gesture
//! lands on.
//!
//! ```ignore
//! StackLayout(transition: IosSlide::default(), render: render) {
//!     AndroidPredictiveBack()
//! }
//! ```
//!
//! Mounting the component while the host Activity has no other back
//! consumer takes over the system back press. Unmounting (e.g. when
//! the user pops back to the root of the stack and the component
//! tears down with the layout) releases it, so the platform's normal
//! back behaviour (finish the Activity) resumes.
//!
//! ## What's NOT here (yet)
//!
//! The current implementation only listens for the commit event —
//! no interactive preview during the drag. API 34+'s
//! `BackEventCompat` progress / cancel callbacks would let the layout
//! pose the outgoing wrapper as the user drags. Adding it means
//! declaring more events (`backProgressed`, `backCancelled`,
//! `backStarted`) in the Kotlin module and routing them through
//! `StackLayoutHandle`'s preview API the same way
//! [`IosSwipeBack`](crate::IosSwipeBack) does. Out of scope for the
//! first cut — the commit-only path already drives the layout
//! transition correctly.

use std::rc::Rc;

use whisker::module;
use whisker::runtime::reactive::owner::on_cleanup;
use whisker::{component, render, use_context};

use crate::layouts::stack::StackLayoutHandle;
use whisker::runtime::view::Element;

/// Android predictive-back gesture component.
///
/// Mount as a child of [`StackLayout`](crate::StackLayout); reads
/// the layout handle from context and subscribes to the
/// `whisker-router:PredictiveBack` module's `backInvoked` event in
/// the component body. Renders no DOM of its own.
#[component]
pub fn android_predictive_back() -> Element {
    let handle = use_context::<StackLayoutHandle>()
        .expect("AndroidPredictiveBack must be a child of StackLayout");
    let commit_back: Rc<dyn Fn()> = handle.commit_preview_and_back.clone();

    // The bridge stores callbacks in a Send + Sync box (the C side
    // can fire from any thread that calls `module_send_event`). For
    // predictive back the Kotlin sender is always the
    // `OnBackPressedDispatcher` callback, which runs on the UI
    // thread — same thread as Whisker's main loop — so the Rc<dyn
    // Fn> is in practice only touched from one thread.
    //
    // To satisfy the type system without paying a per-callback
    // marshal hop, we wrap the Rc in [`MainThreadOnly`]. Soundness
    // rests on: every `sendEvent("backInvoked")` originates from
    // the Kotlin OnBackPressedCallback, which is main-thread.
    let holder = MainThreadOnly { inner: commit_back };

    let module = module!("PredictiveBack");
    let sub = module.on_event("backInvoked", move |_payload| {
        // Bind `holder` (not `holder.inner`) so Rust 2021 disjoint
        // closure captures take the wrapper as a whole — moving the
        // wrapper's `Send + Sync` impls onto the closure. Capturing
        // only `inner: Rc<...>` would re-introduce the !Sync error.
        let h = &holder;
        (h.inner)();
    });

    if let Some(err) = sub.error() {
        eprintln!("[whisker-router] AndroidPredictiveBack failed to subscribe: {err}");
    }

    // Drop the subscription when the component's owner is disposed
    // (unmount). The bridge's `module_remove_event_listener` runs
    // inside `Drop`, so the Kotlin OnStopObserving fires and the
    // host Activity's back dispatcher releases the callback.
    on_cleanup(move || drop(sub));

    render! { fragment() }
}

/// Locally-scoped wrapper that asserts main-thread-only access to
/// `inner`. The unsafe `Send + Sync` is bounded by the closure
/// callsite: `AndroidPredictiveBack`'s sole event source is the
/// Kotlin `OnBackPressedCallback`, which fires on the UI thread.
///
/// Lives here (not in `whisker-runtime`) until the bridge gains a
/// proper main-thread-only listener API. When that lands, this
/// shim goes away.
struct MainThreadOnly<T> {
    inner: T,
}
// Safety: see the type-level comment. Never expose this beyond the
// gesture module — moving the inner value across threads would
// break the Rc invariant.
unsafe impl<T> Send for MainThreadOnly<T> {}
unsafe impl<T> Sync for MainThreadOnly<T> {}
