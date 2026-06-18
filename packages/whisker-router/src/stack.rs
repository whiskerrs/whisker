//! [`RouteStack`] — signal-backed back stack.
//!
//! Designed as a *first-class value*: callers create one with
//! [`route_stack`], pass it as a prop, clone the handle, or hold
//! several in parallel (the tab-per-stack pattern holds one per tab).
//! Cloning shares the underlying reactive storage — there is no
//! "owner" of the stack, just handles into the same vec.
//!
//! Most apps publish the stack through
//! [`RouteProvider`](crate::RouteProvider) and let the layout
//! ([`StackLayout`](crate::StackLayout), [`TabsLayout`](crate::TabsLayout),
//! [`Outlet`](crate::Outlet)) look it up via [`router::<R>()`](crate::router)
//! — the explicit handle is only needed when imperative code (event
//! handlers, deep-link callbacks, tab bars) wants to drive navigation.
//!
//! Internally a single `RwSignal<Vec<RouteEntry<R>>>` drives reads;
//! the per-entry [`EntryState`] signal coordinates animation and
//! freeze metadata without churning the outer vector.

use std::cell::Cell;
use std::rc::Rc;

use whisker::{Owner, ReadSignal, RwSignal, computed};

use crate::route::Route;

/// Lifecycle stage of one [`RouteEntry`].
///
/// Layouts (notably [`StackLayout`](crate::StackLayout)) read this to
/// decide what styles to apply — slide-in vs settled vs slide-out —
/// and whether to pause effects for entries that are out of view.
/// The two `*ing` states are intentionally short-lived: layouts flip
/// them to `Active` / `Suspended` once the transition animation
/// finishes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EntryState {
    /// Just pushed; animation in progress, becomes [`Self::Active`].
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
/// Stable across the entry's lifetime in the stack — even if the
/// surrounding entries reshuffle. Used as the diff key for animations
/// and DOM reconciliation so a physical screen keeps the same
/// wrapper element handle through a navigation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntryId(pub u64);

/// One slot in a [`RouteStack`] — the route value plus its lifecycle
/// signal and stable id.
///
/// Equality compares both [`Self::id`] and [`Self::route`] so two
/// pushes of the "same" route value are still distinguishable.
#[derive(Clone)]
pub struct RouteEntry<R: Route> {
    /// The route this entry represents.
    pub route: R,
    /// Lifecycle signal; updated by layouts as animations progress.
    pub state: RwSignal<EntryState>,
    /// Stable id for this entry's lifetime in the stack.
    pub id: EntryId,
}

impl<R: Route + PartialEq> PartialEq for RouteEntry<R> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.route == other.route
    }
}

/// A signal-backed back stack of routes — the canonical navigation
/// primitive.
///
/// `RouteStack` is a *handle*: cloning it shares the underlying
/// reactive storage with the original, so a stack can be passed
/// freely between components / closures without wrapping in `Rc`.
/// The stack maintains the invariant that **at least one entry is
/// always present** — [`Self::back`] is a no-op (returns `false`) at
/// the root rather than emptying the stack.
///
/// # Example
///
/// ```ignore
/// use whisker_router::route_stack;
///
/// let nav = route_stack(AppRoute::Home);
/// nav.push(AppRoute::Profile { id: 7 });
/// nav.back();
/// assert_eq!(nav.current().get(), AppRoute::Home);
/// ```
///
/// Usually you don't keep the handle around — wrap your tree in a
/// [`RouteProvider`](crate::RouteProvider) and let descendants call
/// [`router::<R>()`](crate::router) to retrieve it. The explicit
/// handle is for imperative call sites (event handlers, deep-link
/// callbacks, tab bars).
pub struct RouteStack<R: Route> {
    entries: RwSignal<Vec<RouteEntry<R>>>,
    next_id: Rc<Cell<u64>>,
    // Owns every signal the stack allocates (the `entries` master
    // signal and each entry's `state`). A detached root so those
    // signals are tied to the stack's own lifetime, not whatever owner
    // happens to be current when `route_stack()` / `push()` runs. The
    // first call is typically from `#[whisker::main]` (no owner on the
    // stack yet) or an event handler (likewise) — minting under the
    // current owner there either warns + leaks into a detached fallback
    // owner, or worse pins app-lifetime state to a transient scope.
    // See `whisker::Owner::detached_root`.
    owner: Owner,
}

impl<R: Route> Clone for RouteStack<R> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries,
            next_id: Rc::clone(&self.next_id),
            owner: self.owner,
        }
    }
}

impl<R: Route> RouteStack<R> {
    /// Construct a stack with `initial` as the root entry.
    ///
    /// Prefer the free function [`route_stack`] for symmetry with
    /// other Whisker constructors.
    pub fn new(initial: R) -> Self {
        let next_id = Rc::new(Cell::new(0_u64));
        let id = mint_id(&next_id);
        let owner = Owner::detached_root();
        let entries = owner.with(|| {
            let entry = RouteEntry {
                route: initial,
                state: RwSignal::new(EntryState::Active),
                id,
            };
            RwSignal::new(vec![entry])
        });
        Self {
            entries,
            next_id,
            owner,
        }
    }

    /// Push a new route onto the top of the stack.
    ///
    /// The previous top transitions to [`EntryState::Suspended`];
    /// the new entry starts as [`EntryState::Entering`] so animated
    /// layouts can run their slide-in.
    pub fn push(&self, route: R) {
        let id = mint_id(&self.next_id);
        let new_entry = self.owner.with(|| RouteEntry {
            route,
            state: RwSignal::new(EntryState::Entering),
            id,
        });
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

    /// Pop entries until `predicate` returns `true` on the new top.
    ///
    /// Stops at the root regardless of the predicate. Useful for
    /// "pop back to the home tab" patterns:
    ///
    /// ```ignore
    /// nav.back_to(|r| matches!(r, AppRoute::Home));
    /// ```
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

    /// Replace the topmost entry with `route` — depth unchanged.
    ///
    /// Use this for "redirect" navigations (e.g. login → home) where
    /// the user should not be able to swipe back into the replaced
    /// entry.
    pub fn replace(&self, route: R) {
        let id = mint_id(&self.next_id);
        let entry = self.owner.with(|| RouteEntry {
            route,
            state: RwSignal::new(EntryState::Active),
            id,
        });
        self.entries.update(|v| {
            v.pop();
            v.push(entry);
        });
    }

    /// Clear the stack and start over with `route` at the root.
    ///
    /// Typical use: logout, deep-link cold-launch into a non-home
    /// destination, end-of-onboarding handoff.
    pub fn replace_all(&self, route: R) {
        let id = mint_id(&self.next_id);
        let entry = self.owner.with(|| RouteEntry {
            route,
            state: RwSignal::new(EntryState::Active),
            id,
        });
        self.entries.set(vec![entry]);
    }

    /// Reactive read of the topmost route.
    ///
    /// Re-fires whenever the top changes. The most common reader —
    /// most screens just want "what route is showing right now".
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

    /// Reactive read of the full stack as `Vec<R>` — useful for tab
    /// bars or breadcrumbs that need to render every level.
    pub fn stack(&self) -> ReadSignal<Vec<R>> {
        let entries = self.entries;
        computed(move || entries.get().iter().map(|e| e.route.clone()).collect())
    }

    /// Reactive read of the entries themselves (including
    /// [`EntryState`] and [`EntryId`]) — used by layouts that animate
    /// per-entry state. Most app code wants [`Self::stack`] instead.
    pub fn entries(&self) -> ReadSignal<Vec<RouteEntry<R>>> {
        let entries = self.entries;
        computed(move || entries.get())
    }

    /// Reactive flag indicating whether [`Self::back`] would pop —
    /// `true` once there's something on top of the root.
    ///
    /// Drive your in-app back button's visibility from this signal.
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
///
/// ```ignore
/// let nav = whisker_router::route_stack(AppRoute::Home);
/// ```
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
    fn stack_signals_outlive_the_owner_current_at_construction() {
        // `route_stack()` is typically first called from
        // `#[whisker::main]` (no owner) or an event handler (transient
        // owner). The stack's `entries`/`state` signals must NOT be
        // pinned to that scope — they belong to the app-lifetime stack.
        // Here we construct it under a transient owner, dispose that
        // owner, then keep using the stack; minting under
        // `Owner::detached_root` is what makes this safe.
        whisker::runtime::reactive::__reset_for_tests();
        let transient = whisker::runtime::reactive::Owner::new(None);
        let nav = transient.with(|| route_stack(TestRoute::Home));
        transient.dispose();

        // Reads and mutations still work after the construction-time
        // owner is gone (would panic "signal disposed" if pinned to it).
        // Reads go through `computed()` readers that allocate in the
        // current owner, so do them under a live one — exactly how a
        // component would.
        nav.push(TestRoute::Profile(7));
        let reader = whisker::runtime::reactive::Owner::new(None);
        reader.with(|| {
            assert_eq!(nav.current().get(), TestRoute::Profile(7));
            assert_eq!(nav.depth().get(), 2);
            let entries = nav.entries().get();
            assert_eq!(entries[1].state.get(), EntryState::Entering);
        });
        reader.dispose();
    }

    #[test]
    fn route_stack_constructs_without_an_ambient_owner() {
        // Mirrors the real first call from `#[whisker::main]` before any
        // render owner is established: no owner on the stack at all.
        // Construction + push (the `signal()` allocations) must not warn
        // "signal() called outside any owner scope" — the stack supplies
        // its own owner. (The `computed()` readers below are run under a
        // live owner, as components do.)
        whisker::runtime::reactive::__reset_for_tests();
        let nav = route_stack(TestRoute::Home);
        nav.push(TestRoute::Settings);

        let reader = whisker::runtime::reactive::Owner::new(None);
        reader.with(|| {
            assert_eq!(nav.current().get(), TestRoute::Settings);
            assert_eq!(nav.depth().get(), 2);
        });
        reader.dispose();
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
