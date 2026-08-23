//! Platform-neutral mobile module dispatch.
//!
//! `WhiskerView` supplies a small callback table when it creates the retained
//! runtime. Rust module handles use that table only while application work is
//! being driven, so module code never depends on UIKit, Android, JNI, or the
//! concrete Host registry.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, Weak};

use crate::mobile_abi::{RawValueArena, WhiskerValueRaw, decode_value};

pub use whisker_runtime::value::{WhiskerModuleError, WhiskerValue};

/// Receives one borrowed typed module result.
pub type ModuleResultCallback = extern "C" fn(*mut c_void, *const WhiskerValueRaw);

/// Dispatches one sync or async call to the Host module registry.
pub type InvokeModuleCallback = extern "C" fn(
    *mut c_void,
    *const u8,
    usize,
    *const u8,
    usize,
    *const WhiskerValueRaw,
    usize,
    bool,
    ModuleResultCallback,
    *mut c_void,
) -> bool;

/// Notifies the Host about the first/last Rust listener for one event.
pub type ObserveModuleCallback =
    extern "C" fn(*mut c_void, *const u8, usize, *const u8, usize, bool);

type EventCallback = Arc<dyn Fn(WhiskerValue) + Send + Sync>;

struct Listener {
    module: String,
    event: String,
    callback: EventCallback,
}

/// Callback table owned by one mobile `WhiskerView` runtime.
pub struct MobileModuleHost {
    data: usize,
    invoke: InvokeModuleCallback,
    observe: ObserveModuleCallback,
    next_listener: AtomicI32,
    listeners: Mutex<HashMap<i32, Listener>>,
}

impl MobileModuleHost {
    /// Creates the binding installed by the owning mobile View.
    pub fn new(
        data: *mut c_void,
        invoke: InvokeModuleCallback,
        observe: ObserveModuleCallback,
    ) -> Arc<Self> {
        Arc::new(Self {
            data: data as usize,
            invoke,
            observe,
            next_listener: AtomicI32::new(1),
            listeners: Mutex::new(HashMap::new()),
        })
    }

    fn dispatch(
        &self,
        module: &str,
        method: &str,
        args: &[WhiskerValueRaw],
        is_async: bool,
        result: ModuleResultCallback,
        result_data: *mut c_void,
    ) -> bool {
        (self.invoke)(
            self.data as *mut c_void,
            module.as_ptr(),
            module.len(),
            method.as_ptr(),
            method.len(),
            if args.is_empty() {
                std::ptr::null()
            } else {
                args.as_ptr()
            },
            args.len(),
            is_async,
            result,
            result_data,
        )
    }

    fn observe(&self, module: &str, event: &str, observing: bool) {
        (self.observe)(
            self.data as *mut c_void,
            module.as_ptr(),
            module.len(),
            event.as_ptr(),
            event.len(),
            observing,
        );
    }

    fn add_listener<F>(
        self: &Arc<Self>,
        module: &str,
        event: &str,
        callback: F,
    ) -> ModuleSubscription
    where
        F: Fn(WhiskerValue) + Send + Sync + 'static,
    {
        let id = self.next_listener.fetch_add(1, Ordering::Relaxed);
        let first = {
            let mut listeners = self
                .listeners
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let first = !listeners
                .values()
                .any(|listener| listener.module == module && listener.event == event);
            listeners.insert(
                id,
                Listener {
                    module: module.to_owned(),
                    event: event.to_owned(),
                    callback: Arc::new(callback),
                },
            );
            first
        };
        if first {
            self.observe(module, event, true);
        }
        ModuleSubscription {
            id,
            host: Arc::downgrade(self),
            error: None,
        }
    }

    fn remove_listener(&self, id: i32) {
        let last = {
            let mut listeners = self
                .listeners
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let Some(removed) = listeners.remove(&id) else {
                return;
            };
            let last = !listeners.values().any(|listener| {
                listener.module == removed.module && listener.event == removed.event
            });
            last.then_some((removed.module, removed.event))
        };
        if let Some((module, event)) = last {
            self.observe(&module, &event, false);
        }
    }

    /// Delivers a Host module event to every matching Rust subscription.
    pub fn dispatch_event(&self, module: &str, event: &str, payload: WhiskerValue) -> bool {
        let callbacks: Vec<_> = self
            .listeners
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .filter(|listener| listener.module == module && listener.event == event)
            .map(|listener| Arc::clone(&listener.callback))
            .collect();
        for callback in &callbacks {
            callback(payload.clone());
        }
        !callbacks.is_empty()
    }
}

thread_local! {
    static CURRENT_HOST: RefCell<Option<Arc<MobileModuleHost>>> = const { RefCell::new(None) };
}

struct BindingGuard(Option<Arc<MobileModuleHost>>);

impl Drop for BindingGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        CURRENT_HOST.with(|slot| *slot.borrow_mut() = previous);
    }
}

/// Runs application/runtime work with one View's module binding installed.
pub fn with_mobile_module_host<T>(host: &Arc<MobileModuleHost>, work: impl FnOnce() -> T) -> T {
    let previous = CURRENT_HOST.with(|slot| slot.borrow_mut().replace(Arc::clone(host)));
    let _guard = BindingGuard(previous);
    work()
}

fn current_host() -> Option<Arc<MobileModuleHost>> {
    CURRENT_HOST.with(|slot| slot.borrow().clone())
}

const UNAVAILABLE: &str = "native module Host binding is not active";

extern "C" fn capture_sync_result(data: *mut c_void, value: *const WhiskerValueRaw) {
    if let Some(slot) = unsafe { data.cast::<Option<WhiskerValue>>().as_mut() } {
        *slot = Some(unsafe { decode_value(value) });
    }
}

/// Calls one Host module function synchronously.
pub fn invoke(name: &str, method: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
    let Some(host) = current_host() else {
        return WhiskerValue::Error(format!("{UNAVAILABLE}: {name}.{method}"));
    };
    let mut arena = RawValueArena::default();
    let args = args
        .iter()
        .map(|value| arena.encode(value))
        .collect::<Vec<_>>();
    let mut result = None;
    let accepted = host.dispatch(
        name,
        method,
        &args,
        false,
        capture_sync_result,
        (&mut result as *mut Option<WhiskerValue>).cast(),
    );
    if !accepted {
        return WhiskerValue::Error(format!("Host refused module call: {name}.{method}"));
    }
    result.unwrap_or_else(|| {
        WhiskerValue::Error(format!(
            "Host did not synchronously resolve: {name}.{method}"
        ))
    })
}

extern "C" fn resolve_async_result(data: *mut c_void, value: *const WhiskerValueRaw) {
    if data.is_null() {
        return;
    }
    let sender = unsafe {
        Box::from_raw(data.cast::<Option<futures_channel::oneshot::Sender<WhiskerValue>>>())
    };
    if let Some(sender) = *sender {
        let _ = sender.send(unsafe { decode_value(value) });
    }
}

/// Calls one Host module function asynchronously.
pub async fn invoke_async(name: &str, method: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
    let Some(host) = current_host() else {
        return WhiskerValue::Error(format!("{UNAVAILABLE}: {name}.{method}"));
    };
    let mut arena = RawValueArena::default();
    let args = args
        .iter()
        .map(|value| arena.encode(value))
        .collect::<Vec<_>>();
    let (sender, receiver) = futures_channel::oneshot::channel();
    let sender = Box::into_raw(Box::new(Some(sender))).cast::<c_void>();
    if !host.dispatch(name, method, &args, true, resolve_async_result, sender) {
        unsafe {
            drop(Box::from_raw(
                sender.cast::<Option<futures_channel::oneshot::Sender<WhiskerValue>>>(),
            ));
        }
        return WhiskerValue::Error(format!("Host refused async module call: {name}.{method}"));
    }
    receiver
        .await
        .unwrap_or_else(|_| WhiskerValue::Error("async module callback was dropped".into()))
}

#[derive(Debug, Clone)]
pub struct PlatformModule {
    name: String,
}

impl PlatformModule {
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn invoke(&self, function: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
        invoke(&self.name, function, args)
    }

    pub async fn invoke_async(&self, function: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
        invoke_async(&self.name, function, args).await
    }

    pub fn on_event<F>(&self, event: &str, callback: F) -> ModuleSubscription
    where
        F: Fn(WhiskerValue) + Send + Sync + 'static,
    {
        let Some(host) = current_host() else {
            return ModuleSubscription::failed(format!("{UNAVAILABLE}: {}.{event}", self.name));
        };
        host.add_listener(&self.name, event, callback)
    }
}

pub struct ModuleSubscription {
    id: i32,
    host: Weak<MobileModuleHost>,
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

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

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
