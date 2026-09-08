use crate::reactive::{Owner, try_with_runtime, with_runtime};
use slotmap::DefaultKey;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

pub struct Scoped<T: 'static> {
    inner: Rc<Registered<T>>,
}
struct Registered<T> {
    owner: Owner,
    runtime: Weak<()>,
    key: Cell<Option<DefaultKey>>,
    active: Cell<bool>,
    value: RefCell<Option<T>>,
}

trait Cancel {
    fn cancel(&self);
    fn is_active(&self) -> bool;
}
#[derive(Clone)]
pub struct Cancellation(Weak<dyn Cancel>);
impl Cancellation {
    pub fn is_active(&self) -> bool {
        self.0.upgrade().is_some_and(|value| value.is_active())
    }
    pub fn cancel(&self) {
        if let Some(value) = self.0.upgrade() {
            value.cancel();
        }
    }
    pub fn is_live(&self) -> bool {
        self.0.strong_count() != 0
    }
}
impl<T> Registered<T> {
    fn unregister(&self) {
        try_with_runtime(|rt| {
            if self.runtime.ptr_eq(&Rc::downgrade(&rt.identity)) {
                if let (Some(owner), Some(key)) = (rt.owners.get_mut(self.owner), self.key.take()) {
                    owner.registrations.remove(key);
                }
            }
        });
    }
    fn release_cancelled(&self) {
        if !self.active.get() {
            let removed = self
                .value
                .try_borrow_mut()
                .ok()
                .and_then(|mut value| value.take());
            drop(removed);
        }
    }
}
impl<T> Cancel for Registered<T> {
    fn is_active(&self) -> bool {
        self.active.get()
            && with_runtime(|rt| {
                !rt.shutting_down
                    && self.runtime.ptr_eq(&Rc::downgrade(&rt.identity))
                    && rt
                        .owners
                        .get(self.owner)
                        .is_some_and(|scope| !scope.closing)
            })
    }

    fn cancel(&self) {
        self.active.set(false);
        self.unregister();
        self.release_cancelled();
    }
}
impl<T> Drop for Registered<T> {
    fn drop(&mut self) {
        self.unregister();
    }
}
impl<T: 'static> Clone for Scoped<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
impl<T: 'static> Scoped<T> {
    pub fn new(value: T) -> Self {
        let owner = Owner::current().unwrap_or_else(Owner::service);
        let inner = Rc::new(Registered {
            owner,
            runtime: with_runtime(|rt| Rc::downgrade(&rt.identity)),
            key: Cell::new(None),
            active: Cell::new(true),
            value: RefCell::new(Some(value)),
        });
        let weak = Rc::downgrade(&inner);
        let key = with_runtime(|rt| {
            assert!(
                !rt.shutting_down,
                "cannot register work during runtime shutdown"
            );
            let scope = rt
                .owners
                .get_mut(owner)
                .expect("cannot register work in a disposed owner");
            assert!(!scope.closing, "cannot register work in a closing owner");
            scope.registrations.insert(Box::new(move || {
                if let Some(value) = weak.upgrade() {
                    value.cancel();
                }
            }))
        });
        inner.key.set(Some(key));
        Self { inner }
    }
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }
    pub fn cancel(&self) {
        self.inner.cancel();
    }
    pub fn cancellation(&self) -> Cancellation {
        let inner: Rc<dyn Cancel> = self.inner.clone();
        Cancellation(Rc::downgrade(&inner))
    }
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        if !self.is_active() {
            return None;
        }
        let _execution = Execution::enter();
        let _release = ReleaseCancelled(&self.inner);
        self.inner
            .owner
            .with(|| self.inner.value.borrow().as_ref().map(f))
    }
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        if !self.is_active() {
            return None;
        }
        let _execution = Execution::enter();
        let _release = ReleaseCancelled(&self.inner);
        self.inner
            .owner
            .with(|| self.inner.value.borrow_mut().as_mut().map(f))
    }
}
struct ReleaseCancelled<'a, T>(&'a Registered<T>);
impl<T> Drop for ReleaseCancelled<'_, T> {
    fn drop(&mut self) {
        self.0.release_cancelled();
    }
}

pub(crate) struct Execution;
impl Execution {
    pub(crate) fn enter() -> Self {
        with_runtime(|rt| rt.execution_depth += 1);
        Self
    }
}
impl Drop for Execution {
    fn drop(&mut self) {
        let ready = with_runtime(|rt| {
            rt.execution_depth -= 1;
            rt.execution_depth == 0
        });
        if ready {
            Owner::drain_disposals();
        }
    }
}

pub struct OwnedOwner {
    owner: Owner,
    runtime: Weak<()>,
    dispatcher: Option<crate::RuntimeDispatcher>,
}

impl OwnedOwner {
    pub fn new() -> Self {
        Self {
            owner: Owner::detached_root(),
            runtime: with_runtime(|rt| Rc::downgrade(&rt.identity)),
            dispatcher: crate::runtime_dispatcher(),
        }
    }

    pub fn with<R>(&self, f: impl FnOnce() -> R) -> R {
        self.owner.with(f)
    }
}

impl Default for OwnedOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OwnedOwner {
    fn drop(&mut self) {
        let current = try_with_runtime(|rt| self.runtime.ptr_eq(&Rc::downgrade(&rt.identity)))
            .unwrap_or(false);
        if current {
            self.owner.dispose();
        } else if self.runtime.upgrade().is_some() {
            if let Some(dispatcher) = &self.dispatcher {
                let owner = self.owner;
                dispatcher.post(move || owner.dispose());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeContext, RuntimeWakeHandle};

    #[test]
    fn completed_and_cancelled_registrations_do_not_accumulate() {
        let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = Owner::new(None);
            owner.with(|| {
                for _ in 0..1000 {
                    crate::tasks::spawn_local(async {});
                }
            });
            crate::tasks::run_until_stalled();
            assert_eq!(with_runtime(|rt| rt.owners[owner].registrations.len()), 0);
            owner.with(|| {
                for _ in 0..1000 {
                    let callback = Scoped::new(|| {});
                    callback.cancel();
                }
            });
            assert_eq!(with_runtime(|rt| rt.owners[owner].registrations.len()), 0);
        });
    }

    #[test]
    fn cancelling_a_pending_task_releases_its_pool_entry_without_an_external_wake() {
        let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = Owner::new(None);
            let task = owner.with(|| crate::tasks::spawn_cancelable(std::future::pending()));
            crate::tasks::run_until_stalled();
            owner.dispose();
            crate::tasks::run_until_stalled();
            assert!(!task.is_live());
        });
    }

    #[test]
    fn explicit_cancellation_inside_a_callback_releases_it_after_return() {
        let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = Owner::new(None);
            let value = Rc::new(());
            let callback = owner.with(|| Scoped::new(value.clone()));
            let cancel = callback.cancellation();
            callback.with(|_| {
                cancel.cancel();
                assert!(!callback.is_active());
                assert_eq!(Rc::strong_count(&value), 2);
            });
            assert_eq!(Rc::strong_count(&value), 1);
        });
    }
}
