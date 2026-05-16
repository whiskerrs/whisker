//! Owner-bound non-reactive value slot.
//!
//! [`StoredValue<T>`] is a `Copy` handle to a value that lives in the
//! reactive arena alongside signals/effects, gets cleaned up with its
//! owner on dispose, but **does not participate in reactivity**.
//!
//! It's the right primitive for "I need to share some non-reactive
//! state across closures inside a component, and I want it freed when
//! the component unmounts" — i.e. exactly the role
//! `Rc<RefCell<...>>` plays in vanilla Rust, but tied to a scope so
//! you don't accidentally leak.
//!
//! Internally a `StoredValue` is a `Signal`-shaped node minus the
//! reactive plumbing: it has a value, no sources, no subscribers, and
//! reads/writes never tick the dependency graph.

use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use super::runtime::{NodeData, NodeId, Owner, ReactiveNode};
use super::with_runtime;

/// A non-reactive, owner-bound value slot. `Copy`.
pub struct StoredValue<T: 'static> {
    id: NodeId,
    _ty: PhantomData<fn() -> T>,
}

impl<T: 'static> Clone for StoredValue<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> Copy for StoredValue<T> {}

impl<T: 'static> StoredValue<T> {
    /// Allocate a new stored value in the current owner.
    pub fn new(initial: T) -> Self {
        let value: Rc<RefCell<dyn Any>> = Rc::new(RefCell::new(initial));
        let id = with_runtime(|rt| {
            let owner = rt.current_owner().unwrap_or_else(|| {
                let detached = rt.owners.insert(Owner::new(None));
                rt.owner_stack.push(detached);
                detached
            });
            let id = rt.nodes.insert(ReactiveNode {
                owner,
                data: NodeData::Signal { value },
                sources: Default::default(),
                subscribers: Default::default(),
            });
            if let Some(o) = rt.owners.get_mut(owner) {
                o.nodes.push(id);
            }
            id
        });
        Self {
            id,
            _ty: PhantomData,
        }
    }

    /// Borrowed read. Does NOT register a dependency.
    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        let value = with_runtime(|rt| {
            rt.nodes
                .get(self.id)
                .and_then(|n| n.data.value().cloned())
                .expect("StoredValue: disposed")
        });
        let borrow = value.borrow();
        let typed = borrow
            .downcast_ref::<T>()
            .expect("StoredValue::with: type mismatch");
        f(typed)
    }

    /// Borrowed mutation. Does NOT notify subscribers (this primitive
    /// has none by construction).
    pub fn update<R>(self, f: impl FnOnce(&mut T) -> R) -> R {
        let value = with_runtime(|rt| {
            rt.nodes
                .get(self.id)
                .and_then(|n| n.data.value().cloned())
                .expect("StoredValue: disposed")
        });
        let mut borrow = value.borrow_mut();
        let typed = borrow
            .downcast_mut::<T>()
            .expect("StoredValue::update: type mismatch");
        f(typed)
    }

    /// Replace the value.
    pub fn set(self, value: T) {
        self.update(move |slot| *slot = value);
    }
}

impl<T: 'static + Clone> StoredValue<T> {
    pub fn get(self) -> T {
        self.with(|v| v.clone())
    }
}
