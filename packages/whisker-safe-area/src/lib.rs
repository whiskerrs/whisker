//! `whisker-safe-area` — reactive accessor for the host view's
//! safe-area insets.
//!
//! **API shape — 4 (Free fn → signal).** See
//! [`docs/module-api-design.md`](https://github.com/whiskerrs/whisker/blob/main/docs/module-api-design.md)
//! §"Shape 4". A singleton observable: [`safe_area_insets`] returns
//! a runtime-local `ReadSignal<SafeAreaInsets>`, lazily wired to
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
//!         View(style: outer_style()) {
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
//! - **Web and Desktop**: returns zero on every edge. Whisker content owns the
//!   browser viewport or desktop window and has no mobile system-bar inset to
//!   avoid. No Host module is linked on these platforms.
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
//! Calls within one runtime share a signal and a native-event subscription.
//! They survive individual component and route owners, but are released when
//! the runtime ends. A new runtime (for example after an Android Activity is
//! recreated) initializes its own signal and subscription.
//!
//! ## Native source
//!
//! Contributors: the matching platform module lives at
//!
//! - iOS: `packages/whisker-safe-area/ios/Sources/WhiskerSafeArea/SafeAreaModule.swift`
//! - Android: `packages/whisker-safe-area/android/src/main/kotlin/rs/whisker/modules/safe_area/SafeAreaModule.kt`

#[cfg(test)]
use whisker::Owner;
#[cfg(any(target_os = "android", target_os = "ios", test))]
use whisker::WhiskerValue;
#[cfg(any(target_os = "android", target_os = "ios", test))]
use whisker::module;
use whisker::runtime::module::ModuleSubscription;
use whisker::{ReadSignal, RwSignal};

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

/// Reactive accessor for the current host view's safe-area insets.
///
/// Calls within the currently entered runtime share one signal and native
/// subscription. The initial value is [`SafeAreaInsets::default()`]; mobile
/// Hosts push the current insets when observation starts and whenever they
/// change. Web and desktop always expose the all-zero value.
///
/// The `Copy` handle belongs to the Runtime cache owner, so disposing
/// a component or route does not invalidate other readers. It must not be
/// retained across runtime shutdown or used in a different runtime.
///
/// **Must be called on the runtime's UI thread with that runtime entered.**
pub fn safe_area_insets() -> ReadSignal<SafeAreaInsets> {
    INSETS.with(|slot| slot.read)
}

struct Slot {
    read: ReadSignal<SafeAreaInsets>,
    _subscription: Option<ModuleSubscription>,
}

impl Slot {
    fn new(subscribe: impl FnOnce(RwSignal<SafeAreaInsets>) -> Option<ModuleSubscription>) -> Self {
        let signal = RwSignal::new(SafeAreaInsets::default());
        Self {
            read: signal.read_only(),
            _subscription: subscribe(signal),
        }
    }
}

whisker::runtime_local! {
    static INSETS: Slot = {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        { Slot::new(subscribe_to_native) }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        { Slot::new(|_| None) }
    };
}

#[cfg(any(target_os = "android", target_os = "ios", test))]
fn subscribe_to_native(signal: RwSignal<SafeAreaInsets>) -> Option<ModuleSubscription> {
    let sub = module!("SafeArea").on_event("insetsChanged", move |payload| {
        if let Some(insets) = decode_payload(payload) {
            signal.set(insets);
        }
    });
    if let Some(err) = sub.error() {
        eprintln!("[whisker-safe-area] failed to subscribe: {err}");
    }
    Some(sub)
}

// Missing or non-numeric keys default to `0.0` — a malformed message
// degrades silently rather than wedging the subscription.
#[cfg(any(target_os = "android", target_os = "ios", test))]
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;
    use whisker::runtime::module::{ModuleHost, with_module_host};
    use whisker::runtime::{RuntimeContext, RuntimeWakeHandle};

    use super::*;

    #[test]
    fn insets_survive_runtime_recreation() {
        for _ in 0..3 {
            let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
            runtime.enter(|| {
                assert_eq!(safe_area_insets().get(), SafeAreaInsets::default());
            });
            runtime.shutdown();
        }
    }

    fn runtime() -> RuntimeContext {
        RuntimeContext::new(RuntimeWakeHandle::new(|| {}))
    }

    fn payload(top: f64) -> WhiskerValue {
        WhiskerValue::Map(BTreeMap::from([("top".into(), WhiskerValue::Float(top))]))
    }

    fn host() -> (Rc<ModuleHost>, Rc<RefCell<Vec<bool>>>) {
        let observations = Rc::new(RefCell::new(Vec::new()));
        let events = observations.clone();
        let host = ModuleHost::new(
            |_, _, _, _, _| false,
            move |module, event, observing| {
                assert_eq!(
                    (module, event),
                    ("whisker-safe-area:SafeArea", "insetsChanged")
                );
                events.borrow_mut().push(observing);
            },
        );
        (host, observations)
    }

    fn native_insets() -> ReadSignal<SafeAreaInsets> {
        whisker::runtime_local! { static NATIVE: Slot = Slot::new(subscribe_to_native); }
        NATIVE.with(|slot| slot.read)
    }

    #[test]
    fn readers_share_updates_after_the_first_component_is_disposed() {
        let runtime = runtime();
        let (host, observations) = host();
        runtime.enter(|| {
            with_module_host(&host, || {
                let component = Owner::new(None);
                let first = component.with(native_insets);
                component.dispose();
                let second = native_insets();
                let padding =
                    Owner::new(None).with(|| whisker::computed(move || first.get().top + 16.0));
                assert_eq!(padding.get(), 16.0);
                assert_eq!(*observations.borrow(), [true]);
                assert!(host.dispatch_event(
                    "whisker-safe-area:SafeArea",
                    "insetsChanged",
                    payload(24.0)
                ));
                assert_eq!(first.get().top, 24.0);
                assert_eq!(second.get().top, 24.0);
                whisker::runtime::reactive::flush();
                assert_eq!(padding.get(), 40.0);
                // Malformed events must not replace the last valid value.
                host.dispatch_event(
                    "whisker-safe-area:SafeArea",
                    "insetsChanged",
                    WhiskerValue::Null,
                );
                assert_eq!(first.get().top, 24.0);
            })
        });
        runtime.shutdown();
        assert_eq!(*observations.borrow(), [true, false]);
    }

    #[test]
    fn recreated_runtime_resubscribes_and_releases_the_old_listener() {
        let (host, observations) = host();
        for generation in 1..=3 {
            let runtime = runtime();
            runtime.enter(|| {
                with_module_host(&host, || {
                    let insets = native_insets();
                    assert_eq!(insets.get(), SafeAreaInsets::default());
                    assert!(host.dispatch_event(
                        "whisker-safe-area:SafeArea",
                        "insetsChanged",
                        payload(10.0 * generation as f64)
                    ));
                    assert_eq!(insets.get().top, 10.0 * generation as f64);
                })
            });
            runtime.shutdown();
            assert!(!host.dispatch_event(
                "whisker-safe-area:SafeArea",
                "insetsChanged",
                payload(99.0)
            ));
            assert_eq!(observations.borrow().len(), generation * 2);
        }
        assert_eq!(
            *observations.borrow(),
            [true, false, true, false, true, false]
        );
    }

    #[test]
    fn simultaneous_runtimes_keep_their_insets_and_hosts_separate() {
        let first_runtime = runtime();
        let second_runtime = runtime();
        let (first_host, first_observations) = host();
        let (second_host, second_observations) = host();
        let first = first_runtime.enter(|| with_module_host(&first_host, native_insets));
        let second = second_runtime.enter(|| with_module_host(&second_host, native_insets));
        first_runtime.enter(|| {
            first_host.dispatch_event("whisker-safe-area:SafeArea", "insetsChanged", payload(12.0));
            assert_eq!(first.get().top, 12.0);
        });
        second_runtime.enter(|| {
            assert_eq!(second.get().top, 0.0);
            second_host.dispatch_event(
                "whisker-safe-area:SafeArea",
                "insetsChanged",
                payload(40.0),
            );
            assert_eq!(second.get().top, 40.0);
        });
        first_runtime.enter(|| assert_eq!(first.get().top, 12.0));
        // Subscription teardown must use its original Host, even while a
        // different runtime and Host are active on this thread.
        second_runtime.enter(|| with_module_host(&second_host, || first_runtime.shutdown()));
        assert_eq!(*first_observations.borrow(), [true, false]);
        assert_eq!(*second_observations.borrow(), [true]);
        second_runtime.enter(|| {
            second_host.dispatch_event(
                "whisker-safe-area:SafeArea",
                "insetsChanged",
                payload(44.0),
            );
            assert_eq!(second.get().top, 44.0);
        });
        drop(second_runtime);
        assert_eq!(*second_observations.borrow(), [true, false]);
    }

    #[test]
    fn synchronous_initial_insets_are_visible_on_first_read() {
        let runtime = runtime();
        let slot = Rc::new(RefCell::new(std::rc::Weak::<ModuleHost>::new()));
        let callback_host = slot.clone();
        let host = ModuleHost::new(
            |_, _, _, _, _| false,
            move |_, _, observing| {
                if observing {
                    callback_host.borrow().upgrade().unwrap().dispatch_event(
                        "whisker-safe-area:SafeArea",
                        "insetsChanged",
                        payload(18.0),
                    );
                }
            },
        );
        *slot.borrow_mut() = Rc::downgrade(&host);
        runtime.enter(|| {
            with_module_host(&host, || {
                assert_eq!(native_insets().get().top, 18.0);
            })
        });
        runtime.shutdown();
    }

    #[test]
    fn decodes_native_payload_and_defaults_missing_edges() {
        let payload = WhiskerValue::Map(BTreeMap::from([
            ("top".into(), WhiskerValue::Float(12.5)),
            ("bottom".into(), WhiskerValue::Int(8)),
        ]));

        assert_eq!(
            decode_payload(payload),
            Some(SafeAreaInsets {
                top: 12.5,
                leading: 0.0,
                trailing: 0.0,
                bottom: 8.0,
            })
        );
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn common_platform_fallback_is_all_zero() {
        runtime().enter(|| assert_eq!(safe_area_insets().get(), SafeAreaInsets::default()));
    }
}
