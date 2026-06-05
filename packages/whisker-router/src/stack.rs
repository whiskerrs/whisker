//! [`RouteStack`] — signal-backed back stack.
//!
//! Designed as a *first-class value*: callers can create one with
//! [`route_stack`], pass it as a prop, clone the handle, or hold
//! several in parallel (tab patterns hold one per tab).
//!
//! Internally a single `RwSignal<Vec<RouteEntry<R>>>` drives reads;
//! the per-entry [`EntryState`] signal coordinates animation and
//! freeze metadata without churning the outer vector.

use std::cell::Cell;
use std::rc::Rc;

use whisker::{computed, ReadSignal, RwSignal};

use crate::route::Route;

/// Lifecycle stage of one [`RouteEntry`].
///
/// `Outlet` reads this to decide what styles to apply (slide-in vs
/// settled vs slide-out) and whether to pause effects for entries
/// that are out of view. The two `*ing` states are intentionally
/// short-lived: layouts flip them to `Active` / `Suspended` once
/// any transition animation finishes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EntryState {
    /// Just pushed; animation in progress, becomes [`Active`].
    Entering,
    /// Settled on top of the stack.
    Active,
    /// Settled beneath the top of the stack (kept mounted, frozen).
    Suspended,
    /// Just popped; animation in progress, then the entry is dropped.
    Leaving,
}

/// Unique identifier for a [`RouteEntry`].
///
/// Stable across the entry's lifetime in the stack. Used as a key
/// for animations / DOM diffing so the same physical screen keeps
/// the same element handle even as the surrounding stack shifts.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntryId(pub u64);

/// One slot in a [`RouteStack`].
#[derive(Clone)]
pub struct RouteEntry<R: Route> {
    /// The route this entry represents.
    pub route: R,
    /// Lifecycle signal; updated by layouts as animations progress.
    pub state: RwSignal<EntryState>,
    /// Stable id for this entry's lifetime.
    pub id: EntryId,
}

impl<R: Route + PartialEq> PartialEq for RouteEntry<R> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.route == other.route
    }
}

/// A signal-backed back stack of routes.
///
/// `RouteStack` is a *handle*: cloning it shares the underlying
/// reactive storage with the original, so a stack can be passed
/// freely between components / closures without wrapping in `Rc`.
pub struct RouteStack<R: Route> {
    entries: RwSignal<Vec<RouteEntry<R>>>,
    next_id: Rc<Cell<u64>>,
}

impl<R: Route> Clone for RouteStack<R> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries,
            next_id: Rc::clone(&self.next_id),
        }
    }
}

impl<R: Route> RouteStack<R> {
    /// Construct a stack with one initial entry.
    pub fn new(initial: R) -> Self {
        let next_id = Rc::new(Cell::new(0_u64));
        let id = mint_id(&next_id);
        let entry = RouteEntry {
            route: initial,
            state: RwSignal::new(EntryState::Active),
            id,
        };
        Self {
            entries: RwSignal::new(vec![entry]),
            next_id,
        }
    }

    // ---- writers ----

    /// Push a new route onto the top of the stack.
    ///
    /// The previous top transitions to [`EntryState::Suspended`];
    /// the new entry starts as [`EntryState::Entering`] so animated
    /// layouts can run their slide-in.
    pub fn push(&self, route: R) {
        let id = mint_id(&self.next_id);
        let new_entry = RouteEntry {
            route,
            state: RwSignal::new(EntryState::Entering),
            id,
        };
        self.entries.update(|v| {
            if let Some(last) = v.last() {
                last.state.set(EntryState::Suspended);
            }
            v.push(new_entry);
        });
    }

    /// Pop the topmost entry, if more than one remains.
    ///
    /// Returns `true` when something was popped, `false` when the
    /// stack was already at its root (callers typically forward
    /// that case to native back).
    pub fn back(&self) -> bool {
        let mut popped = false;
        self.entries.update(|v| {
            if v.len() > 1 {
                v.pop();
                if let Some(last) = v.last() {
                    last.state.set(EntryState::Active);
                }
                popped = true;
            }
        });
        popped
    }

    /// Pop entries until `predicate` returns true on the new top.
    pub fn back_to(&self, predicate: impl Fn(&R) -> bool) {
        self.entries.update(|v| {
            while v.len() > 1 {
                let keep = v.last().map(|e| predicate(&e.route)).unwrap_or(false);
                if keep {
                    break;
                }
                v.pop();
            }
            if let Some(last) = v.last() {
                last.state.set(EntryState::Active);
            }
        });
    }

    /// Replace the topmost entry with `route` (no history growth).
    pub fn replace(&self, route: R) {
        let id = mint_id(&self.next_id);
        self.entries.update(|v| {
            v.pop();
            v.push(RouteEntry {
                route,
                state: RwSignal::new(EntryState::Active),
                id,
            });
        });
    }

    /// Clear the stack and start over with `route` at the root.
    pub fn replace_all(&self, route: R) {
        let id = mint_id(&self.next_id);
        self.entries.set(vec![RouteEntry {
            route,
            state: RwSignal::new(EntryState::Active),
            id,
        }]);
    }

    // ---- readers ----

    /// Reactive read of the topmost route.
    pub fn current(&self) -> ReadSignal<R> {
        let entries = self.entries;
        computed(move || {
            entries
                .get()
                .last()
                .map(|e| e.route.clone())
                .expect("RouteStack invariant: at least one entry")
        })
    }

    /// Reactive read of the full stack (as `Vec<R>`).
    pub fn stack(&self) -> ReadSignal<Vec<R>> {
        let entries = self.entries;
        computed(move || entries.get().iter().map(|e| e.route.clone()).collect())
    }

    /// Reactive read of the entries themselves — needed by layouts
    /// that animate per-entry state.
    pub fn entries(&self) -> ReadSignal<Vec<RouteEntry<R>>> {
        let entries = self.entries;
        computed(move || entries.get())
    }

    /// Reactive flag indicating whether [`Self::back`] would pop.
    pub fn can_back(&self) -> ReadSignal<bool> {
        let entries = self.entries;
        computed(move || entries.get().len() > 1)
    }

    /// Reactive stack depth.
    pub fn depth(&self) -> ReadSignal<usize> {
        let entries = self.entries;
        computed(move || entries.get().len())
    }
}

fn mint_id(counter: &Rc<Cell<u64>>) -> EntryId {
    let v = counter.get();
    counter.set(v.wrapping_add(1));
    EntryId(v)
}

/// Construct a fresh [`RouteStack`] with `initial` as its root entry.
pub fn route_stack<R: Route>(initial: R) -> RouteStack<R> {
    RouteStack::new(initial)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum TestRoute {
        Home,
        Profile(u64),
        Settings,
    }

    impl Route for TestRoute {
        fn parse(_path: &str) -> Result<Self, crate::RouteError> {
            unimplemented!("parser not used in stack tests")
        }
        fn to_path(&self) -> String {
            match self {
                TestRoute::Home => "/".into(),
                TestRoute::Profile(id) => format!("/profile/{id}"),
                TestRoute::Settings => "/settings".into(),
            }
        }
    }

    fn with_runtime<F: FnOnce() -> T, T>(f: F) -> T {
        whisker::runtime::reactive::__reset_for_tests();
        let owner = whisker::runtime::reactive::Owner::new(None);
        let out = owner.with(f);
        owner.dispose();
        out
    }

    #[test]
    fn new_stack_has_initial_entry() {
        with_runtime(|| {
            let nav = route_stack(TestRoute::Home);
            assert_eq!(nav.current().get(), TestRoute::Home);
            assert_eq!(nav.depth().get(), 1);
            assert!(!nav.can_back().get());
        });
    }

    #[test]
    fn push_grows_stack_and_marks_top_active() {
        with_runtime(|| {
            let nav = route_stack(TestRoute::Home);
            nav.push(TestRoute::Profile(7));
            assert_eq!(nav.current().get(), TestRoute::Profile(7));
            assert_eq!(nav.depth().get(), 2);
            assert!(nav.can_back().get());

            let entries = nav.entries().get();
            assert_eq!(entries[0].state.get(), EntryState::Suspended);
            assert_eq!(entries[1].state.get(), EntryState::Entering);
        });
    }

    #[test]
    fn back_pops_top_and_reactivates_previous() {
        with_runtime(|| {
            let nav = route_stack(TestRoute::Home);
            nav.push(TestRoute::Profile(7));
            nav.push(TestRoute::Settings);

            assert!(nav.back());
            assert_eq!(nav.current().get(), TestRoute::Profile(7));
            assert_eq!(
                nav.entries().get().last().unwrap().state.get(),
                EntryState::Active
            );
        });
    }

    #[test]
    fn back_at_root_returns_false() {
        with_runtime(|| {
            let nav = route_stack(TestRoute::Home);
            assert!(!nav.back());
            assert_eq!(nav.current().get(), TestRoute::Home);
        });
    }

    #[test]
    fn back_to_pops_until_predicate() {
        with_runtime(|| {
            let nav = route_stack(TestRoute::Home);
            nav.push(TestRoute::Profile(1));
            nav.push(TestRoute::Profile(2));
            nav.push(TestRoute::Settings);
            nav.back_to(|r| matches!(r, TestRoute::Home));
            assert_eq!(nav.current().get(), TestRoute::Home);
            assert_eq!(nav.depth().get(), 1);
        });
    }

    #[test]
    fn replace_swaps_top_only() {
        with_runtime(|| {
            let nav = route_stack(TestRoute::Home);
            nav.push(TestRoute::Profile(1));
            nav.replace(TestRoute::Settings);
            assert_eq!(nav.depth().get(), 2);
            assert_eq!(nav.current().get(), TestRoute::Settings);
        });
    }

    #[test]
    fn replace_all_resets_root() {
        with_runtime(|| {
            let nav = route_stack(TestRoute::Home);
            nav.push(TestRoute::Profile(1));
            nav.push(TestRoute::Profile(2));
            nav.replace_all(TestRoute::Settings);
            assert_eq!(nav.depth().get(), 1);
            assert_eq!(nav.current().get(), TestRoute::Settings);
        });
    }

    #[test]
    fn clones_share_state() {
        with_runtime(|| {
            let a = route_stack(TestRoute::Home);
            let b = a.clone();
            b.push(TestRoute::Profile(42));
            assert_eq!(a.current().get(), TestRoute::Profile(42));
            assert_eq!(a.depth().get(), 2);
        });
    }
}
