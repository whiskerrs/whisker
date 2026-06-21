//! Reactive-layer tests for the render module.
//!
//! These cover the parts that do **not** need a device / renderer: the
//! [`RouterHandle`] verbs mutate the state signal correctly (and
//! `current` / `slice_at` / `selected_at` derive from it), the
//! [`RouteRegistry`] resolves ids, and the `state_at` slice walk picks
//! the right active child. The visual transition + gesture + actual
//! mount/swap behaviour is verified on-device.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use whisker::runtime::reactive::Owner;

use crate::core::{
    CompiledTree, NodePath, RouteInstance, RouteState, RouteTree, SwitchDef, Target,
};
use crate::render::handle::{RouterHandle, state_at};
use crate::render::node::mount_node;
use crate::render::registry::{RouteRegistry, Transition};

/// Run `f` under a fresh runtime + a live owner (so `computed` reads have
/// somewhere to allocate), then tear it down.
fn with_runtime<F: FnOnce() -> T, T>(f: F) -> T {
    whisker::runtime::reactive::__reset_for_tests();
    let owner = Owner::new(None);
    let out = owner.with(f);
    owner.dispose();
    out
}

/// Drain the reactive queue so persistent `computed`s created *before* a
/// navigation re-run and observe the new state. In the app this happens
/// once per frame in the driver's tick; tests drive it by hand.
fn flush() {
    whisker::runtime::reactive::flush();
}

/// A registry whose render closures are never invoked in these tests
/// (invoking one would create elements and need a renderer). We only
/// assert id resolution + transition lookup.
fn registry() -> RouteRegistry {
    RouteRegistry::new()
        .route("home", |_: &RouteInstance| {
            whisker::runtime::view::create_phantom_element()
        })
        .route_with("detail", Transition::Slide, |_: &RouteInstance| {
            whisker::runtime::view::create_phantom_element()
        })
}

/// root Stack { Route("", home)  Route("detail/:id", detail) }
fn simple_handle() -> RouterHandle {
    let tree = CompiledTree::new(RouteTree::stack(vec![
        RouteTree::route("", "home"),
        RouteTree::route("detail/:id", "detail"),
    ]));
    RouterHandle::new(tree, registry())
}

/// root Stack { Switch(tabs) { Stack{home, detail} Stack{list, detail} } }
fn tabbed_handle() -> RouterHandle {
    let tree = CompiledTree::new(RouteTree::stack(vec![RouteTree::switch(
        SwitchDef::new("tabs", 0),
        vec![
            RouteTree::stack(vec![
                RouteTree::route("", "home"),
                RouteTree::route("detail/:id", "detail"),
            ]),
            RouteTree::stack(vec![
                RouteTree::route("list", "list"),
                RouteTree::route("detail/:id", "detail"),
            ]),
        ],
    )]));
    RouterHandle::new(tree, registry())
}

// ----- registry -------------------------------------------------------

#[test]
fn registry_resolves_ids_and_transitions() {
    with_runtime(|| {
        let h = simple_handle();
        assert!(h.registry().contains("home"));
        assert!(h.registry().contains("detail"));
        assert!(!h.registry().contains("missing"));
        assert!(h.render_fn("home").is_some());
        assert!(h.render_fn("missing").is_none());
        // detail is registered Slide; home defaults to Slide; unknown
        // falls back to Slide.
        assert_eq!(h.transition("detail"), Transition::Slide);
        assert_eq!(h.transition("home"), Transition::Slide);
    });
}

// ----- navigate / back ------------------------------------------------

#[test]
fn navigate_pushes_and_current_tracks_signal() {
    with_runtime(|| {
        let h = simple_handle();
        let current = h.current();
        // Starts on home (root stack child 0).
        assert_eq!(current.get().path, NodePath(vec![0]));

        h.navigate(&Target::id("detail")).unwrap();
        flush();
        // Now on detail (child 1).
        assert_eq!(current.get().path, NodePath(vec![1]));

        assert!(h.back());
        flush();
        assert_eq!(current.get().path, NodePath(vec![0]));
        // Nothing left to pop.
        assert!(!h.back());
    });
}

#[test]
fn navigate_with_params_lands_on_signal() {
    with_runtime(|| {
        let h = simple_handle();
        h.navigate_with(
            &Target::id("detail"),
            RouteInstance::with_param(NodePath::root(), "id", "7"),
        )
        .unwrap();
        let inst = h.current().get();
        assert_eq!(inst.params.get("id").map(String::as_str), Some("7"));
    });
}

// ----- replace / pop_to / reset --------------------------------------

#[test]
fn replace_swaps_top_same_depth() {
    with_runtime(|| {
        let h = simple_handle();
        h.navigate_with(
            &Target::id("detail"),
            RouteInstance::with_param(NodePath::root(), "id", "1"),
        )
        .unwrap();
        // replace the top detail with another detail (same stack).
        h.replace(&Target::id("detail")).unwrap();
        // Still depth 2 (home + detail), still on detail.
        assert_eq!(h.current().get().path, NodePath(vec![1]));
        let RouteState::Stack(s) = h.state().get() else {
            panic!("root is a stack")
        };
        assert_eq!(s.history.len(), 2);
    });
}

#[test]
fn pop_to_returns_to_target() {
    with_runtime(|| {
        let h = simple_handle();
        h.navigate(&Target::id("detail")).unwrap();
        h.navigate(&Target::id("detail")).unwrap();
        // pop back to home.
        h.pop_to(&Target::id("home")).unwrap();
        assert_eq!(h.current().get().path, NodePath(vec![0]));
    });
}

#[test]
fn reset_clears_stack_to_target() {
    with_runtime(|| {
        let h = simple_handle();
        h.navigate(&Target::id("detail")).unwrap();
        h.navigate(&Target::id("detail")).unwrap();
        h.reset(&Target::id("home")).unwrap();
        let RouteState::Stack(s) = h.state().get() else {
            panic!("root is a stack")
        };
        assert_eq!(s.history.len(), 1);
        assert_eq!(h.current().get().path, NodePath(vec![0]));
    });
}

// ----- select / tabs --------------------------------------------------

#[test]
fn select_switches_tab_and_keeps_history() {
    with_runtime(|| {
        let h = tabbed_handle();
        let switch_path = NodePath(vec![0]);
        let selected = h.selected_at(switch_path.clone());
        assert_eq!(selected.get(), Some(0));

        // Push a detail in tab 0, then switch to tab 1.
        h.navigate(&Target::id("detail")).unwrap();
        h.select(&Target::id("list")).unwrap();
        flush();
        assert_eq!(selected.get(), Some(1));
        // Tab 1 shows its own home (list), depth 1.
        assert_eq!(
            h.current().get().path,
            // tab 1 = branch index 1 under the switch; its stack's first
            // child (list) is path [0,1,0].
            NodePath(vec![0, 1, 0])
        );

        // Switch back to tab 0 — its pushed detail is retained.
        h.select(&Target::id("home")).unwrap();
        flush();
        assert_eq!(selected.get(), Some(0));
        // Back in tab 0 on the detail we pushed (path [0,0,1]).
        assert_eq!(h.current().get().path, NodePath(vec![0, 0, 1]));
    });
}

#[test]
fn slice_only_changes_for_touched_tab() {
    with_runtime(|| {
        let h = tabbed_handle();
        let tab_a = NodePath(vec![0, 0]); // tab 0's stack
        let tab_b = NodePath(vec![0, 1]); // tab 1's stack
        let slice_a = h.slice_at(tab_a);
        let slice_b = h.slice_at(tab_b);

        let before_b = slice_b.get();
        let before_a = slice_a.get();

        // Push a detail into tab A.
        h.navigate(&Target::id("detail")).unwrap();
        flush();

        // Tab A's slice changed; tab B's did NOT — the memoised computed
        // is what keeps tab B from re-rendering.
        assert_ne!(slice_a.get(), before_a);
        assert_eq!(slice_b.get(), before_b);
    });
}

// ----- state_at slice walk -------------------------------------------

#[test]
fn state_at_walks_to_active_child() {
    with_runtime(|| {
        let h = tabbed_handle();
        h.navigate(&Target::id("detail")).unwrap();
        let root = h.state().get();

        // Root is the stack; [0] is the switch.
        assert!(matches!(
            state_at(&root, &NodePath(vec![0])),
            Some(RouteState::Switch(_))
        ));
        // [0,0] is tab 0's stack.
        assert!(matches!(
            state_at(&root, &NodePath(vec![0, 0])),
            Some(RouteState::Stack(_))
        ));
        // [0,0,1] is the pushed detail leaf.
        assert!(matches!(
            state_at(&root, &NodePath(vec![0, 0, 1])),
            Some(RouteState::Route(_))
        ));
    });
}

// ----- single draw path (no double-mount) ----------------------------

/// Per-id render-invocation counts. Render fns return a phantom (no
/// renderer needed) and bump their id's counter, so a test can assert how
/// many times each screen was mounted.
type Counts = Rc<RefCell<HashMap<&'static str, usize>>>;

fn counting_tabbed_handle(counts: Counts) -> RouterHandle {
    let tree = CompiledTree::new(RouteTree::switch(
        SwitchDef::new("tabs", 0),
        vec![
            RouteTree::stack(vec![
                RouteTree::route("", "home"),
                RouteTree::route("detail/:id", "detail"),
            ]),
            RouteTree::stack(vec![
                RouteTree::route("list", "list"),
                RouteTree::route("detail/:id", "detail"),
            ]),
        ],
    ));
    let mk = |id: &'static str, counts: Counts| {
        move |_: &RouteInstance| {
            *counts.borrow_mut().entry(id).or_insert(0) += 1;
            whisker::runtime::view::create_phantom_element()
        }
    };
    let registry = RouteRegistry::new()
        .route("home", mk("home", counts.clone()))
        .route("list", mk("list", counts.clone()))
        .route_with("detail", Transition::None, mk("detail", counts.clone()));
    RouterHandle::new(tree, registry)
}

#[test]
fn tree_is_drawn_once_no_double_mount() {
    with_runtime(|| {
        let counts: Counts = Rc::new(RefCell::new(HashMap::new()));
        let h = counting_tabbed_handle(counts.clone());

        // Draw the whole tree once from the root (the single Outlet path).
        let _slot = mount_node(&h, NodePath::root());
        flush();

        // home (the selected tab's leaf) mounts exactly once; the List
        // tab's `list` also mounts once (Switch keeps all branches alive),
        // but neither mounts twice.
        let c = counts.borrow();
        assert_eq!(c.get("home").copied(), Some(1), "home mounted once");
        assert_eq!(c.get("list").copied(), Some(1), "list mounted once");
        // detail not navigated to yet.
        assert_eq!(c.get("detail").copied(), None);
    });
}

#[test]
fn navigate_mounts_new_leaf_exactly_once() {
    with_runtime(|| {
        let counts: Counts = Rc::new(RefCell::new(HashMap::new()));
        let h = counting_tabbed_handle(counts.clone());
        let _slot = mount_node(&h, NodePath::root());
        flush();

        // Push a detail into the Home tab.
        h.navigate(&Target::id("detail")).unwrap();
        flush();

        // detail mounted once; home was NOT re-mounted by the push.
        let c = counts.borrow();
        assert_eq!(c.get("detail").copied(), Some(1), "detail mounted once");
        assert_eq!(c.get("home").copied(), Some(1), "home not re-mounted");
    });
}
