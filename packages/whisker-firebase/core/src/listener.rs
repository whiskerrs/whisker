//! Routes native listener events to Rust callbacks.
//!
//! Each service module declares one event (e.g. Firestore `snapshot`) whose payload is
//! `{id, value}` or `{id, error}`. One module subscription per runtime fans events out
//! to callbacks by listener id; native code keeps an `id → SDK registration` table.
use crate::{Result, unwrap_response};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicI64, Ordering};
use whisker::platform_module::WhiskerValue;
use whisker::runtime::module::ModuleSubscription;
use whisker::{PlatformModule, ReadSignal, RwSignal, on_cleanup};

type Callback = Rc<dyn Fn(Result<WhiskerValue>)>;

static NEXT_ID: AtomicI64 = AtomicI64::new(1);

/// Per-runtime callback table for one native module event.
pub struct Listeners {
    module: PlatformModule,
    remove_all: &'static str,
    callbacks: Rc<RefCell<HashMap<i64, Callback>>>,
    _subscription: ModuleSubscription,
}

impl Listeners {
    /// `remove_all` is a native function that unregisters every listener; it runs
    /// when the runtime (e.g. after a hot reload) drops this table.
    pub fn new(
        module: PlatformModule,
        service: &'static str,
        event: &str,
        remove_all: &'static str,
    ) -> Rc<Self> {
        let callbacks: Rc<RefCell<HashMap<i64, Callback>>> = Rc::default();
        let routes = Rc::downgrade(&callbacks);
        let subscription = module.on_event(event, move |payload| {
            let Some(routes) = routes.upgrade() else {
                return;
            };
            let WhiskerValue::Map(mut fields) = payload else {
                return;
            };
            let Some(WhiskerValue::Int(id)) = fields.remove("id") else {
                return;
            };
            // Release the borrow before running user code, which may add or drop listeners.
            let callback = routes.borrow().get(&id).cloned();
            if let Some(callback) = callback {
                callback(unwrap_response(service, WhiskerValue::Map(fields)));
            }
        });
        Rc::new(Self {
            module,
            remove_all,
            callbacks,
            _subscription: subscription,
        })
    }
}

impl Drop for Listeners {
    fn drop(&mut self) {
        if !self.callbacks.borrow().is_empty() {
            let _ = self.module.invoke(self.remove_all, vec![]);
        }
    }
}

/// Register `callback`, then ask native code to start listening under the new id.
/// `start` receives the id and performs the native call.
pub fn listen(
    listeners: &Rc<Listeners>,
    remove: &'static str,
    callback: impl Fn(Result<WhiskerValue>) + 'static,
    start: impl FnOnce(i64) -> Result<()>,
) -> Result<ListenerRegistration> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    listeners
        .callbacks
        .borrow_mut()
        .insert(id, Rc::new(callback));
    let registration = ListenerRegistration {
        _inner: Registration::Native {
            id,
            listeners: Rc::downgrade(listeners),
            remove,
        },
    };
    start(id)?; // Dropping `registration` on failure removes the callback.
    Ok(registration)
}

/// Subscribe to a module event broadcast to every listener, such as an incoming message.
/// The payload is the `{value}` / `{error}` envelope.
pub fn listen_event(
    module: &PlatformModule,
    service: &'static str,
    event: &str,
    callback: impl Fn(Result<WhiskerValue>) + 'static,
) -> Result<ListenerRegistration> {
    let subscription = module.on_event(event, move |payload| {
        callback(unwrap_response(service, payload))
    });
    if let Some(error) = subscription.error() {
        return Err(crate::FirebaseError::new(service, "bridge-error", error));
    }
    Ok(ListenerRegistration {
        _inner: Registration::Event {
            _subscription: subscription,
        },
    })
}

/// Expose a listener as a reactive signal owned by the current component.
/// The listener is removed when that owner is disposed.
///
/// # Panics
/// In debug builds, when called outside a reactive owner (e.g. from a detached task).
pub fn signal_from_listener<T: Clone + 'static>(
    initial: T,
    subscribe: impl FnOnce(Box<dyn Fn(T)>) -> Result<ListenerRegistration>,
    on_error: impl FnOnce(crate::FirebaseError) -> T,
) -> ReadSignal<T> {
    let signal = RwSignal::new(initial);
    match subscribe(Box::new(move |value| {
        signal.try_set(value);
    })) {
        Ok(registration) => on_cleanup(move || drop(registration)),
        Err(error) => signal.set(on_error(error)),
    }
    signal.read_only()
}

/// Keeps a native listener (snapshot, auth state, …) active.
///
/// Dropping the registration removes the listener, matching the SDKs' `remove()` /
/// unsubscribe functions. Keep it alive for as long as you need updates.
#[must_use = "the listener is removed as soon as the registration is dropped"]
pub struct ListenerRegistration {
    _inner: Registration,
}

enum Registration {
    Native {
        id: i64,
        listeners: Weak<Listeners>,
        remove: &'static str,
    },
    Event {
        _subscription: ModuleSubscription,
    },
}

impl ListenerRegistration {
    /// Remove the listener now. Equivalent to dropping the registration.
    pub fn remove(self) {}
}

impl std::fmt::Debug for ListenerRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListenerRegistration")
            .finish_non_exhaustive()
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Registration::Native {
            id,
            listeners,
            remove,
        } = self
            && let Some(listeners) = listeners.upgrade()
        {
            listeners.callbacks.borrow_mut().remove(id);
            let _ = listeners
                .module
                .invoke(remove, vec![WhiskerValue::Int(*id)]);
        }
    }
}
