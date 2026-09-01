use super::*;

/// Enter a handle, then navigate to the auth screen which runs the OAuth flow.
#[component]
pub(super) fn login_screen() -> Element {
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
    //
    // Keyboard avoidance: add the keyboard's height to the bottom padding
    // (taking the larger of it and the home-indicator inset, since the
    // keyboard already covers that region). On a centered column this
    // lifts the whole form clear of the keyboard when the handle field is
    // focused; it settles back as the keyboard dismisses.
    let insets = safe_area_insets();
    let kb = keyboard_height();
    let root_style = computed(move || {
        let i = insets.get();
        let bottom = (i.bottom as f32).max(kb.get() as f32);
        css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Stretch,
            background_color: theme::BG,
            padding_top: px(24.0 + i.top as f32),
            padding_bottom: px(24.0 + bottom),
            padding_left: px(24.0 + i.leading as f32),
            padding_right: px(24.0 + i.trailing as f32),
        )
    });

    render! {
        View(style: root_style) {
            Text(
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
                style: Css::new()
                    .height(px(48))
                    .border_radius(px(10))
                    .background_color(Color::hex(0x16191f))
                    .color(Color::hex(0xffffff))
                    .font_size(px(16))
                    .padding_left(px(14))
                    .padding_right(px(14))
                    .margin_bottom(px(12)),
            )
            Show(when: move || !error.get().is_empty(), fallback: || render! { Fragment() }) {
                Text(
                    style: css!(
                        font_size: theme::T_META,
                        color: Color::hex(0xFF6B6B),
                        margin_bottom: px(12),
                    ),
                    value: computed(move || error.get()),
                )
            }
            View(
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
                Text(
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
pub(super) fn auth_screen() -> Element {
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
        View(style: root_style) {
            Show(
                when: move || !auth_url.get().is_empty(),
                fallback: move || render! { AuthLoading(error: error) },
            ) {
                WebView(
                    url: auth_url,
                    on_navigation: on_nav.clone(),
                    style: css!(flex_grow: 1.0),
                )
            }
        }
    }
}

#[component]
pub(super) fn auth_loading(error: RwSignal<String>) -> Element {
    render! {
        View(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: px(24),
        )) {
            Text(
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
