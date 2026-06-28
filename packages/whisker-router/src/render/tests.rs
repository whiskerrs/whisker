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
    CompiledTree, NodePath, RouteDef, RouteInstance, RouteState, RouteTree, SwitchDef,
};
use crate::render::handle::{RouterHandle, state_at};
use crate::render::node::mount_node;
use crate::render::registry::RouteRegistry;
use crate::render::transition::RouteTransition;

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
        .route_with("detail", RouteTransition::slide(), |_: &RouteInstance| {
            whisker::runtime::view::create_phantom_element()
        })
}

/// root Stack { Route("", home)  Route("detail/:id", detail) }
fn simple_handle() -> RouterHandle {
    let tree = CompiledTree::new(RouteTree::stack(vec![
        RouteTree::route("", "home"),
        RouteTree::route("detail/:id", "detail"),
    ]));
    RouterHandle::new((tree, registry()))
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
    RouterHandle::new((tree, registry()))
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
        // detail is registered Slide; home defaults to the platform
        // default; unknown falls back to the platform default.
        assert_eq!(h.transition("detail").name(), "slide");
        assert_eq!(
            h.transition("home").name(),
            RouteTransition::default().name()
        );
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

        h.navigate("/detail/1").unwrap();
        flush();
        // Now on detail (child 1).
        assert_eq!(current.get().path, NodePath(vec![1]));

        assert!(h.back().is_ok());
        flush();
        assert_eq!(current.get().path, NodePath(vec![0]));
        // Nothing left to pop.
        assert_eq!(h.back(), Err(crate::core::NavError::NothingToPop));
    });
}

#[test]
fn navigate_url_params_land_on_signal() {
    with_runtime(|| {
        let h = simple_handle();
        // URL navigation is the param channel: `:id` binds from the path.
        h.navigate("/detail/7").unwrap();
        let inst = h.current().get();
        assert_eq!(inst.params.get("id").map(String::as_str), Some("7"));
    });
}

// ----- replace / pop_to / reset --------------------------------------

#[test]
fn replace_swaps_top_same_depth() {
    with_runtime(|| {
        let h = simple_handle();
        h.navigate("/detail/1").unwrap();
        // replace the top detail with another detail (same stack).
        h.replace("/detail/1").unwrap();
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
        h.navigate("/detail/1").unwrap();
        h.navigate("/detail/1").unwrap();
        // pop back to home.
        h.pop_to("/").unwrap();
        assert_eq!(h.current().get().path, NodePath(vec![0]));
    });
}

#[test]
fn reset_clears_stack_to_target() {
    with_runtime(|| {
        let h = simple_handle();
        h.navigate("/detail/1").unwrap();
        h.navigate("/detail/1").unwrap();
        h.reset("/").unwrap();
        let RouteState::Stack(s) = h.state().get() else {
            panic!("root is a stack")
        };
        assert_eq!(s.history.len(), 1);
        assert_eq!(h.current().get().path, NodePath(vec![0]));
    });
}

/// `Route(layout) { Stack { home, detail/:id } }` — a layout Route (a
/// `Route` with children, like `routes! { Route(component: X) { … } }`) above
/// the active stack.
fn layout_wrapped_handle() -> RouterHandle {
    let tree = CompiledTree::new(RouteTree::route_with(
        RouteDef::new("", "layout"),
        vec![RouteTree::stack(vec![
            RouteTree::route("", "home"),
            RouteTree::route("detail/:id", "detail"),
        ])],
    ));
    RouterHandle::new((tree, registry()))
}

fn layout_stack_history_len(h: &RouterHandle) -> usize {
    let RouteState::Route(r) = h.state().get() else {
        panic!("root is a layout route")
    };
    let RouteState::Stack(s) = &r.children[0] else {
        panic!("layout child is a stack")
    };
    s.history.len()
}

/// Regression: `replace` / `reset` (and `pop_to`) must work when a layout
/// Route sits above the active stack. `active_stack_mut` returned `None` for
/// any `Route`, so it disagreed with `deepest_active_stack_path` and the
/// `.expect("stack exists")` panicked — surfaced by the tabbed router example
/// (tap Replace / Reset → "panic in event handler for `tap`; event dropped").
#[test]
fn stack_ops_work_under_a_layout_route() {
    with_runtime(|| {
        let h = layout_wrapped_handle();
        h.navigate("/detail/1").unwrap();
        flush();

        // replace: swaps the top in place (no panic), depth unchanged.
        h.replace("/detail/2").unwrap();
        assert_eq!(
            h.current().get().params.get("id").map(String::as_str),
            Some("2"),
        );
        assert_eq!(layout_stack_history_len(&h), 2, "replace keeps depth");

        // reset: collapses the stack to a single entry (no panic).
        h.reset("/detail/5").unwrap();
        assert_eq!(
            h.current().get().params.get("id").map(String::as_str),
            Some("5"),
        );
        assert_eq!(layout_stack_history_len(&h), 1, "reset collapses to one");
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
        h.navigate("/detail/1").unwrap();
        h.select("/list").unwrap();
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
        h.select("/").unwrap();
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
        h.navigate("/detail/1").unwrap();
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
        h.navigate("/detail/1").unwrap();
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
        .route_with(
            "detail",
            RouteTransition::none(),
            mk("detail", counts.clone()),
        );
    RouterHandle::new((tree, registry))
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
        h.navigate("/detail/1").unwrap();
        flush();

        // detail mounted once; home was NOT re-mounted by the push.
        let c = counts.borrow();
        assert_eq!(c.get("detail").copied(), Some(1), "detail mounted once");
        assert_eq!(c.get("home").copied(), Some(1), "home not re-mounted");
    });
}

/// A single root Stack with mount-counting leaves (`home` at `""`, `detail`
/// at `detail/:id`).
fn counting_simple_handle(counts: Counts) -> RouterHandle {
    let tree = CompiledTree::new(RouteTree::stack(vec![
        RouteTree::route("", "home"),
        RouteTree::route("detail/:id", "detail"),
    ]));
    let mk = |id: &'static str, counts: Counts| {
        move |_: &RouteInstance| {
            *counts.borrow_mut().entry(id).or_insert(0) += 1;
            whisker::runtime::view::create_phantom_element()
        }
    };
    let registry = RouteRegistry::new()
        .route("home", mk("home", counts.clone()))
        .route_with(
            "detail",
            RouteTransition::none(),
            mk("detail", counts.clone()),
        );
    RouterHandle::new((tree, registry))
}

/// Regression for #264. When `reset` replaces the whole history with a route
/// that differs from the surviving index-0 wrapper, the reconcile must dispose
/// the stale survivor and mount the new route — not re-show the old screen.
/// (Wrappers are keyed by history index; a naive shrink kept the stale
/// survivor, so the cleared screen reappeared and its native views leaked.)
#[test]
fn reset_to_different_route_reinstantiates_revealed_top() {
    with_runtime(|| {
        let counts: Counts = Rc::new(RefCell::new(HashMap::new()));
        let h = counting_simple_handle(counts.clone());
        let _slot = mount_node(&h, NodePath::root());
        flush();
        // history: [home]; home mounted once.
        assert_eq!(counts.borrow().get("home").copied(), Some(1));

        // Push detail → history [home, detail/1].
        h.navigate("/detail/1").unwrap();
        flush();
        assert_eq!(counts.borrow().get("detail").copied(), Some(1));

        // Reset to a route DIFFERENT from index 0 (home) → history [detail/2].
        // The shrink reveals the index-0 survivor (home), which no longer
        // matches the history (detail/2), so it must be swapped.
        h.reset("/detail/2").unwrap();
        flush();

        let RouteState::Stack(s) = h.state().get() else {
            panic!("root is a stack")
        };
        assert_eq!(s.history.len(), 1, "reset collapses to a single entry");

        // The revealed top is a NEW detail leaf (mounted for /2), so detail
        // mounted twice total. Pre-fix this stayed at 1 — the stale `home`
        // wrapper was kept and `detail/2` never mounted.
        assert_eq!(
            counts.borrow().get("detail").copied(),
            Some(2),
            "reset must re-instantiate the revealed top when its route changed"
        );
    });
}

// ----- coordinated pop: survivor returns to the active (0%) pose -------

/// A single-stack handle with Slide routes so a push/pop runs a real
/// transition we can step to completion.
fn slide_stack_handle() -> RouterHandle {
    let tree = CompiledTree::new(RouteTree::stack(vec![
        RouteTree::route("", "home"),
        RouteTree::route("detail/:id", "detail"),
    ]));
    let registry = RouteRegistry::new()
        .route_with("home", RouteTransition::slide(), |_: &RouteInstance| {
            whisker::runtime::view::create_phantom_element()
        })
        .route_with("detail", RouteTransition::slide(), |_: &RouteInstance| {
            whisker::runtime::view::create_phantom_element()
        });
    RouterHandle::new((tree, registry))
}

/// `Route(layout) { Stack { home(slide), detail/:id(slide) } }` — the same
/// layout-Route-above-a-stack shape as the tabbed example, with slide
/// transitions so animation is observable.
fn layout_slide_handle() -> RouterHandle {
    let tree = CompiledTree::new(RouteTree::route_with(
        RouteDef::new("", "layout"),
        vec![RouteTree::stack(vec![
            RouteTree::route("", "home"),
            RouteTree::route("detail/:id", "detail"),
        ])],
    ));
    let registry = RouteRegistry::new()
        .route_with("home", RouteTransition::slide(), |_: &RouteInstance| {
            whisker::runtime::view::create_phantom_element()
        })
        .route_with("detail", RouteTransition::slide(), |_: &RouteInstance| {
            whisker::runtime::view::create_phantom_element()
        });
    RouterHandle::new((tree, registry))
}

/// Advance animation frames + flush until nothing is animating (or a
/// budget is hit), driving any in-flight push/pop/settle to completion.
fn settle_animations() {
    let mut t = 0.0_f64;
    for _ in 0..2000 {
        let still = whisker_animation::__step_for_tests(t);
        flush();
        if !still {
            break;
        }
        t += 16.0;
    }
}

#[test]
fn pop_settles_survivor_to_active_pose() {
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = slide_stack_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();

        // Push detail, let the slide-in finish.
        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        // Pop back to home; let the coordinated slide-out + reveal finish.
        assert!(h.back().is_ok());
        flush();
        settle_animations();

        // The bridge's top is now the revealed Home. Its pose binding must
        // resolve to the ACTIVE pose — Role::Top at progress 1.0, which is
        // translateX(0%): no parallax residue left from the push.
        let bridge = h
            .active_stack_bridge_for_test(&NodePath::root())
            .expect("a stack bridge is published");
        let top = bridge.top_pose.expect("top has a pose binding");
        let role = top.role.get();
        let progress = top.ctrl.get().value().get();
        let pose = RouteTransition::slide().pose(crate::render::transition::PoseContext::new(
            role,
            progress,
            crate::render::transition::Direction::Push,
        ));
        assert_eq!(
            pose.transform, "translateX(0%)",
            "survivor settled to active 0% pose; role={role:?} progress={progress}"
        );
    });
    owner.dispose();
}

#[test]
fn push_settles_top_to_full_progress() {
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = slide_stack_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();

        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        // After the push settles, the top (Detail) must be at progress 1.0
        // — the precondition that makes a later `back()` reverse animate a
        // full 1 → 0 slide-out instead of instant-finishing.
        let bridge = h
            .active_stack_bridge_for_test(&NodePath::root())
            .expect("bridge");
        let detail_ctrl = bridge.top_ctrl.clone().expect("detail ctrl");
        assert_eq!(
            detail_ctrl.value().get_untracked(),
            1.0,
            "top settles at progress 1.0 after push"
        );
    });
    owner.dispose();
}

/// Regression for #265. `replace` used to snap the new top to its final pose
/// (`ctrl.set_value(1.0)`); it must instead slide the new screen in (drive
/// 0 → 1) using the route transition.
#[test]
fn replace_animates_the_new_top_in() {
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = slide_stack_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();

        // Push detail/1 and let it settle (top at progress 1.0).
        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        // Replace the top (detail/1 → detail/2, same depth).
        h.replace("/detail/2").unwrap();
        flush();
        let top_ctrl = h
            .active_stack_bridge_for_test(&NodePath::root())
            .and_then(|b| b.top_ctrl)
            .expect("new top ctrl");

        // Step a few frames WITHOUT settling: the new top's progress must
        // traverse intermediate values (a visible slide-in), not be pinned at
        // 1.0 from the first frame (the pre-fix instant-swap bug).
        let mut t = 1000.0;
        let mut traj = Vec::new();
        for _ in 0..6 {
            whisker_animation::__step_for_tests(t);
            flush();
            traj.push(top_ctrl.value().get_untracked());
            t += 16.0;
        }
        assert!(
            traj.iter().any(|&p| p > 0.05 && p < 0.95),
            "replace should animate the new top in, not snap; traj={traj:?}"
        );

        // …and it must still settle fully present.
        settle_animations();
        assert_eq!(
            top_ctrl.value().get_untracked(),
            1.0,
            "replaced top settles at progress 1.0"
        );
    });
    owner.dispose();
}

/// After a `replace`, a `back` must still animate the revealed survivor (the
/// screen below) sliding in from covered → rest — not snap it in. Regression
/// for the "Home doesn't animate on back after replace" report.
#[test]
fn back_after_replace_animates_the_revealed_survivor() {
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = slide_stack_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();

        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        h.replace("/detail/2").unwrap();
        flush();
        settle_animations();

        // The survivor (home) below the replaced top.
        let under = h
            .active_stack_bridge_for_test(&NodePath::root())
            .and_then(|b| b.under_pose)
            .expect("home is the under after replace");

        // Back: the survivor must be coupled to the leaving controller and
        // slide in through intermediate frames.
        assert!(h.back().is_ok());
        flush();
        let mut t = 1000.0;
        let mut traj = Vec::new();
        for _ in 0..6 {
            whisker_animation::__step_for_tests(t);
            flush();
            traj.push(under.ctrl.get().value().get_untracked());
            t += 16.0;
        }
        assert!(
            traj.iter().any(|&p| p > 0.05 && p < 0.95),
            "the revealed survivor must animate in on back-after-replace; traj={traj:?}"
        );
    });
    owner.dispose();
}

/// Same as above but with a layout Route above the stack (the tabbed example's
/// shape). Reproduces the "Home doesn't animate on back after replace" report.
#[test]
fn back_after_replace_animates_under_a_layout_route() {
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = layout_slide_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();

        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        h.replace("/detail/2").unwrap();
        flush();
        settle_animations();

        let under = h
            .active_stack_bridge()
            .and_then(|b| b.under_pose)
            .expect("home is the under after replace");

        assert!(h.back().is_ok());
        flush();
        let mut t = 1000.0;
        let mut traj = Vec::new();
        for _ in 0..6 {
            whisker_animation::__step_for_tests(t);
            flush();
            traj.push(under.ctrl.get().value().get_untracked());
            t += 16.0;
        }
        assert!(
            traj.iter().any(|&p| p > 0.05 && p < 0.95),
            "survivor must animate in on back-after-replace under a layout route; traj={traj:?}"
        );
    });
    owner.dispose();
}

/// After a `replace`, the revealed under is a paused buried entry (the settle
/// freezes it because its pose controller is idle — unlike a push, whose under
/// is mid-animation and so stays live). An interactive swipe-back must resume
/// it so its pose effect follows the finger through the scrub; the gesture can
/// only re-point the bridge's pose bindings, so the bridge carries the under's
/// owner for exactly this. Regression for "SwipeBack intermediate animation
/// doesn't work after replace".
#[test]
fn swipe_back_resumes_the_paused_under_after_replace() {
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = layout_slide_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();

        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        h.replace("/detail/2").unwrap();
        flush();
        settle_animations();

        let under_owner = h
            .active_stack_bridge()
            .and_then(|b| b.under_owner)
            .expect("under owner present after replace");

        // The replace settle freezes (pauses) the buried under.
        assert!(
            under_owner.is_paused(),
            "precondition: the under is paused after a replace"
        );

        // Starting a swipe-back must resume it so the scrub animates it.
        crate::render::gesture::begin(&h, crate::render::transition::SwipeEdge::Left)
            .expect("swipe-back begins (stack can pop)");
        assert!(
            !under_owner.is_paused(),
            "swipe-back must resume the under so it follows the finger"
        );
    });
    owner.dispose();
}

#[test]
fn pop_animates_outgoing_top_through_intermediate_frames() {
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = slide_stack_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();

        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        let detail_ctrl = h
            .active_stack_bridge_for_test(&NodePath::root())
            .and_then(|b| b.top_ctrl)
            .expect("detail ctrl");

        // Pop, then step a few frames WITHOUT fully settling and record the
        // outgoing top's progress trajectory: it must descend through
        // intermediate values (a visible slide-out), not instant-finish at
        // 0 on the first frame (the "Detail vanishes from frame 1" bug).
        assert!(h.back().is_ok());
        flush();
        let mut t = 1000.0;
        let mut traj = Vec::new();
        for _ in 0..6 {
            whisker_animation::__step_for_tests(t);
            flush();
            traj.push(detail_ctrl.value().get_untracked());
            t += 16.0;
        }
        assert!(
            traj.iter().any(|&p| p > 0.05 && p < 0.95),
            "outgoing top should traverse intermediate reverse frames (visible \
             slide-out), not pop instantly; traj={traj:?}"
        );
    });
    owner.dispose();
}

#[test]
fn popped_leaf_content_survives_until_exit_animation_finishes() {
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        // Detail's render fn registers an `on_cleanup` bumping a counter,
        // so we can observe exactly WHEN its content subtree is disposed.
        let cleanups = Rc::new(RefCell::new(0usize));
        let tree = CompiledTree::new(RouteTree::stack(vec![
            RouteTree::route("", "home"),
            RouteTree::route("detail/:id", "detail"),
        ]));
        let registry = {
            let cleanups = cleanups.clone();
            RouteRegistry::new()
                .route_with("home", RouteTransition::slide(), |_: &RouteInstance| {
                    whisker::runtime::view::create_phantom_element()
                })
                .route_with(
                    "detail",
                    RouteTransition::slide(),
                    move |_: &RouteInstance| {
                        let cleanups = cleanups.clone();
                        whisker::on_cleanup(move || *cleanups.borrow_mut() += 1);
                        whisker::runtime::view::create_phantom_element()
                    },
                )
        };
        let h = RouterHandle::new((tree, registry));
        let _slot = mount_node(&h, NodePath::root());
        flush();

        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();
        assert_eq!(*cleanups.borrow(), 0, "detail content alive after push");

        // Pop. Immediately after `back()` + flush — BEFORE the exit
        // animation finishes — the detail content must STILL be mounted
        // (this is the bug: the leaf used to tear itself down the moment
        // its RouteState entry vanished, blanking the sliding-out screen).
        assert!(h.back().is_ok());
        flush();
        // step a couple of mid-animation frames
        whisker_animation::__step_for_tests(1000.0);
        flush();
        whisker_animation::__step_for_tests(1016.0);
        flush();
        assert_eq!(
            *cleanups.borrow(),
            0,
            "detail content must survive the exit animation (not torn down on \
             RouteState removal)"
        );

        // Once the exit animation finishes, run_pop disposes the popped
        // wrapper's owner, cascading to the detail content.
        settle_animations();
        assert_eq!(
            *cleanups.borrow(),
            1,
            "detail content disposed exactly once, on exit-animation finish"
        );
    });
    owner.dispose();
}

// ----- Android predictive-back: event → coordinated-scrub mapping -----
//
// The native module delivery can't run headless, but the mapping the
// `AndroidPredictiveBack` component performs — `backProgressed{progress}`
// -> scrub the top controller, `backInvoked` -> commit -> `back()` — is
// the shared `begin`/`scrub`/`settle` logic, which IS testable.

use crate::render::gesture::{back_progress, begin, scrub, settle};
use crate::render::transition::SwipeEdge;
use whisker::platform_module::WhiskerValue;

#[test]
fn back_progress_reads_payload() {
    let mut m = std::collections::BTreeMap::new();
    m.insert("progress".to_string(), WhiskerValue::Float(0.42));
    m.insert("swipeEdge".to_string(), WhiskerValue::Int(0));
    assert!((back_progress(&WhiskerValue::Map(m)) - 0.42).abs() < 1e-6);
    // Int progress coerces; out-of-range clamps; wrong shape → 0.
    let mut mi = std::collections::BTreeMap::new();
    mi.insert("progress".to_string(), WhiskerValue::Int(1));
    assert_eq!(back_progress(&WhiskerValue::Map(mi)), 1.0);
    assert_eq!(back_progress(&WhiskerValue::Null), 0.0);
}

#[test]
fn predictive_back_progress_scrubs_top_controller() {
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = slide_stack_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();
        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        // backStarted → begin(): grabs the active stack's bridge. On the
        // host build (the iOS/desktop slide-back path) it uses the route's
        // slide pose and does NOT drive the Material backdrop dim (that is
        // Android predictive-back only).
        let bridge = begin(&h, SwipeEdge::Left).expect("a poppable Slide stack yields a bridge");
        let ctrl = bridge.top_ctrl.clone().expect("top ctrl");
        let dim_drive = bridge.dim_drive.expect("dim drive published");
        assert!(
            dim_drive.get_untracked().is_none(),
            "host (slide-back) does not drive the Material dim"
        );

        // backProgressed{progress} → scrub: controller = 1 - progress.
        scrub(&bridge, back_progress(&progress_payload(0.0)));
        assert_eq!(ctrl.value().get_untracked(), 1.0, "progress 0 → present");
        scrub(&bridge, back_progress(&progress_payload(0.5)));
        assert_eq!(ctrl.value().get_untracked(), 0.5, "progress .5 → half away");
        scrub(&bridge, back_progress(&progress_payload(1.0)));
        assert_eq!(ctrl.value().get_untracked(), 0.0, "progress 1 → fully away");
    });
    owner.dispose();
}

#[test]
fn predictive_back_invoke_commits_pop() {
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = slide_stack_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();
        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();
        assert_eq!(h.current().get().path, NodePath(vec![1]), "on detail");

        // backStarted + a partial drag, then backInvoked (commit).
        let bridge = begin(&h, SwipeEdge::Left).expect("bridge");
        scrub(&bridge, 0.6);
        settle(&h, &bridge, /* commit = */ true, None);
        // The commit's on_finish(true) fires `back()` once the reverse
        // settles — drive the animation to completion.
        settle_animations();

        assert_eq!(h.current().get().path, NodePath(vec![0]), "popped to home");
    });
    owner.dispose();
}

#[test]
fn settle_commit_animates_from_current_value_without_jumping_back() {
    // A *partial* release commits by animating from the current value to 0
    // — it must NOT jump backward (toward 1) first. (Regression for the
    // bad re-anchor that pushed a deep-swipe value 0.05 back up to 0.18.)
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = slide_stack_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();
        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        let bridge = begin(&h, SwipeEdge::Left).expect("bridge");
        let ctrl = bridge.top_ctrl.clone().expect("ctrl");
        // Deep-but-not-complete swipe: back_progress 0.95 → value 0.05.
        scrub(&bridge, 0.95);
        let start = ctrl.value().get_untracked();
        assert!((start - 0.05).abs() < 1e-3, "value tracks the drag");

        settle(&h, &bridge, /* commit = */ true, None);
        // The value must never exceed the release point — no backward jump.
        // (The bad re-anchor set it to 0.18 here; the fix leaves it at 0.05.)
        let after_settle = ctrl.value().get_untracked();
        assert!(
            after_settle <= start + 1e-3,
            "settle must not push back toward 1; start={start} after={after_settle}"
        );
        // One mid-animation frame: still ≤ the start (heads toward 0, never
        // above). Don't over-step — once the short run finishes, `back()`
        // disposes the controller.
        whisker_animation::__step_for_tests(1008.0);
        flush();
        let mid = ctrl.value().get_untracked();
        assert!(
            mid <= start + 1e-3,
            "monotone toward 0, never above start; mid={mid}"
        );

        settle_animations();
        assert_eq!(h.current().get().path, NodePath(vec![0]), "commits the pop");
    });
    owner.dispose();
}

#[test]
fn settle_full_drag_commits_immediately() {
    // A full drag (value already ≈ 0) commits with no extra animation —
    // the dismiss is already visually complete; it just pops.
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = slide_stack_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();
        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        let bridge = begin(&h, SwipeEdge::Left).expect("bridge");
        scrub(&bridge, 1.0); // value ≈ 0
        settle(&h, &bridge, /* commit = */ true, None);
        settle_animations();
        assert_eq!(h.current().get().path, NodePath(vec![0]), "commits the pop");
    });
    owner.dispose();
}

#[test]
fn predictive_back_cancel_restores_top() {
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let h = slide_stack_handle();
        let _slot = mount_node(&h, NodePath::root());
        flush();
        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        let bridge = begin(&h, SwipeEdge::Left).expect("bridge");
        let ctrl = bridge.top_ctrl.clone().expect("top ctrl");
        scrub(&bridge, 0.4);
        // Cancel: forward() back to present; no pop.
        settle(&h, &bridge, /* commit = */ false, None);
        settle_animations();

        assert_eq!(ctrl.value().get_untracked(), 1.0, "top restored to present");
        assert_eq!(h.current().get().path, NodePath(vec![1]), "still on detail");
    });
    owner.dispose();
}

/// Verify that predictive-back works for a **grouped tabbed** tree (the
/// example-app shape: Route-with-children wrapping a Switch with group
/// routes). This is the exact scenario that failed on the real device.
#[test]
fn predictive_back_works_with_grouped_tabs() {
    use crate::core::{RouteDef, RouteTree};
    whisker::runtime::reactive::__reset_for_tests();
    whisker_animation::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(|| {
        let tree = CompiledTree::new(RouteTree::route_with(
            RouteDef {
                id: "tabs_layout".into(),
                segment: None,
                params: vec![],
                component: Some("tabs_layout".into()),
                is_group: false,
            },
            vec![RouteTree::switch(
                SwitchDef::new("tabs", 0),
                vec![
                    RouteTree::route_with(
                        RouteDef {
                            id: "(home)".into(),
                            segment: Some("(home)".into()),
                            params: vec![],
                            component: None,
                            is_group: true,
                        },
                        vec![RouteTree::stack(vec![
                            RouteTree::route("", "home"),
                            RouteTree::route("detail/:id", "detail"),
                        ])],
                    ),
                    RouteTree::route_with(
                        RouteDef {
                            id: "(search)".into(),
                            segment: Some("(search)".into()),
                            params: vec![],
                            component: None,
                            is_group: true,
                        },
                        vec![RouteTree::stack(vec![
                            RouteTree::route("list", "list"),
                            RouteTree::route("detail/:id", "detail"),
                        ])],
                    ),
                ],
            )],
        ));
        let reg = RouteRegistry::new()
            .route_with("home", RouteTransition::slide(), |_: &RouteInstance| {
                whisker::runtime::view::create_phantom_element()
            })
            .route_with("detail", RouteTransition::slide(), |_: &RouteInstance| {
                whisker::runtime::view::create_phantom_element()
            })
            .route_with("list", RouteTransition::slide(), |_: &RouteInstance| {
                whisker::runtime::view::create_phantom_element()
            })
            .route_with(
                "tabs_layout",
                RouteTransition::default(),
                |_: &RouteInstance| whisker::runtime::view::create_phantom_element(),
            );
        let h = RouterHandle::new((tree, reg));
        let _slot = mount_node(&h, NodePath::root());
        flush();

        // Navigate to detail in home tab.
        h.navigate("/detail/1").unwrap();
        flush();
        settle_animations();

        // Verify we're on detail.
        assert_eq!(
            h.current().get().path,
            NodePath(vec![0, 0, 0, 1]),
            "should be on detail in home tab"
        );

        // Try to begin a predictive-back gesture.
        let bridge = begin(&h, SwipeEdge::Left);
        assert!(
            bridge.is_some(),
            "begin() must return a bridge (stack has >1 entry)"
        );

        let bridge = bridge.unwrap();
        assert!(bridge.can_back, "can_back must be true");

        // Scrub and commit.
        scrub(&bridge, 0.5);
        settle(&h, &bridge, true, None);
        settle_animations();

        // Back should have popped to Home.
        assert_eq!(
            h.current().get().path,
            NodePath(vec![0, 0, 0, 0]),
            "commits the pop to home"
        );
    });
    owner.dispose();
}

fn progress_payload(p: f64) -> WhiskerValue {
    let mut m = std::collections::BTreeMap::new();
    m.insert("progress".to_string(), WhiskerValue::Float(p));
    WhiskerValue::Map(m)
}

// ----- Material predictive-back pose ----------------------------------

#[test]
fn back_edge_decodes_payload() {
    use crate::render::transition::SwipeEdge;
    let mk = |edge: i64| {
        let mut m = std::collections::BTreeMap::new();
        m.insert("swipeEdge".to_string(), WhiskerValue::Int(edge));
        WhiskerValue::Map(m)
    };
    assert_eq!(SwipeEdge::from_android(0), SwipeEdge::Left);
    assert_eq!(SwipeEdge::from_android(1), SwipeEdge::Right);
    // Unknown / missing → Left default.
    assert_eq!(SwipeEdge::from_android(7), SwipeEdge::Left);
    assert_eq!(crate::render::gesture::back_progress(&mk(1)), 0.0); // no progress key
}

#[test]
fn predictive_pose_material_shape() {
    use crate::render::transition::{Role, SwipeEdge, predictive_pose, set_device_corner_radius};

    // Pin a known device radius so the animated rounding is deterministic.
    set_device_corner_radius(40.0);

    // Two phases share the controller timeline:
    //  value 1.0 = present, 0.5 = preview max, 0.0 = committed/dismissed.

    // At rest (value 1.0): identity, square.
    let rest = predictive_pose(Role::Top, 1.0, SwipeEdge::Left);
    assert!(
        rest.transform.contains("scale(1)"),
        "scale 1 at rest: {rest:?}"
    );
    assert_eq!(rest.radius_px, 0.0, "square at rest");

    // Preview max (value 0.5): top shrunk to 0.9 (shared-element card),
    // rounded to the device radius, shifted toward the swipe edge.
    let preview = predictive_pose(Role::Top, 0.5, SwipeEdge::Left);
    assert!(
        preview.transform.contains("scale(0.9)"),
        "shrinks to 0.9 at preview max: {preview:?}"
    );
    assert!(
        (preview.radius_px - 40.0).abs() < 1e-3,
        "rounds to the device radius at preview max: {preview:?}"
    );
    assert!(
        preview.transform.contains("translateX(6%)"),
        "left-edge swipe shifts the card right: {preview:?}"
    );
    // Right-edge swipe shifts the card the other way (negative translateX).
    let preview_right = predictive_pose(Role::Top, 0.5, SwipeEdge::Right);
    assert!(
        preview_right.transform.contains("translateX(-6%)"),
        "right-edge swipe shifts the card left: {preview_right:?}"
    );
    // The shrink is DECELERATED (more apparent early): at value 0.75 the
    // preview is already well past linear (radius > the linear 20px).
    let mid = predictive_pose(Role::Top, 0.75, SwipeEdge::Left);
    assert!(
        mid.radius_px > 20.0 && mid.radius_px < 40.0,
        "decelerated radius is front-loaded (>linear 20, <max 40): {mid:?}"
    );

    // Committed (value 0.0): the top fades out as it leaves.
    let committed = predictive_pose(Role::Top, 0.0, SwipeEdge::Left);
    assert_eq!(
        committed.opacity, 0.0,
        "top fades out on commit: {committed:?}"
    );

    // Under (entering) scales together with the top card (down to 0.9) and
    // peeks from the left during the drag, then slides in to fully present
    // and grows back to full size on commit.
    let under_preview = predictive_pose(Role::Under, 0.5, SwipeEdge::Left);
    assert!(
        under_preview.transform.contains("scale(0.9)"),
        "under scales with the card to 0.9 at preview: {under_preview:?}"
    );
    assert!(
        under_preview.transform.contains("translateX(-60%)"),
        "under peeks from the left: {under_preview:?}"
    );
    // Mid-drag (value 0.75) the under screen is held at the SAME -60% peek —
    // it only scales while the finger is down, it does not slide.
    let under_mid = predictive_pose(Role::Under, 0.75, SwipeEdge::Left);
    assert!(
        under_mid.transform.contains("translateX(-60%)"),
        "under is fixed at the peek mid-drag (scale only, no slide): {under_mid:?}"
    );
    let under_committed = predictive_pose(Role::Under, 0.0, SwipeEdge::Left);
    assert!(
        under_committed.transform.contains("translateX(0%)"),
        "under slides to present on commit: {under_committed:?}"
    );
    assert!(
        under_committed.transform.contains("scale(1)"),
        "under grows back to full size on commit: {under_committed:?}"
    );
    assert!(
        under_committed.radius_px < 1e-3,
        "under un-rounds when present: {under_committed:?}"
    );

    // Restore the default so test ordering can't leak the global.
    set_device_corner_radius(24.0);
}

#[test]
fn screen_corner_radius_follows_device() {
    use crate::render::transition::{
        max_corner_radius, screen_corner_radius, set_device_corner_radius,
    };

    // The screen clip is the device display radius (24dp until set).
    set_device_corner_radius(24.0);
    assert!((max_corner_radius() - 24.0).abs() < 1e-3);
    assert!((screen_corner_radius() - 24.0).abs() < 1e-3);

    // A real device radius flows through to the screen clip.
    set_device_corner_radius(52.0);
    assert!((screen_corner_radius() - 52.0).abs() < 1e-3);

    // Restore so test ordering can't leak the global.
    set_device_corner_radius(24.0);
}

#[test]
fn registry_merge_keeps_first_and_folds_new_ids() {
    use crate::core::RouteTree;
    use crate::render::registry::RouteFragment;

    // A parent registry that already owns `detail` (with a slide).
    let base = RouteRegistry::new()
        .route("home", |_: &RouteInstance| {
            whisker::runtime::view::create_phantom_element()
        })
        .route_with("detail", RouteTransition::slide(), |_: &RouteInstance| {
            whisker::runtime::view::create_phantom_element()
        });

    // A spreadable fragment that re-declares `detail` (fade) and adds `post`.
    let frag = RouteFragment::new(
        vec![
            RouteTree::route("detail/:id", "detail"),
            RouteTree::route("post/:id", "post"),
        ],
        RouteRegistry::new()
            .route_with("detail", RouteTransition::fade(), |_: &RouteInstance| {
                whisker::runtime::view::create_phantom_element()
            })
            .route("post", |_: &RouteInstance| {
                whisker::runtime::view::create_phantom_element()
            }),
    );

    let merged = base.merge(frag.registry());

    // First declaration wins: the parent's `detail` (slide) is kept, not the
    // fragment's fade.
    assert_eq!(merged.transition("detail").name(), "slide");
    // A new id from the fragment is folded in.
    assert!(merged.contains("post"));
    assert!(merged.contains("home"));
    // The fragment exposes its roots for splicing at each `..` site.
    assert_eq!(frag.roots().len(), 2);
}

#[test]
fn predictive_dim_constant_during_drag_then_fades_on_commit() {
    use crate::render::transition::{PB_MAX_DIM, predictive_dim};

    // Across the whole drag (value 1.0 → 0.5) the scrim is held CONSTANT at
    // PB_MAX_DIM — it does not deepen as the finger drags further.
    for v in [1.0_f32, 0.9, 0.75, 0.6, 0.5] {
        assert!(
            (predictive_dim(v) - PB_MAX_DIM).abs() < 1e-3,
            "dim is constant at PB_MAX_DIM during the drag (value {v})"
        );
    }
    // Then it fades back out over the commit (value 0.5 → 0): the revealed
    // previous screen ends at full brightness, not darkened.
    assert!(
        predictive_dim(0.25) < predictive_dim(0.5),
        "dim starts fading once the finger releases (commit settle)"
    );
    assert!(
        predictive_dim(0.0) < 1e-3,
        "dim is gone once the back has committed (previous screen present)"
    );
}

#[test]
fn one_transition_poses_all_four_directional_slots() {
    use crate::render::transition::{
        Direction, Pose, PoseContext, PoseMode, Role, RouteTransition, Transition, pose_for,
    };

    // A single asymmetric transition that tags each (role × direction) case —
    // the four Jetpack-Compose slots expressed by ONE `Transition`.
    struct Asym;
    impl Transition for Asym {
        fn config(&self) -> whisker::AnimConfig {
            whisker::AnimConfig::ease_out(1)
        }
        fn pose(&self, ctx: PoseContext) -> Pose {
            let slot = match (ctx.role, ctx.direction) {
                (Role::Top, Direction::Push) => "enter",
                (Role::Under, Direction::Push) => "exit",
                (Role::Top, Direction::Pop) => "pop_exit",
                (Role::Under, Direction::Pop) => "pop_enter",
            };
            Pose::new(slot.to_string(), 1.0)
        }
    }

    let t = RouteTransition::custom(Asym);
    let push = PoseMode::Transition(t.clone(), Direction::Push);
    let pop = PoseMode::Transition(t, Direction::Pop);

    assert_eq!(pose_for(&push, Role::Top, 0.5).transform, "enter");
    assert_eq!(pose_for(&push, Role::Under, 0.5).transform, "exit");
    assert_eq!(pose_for(&pop, Role::Top, 0.5).transform, "pop_exit");
    assert_eq!(pose_for(&pop, Role::Under, 0.5).transform, "pop_enter");
}
