//! Bluesky (AT Protocol) client example.
//!
//! Navigation is driven by `whisker-router`. The root is a stack: an index
//! `tabs_layout` (the authenticated, tabbed shell) plus full-screen
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
use whisker::css::{AlignItems, Display, FlexDirection, FontWeight, JustifyContent, PositionKind};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{Icon, lucide};
use whisker_image::{Image, ImageMode};
use whisker_input::{AutoCapitalize, Input, KeyboardType, ReturnKey};
use whisker_keyboard::keyboard_height;
use whisker_router::render::{Outlet, Router, RouterHandle, use_group, use_navigator, use_param};
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
        View(style: css!(
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
                    Switch(initial: "(home)") {
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
    let active_group = use_group();
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
                let _ = nav.navigate("/(auth)");
            }
        });
    });

    let on_auth = computed(move || active_group.get().as_deref() == Some("auth"));

    render! {
        View(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
            View(style: css!(
                flex_grow: 1.0,
                flex_shrink: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            )) {
                Outlet {}
            }
            Show(when: move || !on_auth.get(), fallback: || render! { Fragment() }) {
                TabBar {}
            }
        }
    }
}

/// Bottom tab bar. Active tab is derived from the current route group;
/// tapping navigates to that branch,
/// preserving each tab's own stack.
#[component]
fn tab_bar() -> Element {
    let nav = use_navigator();
    let active_group = use_group();
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
        View(style: bar_style) {
            TabBarItem(group: "home", url: "/(home)", icon: lucide::House, active_group: active_group, nav: nav.clone())
            TabBarItem(group: "search", url: "/(search)", icon: lucide::Search, active_group: active_group, nav: nav.clone())
            TabBarItem(group: "notifications", url: "/(notifications)", icon: lucide::Bell, active_group: active_group, nav: nav.clone())
            TabBarItem(group: "profile", url: "/(profile)", icon: lucide::User, active_group: active_group, nav: nav.clone())
        }
    }
}

#[component]
fn tab_bar_item(
    group: &'static str,
    url: &'static str,
    icon: Signal<String>,
    active_group: ReadSignal<Option<String>>,
    nav: RouterHandle,
) -> Element {
    let is_active = computed(move || active_group.get().as_deref() == Some(group));
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
        View(
            style: css!(
                flex_grow: 1.0,
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                height: px(52),
            ),
            on_tap: move |_| {
                let _ = nav.navigate(url);
            },
        ) {
            Icon(svg: icon, color: color, size: "26")
        }
    }
}

/// Placeholder shown by tabs whose real screen lands in a later phase.
#[component]
fn placeholder_screen(title: String) -> Element {
    let title = StoredValue::new(title.clone());
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
        View(style: style) {
            Text(
                style: css!(font_size: theme::T_BODY, color: theme::TEXT_SECONDARY),
                value: title.get(),
            )
        }
    }
}

/// Which result set the search screen is showing.
#[path = "screens/auth.rs"]
mod auth;
#[path = "screens/notifications.rs"]
mod notifications;
#[path = "screens/profile.rs"]
mod profile;
#[path = "screens/search.rs"]
mod search;
#[path = "screens/timeline.rs"]
mod timeline;

use auth::*;
use notifications::*;
use profile::*;
use search::*;
use timeline::*;
