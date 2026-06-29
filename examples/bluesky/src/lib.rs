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
use whisker::css::{AlignItems, Display, FlexDirection, FontWeight, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{Icon, lucide};
use whisker_input::{AutoCapitalize, Input, KeyboardType};
use whisker_router::render::{
    AndroidPredictiveBack, Outlet, Router, RouterHandle, SwipeBack, use_navigator, use_param,
    use_pathname,
};
use whisker_router::routes;
use whisker_safe_area::safe_area_insets;
use whisker_webview::WebView;

use bsky_theme as theme;

#[whisker::main]
pub fn app() -> Element {
    render! {
        view(style: css!(
            flex_grow: 1.0,
            background_color: theme::BG,
            flex_direction: FlexDirection::Column,
        )) {
            Router(routes: routes! {
                Stack {
                    Route(path: "", component: TabsLayout) {
                        Switch {
                            Route(path: "(home)") {
                                Stack {
                                    Route(path: "", component: TimelineScreen)
                                    Route(path: "post/:uri", component: PostDetailScreen)
                                }
                            }
                            Route(path: "(search)") {
                                Stack {
                                    Route(path: "", component: SearchScreen)
                                }
                            }
                            Route(path: "(notifications)") {
                                Stack {
                                    Route(path: "", component: NotificationsScreen)
                                }
                            }
                            Route(path: "(profile)") {
                                Stack {
                                    Route(path: "", component: MyProfileScreen)
                                }
                            }
                        }
                    }
                    Route(path: "login", component: LoginScreen)
                    Route(path: "auth/:handle", component: AuthScreen)
                }
            }) {
                Outlet {}
                SwipeBack {}
                AndroidPredictiveBack {}
            }
        }
    }
}

/// Authenticated shell: the active tab's `Outlet` above a bottom tab bar.
#[component]
fn tabs_layout() -> Element {
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
            TabBar {}
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
            !p.contains("/(search)") && !p.contains("/(notifications)") && !p.contains("/(profile)")
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

#[component]
fn search_screen() -> Element {
    render! { placeholder_screen(title: "検索（準備中）") }
}

#[component]
fn notifications_screen() -> Element {
    render! { placeholder_screen(title: "通知（準備中）") }
}

#[component]
fn my_profile_screen() -> Element {
    render! { placeholder_screen(title: "プロフィール（準備中）") }
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
                    // Clear the auth stack so the timeline (the root route)
                    // is the only entry — no login/auth screens linger.
                    let _ = nav.reset("/");
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
    let nav = use_navigator();
    // Auth gate. Start from what this process already knows; on a cold
    // launch (`authed` false) restore the persisted session, flipping
    // `authed` on success or hopping to /login when there's nothing to
    // restore — so the timeline is the default screen and login is only
    // the fallback. This runs once (`on_mount`), so it can't feed back
    // into a reactive loop the way a router-mutating `effect` could.
    let authed = RwSignal::new(bsky_auth::is_authenticated());
    on_mount(move || {
        if authed.get() {
            return;
        }
        let nav = nav.clone();
        spawn_local(async move {
            if bsky_auth::restore_session().await {
                authed.set(true);
            } else {
                let _ = nav.reset("/login");
            }
        });
    });

    // Load the feed once authenticated. Reading `authed` in the fetcher's
    // synchronous prefix makes the resource re-run when restore flips it
    // true; until then it yields the empty "still waiting" sentinel.
    let feed = resource(move || {
        let ready = authed.get();
        async move {
            if !ready {
                return Err(String::new());
            }
            bsky_auth::fetch_timeline(50).await
        }
    });

    // Inset the feed by the safe-area: top keeps the first post clear of the
    // status bar / notch, bottom keeps the last clear of the home indicator.
    let insets = safe_area_insets();
    let root_style = computed(move || {
        let i = insets.get();
        css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
            padding_top: px(i.top as f32),
            padding_bottom: px(i.bottom as f32),
            padding_left: px(i.leading as f32),
            padding_right: px(i.trailing as f32),
        )
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
                post_list(posts: feed.get().map(|t| t.posts).unwrap_or_default())
            }
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
            key: |p: &bsky_domain::FeedPost| p.uri.clone(),
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
