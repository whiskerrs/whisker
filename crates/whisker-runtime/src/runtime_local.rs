//! Lazy caches and internal subsystem state owned by the entered Runtime.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub(crate) struct RuntimeLocalState {
    values: HashMap<TypeId, Box<dyn Any>>,
    caches: HashMap<TypeId, Cache>,
    initialized: Vec<TypeId>,
}

impl RuntimeLocalState {
    pub(crate) fn new() -> Self {
        Self {
            values: HashMap::new(),
            caches: HashMap::new(),
            initialized: Vec::new(),
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

enum Cache {
    Initializing,
    Ready {
        value: Box<dyn Any>,
        owner: crate::reactive::Owner,
    },
}

/// A declaration-specific key for a lazily initialized value in each Runtime.
/// Declare keys with [`crate::runtime_local!`]. Values need not be `Send` or `Sync`.
pub struct RuntimeLocalKey<T: 'static> {
    key: fn() -> TypeId,
    initialize: fn() -> T,
    name: &'static str,
}

impl<T: 'static> RuntimeLocalKey<T> {
    #[doc(hidden)]
    pub const fn new(key: fn() -> TypeId, initialize: fn() -> T, name: &'static str) -> Self {
        Self {
            key,
            initialize,
            name,
        }
    }

    /// Borrows the current Runtime's value, initializing it once on first use.
    /// Initialization is untracked and owns its signals, subscriptions and tasks.
    /// Panics outside an entered Runtime or on recursive initialization of this key.
    pub fn with<R>(&'static self, f: impl FnOnce(&T) -> R) -> R {
        assert!(
            crate::runtime_dispatcher().is_some(),
            "runtime_local requires an entered Runtime"
        );
        let key = (self.key)();
        let value = ACTIVE.with_borrow_mut(|active| match active.caches.get(&key) {
            Some(Cache::Ready { value, .. }) => Some(
                value
                    .downcast_ref::<Rc<T>>()
                    .expect("runtime-local key type")
                    .clone(),
            ),
            Some(Cache::Initializing) => {
                panic!("recursive runtime_local initialization: {}", self.name)
            }
            None => {
                crate::reactive::with_runtime(|rt| {
                    assert!(
                        !rt.shutting_down,
                        "cannot initialize runtime_local during shutdown"
                    )
                });
                active.caches.insert(key, Cache::Initializing);
                None
            }
        });
        let value = value.unwrap_or_else(|| {
            use crate::reactive::Owner;
            let owner = Owner::new(Some(Owner::service()));
            struct Rollback {
                key: TypeId,
                owner: Owner,
                committed: bool,
            }
            impl Drop for Rollback {
                fn drop(&mut self) {
                    if !self.committed {
                        ACTIVE.with_borrow_mut(|active| {
                            active.caches.remove(&self.key);
                        });
                        self.owner.dispose();
                    }
                }
            }
            let mut rollback = Rollback {
                key,
                owner,
                committed: false,
            };
            let value = Rc::new(owner.with(|| crate::reactive::untrack(self.initialize)));
            ACTIVE.with_borrow_mut(|active| {
                active.caches.insert(
                    key,
                    Cache::Ready {
                        value: Box::new(value.clone()),
                        owner,
                    },
                );
                active.initialized.push(key);
            });
            rollback.committed = true;
            value
        });
        f(&value)
    }
}

/// Declares lazily initialized Runtime-local values with independent keys.
///
/// ```ignore
/// whisker::runtime_local! {
///     static INFO: DeviceInfo = load_device_info();
/// }
/// let name = INFO.with(|info| info.name.clone());
/// ```
#[macro_export]
macro_rules! runtime_local {
    ($( $(#[$attr:meta])* $vis:vis static $name:ident: $ty:ty = $init:expr; )+) => {
        $(
            $(#[$attr])*
            $vis static $name: $crate::runtime_local::RuntimeLocalKey<$ty> = {
                struct Key;
                $crate::runtime_local::RuntimeLocalKey::new(
                    || ::std::any::TypeId::of::<Key>(), || $init, stringify!($name)
                )
            };
        )+
    };
}

pub(crate) fn dispose_caches() {
    let keys = ACTIVE.with_borrow_mut(|active| std::mem::take(&mut active.initialized));
    for key in keys.into_iter().rev() {
        let entry = ACTIVE.with_borrow_mut(|active| active.caches.remove(&key));
        if let Some(Cache::Ready { value, owner }) = entry {
            owner.with(|| drop(value));
            owner.dispose();
        }
    }
}

pub(crate) fn clear() {
    let removed =
        ACTIVE.with_borrow_mut(|active| std::mem::replace(active, RuntimeLocalState::new()));
    drop(removed);
}
