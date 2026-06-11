//! `whisker-safe-area` — reactive accessor for the host view's
//! safe-area insets.
//!
//! **API shape — 4 (Free fn → signal).** See
//! [`docs/module-api-design.md`](https://github.com/whiskerrs/whisker/blob/main/docs/module-api-design.md)
//! §"Shape 4". A singleton observable: [`safe_area_insets`] returns
//! a process-global `ReadSignal<SafeAreaInsets>`, lazily wired to
//! the native event on first call.
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
//!
//! ## Native source
//!
//! Contributors: the matching platform module lives at
//!
//! - iOS: `packages/whisker-safe-area/ios/Sources/WhiskerSafeArea/SafeAreaModule.swift`
//! - Android: `packages/whisker-safe-area/android/src/main/kotlin/rs/whisker/modules/safe_area/SafeAreaModule.kt`

use std::sync::OnceLock;

use whisker::module;
use whisker::{ArcRwSignal, ArcWriteSignal, Owner, ReadSignal, WhiskerValue};

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
/// All calls share one underlying signal: the value lives in a
/// process-global [`ArcRwSignal`] stashed in [`SLOT`], so reading
/// from many components or effects is cheap (no extra subscription,
/// no extra cross-platform round-trip). The signal starts at
/// `SafeAreaInsets::default()` and updates as soon as the native side
/// can push the host view's current values.
///
/// The returned handle is a `Copy` [`ReadSignal`]. Crucially it is
/// minted **once**, under a process-lifetime
/// [`Owner::detached_root`], and cached in [`SLOT`] — every call hands
/// back the *same* arena entry. An earlier design converted the
/// underlying `ArcReadSignal` on every call, minting a fresh arena
/// entry in *whichever owner was current*; when that owner was a
/// short-lived per-route / per-component scope (e.g. under
/// `whisker_router::StackLayout`), disposing it freed the entry, and a
/// surviving reader (a `computed`/effect, or a late native event)
/// would then read a disposed node and panic — aborting at the FFI
/// tick boundary. Pinning the entry to a never-disposed root removes
/// that footgun entirely.
///
/// **Must be called from the main thread.** The underlying reactive
/// runtime is thread-local.
pub fn safe_area_insets() -> ReadSignal<SafeAreaInsets> {
    install();
    SLOT.get().expect("install() ran above").read.inner
}

// ---- Internals -------------------------------------------------------------

// Slot contents. `read` is the cached `Copy` arena handle minted
// once under a detached root owner (see `install`) — what
// `safe_area_insets()` hands out. The write half stays here so the
// native event callback can `set()` through it.
struct Slot {
    read: MainThreadOnly<ReadSignal<SafeAreaInsets>>,
    // Kept reachable from the on-event closure; never read here.
    #[allow(dead_code)]
    write: MainThreadOnly<ArcWriteSignal<SafeAreaInsets>>,
}

/// One-shot install of the global signal + the native subscription.
/// Idempotent — re-entry is a single `OnceLock::get()` check.
///
/// The signal is allocated as an [`ArcRwSignal`] so its lifetime is
/// governed by [`SLOT`]'s Arc strong count rather than the caller's
/// owner; the owner that triggered the first call can come and go
/// without affecting subsequent reads.
///
/// The `Copy` arena handle that callers receive is also minted here,
/// **once**, under an [`Owner::detached_root`] that we deliberately
/// leak (never dispose). Pinning it to a process-lifetime root — not
/// the owner that happens to be current on first call — is what keeps
/// `safe_area_insets()` from handing out a handle that a transient
/// scope can free out from under a surviving reader. See the
/// `safe_area_insets` doc for the failure mode this avoids.
fn install() {
    SLOT.get_or_init(|| {
        let (read, write) = ArcRwSignal::new(SafeAreaInsets::default()).split();
        // `ArcWriteSignal` is `!Send`; both platforms post their
        // events from the UI thread (iOS `safeAreaInsetsDidChange`,
        // Android `OnApplyWindowInsetsListener`), so wrap in
        // `MainThreadOnly` and trust the contract. If a future host
        // breaks it, route through `run_on_main_thread` first.
        subscribe_to_native(MainThreadOnly {
            inner: write.clone(),
        });
        // Mint the shared arena handle under a never-disposed root so
        // it outlives every per-route / per-component owner.
        let root = Owner::detached_root();
        let read_handle: ReadSignal<SafeAreaInsets> = root.with(|| read.into());
        Slot {
            read: MainThreadOnly { inner: read_handle },
            write: MainThreadOnly { inner: write },
        }
    });
}

/// Wire the global signal to the native module's `insetsChanged`
/// event. The returned `ModuleSubscription` is intentionally leaked
/// — the signal lives for the process lifetime; dropping the
/// subscription would also drop the closure the bridge holds.
fn subscribe_to_native(writer: MainThreadOnly<ArcWriteSignal<SafeAreaInsets>>) {
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
    std::mem::forget(sub);
}

// Decode a `{ top, leading, trailing, bottom }` map payload. Missing
// or non-numeric keys default to `0.0` — a malformed message
// degrades silently rather than wedging the subscription.
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

// `OnceLock<T>` requires `T: Sync`; the inner `ArcReadSignal` /
// `ArcWriteSignal` are `!Send` / `!Sync` because the underlying
// `Rc` is thread-local. `MainThreadOnly` asserts the contract
// rather than enforcing it.
static SLOT: OnceLock<Slot> = OnceLock::new();

/// Locally-scoped wrapper asserting main-thread-only access to
/// `inner`. Used twice: once for the static slot
/// (`OnceLock<…>: Sync`), once for the `on_event` closure capture
/// (bridge callback's `Send + Sync`). Mirrors the pattern
/// `whisker-router::AndroidPredictiveBack` uses for its
/// `Rc<dyn Fn()>` capture. Lives here (not in `whisker-runtime`)
/// until the bridge gains a proper main-thread-only listener API.
#[derive(Copy, Clone)]
struct MainThreadOnly<T> {
    inner: T,
}
// Safety: every access path (signal read in `safe_area_insets`,
// signal write in the `on_event` callback) runs on the Lynx TASM
// thread by contract. Misuse would corrupt the reactive arena —
// same risk as touching any signal API from a worker thread.
unsafe impl<T> Send for MainThreadOnly<T> {}
unsafe impl<T> Sync for MainThreadOnly<T> {}
