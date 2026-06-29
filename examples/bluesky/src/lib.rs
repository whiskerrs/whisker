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
use whisker_image::{Image, ImageMode};
use whisker_input::{AutoCapitalize, Input, KeyboardType};
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
                            }
                        }
                        Route(path: "(search)") {
                            Stack {
                                Route(path: "", component: SearchScreen)
                                Route(path: "post/:uri", component: PostDetailScreen)
                                Route(path: "profile/:did", component: ProfileScreen)
                            }
                        }
                        Route(path: "(notifications)") {
                            Stack {
                                Route(path: "", component: NotificationsScreen)
                                Route(path: "post/:uri", component: PostDetailScreen)
                                Route(path: "profile/:did", component: ProfileScreen)
                            }
                        }
                        Route(path: "(profile)") {
                            Stack {
                                Route(path: "", component: MyProfileScreen)
                                Route(path: "post/:uri", component: PostDetailScreen)
                                Route(path: "profile/:did", component: ProfileScreen)
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

#[component]
fn search_screen() -> Element {
    render! { placeholder_screen(title: "検索（準備中）") }
}

#[component]
fn notifications_screen() -> Element {
    render! { placeholder_screen(title: "通知（準備中）") }
}

/// The signed-in user's own profile (Profile tab root). Resolves the own
/// DID, then renders the shared profile view with a logout action.
#[component]
fn my_profile_screen() -> Element {
    let me = resource(|| async {
        bsky_auth::my_did()
            .await
            .ok_or_else(|| "not authenticated".to_string())
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

/// Profile header (banner / avatar / name / bio / counts + follow or
/// logout) followed by the account's authored posts.
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

    render! {
        view(style: css!(flex_grow: 1.0, flex_shrink: 1.0, flex_direction: FlexDirection::Column)) {
            Show(
                when: move || prof.get().is_some(),
                fallback: move || render! {
                    status_pane(
                        message: match prof.error() {
                            Some(e) if !e.is_empty() => e,
                            _ => "読み込み中…".to_string(),
                        },
                    )
                },
            ) {
                profile_header(
                    profile: prof.get().map(|(p, _)| p).unwrap_or_default(),
                    my_did: prof.get().map(|(_, me)| me).unwrap_or_default(),
                    show_logout: show_logout,
                )
                post_list(posts: feed.get().unwrap_or_default())
            }
        }
    }
}

#[component]
fn profile_header(profile: bsky_domain::Profile, my_did: String, show_logout: bool) -> Element {
    let banner = profile.banner.clone().unwrap_or_default();
    let avatar = profile.avatar.clone().unwrap_or_default();
    let is_me = profile.did == my_did;
    let counts = format!(
        "{} フォロー中 · {} フォロワー · {} ポスト",
        profile.follows_count, profile.followers_count, profile.posts_count
    );
    // Cloned for the (re-invokable) follow-button Show children closure.
    // `following_uri` is passed as a String (empty == not following).
    let follow_did = profile.did.clone();
    let follow_uri = profile.following_uri.clone().unwrap_or_default();
    // Extract every field to an owned local so `profile` isn't referenced
    // inside the render closures (it's not `Copy`).
    let name = profile.name();
    let handle = format!("@{}", profile.handle);
    let description = profile.description.clone().unwrap_or_default();
    let has_desc = !description.is_empty();

    render! {
        view(style: css!(
            flex_direction: FlexDirection::Column,
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
                Show(when: move || show_logout, fallback: || render! { fragment() }) {
                    logout_button()
                }
                Show(when: move || !show_logout && !is_me, fallback: || render! { fragment() }) {
                    follow_button(
                        did: follow_did.clone(),
                        following_uri: follow_uri.clone(),
                    )
                }
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
            text(
                style: css!(
                    font_size: theme::T_META,
                    color: theme::TEXT_SECONDARY,
                    margin_top: px(10),
                    margin_left: theme::GUTTER,
                ),
                value: counts.clone(),
            )
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
                post_list(posts: feed.get().map(|t| t.posts).unwrap_or_default())
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
