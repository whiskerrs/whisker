//! Type-indexed state owned by the currently entered runtime instance.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub(crate) struct RuntimeLocalState {
    values: HashMap<TypeId, Box<dyn Any>>,
}

impl RuntimeLocalState {
    pub(crate) fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }
}

thread_local! {
    static ACTIVE: RefCell<RuntimeLocalState> = RefCell::new(RuntimeLocalState::new());
}

/// Returns one typed state cell owned by the currently entered runtime.
///
/// The lookup happens when a subsystem first captures its state. Hot paths can
/// retain the returned `Rc` and do not need to repeat the type-map lookup.
#[doc(hidden)]
pub fn state<T: Default + 'static>() -> Rc<RefCell<T>> {
    ACTIVE.with_borrow_mut(|active| {
        active
            .values
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(Rc::new(RefCell::new(T::default()))))
            .downcast_ref::<Rc<RefCell<T>>>()
            .expect("runtime-local TypeId entry has a consistent type")
            .clone()
    })
}

pub(crate) fn swap_state(state: &mut RuntimeLocalState) {
    ACTIVE.with_borrow_mut(|active| std::mem::swap(active, state));
}
