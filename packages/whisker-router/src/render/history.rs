//! Private Router ↔ Host history seam.
//!
//! The Router core stays independent of browser bindings. On Web this module
//! talks to the checked-in `whisker-router-web` implementation through the
//! same `WhiskerValue` service-module path used by native modules. Other Hosts
//! compile the no-op adapter and keep navigation entirely in Rust.

#[cfg(target_arch = "wasm32")]
mod implementation {
    use whisker::runtime::module::ModuleSubscription;
    use whisker::{WhiskerValue, module};

    pub(crate) struct InitialLocation {
        pub(crate) target: String,
    }

    pub(crate) struct Subscription {
        _inner: ModuleSubscription,
    }

    pub(crate) fn initialize() -> Option<InitialLocation> {
        let value = module!("History").invoke("initialize", Vec::new());
        let WhiskerValue::Map(fields) = value else {
            report("initialize", &value);
            return None;
        };
        let Some(WhiskerValue::String(target)) = fields.get("target") else {
            eprintln!("[whisker-router] History.initialize returned no target");
            return None;
        };
        Some(InitialLocation {
            target: target.clone(),
        })
    }

    pub(crate) fn push(url: &str, target: &str) {
        invoke_write("push", url, target);
    }

    pub(crate) fn replace(url: &str, target: &str) {
        invoke_write("replace", url, target);
    }

    pub(crate) fn back() -> Option<bool> {
        let value = module!("History").invoke("back", Vec::new());
        match value {
            WhiskerValue::Bool(handled) => Some(handled),
            other => {
                report("back", &other);
                None
            }
        }
    }

    pub(crate) fn subscribe(callback: impl Fn(String) + 'static) -> Option<Subscription> {
        let subscription = module!("History").on_event("changed", move |payload| {
            let WhiskerValue::Map(fields) = payload else {
                return;
            };
            if let Some(WhiskerValue::String(target)) = fields.get("target") {
                callback(target.clone());
            }
        });
        if let Some(error) = subscription.error() {
            eprintln!("[whisker-router] failed to subscribe to browser history: {error}");
            None
        } else {
            Some(Subscription {
                _inner: subscription,
            })
        }
    }

    fn invoke_write(method: &str, url: &str, target: &str) {
        let value = module!("History").invoke(
            method,
            vec![
                WhiskerValue::String(url.to_owned()),
                WhiskerValue::String(target.to_owned()),
            ],
        );
        if !matches!(value, WhiskerValue::Null) {
            report(method, &value);
        }
    }

    fn report(method: &str, value: &WhiskerValue) {
        match value {
            WhiskerValue::Error(error) => {
                eprintln!("[whisker-router] browser History.{method} failed: {error}");
            }
            other => {
                eprintln!(
                    "[whisker-router] browser History.{method} returned an invalid value: {other:?}"
                );
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod implementation {
    pub(crate) struct InitialLocation {
        pub(crate) target: String,
    }

    pub(crate) struct Subscription;

    pub(crate) fn initialize() -> Option<InitialLocation> {
        None
    }

    pub(crate) fn push(_url: &str, _target: &str) {}

    pub(crate) fn replace(_url: &str, _target: &str) {}

    pub(crate) fn back() -> Option<bool> {
        None
    }

    pub(crate) fn subscribe(_callback: impl Fn(String) + 'static) -> Option<Subscription> {
        None
    }
}

pub(crate) use implementation::*;
