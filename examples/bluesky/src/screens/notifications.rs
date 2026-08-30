use super::*;

#[component]
pub(super) fn notifications_screen() -> Element {
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
pub(super) fn notification_list(items: Vec<bsky_domain::Notification>) -> Element {
    render! {
        list(
            style: css!(flex_grow: 1.0, flex_shrink: 1.0, width: percent(100)),
            each: {
                let items = items.clone();
                move || items.clone()
            },
            key: |n: &bsky_domain::Notification| n.uri.clone(),
            children: |n: ReadSignal<bsky_domain::Notification>| render! { notification_row(item: n.get()) },
        )
    }
}

/// One notification: a reason glyph + "<name> <action>", with the post
/// body shown for reply / mention / quote. Tapping opens the relevant
/// post (or the follower's profile).
#[component]
pub(super) fn notification_row(item: bsky_domain::Notification) -> Element {
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
