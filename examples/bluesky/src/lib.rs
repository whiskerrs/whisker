//! Bluesky (AT Protocol) client example.
//!
//! Navigation is driven by `whisker-router`. The root is a stack: an index
//! [`tabs_layout`] (the authenticated, tabbed shell) plus full-screen
//! `login` / `auth/:handle` routes pushed over it.
//!
//! The tabbed shell is a `Switch` of four branches — Home / Search /
//! Notifications / Profile — each its own `Stack` so per-tab pushes (post
//! detail, profiles) keep independent back history.
//!
//! Auth gate: the Home tab restores a persisted session on launch and, if
//! none can be restored, resets to `/login`. After a successful login the
//! auth screen resets to `/` (the Home tab).

use bsky_ui_kit::PostCard;
use whisker::ListHandle;
use whisker::css::{AlignItems, Display, FlexDirection, FontWeight, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{Icon, lucide};
use whisker_image::{Image, ImageMode};
use whisker_input::{AutoCapitalize, Input, KeyboardType, ReturnKey};
use whisker_router::render::{
    AndroidPredictiveBack, Outlet, Router, RouterHandle, SwipeBack, use_navigator, use_param,
    use_pathname,
};
use whisker_router::routes;
use whisker_safe_area::safe_area_insets;
use whisker_webview::WebView;

use bsky_theme as theme;

/// App-wide auth state, provided at the root and read by screens that
/// gate on it. Flipped by the boot restore / login / logout flows.
/// Keeping it reactive lets the (keep-alive) timeline re-fetch the moment
/// a login completes, without remounting the tab shell.
#[derive(Clone, Copy)]
struct AuthState(RwSignal<bool>);

#[whisker::main]
pub fn app() -> Element {
    // Seed from any in-process session (none on a cold start).
    provide_context(AuthState(RwSignal::new(bsky_auth::is_authenticated())));
    render! {
        view(style: css!(
            flex_grow: 1.0,
            background_color: theme::BG,
            flex_direction: FlexDirection::Column,
        )) {
            // Root is the tab layout with the tab `Switch` directly under it
            // (mirrors whisker-router's example): tab switches are instant
            // `Switch` toggles, while per-tab `Stack`s animate push/pop. The
            // pre-auth login flow is a sibling `(auth)` branch (no tab bar).
            Router(routes: routes! {
                Route(component: TabsLayout) {
                    Switch {
                        Route(path: "(home)") {
                            Stack {
                                Route(path: "", component: TimelineScreen)
                                Route(path: "compose", component: ComposeScreen)
                                Route(path: "post/:uri", component: PostDetailScreen)
                                Route(path: "profile/:did", component: ProfileScreen)
                                Route(path: "followers/:did", component: FollowersScreen)
                                Route(path: "following/:did", component: FollowingScreen)
                            }
                        }
                        Route(path: "(search)") {
                            Stack {
                                Route(path: "", component: SearchScreen)
                                Route(path: "post/:uri", component: PostDetailScreen)
                                Route(path: "profile/:did", component: ProfileScreen)
                                Route(path: "followers/:did", component: FollowersScreen)
                                Route(path: "following/:did", component: FollowingScreen)
                            }
                        }
                        Route(path: "(notifications)") {
                            Stack {
                                Route(path: "", component: NotificationsScreen)
                                Route(path: "post/:uri", component: PostDetailScreen)
                                Route(path: "profile/:did", component: ProfileScreen)
                                Route(path: "followers/:did", component: FollowersScreen)
                                Route(path: "following/:did", component: FollowingScreen)
                            }
                        }
                        Route(path: "(profile)") {
                            Stack {
                                Route(path: "", component: MyProfileScreen)
                                Route(path: "post/:uri", component: PostDetailScreen)
                                Route(path: "profile/:did", component: ProfileScreen)
                                Route(path: "followers/:did", component: FollowersScreen)
                                Route(path: "following/:did", component: FollowingScreen)
                                Route(path: "settings", component: SettingsScreen)
                                Route(path: "settings/muted", component: MutedAccountsScreen)
                                Route(path: "settings/blocked", component: BlockedAccountsScreen)
                            }
                        }
                        Route(path: "(auth)") {
                            Stack {
                                Route(path: "", component: LoginScreen)
                                Route(path: "auth/:handle", component: AuthScreen)
                            }
                        }
                    }
                }
            }) {
                Outlet {}
                SwipeBack {}
                AndroidPredictiveBack {}
            }
        }
    }
}

/// Root shell: the active branch's `Outlet` above a bottom tab bar. The
/// tab bar is hidden on the pre-auth `(auth)` branch. On mount it restores
/// a persisted session (flipping `AuthState`) or selects the login branch.
#[component]
fn tabs_layout() -> Element {
    let nav = use_navigator();
    let pathname = use_pathname();
    let AuthState(authed) = use_context::<AuthState>().expect("AuthState provided at root");

    on_mount(move || {
        if authed.get() {
            return;
        }
        let nav = nav.clone();
        spawn_local(async move {
            if bsky_auth::restore_session().await {
                authed.set(true);
            } else {
                let _ = nav.select("/(auth)");
            }
        });
    });

    let on_auth = computed(move || pathname.get().contains("/(auth)"));

    render! {
        view(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
            view(style: css!(
                flex_grow: 1.0,
                flex_shrink: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            )) {
                Outlet {}
            }
            Show(when: move || !on_auth.get(), fallback: || render! { fragment() }) {
                TabBar {}
            }
        }
    }
}

/// Bottom tab bar. Active tab is derived from the current pathname (the
/// group segment, e.g. `/(search)`); tapping selects that branch,
/// preserving each tab's own stack.
#[component]
fn tab_bar() -> Element {
    let nav = use_navigator();
    let pathname = use_pathname();
    let insets = safe_area_insets();
    let bar_style = computed(move || {
        css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceAround,
            align_items: AlignItems::Center,
            height: px(52.0 + insets.get().bottom as f32),
            padding_bottom: px(insets.get().bottom as f32),
            background_color: theme::BG,
            border_top_width: px(1),
            border_top_color: theme::BORDER,
        )
    });
    render! {
        view(style: bar_style) {
            TabBarItem(group: "(home)", url: "/(home)", icon: lucide::House, pathname: pathname, nav: nav.clone())
            TabBarItem(group: "(search)", url: "/(search)", icon: lucide::Search, pathname: pathname, nav: nav.clone())
            TabBarItem(group: "(notifications)", url: "/(notifications)", icon: lucide::Bell, pathname: pathname, nav: nav.clone())
            TabBarItem(group: "(profile)", url: "/(profile)", icon: lucide::User, pathname: pathname, nav: nav.clone())
        }
    }
}

#[component]
fn tab_bar_item(
    group: &'static str,
    url: &'static str,
    icon: Signal<String>,
    pathname: ReadSignal<String>,
    nav: RouterHandle,
) -> Element {
    // The home group has no segment in the pathname, so it's active when no
    // other group segment is present.
    let is_active = computed(move || {
        let p = pathname.get();
        if group == "(home)" {
            !p.contains("/(search)")
                && !p.contains("/(notifications)")
                && !p.contains("/(profile)")
                && !p.contains("/(auth)")
        } else {
            p.contains(group)
        }
    });
    let color = computed(move || {
        if is_active.get() {
            "#1083FE".to_string()
        } else {
            "#8B98A5".to_string()
        }
    });
    // Clone per body invocation so the `FnMut` component can move an owned
    // handle into the (re-usable) `on_tap` closure.
    let nav = nav.clone();
    render! {
        view(
            style: css!(
                flex_grow: 1.0,
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                height: px(52),
            ),
            on_tap: move |_| {
                let _ = nav.select(url);
            },
        ) {
            Icon(svg: icon, color: color, size: "26")
        }
    }
}

/// Placeholder shown by tabs whose real screen lands in a later phase.
#[component]
fn placeholder_screen(title: String) -> Element {
    let insets = safe_area_insets();
    let style = computed(move || {
        let i = insets.get();
        css!(
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            background_color: theme::BG,
            padding_top: px(i.top as f32),
        )
    });
    render! {
        view(style: style) {
            text(
                style: css!(font_size: theme::T_BODY, color: theme::TEXT_SECONDARY),
                value: title.clone(),
            )
        }
    }
}

/// Which result set the search screen is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    People,
    Posts,
}

#[component]
fn search_screen() -> Element {
    let insets = safe_area_insets();
    // `draft` is the live field text; `query` is the committed term (set on
    // Return). Separating them means we only hit the network on submit, not
    // on every keystroke.
    let draft = RwSignal::new(String::new());
    let query = RwSignal::new(String::new());
    let mode = RwSignal::new(SearchMode::People);
    // Drives the swipeable pager: `scroll_to_position` on a tab tap, bound
    // via `ref:` on the horizontal `<list>` below.
    let pager = ListHandle::new();

    // One resource per result kind. Both fetch on every committed `query`
    // (not gated on the active tab) so both pager pages are populated and
    // swiping shows results immediately without a re-fetch.
    let actors = resource(move || {
        let q = query.get();
        async move {
            let q = q.trim();
            if q.is_empty() {
                return Ok::<_, String>(Vec::new());
            }
            bsky_auth::search_actors(q, 30).await
        }
    });
    let posts = resource(move || {
        let q = query.get();
        async move {
            let q = q.trim();
            if q.is_empty() {
                return Ok::<_, String>(Vec::new());
            }
            bsky_auth::search_posts(q, 30).await
        }
    });

    let top_pad =
        computed(move || css!(flex_shrink: 0.0, padding_top: px(insets.get().top as f32 + 8.0)));

    render! {
        view(style: css!(flex_grow: 1.0, flex_direction: FlexDirection::Column, background_color: theme::BG)) {
            // Search field + segmented control: fixed above the results list
            // (the same fixed-header + list shape the profile uses). Each is
            // pinned `flex-shrink: 0` so the virtualised results `list`
            // (flex-grow:1, huge intrinsic height) can't squeeze them to zero.
            view(style: top_pad) {}
            view(style: css!(
                flex_shrink: 0.0,
                padding_left: theme::GUTTER,
                padding_right: theme::GUTTER,
                padding_bottom: px(8),
            )) {
                Input(
                    text: draft,
                    placeholder: "ユーザーや投稿を検索",
                    return_key: ReturnKey::Search,
                    keyboard_type: KeyboardType::Default,
                    auto_capitalize: AutoCapitalize::None,
                    autocorrect: false,
                    spell_check: false,
                    on_submit: move |v: String| query.set(v),
                    placeholder_color: "#8B98A5",
                    caret_color: "#1083FE",
                    style: "width: 100%; height: 40px; border-radius: 10px; \
                            background-color: #16191F; color: #FFFFFF; font-size: 16px; \
                            padding-left: 14px; padding-right: 14px;",
                )
            }
            view(style: css!(
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                border_bottom_width: px(1),
                border_bottom_color: theme::BORDER,
            )) {
                search_tab(label: "ユーザー", active: computed(move || mode.get() == SearchMode::People), on_tap: std::rc::Rc::new(move || { mode.set(SearchMode::People); pager.scroll_to_position(0, true); }) as std::rc::Rc<dyn Fn()>)
                search_tab(label: "投稿", active: computed(move || mode.get() == SearchMode::Posts), on_tap: std::rc::Rc::new(move || { mode.set(SearchMode::Posts); pager.scroll_to_position(1, true); }) as std::rc::Rc<dyn Fn()>)
            }
            // Swipeable pager: a horizontal `<list>` of two full-viewport-width
            // pages (People / Posts) with `item_snap` for ViewPager-style
            // paging. Always mounted — swipeable even before a query, with each
            // page showing its own empty state. Swiping snaps to a page →
            // `on_snap` syncs the tab highlight; tapping a tab calls
            // `scroll_to_position` to page over.
            list(
                ref: pager.r(),
                style: css!(flex_grow: 1.0, width: percent(100)),
                scroll_orientation: ScrollOrientation::Horizontal,
                item_snap: (0.0, 0.0),
                on_snap: move |e| {
                    let m = if e.detail.position <= 0 {
                        SearchMode::People
                    } else {
                        SearchMode::Posts
                    };
                    if mode.get() != m {
                        mode.set(m);
                    }
                },
                each: move || vec![SearchMode::People, SearchMode::Posts],
                meta: |m: &SearchMode| match m {
                    SearchMode::People => ItemMeta::key("people".to_string())
                        .reuse_identifier("page-people")
                        .recyclable(false),
                    SearchMode::Posts => ItemMeta::key("posts".to_string())
                        .reuse_identifier("page-posts")
                        .recyclable(false),
                },
                children: move |m: SearchMode| match m {
                    SearchMode::People => render! {
                        view(style: css!(width: vw(100), flex_grow: 1.0, flex_direction: FlexDirection::Column)) {
                            Show(when: move || !query.get().trim().is_empty(), fallback: || render! { status_pane(message: "ユーザーを検索できます".to_string()) }) {
                                Show(when: move || actors.get().is_some(), fallback: || render! { status_pane(message: "検索中…".to_string()) }) {
                                    actor_list(actors: actors.get().unwrap_or_default())
                                }
                            }
                        }
                    },
                    SearchMode::Posts => render! {
                        view(style: css!(width: vw(100), flex_grow: 1.0, flex_direction: FlexDirection::Column)) {
                            Show(when: move || !query.get().trim().is_empty(), fallback: || render! { status_pane(message: "投稿を検索できます".to_string()) }) {
                                Show(when: move || posts.get().is_some(), fallback: || render! { status_pane(message: "検索中…".to_string()) }) {
                                    post_list(posts: posts.get().unwrap_or_default())
                                }
                            }
                        }
                    },
                },
            )
        }
    }
}

/// One tab of the search segmented control. Active tab gets an accent
/// underline + brighter label.
#[component]
fn search_tab(label: &'static str, active: Signal<bool>, on_tap: std::rc::Rc<dyn Fn()>) -> Element {
    let cb = on_tap.clone();
    let label_style = computed(move || {
        css!(
            font_size: theme::T_NAME,
            font_weight: FontWeight::Bold,
            color: if active.get() { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY },
        )
    });
    let underline_style = computed(move || {
        css!(
            height: px(3),
            margin_top: px(8),
            border_radius: px(2),
            width: percent(100),
            background_color: if active.get() { theme::ACCENT } else { theme::BG },
        )
    });
    render! {
        view(
            style: css!(
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding_top: px(10),
            ),
            on_tap: move |_| (cb)(),
        ) {
            text(style: label_style, value: label)
            view(style: underline_style) {}
        }
    }
}

/// Virtualised list of [`actor_row`]s (people search results).
#[component]
fn actor_list(actors: Vec<bsky_domain::ActorView>) -> Element {
    render! {
        list(
            style: css!(flex_grow: 1.0, flex_shrink: 1.0, width: percent(100)),
            each: {
                let actors = actors.clone();
                move || actors.clone()
            },
            meta: |a: &bsky_domain::ActorView| ItemMeta::key(a.did.clone()),
            children: |a: bsky_domain::ActorView| render! { actor_row(actor: a) },
        )
    }
}

/// One people-search row: avatar + name / handle + bio snippet. Tapping
/// opens the account's profile.
#[component]
fn actor_row(actor: bsky_domain::ActorView) -> Element {
    let nav = use_navigator();
    let did = actor.did.clone();
    let avatar = actor.avatar.clone().unwrap_or_default();
    let name = actor.name();
    let handle = format!("@{}", actor.handle);
    let description = actor.description.clone().unwrap_or_default();
    let has_desc = !description.trim().is_empty();
    render! {
        view(
            style: css!(
                flex_direction: FlexDirection::Row,
                width: percent(100),
                padding: theme::GUTTER,
                border_bottom_width: px(1),
                border_bottom_color: theme::BORDER,
            ),
            on_tap: move |_| {
                let enc = urlencoding::encode(&did);
                let _ = nav.navigate(&format!("/profile/{enc}"));
            },
        ) {
            row_avatar(src: avatar)
            view(style: css!(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                margin_left: theme::ROW_GAP,
            )) {
                text(
                    style: css!(font_size: theme::T_NAME, font_weight: FontWeight::Bold, color: theme::TEXT_PRIMARY),
                    value: name,
                )
                text(
                    style: css!(font_size: theme::T_HANDLE, color: theme::TEXT_SECONDARY),
                    value: handle,
                )
                Show(when: move || has_desc, fallback: || render! { fragment() }) {
                    text(
                        style: css!(font_size: theme::T_BODY, color: theme::TEXT_PRIMARY, margin_top: px(2)),
                        value: description.clone(),
                    )
                }
            }
        }
    }
}

/// A 44px circular avatar for list rows (CDN image, or a flat accent disc
/// when the account has none).
#[component]
fn row_avatar(src: String) -> Element {
    if src.is_empty() {
        render! {
            view(style: css!(
                width: px(44),
                height: px(44),
                border_radius: px(22),
                background_color: theme::ACCENT,
            )) {}
        }
    } else {
        render! {
            Image(
                style: css!(
                    width: px(44),
                    height: px(44),
                    border_radius: px(22),
                    background_color: theme::SURFACE,
                ),
                src: src.clone(),
                mode: ImageMode::AspectFill,
            )
        }
    }
}

#[component]
fn notifications_screen() -> Element {
    let AuthState(authed) = use_context::<AuthState>().expect("AuthState provided at root");
    let insets = safe_area_insets();
    // Re-runs once auth flips true (same gate as the home timeline).
    let notifs = resource(move || {
        let ready = authed.get();
        async move {
            if !ready {
                return Err(String::new());
            }
            bsky_auth::list_notifications(40).await
        }
    });
    let header_style = computed(move || {
        css!(
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
            border_bottom_width: px(1),
            border_bottom_color: theme::BORDER,
            padding_top: px(insets.get().top as f32 + 8.0),
            padding_bottom: px(10),
            padding_left: theme::GUTTER,
        )
    });
    render! {
        view(style: css!(flex_grow: 1.0, flex_direction: FlexDirection::Column, background_color: theme::BG)) {
            view(style: header_style) {
                text(
                    style: css!(font_size: px(20), font_weight: FontWeight::Bold, color: theme::TEXT_PRIMARY),
                    value: "通知",
                )
            }
            Show(
                when: move || notifs.get().is_some(),
                fallback: move || render! {
                    status_pane(message: match notifs.error() {
                        Some(e) if !e.is_empty() => e,
                        _ => "読み込み中…".to_string(),
                    })
                },
            ) {
                notification_list(items: notifs.get().unwrap_or_default())
            }
        }
    }
}

/// Active-state colours for the notification reason glyphs.
const NOTIF_LIKE: &str = "#EC4899";
const NOTIF_REPOST: &str = "#43D17A";
const NOTIF_ACCENT: &str = "#1083FE";

/// Virtualised list of [`notification_row`]s.
#[component]
fn notification_list(items: Vec<bsky_domain::Notification>) -> Element {
    render! {
        list(
            style: css!(flex_grow: 1.0, flex_shrink: 1.0, width: percent(100)),
            each: {
                let items = items.clone();
                move || items.clone()
            },
            meta: |n: &bsky_domain::Notification| ItemMeta::key(n.uri.clone()),
            children: |n: bsky_domain::Notification| render! { notification_row(item: n) },
        )
    }
}

/// One notification: a reason glyph + "<name> <action>", with the post
/// body shown for reply / mention / quote. Tapping opens the relevant
/// post (or the follower's profile).
#[component]
fn notification_row(item: bsky_domain::Notification) -> Element {
    let nav = use_navigator();
    let avatar = item.author.avatar.clone().unwrap_or_default();
    let name = item.author.name();
    let (icon, action, icon_color) = match item.reason.as_str() {
        "like" => (
            lucide::Heart,
            "さんがあなたの投稿をいいねしました",
            NOTIF_LIKE,
        ),
        "repost" => (
            lucide::Repeat2,
            "さんがあなたの投稿をリポストしました",
            NOTIF_REPOST,
        ),
        "follow" => (lucide::UserPlus, "さんにフォローされました", NOTIF_ACCENT),
        "reply" => (lucide::MessageCircle, "さんが返信しました", NOTIF_ACCENT),
        "mention" => (
            lucide::AtSign,
            "さんがあなたをメンションしました",
            NOTIF_ACCENT,
        ),
        "quote" => (
            lucide::Quote,
            "さんがあなたの投稿を引用しました",
            NOTIF_ACCENT,
        ),
        _ => (lucide::Bell, "さんからの通知", NOTIF_ACCENT),
    };
    let line = format!("{name}{action}");
    let body = item.text.clone().unwrap_or_default();
    let has_body = !body.trim().is_empty();

    // Tap target: a follow → the follower's profile; a like / repost → the
    // subject post; a reply / mention / quote → the post itself (`uri`).
    let target = match item.reason.as_str() {
        "follow" => format!("/profile/{}", urlencoding::encode(&item.author.did)),
        "like" | "repost" => match &item.reason_subject {
            Some(s) => format!("/post/{}", urlencoding::encode(s)),
            None => format!("/profile/{}", urlencoding::encode(&item.author.did)),
        },
        _ => format!("/post/{}", urlencoding::encode(&item.uri)),
    };

    render! {
        view(
            style: css!(
                flex_direction: FlexDirection::Row,
                width: percent(100),
                padding: theme::GUTTER,
                border_bottom_width: px(1),
                border_bottom_color: theme::BORDER,
            ),
            on_tap: move |_| {
                let _ = nav.navigate(&target);
            },
        ) {
            row_avatar(src: avatar)
            view(style: css!(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                margin_left: theme::ROW_GAP,
            )) {
                view(style: css!(
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                )) {
                    Icon(svg: icon, color: icon_color, size: "15")
                    text(
                        style: css!(
                            font_size: theme::T_NAME,
                            color: theme::TEXT_PRIMARY,
                            margin_left: px(6),
                            flex_grow: 1.0,
                            flex_shrink: 1.0,
                        ),
                        value: line,
                    )
                }
                Show(when: move || has_body, fallback: || render! { fragment() }) {
                    text(
                        style: css!(font_size: theme::T_BODY, color: theme::TEXT_SECONDARY, margin_top: px(4)),
                        value: body.clone(),
                    )
                }
            }
        }
    }
}

/// The signed-in user's own profile (Profile tab root). Resolves the own
/// DID, then renders the shared profile view with a logout action.
#[component]
fn my_profile_screen() -> Element {
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
fn profile_screen() -> Element {
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
fn profile_view(actor: String, show_logout: bool) -> Element {
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
                    meta: |r: &ProfileRow| match r {
                        ProfileRow::Header { .. } => ItemMeta::key("header".to_string())
                            .reuse_identifier("profile-header")
                            .estimated_size(320)
                            .full_span(true),
                        ProfileRow::Post(p) => ItemMeta::key(p.uri.clone())
                            .reuse_identifier("post")
                            .estimated_size(96),
                    },
                    children: |r: ProfileRow| match r {
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
fn profile_header(profile: bsky_domain::Profile, my_did: String, show_logout: bool) -> Element {
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
fn moderation_menu(did: String, muted: bool, blocking_uri: String, open: Signal<bool>) -> Element {
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
fn followers_screen() -> Element {
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
fn following_screen() -> Element {
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
fn follow_list(actor: String, followers: bool) -> Element {
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
fn avatar_disc(src: String) -> Element {
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
fn follow_button(did: String, following_uri: String) -> Element {
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
fn logout_button() -> Element {
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
fn settings_button() -> Element {
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
fn settings_screen() -> Element {
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
fn settings_section(title: String) -> Element {
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
fn settings_row(icon: &'static str, label: String, route: String) -> Element {
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
fn muted_accounts_screen() -> Element {
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
fn blocked_accounts_screen() -> Element {
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
fn moderation_account_list(res: Resource<Vec<bsky_domain::ActorView>>) -> Element {
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
fn nav_header(title: String) -> Element {
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

/// Enter a handle, then navigate to the auth screen which runs the OAuth flow.
#[component]
fn login_screen() -> Element {
    let handle = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let nav = use_navigator();

    let nav_go = nav.clone();
    let go = move |_: _| {
        let h = handle.get().trim().to_string();
        if h.is_empty() {
            error.set("ハンドルを入力してください".to_string());
            return;
        }
        let _ = nav_go.navigate(&format!("/auth/{h}"));
    };

    // Keep the 24px gutter, but push every edge out by the host safe-area
    // (status bar / notch / home indicator) so the title and CTA never sit
    // under system chrome. Reactive via `computed` — re-pads on rotation /
    // Dynamic Island / Android edge-to-edge toggle.
    let insets = safe_area_insets();
    let root_style = computed(move || {
        let i = insets.get();
        css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Stretch,
            background_color: theme::BG,
            padding_top: px(24.0 + i.top as f32),
            padding_bottom: px(24.0 + i.bottom as f32),
            padding_left: px(24.0 + i.leading as f32),
            padding_right: px(24.0 + i.trailing as f32),
        )
    });

    render! {
        view(style: root_style) {
            text(
                style: css!(
                    font_size: theme::T_TITLE,
                    font_weight: FontWeight::Bold,
                    color: theme::TEXT_PRIMARY,
                    margin_bottom: px(28),
                ),
                value: "Bluesky",
            )
            Input(
                text: handle,
                placeholder: "you.bsky.social",
                keyboard_type: KeyboardType::Url,
                // A Bluesky handle is a case-sensitive identifier: don't
                // auto-capitalize the first character, and suppress
                // autocorrect / spelling suggestions so a typed handle is
                // never silently rewritten.
                auto_capitalize: AutoCapitalize::None,
                autocorrect: false,
                spell_check: false,
                placeholder_color: "#8B98A5",
                caret_color: "#1083FE",
                style: "height: 48px; border-radius: 10px; \
                        background-color: #16191F; color: #FFFFFF; font-size: 16px; \
                        padding-left: 14px; padding-right: 14px; margin-bottom: 12px;",
            )
            Show(when: move || !error.get().is_empty(), fallback: || render! { fragment() }) {
                text(
                    style: css!(
                        font_size: theme::T_META,
                        color: Color::hex(0xFF6B6B),
                        margin_bottom: px(12),
                    ),
                    value: computed(move || error.get()),
                )
            }
            view(
                style: css!(
                    height: px(48),
                    border_radius: px(10),
                    background_color: theme::ACCENT,
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                ),
                on_tap: go,
            ) {
                text(
                    style: css!(
                        font_size: px(16),
                        font_weight: FontWeight::Bold,
                        color: theme::ON_ACCENT,
                    ),
                    value: "続ける",
                )
            }
        }
    }
}

/// Runs the OAuth authorization for the `:handle` param and hosts the auth
/// WebView. On the redirect, completes login and resets to the timeline.
#[component]
fn auth_screen() -> Element {
    let handle = use_param("handle");
    // Empty == still preparing (or errored); non-empty == authorization URL to
    // load in the WebView.
    let auth_url = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let completing = RwSignal::new(false);
    let nav = use_navigator();
    let AuthState(authed) = use_context::<AuthState>().expect("AuthState provided at root");

    // Kick off the (network-heavy) OAuth authorize once, on mount.
    on_mount(move || {
        let h = handle.get().unwrap_or_default();
        spawn_local(async move {
            if h.is_empty() {
                error.set("ハンドルが指定されていません".to_string());
                return;
            }
            match bsky_auth::begin_login(&h).await {
                Ok(url) => auth_url.set(url),
                Err(e) => error.set(e),
            }
        });
    });

    let nav_done = nav.clone();
    let on_nav = move |url: String| {
        if !bsky_auth::is_redirect(&url) || completing.get() {
            return;
        }
        completing.set(true);
        let nav = nav_done.clone();
        spawn_local(async move {
            match bsky_auth::complete_login(&url).await {
                Ok(()) => {
                    // Flip auth state (so the keep-alive timeline re-fetches)
                    // and switch from the `(auth)` branch to the Home tab.
                    authed.set(true);
                    let _ = nav.select("/(home)");
                }
                Err(e) => {
                    error.set(e);
                    completing.set(false);
                }
            }
        });
    };

    // Opaque white on the OUTER container. The native WebView is transparent
    // on iOS (WKWebView is forced to `.clear` and ignores CSS background), so
    // without an opaque ancestor the leaving screen shows through it during
    // the route transition and until the page paints. White also matches the
    // light bsky auth page, so there's no flash. The safe-area padding insets
    // the web page off the notch / home indicator; the strips paint white too
    // (same container background), so there are no black bars.
    let insets = safe_area_insets();
    let root_style = computed(move || {
        let i = insets.get();
        css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: Color::hex(0xFFFFFF),
            padding_top: px(i.top as f32),
            padding_bottom: px(i.bottom as f32),
            padding_left: px(i.leading as f32),
            padding_right: px(i.trailing as f32),
        )
    });

    render! {
        view(style: root_style) {
            Show(
                when: move || !auth_url.get().is_empty(),
                fallback: move || render! { auth_loading(error: error) },
            ) {
                WebView(
                    url: auth_url,
                    on_navigation: on_nav.clone(),
                    style: "flex-grow: 1;",
                )
            }
        }
    }
}

#[component]
fn auth_loading(error: RwSignal<String>) -> Element {
    render! {
        view(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: px(24),
        )) {
            text(
                // Dark text — the auth screen is on a white background.
                style: css!(font_size: theme::T_BODY, color: Color::hex(0x536471)),
                value: computed(move || {
                    let e = error.get();
                    if e.is_empty() {
                        "認証を準備中…".to_string()
                    } else {
                        format!("エラー: {e}")
                    }
                }),
            )
        }
    }
}

/// Authenticated home timeline.
#[component]
fn timeline_screen() -> Element {
    // Gate on the shared auth state (the boot restore in `tabs_layout`
    // flips it; login does too). Reading it in the fetcher's synchronous
    // prefix makes the feed re-run the moment auth flips true — so a fresh
    // login lands on a populated timeline without remounting the shell.
    let AuthState(authed) = use_context::<AuthState>().expect("AuthState provided at root");
    // Initial page (auth-gated, re-runs when auth flips). `more` accumulates
    // subsequent pages appended by infinite scroll; `next_cursor` tracks the
    // cursor to fetch from once we've paged past the first page (`seeded`).
    let feed = resource(move || {
        let ready = authed.get();
        async move {
            if !ready {
                return Err(String::new());
            }
            bsky_auth::fetch_timeline(50, None).await
        }
    });
    let more = RwSignal::new(Vec::<bsky_domain::FeedPost>::new());
    let next_cursor = RwSignal::new(None::<String>);
    let seeded = RwSignal::new(false);
    let loading_more = RwSignal::new(false);

    // on_scrolltolower fires a few rows before the end (lower_threshold_item_count).
    // Fetch the next page from the current cursor and append it. Guards against
    // re-entrancy (loading_more) and the end of the feed (cursor == None).
    let load_more = move |_| {
        if loading_more.get() {
            return;
        }
        let cursor = if seeded.get() {
            next_cursor.get()
        } else {
            feed.get().and_then(|t| t.cursor)
        };
        let Some(cursor) = cursor else {
            return; // no more pages
        };
        loading_more.set(true);
        spawn_local(async move {
            match bsky_auth::fetch_timeline(50, Some(cursor)).await {
                Ok(page) => {
                    let mut acc = more.get();
                    acc.extend(page.posts);
                    more.set(acc);
                    next_cursor.set(page.cursor);
                    seeded.set(true);
                }
                Err(e) => eprintln!("bluesky: timeline load-more failed: {e}"),
            }
            loading_more.set(false);
        });
    };

    // Inset the feed by the safe-area: top keeps the first post clear of the
    // status bar / notch, bottom keeps the last clear of the home indicator.
    // Only the top inset is ours; the tab bar below the Outlet owns the
    // bottom inset. `position: relative` anchors the floating compose button.
    let insets = safe_area_insets();
    let root_style = computed(move || {
        css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
            padding_top: px(insets.get().top as f32),
        )
        .raw("position", "relative")
    });

    render! {
        view(style: root_style) {
            Show(
                when: move || feed.get().is_some(),
                fallback: move || render! {
                    status_pane(
                        // Empty error == the "waiting for auth/restore"
                        // sentinel — show loading, not a blank error line.
                        message: match feed.error() {
                            Some(e) if !e.is_empty() => e,
                            _ => "読み込み中…".to_string(),
                        },
                    )
                },
            ) {
                list(
                    style: css!(flex_grow: 1.0, flex_shrink: 1.0, width: percent(100)),
                    lower_threshold_item_count: 3,
                    on_scrolltolower: load_more,
                    each: move || {
                        let mut all = feed.get().map(|t| t.posts).unwrap_or_default();
                        all.extend(more.get());
                        all
                    },
                    // Entry identity, not post identity: a post can appear
                    // both as the original and as a repost in one timeline,
                    // and duplicate item-keys corrupt the native list diff.
                    meta: |p: &bsky_domain::FeedPost| {
                        let key = match &p.reposted_by {
                            Some(by) => format!("{}#repost:{}", p.uri, by.did),
                            None => p.uri.clone(),
                        };
                        ItemMeta::key(key).reuse_identifier("post").estimated_size(140)
                    },
                    children: |p: bsky_domain::FeedPost| render! {
                        PostRow(post: p)
                    },
                )
            }
            ComposeFab {}
        }
    }
}

/// Floating compose button, anchored bottom-right above the tab bar.
#[component]
fn compose_fab() -> Element {
    let nav = use_navigator();
    render! {
        view(
            style: css!(
                width: px(56),
                height: px(56),
                border_radius: px(28),
                background_color: theme::ACCENT,
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            )
            .raw("position", "absolute")
            .raw("right", "16px")
            .raw("bottom", "16px"),
            on_tap: move |_| {
                let _ = nav.navigate("/compose");
            },
        ) {
            Icon(svg: lucide::Pencil, color: "#FFFFFF", size: "24")
        }
    }
}

#[component]
fn post_list(posts: Vec<bsky_domain::FeedPost>) -> Element {
    // Lynx's native-virtualised `<list>` — recycles off-screen rows and scrolls
    // vertically by default. Scales to many posts without keeping every row
    // mounted (unlike scroll_view + ForEach).
    render! {
        list(
            style: css!(flex_grow: 1.0, flex_shrink: 1.0, width: percent(100)),
            each: {
                let posts = posts.clone();
                move || posts.clone()
            },
            meta: |p: &bsky_domain::FeedPost| ItemMeta::key(p.uri.clone()),
            children: |p: bsky_domain::FeedPost| render! { PostRow(post: p) },
        )
    }
}

/// Stateful wrapper around the presentational [`PostCard`]: owns the
/// optimistic like / repost signals and drives `bsky-auth`. Tapping the
/// body opens the post detail.
#[component]
fn post_row(post: bsky_domain::FeedPost) -> Element {
    let nav = use_navigator();

    let liked = RwSignal::new(post.like_uri.is_some());
    let reposted = RwSignal::new(post.repost_uri.is_some());
    let like_count = RwSignal::new(post.like_count as i64);
    let repost_count = RwSignal::new(post.repost_count as i64);
    // Record URIs needed to undo (kept in signals; updated as the calls land).
    let like_uri = RwSignal::new(post.like_uri.clone());
    let repost_uri = RwSignal::new(post.repost_uri.clone());

    let subject_uri = post.uri.clone();
    let subject_cid = post.cid.clone();

    // Open detail: percent-encode the at:// URI into one path segment.
    let open_uri = post.uri.clone();
    let nav_open = nav.clone();
    let on_open: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
        let enc = urlencoding::encode(&open_uri);
        let _ = nav_open.navigate(&format!("/post/{enc}"));
    });

    // Tap the avatar → the author's profile.
    let author_did = post.author.did.clone();
    let nav_author = nav.clone();
    let on_author: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
        let enc = urlencoding::encode(&author_did);
        let _ = nav_author.navigate(&format!("/profile/{enc}"));
    });

    // Like / unlike with optimistic toggle + count, reverting on error.
    let su = subject_uri.clone();
    let sc = subject_cid.clone();
    let on_like: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
        let was = liked.get();
        liked.set(!was);
        like_count.set(like_count.get() + if was { -1 } else { 1 });
        let su = su.clone();
        let sc = sc.clone();
        spawn_local(async move {
            if was {
                let uri = like_uri.get();
                let ok = match uri {
                    Some(u) => bsky_auth::unlike(&u).await.is_ok(),
                    None => true,
                };
                if ok {
                    like_uri.set(None);
                } else {
                    liked.set(true);
                    like_count.set(like_count.get() + 1);
                }
            } else {
                match bsky_auth::like(&su, &sc).await {
                    Ok(u) => like_uri.set(Some(u)),
                    Err(_) => {
                        liked.set(false);
                        like_count.set(like_count.get() - 1);
                    }
                }
            }
        });
    });

    // Repost / unrepost, same shape.
    let su = subject_uri.clone();
    let sc = subject_cid.clone();
    let on_repost: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
        let was = reposted.get();
        reposted.set(!was);
        repost_count.set(repost_count.get() + if was { -1 } else { 1 });
        let su = su.clone();
        let sc = sc.clone();
        spawn_local(async move {
            if was {
                let uri = repost_uri.get();
                let ok = match uri {
                    Some(u) => bsky_auth::unrepost(&u).await.is_ok(),
                    None => true,
                };
                if ok {
                    repost_uri.set(None);
                } else {
                    reposted.set(true);
                    repost_count.set(repost_count.get() + 1);
                }
            } else {
                match bsky_auth::repost(&su, &sc).await {
                    Ok(u) => repost_uri.set(Some(u)),
                    Err(_) => {
                        reposted.set(false);
                        repost_count.set(repost_count.get() - 1);
                    }
                }
            }
        });
    });

    render! {
        PostCard(
            post: post.clone(),
            liked: liked,
            reposted: reposted,
            like_count: like_count,
            repost_count: repost_count,
            on_open: on_open,
            on_like: on_like,
            on_repost: on_repost,
            on_author: on_author,
        )
    }
}

/// Post detail / thread: the focused post followed by its direct
/// replies, with a back header. The at:// URI arrives percent-encoded as
/// the `:uri` route param.
#[component]
fn post_detail_screen() -> Element {
    let nav = use_navigator();
    let uri_param = use_param("uri");

    let thread = resource(move || {
        let enc = uri_param.get().unwrap_or_default();
        async move {
            let uri = urlencoding::decode(&enc)
                .map(|c| c.into_owned())
                .unwrap_or(enc);
            bsky_auth::get_post_thread(&uri).await
        }
    });

    let insets = safe_area_insets();
    let header_style = computed(move || {
        let i = insets.get();
        css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            background_color: theme::BG,
            border_bottom_width: px(1),
            border_bottom_color: theme::BORDER,
            padding_top: px(i.top as f32 + 8.0),
            padding_bottom: px(8),
            padding_left: px(8),
            padding_right: px(16),
        )
    });

    let nav_back = nav.clone();
    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
        )) {
            view(style: header_style) {
                view(
                    style: css!(
                        padding: px(8),
                        display: Display::Flex,
                        align_items: AlignItems::Center,
                    ),
                    on_tap: move |_| {
                        let _ = nav_back.back();
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
                    value: "ポスト",
                )
            }
            Show(
                when: move || thread.get().is_some(),
                fallback: move || render! {
                    status_pane(
                        message: match thread.error() {
                            Some(e) if !e.is_empty() => e,
                            _ => "読み込み中…".to_string(),
                        },
                    )
                },
            ) {
                post_list(posts: {
                    let t = thread.get().unwrap_or_default();
                    let mut v = Vec::new();
                    if let Some(p) = t.post {
                        v.push(p);
                    }
                    v.extend(t.replies);
                    v
                })
            }
        }
    }
}

/// New-post composer (full-screen route over the tabs). Text only —
/// media upload is skipped (no whisker picker module; see MEMO). On
/// success it pops back to the feed.
#[component]
fn compose_screen() -> Element {
    let nav = use_navigator();
    let text = RwSignal::new(String::new());
    let posting = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let insets = safe_area_insets();

    let remaining = computed(move || 300i64 - text.get().chars().count() as i64);
    let can_post =
        computed(move || !text.get().trim().is_empty() && remaining.get() >= 0 && !posting.get());

    let header_style = computed(move || {
        css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding_top: px(insets.get().top as f32 + 8.0),
            padding_bottom: px(8),
            padding_left: px(16),
            padding_right: px(16),
            border_bottom_width: px(1),
            border_bottom_color: theme::BORDER,
        )
    });

    let nav_cancel = nav.clone();
    let nav_post = nav.clone();
    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
        )) {
            view(style: header_style) {
                view(
                    style: css!(padding: px(4)),
                    on_tap: move |_| {
                        let _ = nav_cancel.back();
                    },
                ) {
                    text(
                        style: css!(font_size: px(16), color: theme::TEXT_PRIMARY),
                        value: "キャンセル",
                    )
                }
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                )) {
                    text(
                        style: computed(move || css!(
                            font_size: theme::T_META,
                            color: if remaining.get() < 0 {
                                Color::hex(0xFF6B6B)
                            } else {
                                theme::TEXT_SECONDARY
                            },
                            margin_right: px(12),
                        )),
                        value: computed(move || remaining.get().to_string()),
                    )
                    view(
                        style: computed(move || css!(
                            height: px(34),
                            padding_left: px(16),
                            padding_right: px(16),
                            border_radius: px(17),
                            background_color: theme::ACCENT,
                            display: Display::Flex,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            opacity: if can_post.get() { 1.0 } else { 0.4 },
                        )),
                        on_tap: move |_| {
                            if !can_post.get() {
                                return;
                            }
                            let body = text.get().trim().to_string();
                            posting.set(true);
                            error.set(String::new());
                            let nav = nav_post.clone();
                            spawn_local(async move {
                                match bsky_auth::create_post(&body).await {
                                    Ok(_) => {
                                        let _ = nav.back();
                                    }
                                    Err(e) => {
                                        error.set(e);
                                        posting.set(false);
                                    }
                                }
                            });
                        },
                    ) {
                        text(
                            style: css!(
                                font_size: px(15),
                                font_weight: FontWeight::Bold,
                                color: theme::ON_ACCENT,
                            ),
                            value: "投稿",
                        )
                    }
                }
            }
            Input(
                text: text,
                placeholder: "いまどうしてる？",
                multiline: true,
                auto_focus: true,
                placeholder_color: "#8B98A5",
                caret_color: "#1083FE",
                style: "flex-grow: 1; padding: 16px; color: #FFFFFF; font-size: 18px;",
            )
            Show(when: move || !error.get().is_empty(), fallback: || render! { fragment() }) {
                text(
                    style: css!(
                        font_size: theme::T_META,
                        color: Color::hex(0xFF6B6B),
                        padding: px(16),
                    ),
                    value: computed(move || error.get()),
                )
            }
        }
    }
}

#[component]
fn status_pane(message: String) -> Element {
    render! {
        view(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        )) {
            text(
                style: css!(font_size: theme::T_META, color: theme::TEXT_SECONDARY),
                value: message.clone(),
            )
        }
    }
}
