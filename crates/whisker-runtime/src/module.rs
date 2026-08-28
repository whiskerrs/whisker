//! Host-independent native-module dispatch.
//!
//! Core owns the public module API and the listener lifecycle. A Host installs
//! a [`ModuleHost`] while it drives application work. Native Hosts can adapt
//! this interface to FFI, while Rust Hosts can provide ordinary closures.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};

use whisker_value::WhiskerValue;

/// One-shot completion passed to a Host module invocation.
pub type ModuleResult = Box<dyn FnOnce(WhiskerValue) + Send + 'static>;
type InvokeCallback = dyn Fn(&str, &str, &[WhiskerValue], bool, ModuleResult) -> bool;
type ObserveCallback = dyn Fn(&str, &str, bool);
type EventCallback = Rc<dyn Fn(WhiskerValue)>;

struct Listener {
    module: String,
    event: String,
    callback: EventCallback,
}

/// Host implementation for function-shaped native modules.
///
/// The invocation callback returns whether the Host accepted the call and
/// completes it exactly once through `result`. Synchronous calls must complete
/// before returning. `observe` receives first-listener and last-listener
/// transitions for each `(module, event)` pair.
pub struct ModuleHost {
    invoke: Rc<InvokeCallback>,
    observe: Rc<ObserveCallback>,
    next_listener: Cell<i32>,
    listeners: RefCell<HashMap<i32, Listener>>,
}

impl ModuleHost {
    /// Creates a Host binding from platform-neutral callbacks.
    pub fn new(
        invoke: impl Fn(&str, &str, &[WhiskerValue], bool, ModuleResult) -> bool + 'static,
        observe: impl Fn(&str, &str, bool) + 'static,
    ) -> Rc<Self> {
        Rc::new(Self {
            invoke: Rc::new(invoke),
            observe: Rc::new(observe),
            next_listener: Cell::new(1),
            listeners: RefCell::new(HashMap::new()),
        })
    }

    fn dispatch(
        &self,
        module: &str,
        method: &str,
        args: &[WhiskerValue],
        is_async: bool,
        result: ModuleResult,
    ) -> bool {
        (self.invoke)(module, method, args, is_async, result)
    }

    fn add_listener<F>(
        self: &Rc<Self>,
        module: &str,
        event: &str,
        callback: F,
    ) -> ModuleSubscription
    where
        F: Fn(WhiskerValue) + 'static,
    {
        let id = self.next_listener.get();
        self.next_listener.set(id.saturating_add(1));
        let first = {
            let mut listeners = self.listeners.borrow_mut();
            let first = !listeners
                .values()
                .any(|listener| listener.module == module && listener.event == event);
            listeners.insert(
                id,
                Listener {
                    module: module.to_owned(),
                    event: event.to_owned(),
                    callback: Rc::new(callback),
                },
            );
            first
        };
        if first {
            (self.observe)(module, event, true);
        }
        ModuleSubscription {
            id,
            host: Rc::downgrade(self),
            error: None,
        }
    }

    fn remove_listener(&self, id: i32) {
        let last = {
            let mut listeners = self.listeners.borrow_mut();
            let Some(removed) = listeners.remove(&id) else {
                return;
            };
            (!listeners.values().any(|listener| {
                listener.module == removed.module && listener.event == removed.event
            }))
            .then_some((removed.module, removed.event))
        };
        if let Some((module, event)) = last {
            (self.observe)(&module, &event, false);
        }
    }

    /// Delivers a Host event to matching Rust subscriptions.
    pub fn dispatch_event(&self, module: &str, event: &str, payload: WhiskerValue) -> bool {
        let callbacks: Vec<_> = self
            .listeners
            .borrow()
            .values()
            .filter(|listener| listener.module == module && listener.event == event)
            .map(|listener| Rc::clone(&listener.callback))
            .collect();
        for callback in &callbacks {
            callback(payload.clone());
        }
        !callbacks.is_empty()
    }
}

thread_local! {
    static CURRENT_HOST: RefCell<Option<Rc<ModuleHost>>> = const { RefCell::new(None) };
}

struct BindingGuard(Option<Rc<ModuleHost>>);

impl Drop for BindingGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        CURRENT_HOST.with(|slot| *slot.borrow_mut() = previous);
    }
}

/// Runs application work with one Host's module binding installed.
pub fn with_module_host<T>(host: &Rc<ModuleHost>, work: impl FnOnce() -> T) -> T {
    let previous = CURRENT_HOST.with(|slot| slot.borrow_mut().replace(Rc::clone(host)));
    let _guard = BindingGuard(previous);
    work()
}

fn current_host() -> Option<Rc<ModuleHost>> {
    CURRENT_HOST.with(|slot| slot.borrow().clone())
}

const UNAVAILABLE: &str = "native module Host binding is not active";

/// Calls one Host module function synchronously.
pub fn invoke(name: &str, method: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
    let Some(host) = current_host() else {
        return WhiskerValue::Error(format!("{UNAVAILABLE}: {name}.{method}"));
    };
    let result = Arc::new(Mutex::new(None));
    let callback_result = Arc::clone(&result);
    let accepted = host.dispatch(
        name,
        method,
        &args,
        false,
        Box::new(move |value| {
            *callback_result
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(value);
        }),
    );
    if !accepted {
        return WhiskerValue::Error(format!("Host refused module call: {name}.{method}"));
    }
    result
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
        .unwrap_or_else(|| {
            WhiskerValue::Error(format!(
                "Host did not synchronously resolve: {name}.{method}"
            ))
        })
}

/// Calls one Host module function asynchronously.
pub async fn invoke_async(name: &str, method: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
    let Some(host) = current_host() else {
        return WhiskerValue::Error(format!("{UNAVAILABLE}: {name}.{method}"));
    };
    let (sender, receiver) = futures_channel::oneshot::channel();
    let accepted = host.dispatch(
        name,
        method,
        &args,
        true,
        Box::new(move |value| {
            let _ = sender.send(value);
        }),
    );
    if !accepted {
        return WhiskerValue::Error(format!("Host refused async module call: {name}.{method}"));
    }
    receiver
        .await
        .unwrap_or_else(|_| WhiskerValue::Error("async module callback was dropped".into()))
}

/// Name-bound handle for one native module.
#[derive(Debug, Clone)]
pub struct PlatformModule {
    name: String,
}

impl PlatformModule {
    /// Creates a module handle from its package-qualified Host name.
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// Returns the package-qualified Host name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Calls a synchronous module function.
    pub fn invoke(&self, function: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
        invoke(&self.name, function, args)
    }

    /// Calls an asynchronous module function.
    pub async fn invoke_async(&self, function: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
        invoke_async(&self.name, function, args).await
    }

    /// Subscribes to one module event.
    pub fn on_event<F>(&self, event: &str, callback: F) -> ModuleSubscription
    where
        F: Fn(WhiskerValue) + 'static,
    {
        let Some(host) = current_host() else {
            return ModuleSubscription::failed(format!("{UNAVAILABLE}: {}.{event}", self.name));
        };
        host.add_listener(&self.name, event, callback)
    }
}

/// RAII handle for a module event subscription.
pub struct ModuleSubscription {
    id: i32,
    host: Weak<ModuleHost>,
    error: Option<String>,
}

impl ModuleSubscription {
    fn failed(error: String) -> Self {
        Self {
            id: 0,
            host: Weak::new(),
            error: Some(error),
        }
    }

    /// Returns the binding error when no Host was installed.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Returns the process-local listener identifier.
    pub fn id(&self) -> i32 {
        self.id
    }
}

impl Drop for ModuleSubscription {
    fn drop(&mut self) {
        if let Some(host) = self.host.upgrade() {
            host.remove_listener(self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_rust_host_drives_the_same_module_api() {
        let calls = Rc::new(Cell::new(0));
        let calls_for_host = Rc::clone(&calls);
        let host = ModuleHost::new(
            move |module, method, args, _, result| {
                assert_eq!((module, method), ("demo", "echo"));
                calls_for_host.set(calls_for_host.get() + 1);
                result(args[0].clone());
                true
            },
            |_, _, _| {},
        );
        let value = with_module_host(&host, || {
            PlatformModule::named("demo").invoke("echo", vec![WhiskerValue::Int(7)])
        });
        assert_eq!(value, WhiskerValue::Int(7));
        assert_eq!(calls.get(), 1);
    }
}
