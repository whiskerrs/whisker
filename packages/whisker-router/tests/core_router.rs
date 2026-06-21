//! Exhaustive unit tests for the new router core (`whisker_router::core`).
//!
//! These build the Twitter-style tree from `docs/router-design.md` by
//! hand and assert URL derivation, `current` derivation, the five
//! operations, relative resolution, the buried-container reveal, and the
//! no-stored-marker invariant. The 14 numbered behaviours from the phase
//! brief are tagged in the test names / comments.

use std::collections::BTreeMap;

use whisker_router::core::{
    CompiledTree, NavError, Navigator, NodePath, RouteState, RouteTree, Scope, SwitchDef, Target,
};

// ===================================================================
// The Twitter-style tree (built by hand — no macro yet)
// ===================================================================
//
// The root node has the empty NodePath [], so its children start at [0]:
//
//  root Stack                         []
//   ├ Switch (tabs, default = 0)       [0]
//   │   ├ Stack timeline               [0,0]
//   │   │   ├ Route ""        timeline [0,0,0]
//   │   │   ├ Route "post/:id"  post   [0,0,1]   (shared, id "post")
//   │   │   └ Route "profile/:id" prof [0,0,2]   (shared, id "profile")
//   │   ├ Stack search                 [0,1]
//   │   │   ├ Route "search"  search   [0,1,0]
//   │   │   ├ Route "post/:id"  post   [0,1,1]
//   │   │   └ Route "profile/:id" prof [0,1,2]
//   │   ├ Stack notifications          [0,2]
//   │   │   ├ Route "notifications" .. [0,2,0]
//   │   │   ├ Route "post/:id"         [0,2,1]
//   │   │   └ Route "profile/:id"      [0,2,2]
//   │   └ Stack mypage                 [0,3]
//   │       ├ Route "mypage"           [0,3,0]
//   │       ├ Route "post/:id"         [0,3,1]
//   │       └ Route "profile/:id"      [0,3,2]
//   ├ Route "video/:id" video          [1]
//   └ Route "login"     login          [2]

fn shared_routes() -> Vec<RouteTree> {
    vec![
        RouteTree::route("post/:id", "post"),
        RouteTree::route("profile/:id", "profile"),
    ]
}

fn tab(root_segment: &str, root_id: &str) -> RouteTree {
    let mut children = vec![RouteTree::route(root_segment, root_id)];
    children.extend(shared_routes());
    RouteTree::stack(children)
}

fn twitter_tree() -> CompiledTree {
    let tabs = RouteTree::switch(
        SwitchDef::new("tabs", 0),
        vec![
            tab("", "timeline"),
            tab("search", "search"),
            tab("notifications", "notifications"),
            tab("mypage", "mypage"),
        ],
    );
    let root = RouteTree::stack(vec![
        tabs,
        RouteTree::route("video/:id", "video"),
        RouteTree::route("login", "login"),
    ]);
    CompiledTree::new(root)
}

fn p(indices: &[usize]) -> NodePath {
    NodePath(indices.to_vec())
}

fn one_param(k: &str, v: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(k.to_string(), v.to_string());
    m
}

// ===================================================================
// 1. URL derivation
// ===================================================================

#[test]
fn url_derivation_named_segments_concatenate() {
    let t = twitter_tree();
    assert_eq!(t.url_of(&p(&[0, 0, 0])).as_deref(), Some("/")); // timeline home
    assert_eq!(t.url_of(&p(&[0, 1, 0])).as_deref(), Some("/search"));
    assert_eq!(t.url_of(&p(&[0, 2, 0])).as_deref(), Some("/notifications"));
    assert_eq!(t.url_of(&p(&[0, 3, 0])).as_deref(), Some("/mypage"));
    assert_eq!(t.url_of(&p(&[1])).as_deref(), Some("/video/:id"));
    assert_eq!(t.url_of(&p(&[2])).as_deref(), Some("/login"));
    // shared post in each tab derives the same URL
    assert_eq!(t.url_of(&p(&[0, 0, 1])).as_deref(), Some("/post/:id"));
    assert_eq!(t.url_of(&p(&[0, 1, 1])).as_deref(), Some("/post/:id"));
    assert_eq!(t.url_of(&p(&[0, 3, 1])).as_deref(), Some("/post/:id"));
}

#[test]
fn url_pathless_containers_contribute_nothing() {
    let t = twitter_tree();
    // root Stack and the tabs Switch are pathless ⇒ no URL.
    assert_eq!(t.url_of(&p(&[])), None); // root Stack
    assert_eq!(t.url_of(&p(&[0])), None); // tabs Switch
}

#[test]
fn url_shared_post_dedupes_to_one_url_and_one_nav_id() {
    let t = twitter_tree();
    // Four placements of post, but ONE url and ONE nav-target id.
    let post_paths = t.paths_with_route_id("post");
    assert_eq!(post_paths.len(), 4, "four physical placements");
    let urls: std::collections::BTreeSet<_> =
        post_paths.iter().map(|pp| t.url_of(pp).unwrap()).collect();
    assert_eq!(urls.len(), 1, "all share /post/:id");
    assert!(urls.contains("/post/:id"));
    // And all four resolve by the single id "post".
    assert_eq!(t.paths_with_url("/post/:id").len(), 4);
}

// ===================================================================
// 2. current derivation after construction (defaults)
// ===================================================================

#[test]
fn initial_current_honours_switch_default_and_stack_first() {
    let t = twitter_tree();
    let st = RouteState::initial(&t);
    // root stack → first child (the Switch); Switch default 0 → timeline
    // stack → its first route "" (timeline home).
    assert_eq!(st.current().path, p(&[0, 0, 0]));
}

#[test]
fn initial_current_respects_nonzero_switch_default() {
    // A tiny tree with a Switch defaulting to branch 1.
    let tree = CompiledTree::new(RouteTree::stack(vec![RouteTree::switch(
        SwitchDef::new("s", 1),
        vec![
            RouteTree::stack(vec![RouteTree::route("a", "a")]),
            RouteTree::stack(vec![RouteTree::route("b", "b")]),
        ],
    )]));
    let st = RouteState::initial(&tree);
    assert_eq!(st.current().path, p(&[0, 1, 0])); // branch 1, route "b"
}

// ===================================================================
// 3. navigate within same tab (timeline → post) stays in timeline
// ===================================================================

#[test]
fn navigate_within_same_tab_lands_in_that_tabs_stack() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
        .unwrap();
    // timeline tab still selected; post is in timeline's stack.
    assert_eq!(nav.current().path, p(&[0, 0, 1]));
    assert_eq!(
        nav.current().params.get("id").map(String::as_str),
        Some("1")
    );
}

// ===================================================================
// 4. navigate to a shared route from a different tab → current tab
// ===================================================================

#[test]
fn navigate_shared_route_resolves_within_current_tab() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    // Move to the search tab first.
    nav.navigate(&Target::id("search")).unwrap();
    assert_eq!(nav.current().path, p(&[0, 1, 0]));
    // Now go to a profile: must resolve inside the SEARCH tab's subtree.
    nav.navigate(&Target::id("profile")).unwrap();
    assert_eq!(nav.current().path, p(&[0, 1, 2]));
}

// ===================================================================
// 5. navigate to a shared route from OUTSIDE the tabs (video)
//    → declaration-order first instance; selects that tab
// ===================================================================

#[test]
fn navigate_shared_route_from_outside_uses_declaration_order() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    // Push video (outside the tabs, on the root stack).
    nav.navigate_with(&Target::id("video"), one_param_inst("id", "9"))
        .unwrap();
    assert_eq!(nav.current().path, p(&[1]));
    // From video, go to post. Common ancestor is the root stack ⇒ the
    // first-declared post = timeline tab's post [0,0,0,1].
    nav.navigate_with(&Target::id("post"), one_param_inst("id", "7"))
        .unwrap();
    assert_eq!(nav.current().path, p(&[0, 0, 1]));
    // And the tabs Switch is now selected on timeline (branch 0).
    if let RouteState::Stack(root) = &st {
        if let RouteState::Switch(sw) = &root.history[0].state {
            assert_eq!(sw.selected, 0);
        } else {
            panic!("first root entry should be the Switch");
        }
    } else {
        panic!("root is a stack");
    }
}

// ===================================================================
// 6. navigate ALWAYS pushes (post(1), post(2) → two entries; even an
//    identical instance pushes again)
// ===================================================================

#[test]
fn navigate_always_pushes_distinct_instances() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
            .unwrap();
        nav.navigate_with(&Target::id("post"), one_param_inst("id", "2"))
            .unwrap();
        assert_eq!(nav.current().params.get("id").unwrap(), "2");
    }
    // timeline stack now: [ "", post(1), post(2) ]
    let hist = timeline_history(&st);
    assert_eq!(hist.len(), 3);
    assert_eq!(hist[1].state.current().params.get("id").unwrap(), "1");
    assert_eq!(hist[2].state.current().params.get("id").unwrap(), "2");
}

#[test]
fn navigate_always_pushes_even_identical_instance() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
        .unwrap();
    nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
        .unwrap();
    let hist = timeline_history(&st);
    // Two post entries even though params are identical ("always push").
    assert_eq!(hist.len(), 3);
    assert_eq!(hist[1].state.current().params.get("id").unwrap(), "1");
    assert_eq!(hist[2].state.current().params.get("id").unwrap(), "1");
}

// ===================================================================
// 7. buried-container reveal: from video, navigate to a tab post → pops
//    video, selects tab, pushes post; path goes back through the Switch
// ===================================================================

#[test]
fn navigate_reveals_buried_tabs_switch() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    {
        let mut nav = Navigator::new(&t, &mut st);
        // Drive the search tab into a post first so we can check the
        // Switch's retained selection survives being buried.
        nav.navigate(&Target::id("search")).unwrap();
        // Now push video over the tabs.
        nav.navigate_with(&Target::id("video"), one_param_inst("id", "1"))
            .unwrap();
        assert_eq!(nav.current().path, p(&[1]));
    }
    // root stack history: [ Switch, video ]
    assert_eq!(root_history_len(&st), 2);

    // Navigate to post. The first-declared post is timeline's, so the
    // buried Switch is revealed (video popped) AND its branch flips to
    // timeline. Current path passes through the Switch again.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate_with(&Target::id("post"), one_param_inst("id", "5"))
            .unwrap();
        assert_eq!(nav.current().path, p(&[0, 0, 1]));
    }
    // video was popped → root stack back to length 1 (just the Switch).
    assert_eq!(root_history_len(&st), 1);
    // The active chain goes root-stack → Switch → timeline-stack → post,
    // i.e. the Switch is on the path (tabs "visible").
    let chain_kinds = active_chain_kinds(&st);
    assert_eq!(chain_kinds, vec!["Stack", "Switch", "Stack", "Route"]);
}

// ===================================================================
// 8. back: deepest non-trivial stack; reveals buried; no-op at tab root;
//    Switch never popped
// ===================================================================

#[test]
fn back_pops_deepest_nontrivial_stack() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
        .unwrap();
    assert_eq!(nav.current().path, p(&[0, 0, 1])); // timeline post
    assert!(nav.back());
    // Back to timeline home.
    assert_eq!(nav.current().path, p(&[0, 0, 0]));
}

#[test]
fn back_from_outside_reveals_tab_screen() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    // timeline → post, then push video over the tabs.
    nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
        .unwrap();
    nav.navigate_with(&Target::id("video"), one_param_inst("id", "2"))
        .unwrap();
    assert_eq!(nav.current().path, p(&[1]));
    // back pops video off the ROOT stack (the deepest non-trivial stack
    // on the active path is the root: the tabs Switch hides the inner
    // post, but `video` lives directly on root).
    assert!(nav.back());
    // Reveals the timeline post that the Switch retained.
    assert_eq!(nav.current().path, p(&[0, 0, 1]));
}

#[test]
fn back_at_tab_root_is_noop() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let before = st.clone();
    let mut nav = Navigator::new(&t, &mut st);
    // At the timeline home with nothing pushed anywhere → no-op.
    assert!(!nav.back());
    assert_eq!(st, before);
}

#[test]
fn back_never_pops_switch_selection() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    // Select search (a pure Switch change), no stack pushes.
    nav.select(&Target::id("search")).unwrap();
    assert_eq!(nav.current().path, p(&[0, 1, 0]));
    // back has nothing to pop (search stack is trivial, root is trivial)
    // and must NOT revert the Switch selection.
    assert!(!nav.back());
    assert_eq!(nav.current().path, p(&[0, 1, 0]));
}

// ===================================================================
// 9. replace: swaps top of current stack; cross-switch replace errors
// ===================================================================

#[test]
fn replace_swaps_top_of_current_stack() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
        .unwrap();
    // Replace the top post with a profile (same timeline stack).
    nav.replace_with(&Target::id("profile"), one_param("id", "9"))
        .unwrap();
    assert_eq!(nav.current().path, p(&[0, 0, 2]));
    assert_eq!(nav.current().params.get("id").unwrap(), "9");
    // History length unchanged: [ "", profile(9) ].
    assert_eq!(timeline_history(&st).len(), 2);
}

#[test]
fn replace_cross_switch_errors() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    // The current stack is the timeline tab's stack. `video` lives on the
    // ROOT stack (a different stack), so replacing to it must error.
    let err = nav.replace(&Target::id("video")).unwrap_err();
    assert_eq!(err, NavError::CrossStack);
    // State unchanged.
    assert_eq!(nav.current().path, p(&[0, 0, 0]));
}

// ===================================================================
// 10. pop_to: unwinds the current stack to a target
// ===================================================================

#[test]
fn pop_to_unwinds_current_stack() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
            .unwrap();
        nav.navigate_with(&Target::id("profile"), one_param_inst("id", "2"))
            .unwrap();
        nav.navigate_with(&Target::id("post"), one_param_inst("id", "3"))
            .unwrap();
    }
    // timeline: [ "", post(1), profile(2), post(3) ]
    assert_eq!(timeline_history(&st).len(), 4);
    // pop_to the timeline home route "" (id "timeline").
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.pop_to(&Target::id("timeline")).unwrap();
        assert_eq!(nav.current().path, p(&[0, 0, 0]));
    }
    assert_eq!(timeline_history(&st).len(), 1);
}

#[test]
fn pop_to_missing_target_errors() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
        .unwrap();
    // profile is a child of this stack but no profile entry is present.
    let err = nav.pop_to(&Target::id("profile")).unwrap_err();
    assert_eq!(err, NavError::NotInStack);
}

// ===================================================================
// 11. reset: clears the current stack to [target] (logout case)
// ===================================================================

#[test]
fn reset_clears_current_stack_to_single_entry() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
            .unwrap();
        nav.navigate_with(&Target::id("post"), one_param_inst("id", "2"))
            .unwrap();
    }
    assert_eq!(timeline_history(&st).len(), 3);
    // Reset the timeline stack to its home.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.reset(&Target::id("timeline")).unwrap();
        assert_eq!(nav.current().path, p(&[0, 0, 0]));
    }
    assert_eq!(timeline_history(&st).len(), 1);
}

#[test]
fn reset_logout_clears_root_back_stack() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    // Push video so the ROOT stack is non-trivial, and make login the
    // current stack's target. To reset the ROOT stack we navigate to a
    // root-level route first so the deepest active stack IS the root.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate_with(&Target::id("video"), one_param_inst("id", "1"))
            .unwrap();
    }
    assert_eq!(root_history_len(&st), 2);
    // current stack is the root stack (video is a leaf directly on root).
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.reset(&Target::id("login")).unwrap();
        assert_eq!(nav.current().path, p(&[2]));
    }
    assert_eq!(root_history_len(&st), 1);
}

// ===================================================================
// 12. cold-start resolution → declaration order; Switch(default) honored
// ===================================================================

#[test]
fn cold_start_resolution_uses_declaration_order() {
    let t = twitter_tree();
    // No current position (cold deep-link): resolve directly with
    // `current = None`. Must pick the first-declared post (timeline's).
    let dest = whisker_router::core::resolve(&t, &Target::id("post"), None).unwrap();
    assert_eq!(dest, p(&[0, 0, 1]));
    // And profile cold-start → first-declared profile too.
    let prof = whisker_router::core::resolve(&t, &Target::id("profile"), None).unwrap();
    assert_eq!(prof, p(&[0, 0, 2]));
}

#[test]
fn switch_default_honored_for_return_branch() {
    // A Switch defaulting to branch 2 is honored on initial state even
    // though it was never explicitly visited.
    let tree = CompiledTree::new(RouteTree::stack(vec![RouteTree::switch(
        SwitchDef::new("s", 2),
        vec![
            RouteTree::stack(vec![RouteTree::route("a", "a")]),
            RouteTree::stack(vec![RouteTree::route("b", "b")]),
            RouteTree::stack(vec![RouteTree::route("c", "c")]),
        ],
    )]));
    let st = RouteState::initial(&tree);
    assert_eq!(st.current().path, p(&[0, 2, 0])); // branch 2, route "c"
}

// ===================================================================
// 13. No-marker invariant: current() is always the walked leaf, and the
//     type has no stored current field. Property-ish over an op sequence.
// ===================================================================

#[test]
fn no_marker_current_is_always_the_walked_leaf() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);

    // A scripted op sequence; after each step assert current() equals an
    // INDEPENDENT manual walk of history-tops / selecteds.
    type Op = Box<dyn Fn(&mut Navigator)>;
    let ops: Vec<Op> = vec![
        Box::new(|n| {
            n.navigate(&Target::id("search")).unwrap();
        }),
        Box::new(|n| {
            n.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
                .unwrap();
        }),
        Box::new(|n| {
            n.navigate_with(&Target::id("video"), one_param_inst("id", "2"))
                .unwrap();
        }),
        Box::new(|n| {
            n.navigate_with(&Target::id("post"), one_param_inst("id", "3"))
                .unwrap();
        }),
        Box::new(|n| {
            n.back();
        }),
        Box::new(|n| {
            n.back();
        }),
        Box::new(|n| {
            n.navigate(&Target::id("mypage")).unwrap();
        }),
    ];

    for op in ops {
        {
            let mut nav = Navigator::new(&t, &mut st);
            op(&mut nav);
        }
        let derived = st.current().path.clone();
        let manual = manual_walk(&st);
        assert_eq!(
            derived, manual,
            "current() must equal an independent history-top/selected walk"
        );
    }
}

/// Independent re-derivation of the current leaf, used to prove there is
/// no stored marker that could drift from the walk.
fn manual_walk(state: &RouteState) -> NodePath {
    match state {
        RouteState::Route(r) => r.path.clone(),
        RouteState::Stack(s) => manual_walk(&s.history.last().unwrap().state),
        RouteState::Switch(s) => manual_walk(&s.branches[s.selected]),
    }
}

// ===================================================================
// 14. each tab keeps an independent stack
// ===================================================================

#[test]
fn tabs_keep_independent_stacks() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);

    // Drive timeline into a post.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate_with(&Target::id("post"), one_param_inst("id", "11"))
            .unwrap();
        assert_eq!(nav.current().path, p(&[0, 0, 1]));
    }
    // Switch to search (a pure `select`) and drive it into a profile.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.select(&Target::id("search")).unwrap();
        nav.navigate_with(&Target::id("profile"), one_param_inst("id", "22"))
            .unwrap();
        assert_eq!(nav.current().path, p(&[0, 1, 2]));
    }
    // Timeline stack untouched: still [ "", post(11) ].
    assert_eq!(timeline_history(&st).len(), 2);
    assert_eq!(
        timeline_history(&st)[1]
            .state
            .current()
            .params
            .get("id")
            .unwrap(),
        "11"
    );
    // Switch back to timeline via `select`: its post is preserved exactly
    // (no fresh home pushed — that is the whole point of `select` vs
    // `navigate`).
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.select(&Target::id("timeline")).unwrap();
        assert_eq!(nav.current().path, p(&[0, 0, 1])); // back on the post
    }
    assert_eq!(timeline_history(&st).len(), 2);
    // search stack still has its profile, untouched by timeline.
    assert_eq!(search_history(&st).len(), 2);
    assert_eq!(
        search_history(&st)[1]
            .state
            .current()
            .params
            .get("id")
            .unwrap(),
        "22"
    );
}

#[test]
fn switching_tabs_preserves_each_stack_via_select() {
    // Use the `select` primitive and confirm the OTHER tab's stack is
    // preserved across the switch.
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
            .unwrap(); // timeline: ["", post]
    }
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.select(&Target::id("search")).unwrap(); // select search
    }
    // timeline preserved its 2-entry stack while search is active.
    assert_eq!(timeline_history(&st).len(), 2);
    // back in search (trivial) is a no-op and doesn't touch timeline.
    {
        let mut nav = Navigator::new(&t, &mut st);
        assert!(!nav.back());
    }
    assert_eq!(timeline_history(&st).len(), 2);
}

#[test]
fn select_is_nondestructive_and_returns_to_retained_screen() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    // Drive timeline deep: ["", post(1), post(2)].
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate_with(&Target::id("post"), one_param_inst("id", "1"))
            .unwrap();
        nav.navigate_with(&Target::id("post"), one_param_inst("id", "2"))
            .unwrap();
    }
    // select away to search, then back to timeline.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.select(&Target::id("search")).unwrap();
        assert_eq!(nav.current().path, p(&[0, 1, 0])); // search home
        nav.select(&Target::id("timeline")).unwrap();
        // Returns to timeline's RETAINED top (post(2)) — nothing pushed.
        assert_eq!(nav.current().path, p(&[0, 0, 1]));
        assert_eq!(nav.current().params.get("id").unwrap(), "2");
    }
    // timeline history untouched (still 3 deep).
    assert_eq!(timeline_history(&st).len(), 3);
}

// ===================================================================
// within(scope) API-surface smoke (deferred behaviour)
// ===================================================================

#[test]
fn within_scope_restricts_resolution_to_branch() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    // From timeline, target post but scope it to the search tab subtree.
    let search_scope = Scope::at(p(&[0, 1]));
    nav.navigate_within(&Target::id("post"), &search_scope)
        .unwrap();
    // Resolves inside the search tab → its post.
    assert_eq!(nav.current().path, p(&[0, 1, 1]));
}

// ===================================================================
// Error: unknown target
// ===================================================================

#[test]
fn navigate_unknown_target_errors() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    let err = nav.navigate(&Target::id("nope")).unwrap_err();
    assert_eq!(err, NavError::NoSuchTarget);
}

// ===================================================================
// helpers that reach into the state tree for assertions
// ===================================================================

fn one_param_inst(k: &str, v: &str) -> whisker_router::core::RouteInstance {
    whisker_router::core::RouteInstance::with_param(NodePath::root(), k, v)
}

fn root_history_len(st: &RouteState) -> usize {
    match st {
        RouteState::Stack(s) => s.history.len(),
        _ => panic!("root is a stack"),
    }
}

fn timeline_history(st: &RouteState) -> &[whisker_router::core::StackEntry] {
    tab_history(st, 0)
}

fn search_history(st: &RouteState) -> &[whisker_router::core::StackEntry] {
    tab_history(st, 1)
}

fn tab_history(st: &RouteState, branch: usize) -> &[whisker_router::core::StackEntry] {
    if let RouteState::Stack(root) = st {
        // The Switch is always root entry 0 (it's never popped; even when
        // revealed it stays at index 0).
        if let RouteState::Switch(sw) = &root.history[0].state {
            if let RouteState::Stack(tab) = &sw.branches[branch] {
                return &tab.history;
            }
        }
    }
    panic!("could not reach tab {branch} history");
}

fn active_chain_kinds(st: &RouteState) -> Vec<&'static str> {
    st.active_chain()
        .into_iter()
        .map(|n| match n {
            RouteState::Route(_) => "Route",
            RouteState::Stack(_) => "Stack",
            RouteState::Switch(_) => "Switch",
        })
        .collect()
}
