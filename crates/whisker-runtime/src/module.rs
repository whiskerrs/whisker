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

/// Promise handed to a Rust Host `AsyncFunction` implementation.
///
/// The first settlement wins. Dropping an unsettled promise completes the
/// invocation with an error so the Rust caller cannot wait forever.
pub struct ModulePromise {
    result: Option<ModuleResult>,
}

impl ModulePromise {
    fn new(result: ModuleResult) -> Self {
        Self {
            result: Some(result),
        }
    }

    /// Completes the invocation with one transferable value.
    pub fn resolve(mut self, value: WhiskerValue) {
        if let Some(result) = self.result.take() {
            result(value);
        }
    }

    /// Completes the invocation with a Host error.
    pub fn reject(self, message: impl Into<String>) {
        self.resolve(WhiskerValue::Error(message.into()));
    }
}

impl Drop for ModulePromise {
    fn drop(&mut self) {
        if let Some(result) = self.result.take() {
            result(WhiskerValue::Error(
                "Host AsyncFunction dropped its promise without settling".into(),
            ));
        }
    }
}

/// Event channel available to Rust Host module handlers.
type ModuleEventSink = Rc<dyn Fn(&str, &str, WhiskerValue) -> bool>;

#[derive(Clone)]
pub struct ModuleEventEmitter {
    module: String,
    emit: ModuleEventSink,
}

impl ModuleEventEmitter {
    /// Enqueues one declared service event for delivery on the next runtime
    /// drive. Returns `false` when the event was not declared by the module.
    pub fn emit(&self, event: &str, payload: WhiskerValue) -> bool {
        if !payload.is_data() {
            return false;
        }
        (self.emit)(&self.module, event, payload)
    }
}

type HostFunction = Rc<dyn Fn(&[WhiskerValue], &ModuleEventEmitter) -> WhiskerValue>;
type HostAsyncFunction = Rc<dyn Fn(&[WhiskerValue], ModulePromise, &ModuleEventEmitter)>;
type ObserverHook = Rc<dyn Fn(&ModuleEventEmitter)>;

/// Service portion shared by the Desktop and Web `ModuleDefinition` APIs.
///
/// Element factories stay in their target crates. This value contains only
/// the portable `Name`, `Function`, `AsyncFunction`, event, and observer
/// declarations used by Rust Hosts.
#[derive(Clone, Default)]
pub struct RustModuleDefinition {
    name: Option<String>,
    functions: HashMap<String, HostFunction>,
    async_functions: HashMap<String, HostAsyncFunction>,
    events: std::collections::HashSet<String>,
    on_start: HashMap<String, ObserverHook>,
    on_stop: HashMap<String, ObserverHook>,
}

impl std::fmt::Debug for RustModuleDefinition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RustModuleDefinition")
            .field("name", &self.name)
            .field("functions", &self.functions.keys())
            .field("async_functions", &self.async_functions.keys())
            .field("events", &self.events)
            .finish_non_exhaustive()
    }
}

impl RustModuleDefinition {
    /// Declares the package-qualified service module name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(self.name.replace(name).is_none(), "duplicate module Name");
        self
    }

    /// Declares one synchronous service function.
    pub fn function(
        mut self,
        name: impl Into<String>,
        handler: impl Fn(&[WhiskerValue], &ModuleEventEmitter) -> WhiskerValue + 'static,
    ) -> Self {
        let name = name.into();
        assert!(!name.trim().is_empty(), "module Function name is empty");
        assert!(
            self.functions
                .insert(name.clone(), Rc::new(handler))
                .is_none(),
            "duplicate module Function {name}"
        );
        self
    }

    /// Declares one deferred service function.
    pub fn async_function(
        mut self,
        name: impl Into<String>,
        handler: impl Fn(&[WhiskerValue], ModulePromise, &ModuleEventEmitter) + 'static,
    ) -> Self {
        let name = name.into();
        assert!(
            !name.trim().is_empty(),
            "module AsyncFunction name is empty"
        );
        assert!(
            self.async_functions
                .insert(name.clone(), Rc::new(handler))
                .is_none(),
            "duplicate module AsyncFunction {name}"
        );
        self
    }

    /// Declares one service-scoped event.
    pub fn event(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(!name.trim().is_empty(), "module Event name is empty");
        assert!(
            self.events.insert(name.clone()),
            "duplicate module Event {name}"
        );
        self
    }

    /// Declares the first-subscriber hook for one event.
    pub fn on_start_observing(
        mut self,
        event: impl Into<String>,
        hook: impl Fn(&ModuleEventEmitter) + 'static,
    ) -> Self {
        let event = event.into();
        assert!(
            self.on_start.insert(event.clone(), Rc::new(hook)).is_none(),
            "duplicate OnStartObserving for {event}"
        );
        self
    }

    /// Declares the last-subscriber hook for one event.
    pub fn on_stop_observing(
        mut self,
        event: impl Into<String>,
        hook: impl Fn(&ModuleEventEmitter) + 'static,
    ) -> Self {
        let event = event.into();
        assert!(
            self.on_stop.insert(event.clone(), Rc::new(hook)).is_none(),
            "duplicate OnStopObserving for {event}"
        );
        self
    }

    /// Returns the required module identity.
    pub fn module_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn validate(&self) -> Result<(), String> {
        let name = self
            .name
            .as_deref()
            .ok_or_else(|| "ModuleDefinition requires exactly one Name".to_string())?;
        if name.trim().is_empty() {
            return Err("ModuleDefinition Name is empty".into());
        }
        if let Some(duplicate) = self
            .functions
            .keys()
            .find(|function| self.async_functions.contains_key(*function))
        {
            return Err(format!(
                "Function and AsyncFunction both declare `{duplicate}` on `{name}`"
            ));
        }
        for event in self.on_start.keys().chain(self.on_stop.keys()) {
            if !self.events.contains(event) {
                return Err(format!(
                    "observer hook references undeclared event `{event}` on `{name}`"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct PendingModuleEvent {
    module: String,
    event: String,
    payload: WhiskerValue,
}

struct RustModuleRuntimeInner {
    definitions: HashMap<String, RustModuleDefinition>,
    pending: RefCell<Vec<PendingModuleEvent>>,
    wake: Rc<dyn Fn()>,
}

/// Bound service-module registry used by direct Rust Hosts.
pub struct RustModuleRuntime {
    inner: Rc<RustModuleRuntimeInner>,
    host: Rc<ModuleHost>,
}

impl RustModuleRuntime {
    /// Validates module declarations and creates the runtime binding.
    pub fn new(
        definitions: impl IntoIterator<Item = RustModuleDefinition>,
        wake: impl Fn() + 'static,
    ) -> Result<Self, String> {
        let mut by_name = HashMap::new();
        for definition in definitions {
            definition.validate()?;
            let name = definition
                .module_name()
                .expect("validated module has a Name")
                .to_owned();
            if by_name.insert(name.clone(), definition).is_some() {
                return Err(format!("duplicate Host module `{name}`"));
            }
        }
        let inner = Rc::new(RustModuleRuntimeInner {
            definitions: by_name,
            pending: RefCell::new(Vec::new()),
            wake: Rc::new(wake),
        });
        let invoke_inner = Rc::downgrade(&inner);
        let observe_inner = Rc::downgrade(&inner);
        let host = ModuleHost::new(
            move |module, method, args, is_async, result| {
                let Some(inner) = invoke_inner.upgrade() else {
                    return false;
                };
                let Some(definition) = inner.definitions.get(module) else {
                    return false;
                };
                let emitter = inner.emitter(module);
                if is_async {
                    let Some(function) = definition.async_functions.get(method) else {
                        return false;
                    };
                    function(args, ModulePromise::new(result), &emitter);
                    true
                } else {
                    let Some(function) = definition.functions.get(method) else {
                        return false;
                    };
                    result(function(args, &emitter));
                    true
                }
            },
            move |module, event, observing| {
                let Some(inner) = observe_inner.upgrade() else {
                    return;
                };
                let Some(definition) = inner.definitions.get(module) else {
                    return;
                };
                let hook = if observing {
                    definition.on_start.get(event)
                } else {
                    definition.on_stop.get(event)
                };
                if let Some(hook) = hook {
                    hook(&inner.emitter(module));
                }
            },
        );
        Ok(Self { inner, host })
    }

    /// Runs application/runtime work with this registry installed.
    pub fn with_host<T>(&self, work: impl FnOnce() -> T) -> T {
        with_module_host(&self.host, work)
    }

    /// Delivers queued Host events to Rust subscribers without re-entering
    /// application code from the originating Host callback.
    pub fn dispatch_pending_events(&self) -> usize {
        let pending = std::mem::take(&mut *self.inner.pending.borrow_mut());
        let count = pending.len();
        for event in pending {
            self.host
                .dispatch_event(&event.module, &event.event, event.payload);
        }
        count
    }
}

impl RustModuleRuntimeInner {
    fn emitter(self: &Rc<Self>, module: &str) -> ModuleEventEmitter {
        let weak = Rc::downgrade(self);
        ModuleEventEmitter {
            module: module.to_owned(),
            emit: Rc::new(move |module, event, payload| {
                let Some(inner) = weak.upgrade() else {
                    return false;
                };
                let declared = inner
                    .definitions
                    .get(module)
                    .is_some_and(|definition| definition.events.contains(event));
                if declared {
                    inner.pending.borrow_mut().push(PendingModuleEvent {
                        module: module.to_owned(),
                        event: event.to_owned(),
                        payload,
                    });
                    (inner.wake)();
                }
                declared
            }),
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

    #[test]
    fn rust_host_definition_supports_functions_events_and_observer_lifecycle() {
        let starts = Rc::new(Cell::new(0));
        let stops = Rc::new(Cell::new(0));
        let wakes = Rc::new(Cell::new(0));
        let definition = RustModuleDefinition::default()
            .name("demo:Echo")
            .function("echo", |args, _| args[0].clone())
            .async_function("echoAsync", |args, promise, _| {
                promise.resolve(args[0].clone());
            })
            .event("ready")
            .on_start_observing("ready", {
                let starts = Rc::clone(&starts);
                move |events| {
                    starts.set(starts.get() + 1);
                    assert!(events.emit("ready", WhiskerValue::String("now".into())));
                }
            })
            .on_stop_observing("ready", {
                let stops = Rc::clone(&stops);
                move |_| stops.set(stops.get() + 1)
            });
        let runtime = RustModuleRuntime::new([definition], {
            let wakes = Rc::clone(&wakes);
            move || wakes.set(wakes.get() + 1)
        })
        .unwrap();

        runtime.with_host(|| {
            assert_eq!(
                PlatformModule::named("demo:Echo").invoke("echo", vec![WhiskerValue::Int(9)]),
                WhiskerValue::Int(9)
            );
        });

        let payload = Rc::new(RefCell::new(None));
        let subscription = runtime.with_host(|| {
            let payload = Rc::clone(&payload);
            PlatformModule::named("demo:Echo").on_event("ready", move |value| {
                *payload.borrow_mut() = Some(value);
            })
        });
        assert_eq!(starts.get(), 1);
        assert_eq!(wakes.get(), 1);
        runtime.with_host(|| assert_eq!(runtime.dispatch_pending_events(), 1));
        assert_eq!(*payload.borrow(), Some(WhiskerValue::String("now".into())));
        drop(subscription);
        assert_eq!(stops.get(), 1);
    }

    #[test]
    fn rust_host_definition_requires_name_and_declared_observer_events() {
        assert!(
            RustModuleRuntime::new([RustModuleDefinition::default()], || {})
                .err()
                .expect("definition without Name must fail")
                .contains("Name")
        );
        let undeclared = RustModuleDefinition::default()
            .name("demo:Bad")
            .on_start_observing("missing", |_| {});
        assert!(
            RustModuleRuntime::new([undeclared], || {})
                .err()
                .expect("observer for undeclared event must fail")
                .contains("undeclared event")
        );

        let duplicate_function = RustModuleDefinition::default()
            .name("demo:Bad")
            .function("load", |_, _| WhiskerValue::Null)
            .async_function("load", |_, promise, _| promise.resolve(WhiskerValue::Null));
        assert!(
            RustModuleRuntime::new([duplicate_function], || {})
                .err()
                .expect("sync and async members must share one namespace")
                .contains("both declare")
        );
    }
}
