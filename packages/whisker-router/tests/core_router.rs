//! Exhaustive unit tests for the router core (`whisker_router::core`).
//!
//! These build the Twitter-style tree from `docs/router-design.md` by
//! hand and assert URL derivation, `current` derivation, the five
//! six operations, relative resolution, the buried-container reveal, and the
//! no-stored-marker invariant. The 14 numbered behaviours the design doc
//! enumerates are tagged in the section comments below.

use whisker_router::core::{
    CompiledTree, NavError, Navigator, NodePath, ReselectBehavior, RouteDef, RouteState, RouteTree,
    Scope, SwitchDef,
};

/// Mirror the router example: a layout `Route` over a `Switch` whose branches
/// are `(group) → Stack` tabs (home tab + search tab).
fn grouped_tabs_tree() -> CompiledTree {
    CompiledTree::new(RouteTree::route_with(
        RouteDef::new("", "layout"),
        vec![RouteTree::switch(
            SwitchDef::new("tabs", 0),
            vec![
                RouteTree::route_with(
                    RouteDef::new("(home)", "home_grp"),
                    vec![RouteTree::stack(vec![
                        RouteTree::route("", "home"),
                        RouteTree::route("detail/:id", "detail"),
                    ])],
                ),
                RouteTree::route_with(
                    RouteDef::new("(search)", "search_grp"),
                    vec![RouteTree::stack(vec![
                        RouteTree::route("list", "list"),
                        RouteTree::route("detail/:id", "detail"),
                    ])],
                ),
            ],
        )],
    ))
}

#[test]
fn reset_to_home_tab_from_another_tab() {
    let t = grouped_tabs_tree();
    let mut st = RouteState::initial(&t);
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate("/list").unwrap(); // switch to the search tab
        nav.navigate("/detail/1").unwrap(); // list → detail
        assert_eq!(
            nav.current().path,
            NodePath(vec![0, 1, 0, 1]),
            "in search/detail"
        );
    }
    {
        let mut nav = Navigator::new(&t, &mut st);
        // The home is the index "" route under the "(home)" group. "/" must
        // resolve to that leaf screen — not the group container (which shares
        // the URL) — so reset("/") from the search tab lands on the Home tab.
        nav.reset("/").unwrap();
        assert_eq!(
            nav.current().path,
            NodePath(vec![0, 0, 0, 0]),
            "reset(\"/\") from the search tab lands on the Home tab"
        );
    }
}

#[test]
fn bare_group_url_resolves_to_its_first_screen() {
    // The "(search)" group has no index "" child. A bare "/(search)" (e.g. a
    // tab-bar `navigate("/(search)")`) selects the group and restores its
    // retained stack without creating a screen entry.
    let t = grouped_tabs_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    nav.navigate("/(search)").unwrap();
    assert_eq!(
        nav.current().path,
        NodePath(vec![0, 1, 0, 0]),
        "navigate(\"/(search)\") lands on the search tab's first screen (list)"
    );
}

#[test]
fn qualified_push_targets_that_group_and_keeps_the_public_location_clean() {
    let t = grouped_tabs_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);

    nav.push("/(search)/detail/42?from=home#reply").unwrap();

    assert_eq!(nav.current().path, NodePath(vec![0, 1, 0, 1]));
    assert_eq!(
        nav.current().params.get("id").map(String::as_str),
        Some("42")
    );
    assert_eq!(nav.current().location.pathname, "/detail/42");
    assert_eq!(
        nav.current().location.to_url(),
        "/detail/42?from=home#reply"
    );
    assert_eq!(grouped_history(&st, 0).len(), 1, "home is untouched");
    assert_eq!(grouped_history(&st, 1).len(), 2, "search receives the push");
}

#[test]
fn group_reselect_pops_to_root_but_switching_back_restores_history() {
    let t = grouped_tabs_tree();
    let mut st = RouteState::initial(&t);
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.push("/detail/1").unwrap();
        nav.navigate("/(search)").unwrap();
        nav.navigate("/(home)").unwrap();
        assert_eq!(nav.current().path, NodePath(vec![0, 0, 0, 1]));
    }
    assert_eq!(grouped_history(&st, 0).len(), 2, "inactive switch restores");

    let mut nav = Navigator::new(&t, &mut st);
    nav.navigate("/(home)").unwrap();
    assert_eq!(nav.current().path, NodePath(vec![0, 0, 0, 0]));
    assert_eq!(
        grouped_history(&st, 0).len(),
        1,
        "active reselect pops root"
    );
}

#[test]
fn group_reselect_can_preserve_the_active_stack() {
    let tree = CompiledTree::new(RouteTree::route_with(
        RouteDef::new("(home)", "home_group"),
        vec![RouteTree::switch(
            SwitchDef::new("tabs", 0).with_reselect(ReselectBehavior::Preserve),
            vec![RouteTree::route_with(
                RouteDef::new("(feed)", "feed_group"),
                vec![RouteTree::stack(vec![
                    RouteTree::route("", "feed"),
                    RouteTree::route("detail/:id", "detail"),
                ])],
            )],
        )],
    ));
    let mut state = RouteState::initial(&tree);
    let mut nav = Navigator::new(&tree, &mut state);

    nav.push("/(home)/(feed)/detail/1").unwrap();
    nav.navigate("/(home)/(feed)").unwrap();

    assert_eq!(nav.current().path, NodePath(vec![0, 0, 0, 1]));
}

#[test]
fn push_rejects_a_group_without_a_screen() {
    let t = grouped_tabs_tree();
    let mut st = RouteState::initial(&t);
    let before = st.clone();
    let mut nav = Navigator::new(&t, &mut st);
    assert_eq!(nav.push("/(search)").unwrap_err(), NavError::ExpectedRoute);
    assert_eq!(st, before);
}

#[test]
fn nested_group_destination_is_recognized_by_the_route_tree() {
    let tree = CompiledTree::new(RouteTree::route_with(
        RouteDef::new("settings", "settings_layout"),
        vec![RouteTree::switch(
            SwitchDef::new("settings_tabs", 0),
            vec![
                RouteTree::route_with(
                    RouteDef::new("(account)", "account_group"),
                    vec![RouteTree::stack(vec![RouteTree::route("", "account")])],
                ),
                RouteTree::route_with(
                    RouteDef::new("(privacy)", "privacy_group"),
                    vec![RouteTree::stack(vec![RouteTree::route("", "privacy")])],
                ),
            ],
        )],
    ));
    let mut state = RouteState::initial(&tree);
    let mut nav = Navigator::new(&tree, &mut state);

    nav.navigate("/settings/(privacy)").unwrap();

    assert_eq!(nav.current().path, NodePath(vec![0, 1, 0, 0]));
    assert_eq!(nav.current().location.pathname, "/settings");
}

#[test]
fn navigate_unwinds_an_identical_entry_while_push_duplicates_it() {
    let t = grouped_tabs_tree();
    let mut st = RouteState::initial(&t);
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.push("/detail/1").unwrap();
        nav.push("/detail/2").unwrap();
        nav.navigate("/detail/1").unwrap();
    }
    assert_eq!(grouped_history(&st, 0).len(), 2);

    let mut nav = Navigator::new(&t, &mut st);
    nav.push("/detail/1").unwrap();
    assert_eq!(grouped_history(&st, 0).len(), 3);
}

// ===================================================================
// The Twitter-style tree, built by hand so the core is tested without
// the `routes!` macro.
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
    nav.navigate("/post/1").unwrap();
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
    nav.navigate("/search").unwrap();
    assert_eq!(nav.current().path, p(&[0, 1, 0]));
    // Now go to a profile: must resolve inside the SEARCH tab's subtree.
    nav.navigate("/profile/1").unwrap();
    assert_eq!(nav.current().path, p(&[0, 1, 2]));
}

// ===================================================================
// 5. navigate to a shared route from OUTSIDE the tabs (video)
//    → ambiguous until the caller supplies a group qualifier
// ===================================================================

#[test]
fn unqualified_shared_route_from_outside_is_ambiguous() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    // Push video (outside the tabs, on the root stack).
    nav.navigate("/video/9").unwrap();
    assert_eq!(nav.current().path, p(&[1]));
    assert_eq!(
        nav.navigate("/post/7").unwrap_err(),
        NavError::AmbiguousRoute
    );
    assert_eq!(nav.current().path, p(&[1]), "failed resolution is atomic");
}

// ===================================================================
// 6. navigate pushes a missing concrete location; push always appends
// ===================================================================

#[test]
fn navigate_pushes_distinct_locations() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate("/post/1").unwrap();
        nav.navigate("/post/2").unwrap();
        assert_eq!(nav.current().params.get("id").unwrap(), "2");
    }
    // timeline stack now: [ "", post(1), post(2) ]
    let hist = timeline_history(&st);
    assert_eq!(hist.len(), 3);
    assert_eq!(hist[1].state.current().params.get("id").unwrap(), "1");
    assert_eq!(hist[2].state.current().params.get("id").unwrap(), "2");
}

#[test]
fn push_always_pushes_even_identical_instance() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    nav.push("/post/1").unwrap();
    nav.push("/post/1").unwrap();
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
        nav.navigate("/search").unwrap();
        // Now push video over the tabs.
        nav.navigate("/video/1").unwrap();
        assert_eq!(nav.current().path, p(&[1]));
    }
    // root stack history: [ Switch, video ]
    assert_eq!(root_history_len(&st), 2);

    // Navigate to post. The first-declared post is timeline's, so the
    // buried Switch is revealed (video popped) AND its branch flips to
    // timeline. Current path passes through the Switch again.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate_within("/post/5", &Scope::at(p(&[0, 0])))
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
    nav.navigate("/post/1").unwrap();
    assert_eq!(nav.current().path, p(&[0, 0, 1])); // timeline post
    nav.back().unwrap();
    // Back to timeline home.
    assert_eq!(nav.current().path, p(&[0, 0, 0]));
}

#[test]
fn back_from_outside_reveals_tab_screen() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    // timeline → post, then push video over the tabs.
    nav.navigate("/post/1").unwrap();
    nav.navigate("/video/2").unwrap();
    assert_eq!(nav.current().path, p(&[1]));
    // back pops video off the ROOT stack (the deepest non-trivial stack
    // on the active path is the root: the tabs Switch hides the inner
    // post, but `video` lives directly on root).
    nav.back().unwrap();
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
    assert!(nav.back().is_err());
    assert_eq!(st, before);
}

#[test]
fn back_never_pops_switch_selection() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    // Select search (a pure Switch change), no stack pushes.
    nav.navigate("/search").unwrap();
    assert_eq!(nav.current().path, p(&[0, 1, 0]));
    // back has nothing to pop (search stack is trivial, root is trivial)
    // and must NOT revert the Switch selection.
    assert!(nav.back().is_err());
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
    nav.navigate("/post/1").unwrap();
    // Replace the top post with a profile (same timeline stack).
    nav.replace("/profile/9").unwrap();
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
    let err = nav.replace("/video/1").unwrap_err();
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
        nav.navigate("/post/1").unwrap();
        nav.navigate("/profile/2").unwrap();
        nav.navigate("/post/3").unwrap();
    }
    // timeline: [ "", post(1), profile(2), post(3) ]
    assert_eq!(timeline_history(&st).len(), 4);
    // pop_to the timeline home route "" (id "timeline").
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.pop_to("/").unwrap();
        assert_eq!(nav.current().path, p(&[0, 0, 0]));
    }
    assert_eq!(timeline_history(&st).len(), 1);
}

#[test]
fn pop_to_missing_target_errors() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    nav.navigate("/post/1").unwrap();
    // profile is a child of this stack but no profile entry is present.
    let err = nav.pop_to("/profile/1").unwrap_err();
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
        nav.navigate("/post/1").unwrap();
        nav.navigate("/post/2").unwrap();
    }
    assert_eq!(timeline_history(&st).len(), 3);
    // Reset the timeline stack to its home.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.reset("/").unwrap();
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
        nav.navigate("/video/1").unwrap();
    }
    assert_eq!(root_history_len(&st), 2);
    // current stack is the root stack (video is a leaf directly on root).
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.reset("/login").unwrap();
        assert_eq!(nav.current().path, p(&[2]));
    }
    assert_eq!(root_history_len(&st), 1);
}

#[test]
fn reset_is_global_clears_every_stack_and_branch() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    // Deepen the timeline stack, switch to the search tab, deepen that too.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate("/post/1").unwrap();
        nav.navigate("/post/2").unwrap(); // timeline depth 3
        nav.navigate("/search").unwrap();
        nav.navigate("/post/9").unwrap(); // search depth 2
    }
    assert_eq!(timeline_history(&st).len(), 3);
    assert_eq!(search_history(&st).len(), 2);

    // A global reset to the timeline home must collapse EVERYTHING — the
    // target branch, the other branch, and the root stack — to one entry.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.reset("/").unwrap();
        assert_eq!(nav.current().path, p(&[0, 0, 0]), "current = timeline home");
    }
    assert_eq!(root_history_len(&st), 1, "root stack collapsed");
    assert_eq!(timeline_history(&st).len(), 1, "target branch collapsed");
    assert_eq!(
        search_history(&st).len(),
        1,
        "the OTHER branch (search) is cleared to its root too"
    );
    assert_eq!(switch_selected(&st), 0, "switch selects the target branch");
}

#[test]
fn reset_crosses_into_another_branch() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate("/post/1").unwrap(); // timeline depth 2, switch on timeline
        // Reset straight into the search tab — a different Switch branch,
        // not just a same-stack reset.
        nav.reset("/search").unwrap();
        assert_eq!(nav.current().path, p(&[0, 1, 0]), "current = search home");
    }
    assert_eq!(switch_selected(&st), 1, "switch moved to search");
    assert_eq!(timeline_history(&st).len(), 1, "timeline cleared");
    assert_eq!(search_history(&st).len(), 1);
}

// ===================================================================
// 12. cold-start resolution follows the configured initial Switch branch
// ===================================================================

#[test]
fn cold_start_resolution_uses_initial_branch() {
    let t = twitter_tree();
    // No current position (cold deep-link): resolve directly with
    // `current = None`. The configured initial branch is timeline.
    let dest = whisker_router::core::resolve(&t, "/post/1", None).unwrap();
    assert_eq!(dest, p(&[0, 0, 1]));
    // Profile follows the same initial-branch rule.
    let prof = whisker_router::core::resolve(&t, "/profile/1", None).unwrap();
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

#[test]
fn cold_shared_route_resolution_prefers_nonzero_initial_branch() {
    let tree = CompiledTree::new(RouteTree::switch(
        SwitchDef::new("s", 1),
        vec![
            RouteTree::stack(vec![RouteTree::route("detail/:id", "detail")]),
            RouteTree::stack(vec![RouteTree::route("detail/:id", "detail")]),
        ],
    ));

    let destination = whisker_router::core::resolve(&tree, "/detail/42", None).unwrap();
    assert_eq!(destination, p(&[1, 0]));
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
            n.navigate("/search").unwrap();
        }),
        Box::new(|n| {
            n.navigate("/post/1").unwrap();
        }),
        Box::new(|n| {
            n.navigate("/video/2").unwrap();
        }),
        Box::new(|n| {
            n.navigate_within("/post/3", &Scope::at(p(&[0, 0])))
                .unwrap();
        }),
        Box::new(|n| {
            let _ = n.back();
        }),
        Box::new(|n| {
            let _ = n.back();
        }),
        Box::new(|n| {
            n.navigate("/mypage").unwrap();
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
        _ => unreachable!("unexpected RouteState variant"),
    }
}

// ===================================================================
// 14. each tab keeps an independent stack
// ===================================================================

#[test]
fn tabs_keep_independent_stacks() {
    let t = grouped_tabs_tree();
    let mut st = RouteState::initial(&t);

    // Drive home into detail.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.push("/detail/11").unwrap();
        assert_eq!(nav.current().path, p(&[0, 0, 0, 1]));
    }
    // Switch to search without touching home history, then push there.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate("/(search)").unwrap();
        nav.push("/detail/22").unwrap();
        assert_eq!(nav.current().path, p(&[0, 1, 0, 1]));
    }
    assert_eq!(grouped_history(&st, 0).len(), 2);
    assert_eq!(
        grouped_history(&st, 0)[1]
            .state
            .current()
            .params
            .get("id")
            .unwrap(),
        "11"
    );
    // Switching back restores Home's retained top because the group was
    // inactive. A second navigate would be the reselect that pops to root.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate("/(home)").unwrap();
        assert_eq!(nav.current().path, p(&[0, 0, 0, 1]));
    }
    assert_eq!(grouped_history(&st, 0).len(), 2);
    assert_eq!(grouped_history(&st, 1).len(), 2);
    assert_eq!(
        grouped_history(&st, 1)[1]
            .state
            .current()
            .params
            .get("id")
            .unwrap(),
        "22"
    );
}

#[test]
fn switching_tabs_preserves_each_stack_via_navigate() {
    let t = grouped_tabs_tree();
    let mut st = RouteState::initial(&t);
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.push("/detail/1").unwrap();
    }
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate("/(search)").unwrap();
    }
    assert_eq!(grouped_history(&st, 0).len(), 2);
    // back in search (trivial) is a no-op and doesn't touch timeline.
    {
        let mut nav = Navigator::new(&t, &mut st);
        assert!(nav.back().is_err());
    }
    assert_eq!(grouped_history(&st, 0).len(), 2);
}

#[test]
fn group_navigation_is_nondestructive_and_returns_to_retained_screen() {
    let t = grouped_tabs_tree();
    let mut st = RouteState::initial(&t);
    // Drive home deep: [home, detail(1), detail(2)].
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.push("/detail/1").unwrap();
        nav.push("/detail/2").unwrap();
    }
    // Switch away, then back. The first return restores; it is not a reselect.
    {
        let mut nav = Navigator::new(&t, &mut st);
        nav.navigate("/(search)").unwrap();
        assert_eq!(nav.current().path, p(&[0, 1, 0, 0]));
        nav.navigate("/(home)").unwrap();
        assert_eq!(nav.current().path, p(&[0, 0, 0, 1]));
        assert_eq!(nav.current().params.get("id").unwrap(), "2");
    }
    assert_eq!(grouped_history(&st, 0).len(), 3);
}

// ===================================================================
// within(scope): resolution restricted to a subtree
// ===================================================================

#[test]
fn within_scope_restricts_resolution_to_branch() {
    let t = twitter_tree();
    let mut st = RouteState::initial(&t);
    let mut nav = Navigator::new(&t, &mut st);
    // From timeline, target post but scope it to the search tab subtree.
    let search_scope = Scope::at(p(&[0, 1]));
    nav.navigate_within("/post/1", &search_scope).unwrap();
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
    let err = nav.navigate("/nope").unwrap_err();
    assert_eq!(err, NavError::NoSuchTarget);
}

// ===================================================================
// helpers that reach into the state tree for assertions
// ===================================================================

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

fn switch_selected(st: &RouteState) -> usize {
    if let RouteState::Stack(root) = st {
        if let RouteState::Switch(sw) = &root.history[0].state {
            return sw.selected;
        }
    }
    panic!("could not reach the tabs switch");
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

fn grouped_history(st: &RouteState, branch: usize) -> &[whisker_router::core::StackEntry] {
    if let RouteState::Route(layout) = st {
        if let RouteState::Switch(switch) = &layout.children[0] {
            if let RouteState::Route(group) = &switch.branches[branch] {
                if let RouteState::Stack(stack) = &group.children[0] {
                    return &stack.history;
                }
            }
        }
    }
    panic!("could not reach grouped tab {branch} history");
}

fn active_chain_kinds(st: &RouteState) -> Vec<&'static str> {
    st.active_chain()
        .into_iter()
        .map(|n| match n {
            RouteState::Route(_) => "Route",
            RouteState::Stack(_) => "Stack",
            RouteState::Switch(_) => "Switch",
            _ => "Unknown",
        })
        .collect()
}
