//! Bluesky (AT Protocol) client example — milestone 1.
//!
//! Navigation is driven by `whisker-router`. Three routes on a stack:
//!   - `""`            — [`login_screen`]: enter a handle.
//!   - `"auth/:handle"` — [`auth_screen`]: runs the atproto OAuth authorization
//!     (identity resolution + PAR, shown as a loading state) then loads the
//!     authorization page in an embedded [`WebView`]. When the auth server
//!     redirects to our loopback URI, the WebView's `on_navigation` hands the
//!     URL to [`bsky_auth::complete_login`], which exchanges the code for a
//!     DPoP-bound session, then `reset`s the stack to the timeline.
//!   - `"timeline"`    — [`timeline_screen`]: the authenticated home feed.
//!
//! The auth WebView is its own screen (not an inline branch of login), so the
//! OAuth pre-flight latency is covered by a dedicated loading screen and the
//! back gesture behaves naturally.

use bsky_ui_kit::PostCard;
use whisker::css::{AlignItems, Display, FlexDirection, FontWeight, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_input::{AutoCapitalize, Input, KeyboardType};
use whisker_router::render::{
    AndroidPredictiveBack, Outlet, Router, SwipeBack, use_navigator, use_param,
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
                    Route(path: "", component: TimelineScreen)
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
                timeline_list(posts: feed.get().map(|t| t.posts).unwrap_or_default())
            }
        }
    }
}

#[component]
fn timeline_list(posts: Vec<bsky_domain::FeedPost>) -> Element {
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
            children: |p: bsky_domain::FeedPost| render! { PostCard(post: p) },
        )
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
