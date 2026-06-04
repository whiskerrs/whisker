//! `whisker-safe-area` — reactive accessor for the host view's
//! safe-area insets.
//!
//! ## Usage
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_safe_area::safe_area_insets;
//!
//! #[component]
//! fn screen() -> Element {
//!     let insets = safe_area_insets();
//!     let outer_style = move || {
//!         let i = insets.get();
//!         format!(
//!             "padding-top: {}px; padding-bottom: {}px; \
//!              padding-left: {}px; padding-right: {}px;",
//!             i.top, i.bottom, i.leading, i.trailing,
//!         )
//!     };
//!     render! {
//!         view(style: outer_style()) {
//!             // ...
//!         }
//!     }
//! }
//! ```
//!
//! ## Platform behaviour
//!
//! - **iOS**: reads `UIView.safeAreaInsets` from the host
//!   `WhiskerView`. Re-fires on `safeAreaInsetsDidChange()`
//!   (rotation, multitasking, notch / Dynamic Island, home indicator).
//! - **Android**: reads
//!   `WindowInsetsCompat.getInsets(systemBars() | displayCutout())`
//!   from the host Activity's decor view. Re-fires through
//!   `OnApplyWindowInsetsListener`, and the module re-installs the
//!   listener on each new host attach so a config-change Activity
//!   recreation (the default behaviour without
//!   `android:configChanges="orientation|screenSize"` on the manifest)
//!   transparently rewires.
//!
//! Both platforms report values that map 1:1 to padding on the host
//! `WhiskerView`. Whisker's `WhiskerActivity` enforces Android edge-
//! to-edge (`setDecorFitsSystemWindows(false)` +
//! `isNavigationBarContrastEnforced = false`), so the WhiskerView
//! always fills the entire window and `padding-top: insets.top`
//! applied to a child of WhiskerView lines up exactly with the
//! status bar's bottom edge. No double-padding caveat to track —
//! same semantics as iOS.
//!
//! ## Single source of truth
//!
//! Every `safe_area_insets()` call hands back a `ReadSignal` cloned
//! from one global `RwSignal<SafeAreaInsets>`. The first call kicks
//! off the native-event subscription; subsequent calls are free.
//! Values stay live for the entire process — the module never
//! unsubscribes.

use std::sync::OnceLock;

use whisker::module;
use whisker::runtime::reactive::with_detached_owner;
use whisker::{signal, ReadSignal, WhiskerValue, WriteSignal};

/// Safe-area inset amounts in **points (iOS) / dp (Android)** — the
/// same density-independent units that the rest of Whisker's CSS
/// pipeline uses for `px` literals.
///
/// `leading` / `trailing` follow the
/// `NSDirectionalEdgeInsets`-style RTL-aware convention. Whisker
/// itself doesn't formally support RTL yet, so for LTR-only layouts
/// `leading == left` and `trailing == right` — read them as such
/// when composing CSS `padding-left` / `padding-right`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SafeAreaInsets {
    pub top: f64,
    pub leading: f64,
    pub trailing: f64,
    pub bottom: f64,
}

/// Reactive accessor for the current safe-area insets.
///
/// All calls share one underlying signal pair, so reading from many
/// components or effects is cheap (no extra subscription, no extra
/// cross-platform round-trip). The signal starts at
/// `SafeAreaInsets::default()` and updates as soon as the native side
/// can push the host view's current values.
///
/// **Must be called from the main thread.** The underlying reactive
/// runtime is thread-local; calling from a worker thread would either
/// allocate a fresh detached signal (silently broken) or panic.
pub fn safe_area_insets() -> ReadSignal<SafeAreaInsets> {
    install();
    SLOT.get().expect("install() ran above").inner
}

// ---- Internals -------------------------------------------------------------

/// One-shot install of the global signal + the native subscription.
/// Idempotent — re-entry on subsequent `safe_area_insets()` calls is
/// a single `OnceLock::get()` check.
///
/// The `signal(...)` call is wrapped in [`with_detached_owner`] so
/// the underlying `RwSignal` lives in a process-global detached
/// owner instead of whichever route-mount owner happens to be
/// current at the first call. Without this, the signal node gets
/// freed the first time that route is popped — and the next caller
/// who pulls the cached `ReadSignal` out of [`SLOT`] reads a dead
/// `NodeId` (`expect("ReadSignal: signal disposed …")` panics across
/// `tick_callback`'s `extern "C"` boundary as a
/// `panic_cannot_unwind` abort). The detached owner this allocation
/// pushes is never disposed — process-global state is allowed to
/// "leak", because the signal needs to outlive every route.
fn install() {
    SLOT.get_or_init(|| {
        with_detached_owner(|| {
            let (read, write) = signal(SafeAreaInsets::default());
            // The bridge's `on_event` callback may fire from any thread
            // depending on the host; the `WriteSignal` is `!Send`. We
            // assert main-thread-only via [`MainThreadOnly`] and trust
            // the platform contract (iOS posts from `safeAreaInsetsDidChange`
            // on UI thread; Android posts from `OnApplyWindowInsetsListener`
            // on UI thread). If a future host changes that, swap the
            // closure body to `run_on_main_thread` first.
            subscribe_to_native(MainThreadOnly { inner: write });
            MainThreadOnly { inner: read }
        })
    });
}

/// Wire the global signal to the native module's `insetsChanged`
/// event. The returned `ModuleSubscription` is intentionally leaked
/// — the signal lives for the process lifetime, so the listener
/// should too. Letting the subscription `Drop` would also drop the
/// underlying closure pointer the bridge holds.
fn subscribe_to_native(writer: MainThreadOnly<WriteSignal<SafeAreaInsets>>) {
    let module = module!("SafeArea");
    let sub = module.on_event("insetsChanged", move |payload| {
        if let Some(insets) = decode_payload(payload) {
            // Bind the wrapper (not `.inner`) so Rust 2021 disjoint
            // captures move the `Send + Sync` impl as a whole.
            let w = &writer;
            w.inner.set(insets);
        }
    });
    if let Some(err) = sub.error() {
        eprintln!("[whisker-safe-area] failed to subscribe: {err}");
    }
    // Leak — see fn doc.
    std::mem::forget(sub);
}

/// Decode a `{ top, leading, trailing, bottom }` map payload into the
/// typed struct. Missing or non-numeric keys default to `0.0` — a
/// malformed message degrades silently rather than wedging the
/// subscription.
fn decode_payload(value: WhiskerValue) -> Option<SafeAreaInsets> {
    let WhiskerValue::Map(fields) = value else {
        return None;
    };
    let f = |k: &str| -> f64 {
        match fields.get(k) {
            Some(WhiskerValue::Float(v)) => *v,
            Some(WhiskerValue::Int(v)) => *v as f64,
            _ => 0.0,
        }
    };
    Some(SafeAreaInsets {
        top: f("top"),
        leading: f("leading"),
        trailing: f("trailing"),
        bottom: f("bottom"),
    })
}

/// Static slot for the global signal pair + the subscription closure
/// wrapper. `OnceLock` requires `Sync`; the inner value is `!Send` /
/// `!Sync` because `ReadSignal` / `WriteSignal` are thread-local
/// handles. We wrap in [`MainThreadOnly`] to satisfy the bound by
/// asserting main-thread-only access; the contract is that
/// `safe_area_insets()` callers run on the main thread (the reactive
/// runtime constraint already requires that anyway).
static SLOT: OnceLock<MainThreadOnly<ReadSignal<SafeAreaInsets>>> = OnceLock::new();

/// Locally-scoped wrapper asserting main-thread-only access to
/// `inner`. Used twice here: once for the static slot (so the
/// `OnceLock<…>` static-Sync bound is satisfied), once for the
/// `on_event` closure capture (so the `Send + Sync` bound on the
/// bridge's callback is satisfied).
///
/// Same pattern `whisker-router::AndroidPredictiveBack` uses for its
/// `Rc<dyn Fn()>` capture — see the comment there for the soundness
/// argument. Lives here (not in `whisker-runtime`) until the bridge
/// gains a proper main-thread-only listener API.
#[derive(Copy, Clone)]
struct MainThreadOnly<T> {
    inner: T,
}
// Safety: every access path (signal read in `safe_area_insets`,
// signal write in the `on_event` callback) is documented to run on
// the Lynx TASM thread (= Whisker main thread). Misuse would
// corrupt the reactive arena, but that's the same risk as calling
// any signal API from a worker thread directly.
unsafe impl<T> Send for MainThreadOnly<T> {}
unsafe impl<T> Sync for MainThreadOnly<T> {}
