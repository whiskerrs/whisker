use super::*;

/// The signed-in user's own profile (Profile tab root). Resolves the own
/// DID, then renders the shared profile view with a logout action.
#[component]
pub(super) fn my_profile_screen() -> Element {
    // This tab mounts at app start (keep-alive Switch), possibly before the
    // boot restore has set the agent. Gate the DID lookup on `AuthState` so
    // it re-runs once auth flips true (`my_did` itself isn't reactive).
    let AuthState(authed) = use_context::<AuthState>().expect("AuthState provided at root");
    let me = resource(move || {
        let ready = authed.get();
        async move {
            if !ready {
                return Err(String::new());
            }
            bsky_auth::my_did()
                .await
                .ok_or_else(|| "not authenticated".to_string())
        }
    });
    let insets = safe_area_insets();
    let pad = computed(move || css!(padding_top: px(insets.get().top as f32 + 8.0)));
    render! {
        view(style: css!(flex_grow: 1.0, flex_direction: FlexDirection::Column, background_color: theme::BG)) {
            view(style: pad) {}
            Show(
                when: move || me.get().is_some(),
                fallback: move || render! { status_pane(message: "読み込み中…".to_string()) },
            ) {
                profile_view(actor: me.get().unwrap_or_default(), show_logout: true)
            }
        }
    }
}

/// Another account's profile (pushed `profile/:did`). DID arrives
/// percent-encoded in the route param.
#[component]
pub(super) fn profile_screen() -> Element {
    let did_param = use_param("did");
    let enc = did_param.get().unwrap_or_default();
    let actor = urlencoding::decode(&enc)
        .map(|c| c.into_owned())
        .unwrap_or(enc);
    render! {
        view(style: css!(flex_grow: 1.0, flex_direction: FlexDirection::Column, background_color: theme::BG)) {
            nav_header(title: "プロフィール".to_string())
            profile_view(actor: actor, show_logout: false)
        }
    }
}

/// One row of the scrollable profile screen.
#[derive(Clone)]
enum ProfileRow {
    /// The profile header (banner / avatar / name / bio / counts + follow
    /// or logout) — rendered as list item 0, full-span, its own recycle group.
    Header {
        profile: bsky_domain::Profile,
        my_did: String,
        show_logout: bool,
    },
    /// One authored post.
    Post(bsky_domain::FeedPost),
}

/// The account's profile as a single virtualised list: the header scrolls
/// with the feed as item 0 (full-span, with a distinct `reuse_identifier` so
/// it never recycles into a post cell), then the authored posts. Enabled by
/// the item-key data source (Lynx fork v3.8.0-whisker.8) — the list diffs by
/// key, so the tall, non-uniform header cell stays stable under recycling
/// (the reason it used to be split out as a fixed sibling).
#[component]
pub(super) fn profile_view(actor: String, show_logout: bool) -> Element {
    let prof = resource({
        let actor = actor.clone();
        move || {
            let actor = actor.clone();
            async move {
                let me = bsky_auth::my_did().await.unwrap_or_default();
                let p = bsky_auth::get_profile(&actor).await?;
                Ok::<_, String>((p, me))
            }
        }
    });
    let feed = resource({
        let actor = actor.clone();
        move || {
            let actor = actor.clone();
            async move { bsky_auth::get_author_feed(&actor, 50).await }
        }
    });

    // The header scrolls with the feed as item 0 of the virtualised list.
    // Item-key data source (fork v3.8.0-whisker.8) keeps the tall, non-uniform
    // header cell stable across recycling, so it no longer needs to be a fixed
    // sibling. The header gets its own `reuse_identifier` (never recycles into
    // a post cell) + `full_span`; posts stream in once the feed resolves.
    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            width: percent(100),
        )) {
            Show(
                when: move || prof.get().is_some(),
                fallback: move || render! {
                    status_pane(message: match prof.error() {
                        Some(e) if !e.is_empty() => e,
                        _ => "読み込み中…".to_string(),
                    })
                },
            ) {
                list(
                    style: css!(flex_grow: 1.0, width: percent(100)),
                    each: move || {
                        let mut rows = Vec::new();
                        if let Some((p, me)) = prof.get() {
                            rows.push(ProfileRow::Header {
                                profile: p,
                                my_did: me,
                                show_logout,
                            });
                        }
                        rows.extend(
                            feed.get().unwrap_or_default().into_iter().map(ProfileRow::Post),
                        );
                        rows
                    },
                    key: |r: &ProfileRow| match r {
                        ProfileRow::Header { .. } => "header".to_string(),
                        ProfileRow::Post(p) => p.uri.clone(),
                    },
                    children: |r: ReadSignal<ProfileRow>| match r.get() {
                        ProfileRow::Header {
                            profile,
                            my_did,
                            show_logout,
                        } => render! {
                            profile_header(
                                profile: profile,
                                my_did: my_did,
                                show_logout: show_logout,
                            )
                        },
                        ProfileRow::Post(p) => render! {
                            PostRow(post: p)
                        },
                    },
                )
            }
        }
    }
}

#[component]
pub(super) fn profile_header(
    profile: bsky_domain::Profile,
    my_did: String,
    show_logout: bool,
) -> Element {
    let nav = use_navigator();
    let banner = profile.banner.clone().unwrap_or_default();
    let avatar = profile.avatar.clone().unwrap_or_default();
    let is_me = profile.did == my_did;
    // Cloned for the (re-invokable) follow-button Show children closure.
    // `following_uri` is passed as a String (empty == not following).
    let follow_did = profile.did.clone();
    let follow_uri = profile.following_uri.clone().unwrap_or_default();
    let count_did = profile.did.clone();
    let mod_did = profile.did.clone();
    let muted = profile.muted;
    let blocking = profile.blocking_uri.clone().unwrap_or_default();
    let follows_count = profile.follows_count;
    let followers_count = profile.followers_count;
    let posts_count = profile.posts_count;
    // Extract every field to an owned local so `profile` isn't referenced
    // inside the render closures (it's not `Copy`).
    let name = profile.name();
    let handle = format!("@{}", profile.handle);
    let description = profile.description.clone().unwrap_or_default();
    let has_desc = !description.is_empty();
    // Other users get follow + an overflow (mute / block) menu; self / the
    // logged-in account get logout instead.
    let show_actions = !show_logout && !is_me;
    let menu_open = RwSignal::new(false);

    render! {
        view(style: css!(
            flex_direction: FlexDirection::Column,
            // The virtualised `<list>` sizes each cell to its content width,
            // not the list's cross-axis width. Without this the header (and
            // its `width: 100%` banner) shrink-wraps to the counts text and
            // ends up narrower than the post rows. Pin to the full width.
            width: percent(100),
            // Don't let the virtualised `<list>` below collapse the header:
            // once the feed populates, its intrinsic height balloons and a
            // shrinkable header (flex-shrink defaults to 1) gets squeezed to
            // nothing. Pin it.
            flex_shrink: 0.0,
            padding_bottom: px(12),
            border_bottom_width: px(1),
            border_bottom_color: theme::BORDER,
        )) {
            Show(when: { let b = !banner.is_empty(); move || b }, fallback: || render! { fragment() }) {
                Image(
                    style: css!(width: percent(100), height: px(120), background_color: theme::SURFACE),
                    src: banner.clone(),
                    mode: ImageMode::AspectFill,
                )
            }
            view(style: css!(
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding_left: theme::GUTTER,
                padding_right: theme::GUTTER,
                margin_top: px(8),
            )) {
                avatar_disc(src: avatar)
                view(style: css!(flex_direction: FlexDirection::Row, align_items: AlignItems::Center)) {
                    Show(when: move || show_logout, fallback: || render! { fragment() }) {
                        settings_button()
                    }
                    Show(when: move || show_actions, fallback: || render! { fragment() }) {
                        view(
                            style: css!(
                                width: px(34),
                                height: px(34),
                                border_radius: px(17),
                                background_color: theme::SURFACE,
                                display: Display::Flex,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                margin_right: px(8),
                            ),
                            on_tap: move |_| menu_open.set(!menu_open.get()),
                        ) {
                            Icon(svg: lucide::Ellipsis, color: "#FFFFFF", size: "18")
                        }
                    }
                    Show(when: move || show_actions, fallback: || render! { fragment() }) {
                        follow_button(
                            did: follow_did.clone(),
                            following_uri: follow_uri.clone(),
                        )
                    }
                }
            }
            Show(when: move || show_actions, fallback: || render! { fragment() }) {
                moderation_menu(
                    did: mod_did.clone(),
                    muted: muted,
                    blocking_uri: blocking.clone(),
                    open: menu_open,
                )
            }
            text(
                style: css!(
                    font_size: px(20),
                    font_weight: FontWeight::Bold,
                    color: theme::TEXT_PRIMARY,
                    margin_top: px(8),
                    margin_left: theme::GUTTER,
                ),
                value: name,
            )
            text(
                style: css!(font_size: theme::T_HANDLE, color: theme::TEXT_SECONDARY, margin_left: theme::GUTTER),
                value: handle,
            )
            Show(when: move || has_desc, fallback: || render! { fragment() }) {
                text(
                    style: css!(
                        font_size: theme::T_BODY,
                        color: theme::TEXT_PRIMARY,
                        margin_top: px(8),
                        margin_left: theme::GUTTER,
                        margin_right: theme::GUTTER,
                    ),
                    value: description.clone(),
                )
            }
            // Counts row: フォロー中 / フォロワー are tappable → their lists.
            view(style: css!(
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                margin_top: px(10),
                margin_left: theme::GUTTER,
            )) {
                view(on_tap: {
                    let nav = nav.clone();
                    let did = count_did.clone();
                    move |_| {
                        let _ = nav.navigate(&format!("/following/{}", urlencoding::encode(&did)));
                    }
                }) {
                    text(
                        style: css!(font_size: theme::T_META, color: theme::TEXT_PRIMARY),
                        value: format!("{follows_count} フォロー中"),
                    )
                }
                text(
                    style: css!(font_size: theme::T_META, color: theme::TEXT_SECONDARY),
                    value: "  ·  ",
                )
                view(on_tap: {
                    let nav = nav.clone();
                    let did = count_did.clone();
                    move |_| {
                        let _ = nav.navigate(&format!("/followers/{}", urlencoding::encode(&did)));
                    }
                }) {
                    text(
                        style: css!(font_size: theme::T_META, color: theme::TEXT_PRIMARY),
                        value: format!("{followers_count} フォロワー"),
                    )
                }
                text(
                    style: css!(font_size: theme::T_META, color: theme::TEXT_SECONDARY),
                    value: format!("  ·  {posts_count} ポスト"),
                )
            }
        }
    }
}

/// Inline mute / block menu, shown below the profile action row when the
/// overflow button toggles `open`. Optimistic, like [`follow_button`];
/// `blocking_uri` empty == not blocking (avoids an `Option` prop).
#[component]
pub(super) fn moderation_menu(
    did: String,
    muted: bool,
    blocking_uri: String,
    open: Signal<bool>,
) -> Element {
    let is_muted = RwSignal::new(muted);
    let block_uri = RwSignal::new(if blocking_uri.is_empty() {
        None
    } else {
        Some(blocking_uri.clone())
    });
    let mute_label = computed(move || {
        if is_muted.get() {
            "ミュート解除".to_string()
        } else {
            "ミュートする".to_string()
        }
    });
    let block_label = computed(move || {
        if block_uri.get().is_some() {
            "ブロック解除".to_string()
        } else {
            "ブロックする".to_string()
        }
    });
    // Clone the captured param into a body-local once; the per-action
    // closures clone *this* (cloning the captured param directly inside a
    // nested `move` block makes the macro's FnMut wrapper move it out).
    let menu_did = did.clone();
    render! {
        Show(when: move || open.get(), fallback: || render! { fragment() }) {
            view(style: css!(
                flex_direction: FlexDirection::Column,
                margin_top: px(10),
                margin_left: theme::GUTTER,
                margin_right: theme::GUTTER,
                border_radius: px(10),
                background_color: theme::SURFACE,
            )) {
                view(
                    style: css!(
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        padding: px(12),
                    ),
                    on_tap: {
                        let did = menu_did.clone();
                        move |_| {
                            let was = is_muted.get();
                            is_muted.set(!was);
                            let did = did.clone();
                            spawn_local(async move {
                                let r = if was {
                                    bsky_auth::unmute(&did).await
                                } else {
                                    bsky_auth::mute(&did).await
                                };
                                if r.is_err() {
                                    is_muted.set(was);
                                }
                            });
                        }
                    },
                ) {
                    Icon(svg: lucide::VolumeX, color: "#FFFFFF", size: "18")
                    text(
                        style: css!(font_size: theme::T_BODY, color: theme::TEXT_PRIMARY, margin_left: px(10)),
                        value: mute_label,
                    )
                }
                view(
                    style: css!(
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        padding: px(12),
                        border_top_width: px(1),
                        border_top_color: theme::BORDER,
                    ),
                    on_tap: {
                        let did = menu_did.clone();
                        move |_| match block_uri.get() {
                            Some(uri) => {
                                block_uri.set(None);
                                spawn_local(async move {
                                    if bsky_auth::unblock(&uri).await.is_err() {
                                        block_uri.set(Some(uri));
                                    }
                                });
                            }
                            None => {
                                let did = did.clone();
                                spawn_local(async move {
                                    if let Ok(uri) = bsky_auth::block(&did).await {
                                        block_uri.set(Some(uri));
                                    }
                                });
                            }
                        }
                    },
                ) {
                    Icon(svg: lucide::Ban, color: "#FF6B6B", size: "18")
                    text(
                        style: css!(font_size: theme::T_BODY, color: Color::hex(0xFF6B6B), margin_left: px(10)),
                        value: block_label,
                    )
                }
            }
        }
    }
}

/// Followers list (`getFollowers`) for the `:did` route param.
#[component]
pub(super) fn followers_screen() -> Element {
    let did_param = use_param("did");
    let enc = did_param.get().unwrap_or_default();
    let actor = urlencoding::decode(&enc)
        .map(|c| c.into_owned())
        .unwrap_or(enc);
    render! {
        view(style: css!(flex_grow: 1.0, flex_direction: FlexDirection::Column, background_color: theme::BG)) {
            nav_header(title: "フォロワー".to_string())
            follow_list(actor: actor, followers: true)
        }
    }
}

/// Following list (`getFollows`) for the `:did` route param.
#[component]
pub(super) fn following_screen() -> Element {
    let did_param = use_param("did");
    let enc = did_param.get().unwrap_or_default();
    let actor = urlencoding::decode(&enc)
        .map(|c| c.into_owned())
        .unwrap_or(enc);
    render! {
        view(style: css!(flex_grow: 1.0, flex_direction: FlexDirection::Column, background_color: theme::BG)) {
            nav_header(title: "フォロー中".to_string())
            follow_list(actor: actor, followers: false)
        }
    }
}

/// Shared body for the followers / following screens: fetch the relevant
/// actor list and render it. `followers` picks the endpoint.
#[component]
pub(super) fn follow_list(actor: String, followers: bool) -> Element {
    let res = resource({
        let actor = actor.clone();
        move || {
            let actor = actor.clone();
            async move {
                if followers {
                    bsky_auth::get_followers(&actor, 50).await
                } else {
                    bsky_auth::get_follows(&actor, 50).await
                }
            }
        }
    });
    render! {
        Show(
            when: move || res.get().is_some(),
            fallback: move || render! {
                status_pane(message: match res.error() {
                    Some(e) if !e.is_empty() => e,
                    _ => "読み込み中…".to_string(),
                })
            },
        ) {
            actor_list(actors: res.get().unwrap_or_default())
        }
    }
}

/// A 64px circular avatar for the profile header.
#[component]
pub(super) fn avatar_disc(src: String) -> Element {
    if src.is_empty() {
        render! {
            view(style: css!(
                width: px(64),
                height: px(64),
                border_radius: px(32),
                background_color: theme::ACCENT,
            )) {}
        }
    } else {
        render! {
            Image(
                style: css!(
                    width: px(64),
                    height: px(64),
                    border_radius: px(32),
                    background_color: theme::SURFACE,
                ),
                src: src.clone(),
                mode: ImageMode::AspectFill,
            )
        }
    }
}

/// Follow / unfollow toggle with optimistic state.
#[component]
pub(super) fn follow_button(did: String, following_uri: String) -> Element {
    // Empty `following_uri` == not following (avoids an `Option` prop,
    // which `#[component]` treats as an optional setter — see MEMO).
    let following = RwSignal::new(!following_uri.is_empty());
    let uri = RwSignal::new(if following_uri.is_empty() {
        None
    } else {
        Some(following_uri.clone())
    });
    let did = did.clone();
    let label = computed(move || {
        if following.get() {
            "フォロー中".to_string()
        } else {
            "フォロー".to_string()
        }
    });
    render! {
        view(
            style: computed(move || {
                let on = following.get();
                css!(
                    height: px(34),
                    padding_left: px(16),
                    padding_right: px(16),
                    border_radius: px(17),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    background_color: if on { theme::SURFACE } else { theme::ACCENT },
                )
            }),
            on_tap: move |_| {
                let was = following.get();
                following.set(!was);
                let did = did.clone();
                spawn_local(async move {
                    if was {
                        if let Some(u) = uri.get() {
                            if bsky_auth::unfollow(&u).await.is_ok() {
                                uri.set(None);
                            } else {
                                following.set(true);
                            }
                        }
                    } else {
                        match bsky_auth::follow(&did).await {
                            Ok(u) => uri.set(Some(u)),
                            Err(_) => following.set(false),
                        }
                    }
                });
            },
        ) {
            text(
                style: computed(move || css!(
                    font_size: px(14),
                    font_weight: FontWeight::Bold,
                    color: if following.get() { theme::TEXT_PRIMARY } else { theme::ON_ACCENT },
                )),
                value: label,
            )
        }
    }
}

/// Logout: clear the session and return to the login branch.
#[component]
pub(super) fn logout_button() -> Element {
    let nav = use_navigator();
    let AuthState(authed) = use_context::<AuthState>().expect("AuthState provided at root");
    render! {
        view(
            style: css!(
                height: px(34),
                padding_left: px(16),
                padding_right: px(16),
                border_radius: px(17),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                background_color: theme::SURFACE,
            ),
            on_tap: move |_| {
                let nav = nav.clone();
                spawn_local(async move {
                    bsky_auth::logout().await;
                    authed.set(false);
                    let _ = nav.select("/(auth)");
                });
            },
        ) {
            text(
                style: css!(font_size: px(14), color: theme::TEXT_PRIMARY),
                value: "ログアウト",
            )
        }
    }
}

/// Gear button on the logged-in profile header → the settings screen.
#[component]
pub(super) fn settings_button() -> Element {
    let nav = use_navigator();
    render! {
        view(
            style: css!(
                width: px(34),
                height: px(34),
                border_radius: px(17),
                background_color: theme::SURFACE,
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            ),
            on_tap: move |_| {
                let _ = nav.navigate("/settings");
            },
        ) {
            Icon(svg: lucide::Settings, color: "#FFFFFF", size: "18")
        }
    }
}

/// Settings screen: account (handle + logout), moderation (muted / blocked
/// account lists), and app info. Reached from the profile gear button.
#[component]
pub(super) fn settings_screen() -> Element {
    let AuthState(authed) = use_context::<AuthState>().expect("AuthState provided at root");
    // Resolve the logged-in handle for the account row. Gated on auth so it
    // re-runs if the boot restore lands after first mount.
    let handle = resource(move || {
        let ready = authed.get();
        async move {
            if !ready {
                return Err(String::new());
            }
            let did = bsky_auth::my_did()
                .await
                .ok_or_else(|| "not authenticated".to_string())?;
            let p = bsky_auth::get_profile(&did).await?;
            Ok(format!("@{}", p.handle))
        }
    });
    let handle_label = computed(move || handle.get().unwrap_or_default());
    render! {
        view(style: css!(flex_grow: 1.0, flex_direction: FlexDirection::Column, background_color: theme::BG)) {
            nav_header(title: "設定".to_string())
            view(style: css!(flex_grow: 1.0, flex_direction: FlexDirection::Column)) {
                settings_section(title: "アカウント".to_string())
                view(style: css!(
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    padding_left: theme::GUTTER,
                    padding_right: theme::GUTTER,
                    padding_top: px(12),
                    padding_bottom: px(12),
                )) {
                    text(
                        style: css!(font_size: theme::T_BODY, color: theme::TEXT_PRIMARY),
                        value: handle_label,
                    )
                    logout_button()
                }
                settings_section(title: "モデレーション".to_string())
                settings_row(
                    icon: lucide::VolumeX,
                    label: "ミュート中のアカウント".to_string(),
                    route: "/settings/muted".to_string(),
                )
                settings_row(
                    icon: lucide::Ban,
                    label: "ブロック中のアカウント".to_string(),
                    route: "/settings/blocked".to_string(),
                )
                settings_section(title: "アプリ情報".to_string())
                view(style: css!(
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    padding_left: theme::GUTTER,
                    padding_right: theme::GUTTER,
                    padding_top: px(12),
                    padding_bottom: px(12),
                )) {
                    text(
                        style: css!(font_size: theme::T_BODY, color: theme::TEXT_PRIMARY),
                        value: "バージョン",
                    )
                    text(
                        style: css!(font_size: theme::T_META, color: theme::TEXT_SECONDARY),
                        // `option_env!` (not `env!`): the tier-1 hot-patch runs
                        // raw `rustc` without Cargo's env, so `env!` is a hard
                        // compile error there — `option_env!` degrades to None.
                        value: option_env!("CARGO_PKG_VERSION").unwrap_or("dev"),
                    )
                }
            }
        }
    }
}

/// A small uppercase-ish section header inside the settings list.
#[component]
pub(super) fn settings_section(title: String) -> Element {
    render! {
        text(
            style: css!(
                font_size: theme::T_META,
                font_weight: FontWeight::Bold,
                color: theme::TEXT_SECONDARY,
                background_color: theme::BG,
                padding_left: theme::GUTTER,
                padding_top: px(16),
                padding_bottom: px(6),
            ),
            value: title.clone(),
        )
    }
}

/// A tappable settings row: leading icon + label + trailing chevron.
#[component]
pub(super) fn settings_row(icon: &'static str, label: String, route: String) -> Element {
    let nav = use_navigator();
    render! {
        view(
            style: css!(
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding_left: theme::GUTTER,
                padding_right: theme::GUTTER,
                padding_top: px(14),
                padding_bottom: px(14),
                border_bottom_width: px(1),
                border_bottom_color: theme::BORDER,
            ),
            on_tap: {
                let nav = nav.clone();
                let route = route.clone();
                move |_| {
                    let _ = nav.navigate(&route);
                }
            },
        ) {
            Icon(svg: icon, color: "#FFFFFF", size: "20")
            text(
                style: css!(
                    flex_grow: 1.0,
                    font_size: theme::T_BODY,
                    color: theme::TEXT_PRIMARY,
                    margin_left: px(12),
                ),
                value: label.clone(),
            )
            Icon(svg: lucide::ChevronRight, color: "#8B98A5", size: "20")
        }
    }
}

/// Muted accounts list (`getMutes`).
#[component]
pub(super) fn muted_accounts_screen() -> Element {
    let res = resource(|| async { bsky_auth::get_mutes(50).await });
    render! {
        view(style: css!(flex_grow: 1.0, flex_direction: FlexDirection::Column, background_color: theme::BG)) {
            nav_header(title: "ミュート中のアカウント".to_string())
            moderation_account_list(res: res)
        }
    }
}

/// Blocked accounts list (`getBlocks`).
#[component]
pub(super) fn blocked_accounts_screen() -> Element {
    let res = resource(|| async { bsky_auth::get_blocks(50).await });
    render! {
        view(style: css!(flex_grow: 1.0, flex_direction: FlexDirection::Column, background_color: theme::BG)) {
            nav_header(title: "ブロック中のアカウント".to_string())
            moderation_account_list(res: res)
        }
    }
}

/// Shared body for the muted / blocked screens: gate on the resource and
/// render the actor list (or a status pane while loading / empty).
#[component]
pub(super) fn moderation_account_list(res: Resource<Vec<bsky_domain::ActorView>>) -> Element {
    render! {
        Show(
            when: move || res.get().is_some(),
            fallback: move || render! {
                status_pane(message: match res.error() {
                    Some(e) if !e.is_empty() => e,
                    _ => "読み込み中…".to_string(),
                })
            },
        ) {
            Show(
                when: move || !res.get().unwrap_or_default().is_empty(),
                fallback: move || render! { status_pane(message: "該当するアカウントはありません".to_string()) },
            ) {
                actor_list(actors: res.get().unwrap_or_default())
            }
        }
    }
}

/// Reusable top bar with a back chevron + title (safe-area aware).
#[component]
pub(super) fn nav_header(title: String) -> Element {
    let nav = use_navigator();
    let insets = safe_area_insets();
    let style = computed(move || {
        css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            // Pin to content height: this nav bar is a flex sibling of a
            // `flex_grow: 1` screen body, so without `flex_shrink: 0` Lynx's
            // default `flex_shrink: 1` lets the body squeeze it (the back
            // button + title looked crushed on the profile screen).
            flex_shrink: 0.0,
            background_color: theme::BG,
            border_bottom_width: px(1),
            border_bottom_color: theme::BORDER,
            padding_top: px(insets.get().top as f32 + 8.0),
            padding_bottom: px(8),
            padding_left: px(8),
            padding_right: px(16),
        )
    });
    render! {
        view(style: style) {
            view(
                style: css!(padding: px(8), display: Display::Flex, align_items: AlignItems::Center),
                on_tap: move |_| {
                    let _ = nav.back();
                },
            ) {
                Icon(svg: lucide::ChevronLeft, color: "#FFFFFF", size: "26")
            }
            text(
                style: css!(
                    font_size: theme::T_NAME,
                    font_weight: FontWeight::Bold,
                    color: theme::TEXT_PRIMARY,
                    margin_left: px(4),
                ),
                value: title.clone(),
            )
        }
    }
}
