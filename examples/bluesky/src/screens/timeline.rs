use super::*;

/// Authenticated home timeline.
#[component]
pub(super) fn timeline_screen() -> Element {
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

    // The virtual list requests more rows before the visible window reaches the end.
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
            position: PositionKind::Relative,
        )
    });

    render! {
        View(style: root_style) {
            Show(
                when: move || feed.get().is_some(),
                fallback: move || render! {
                    StatusPane(
                        // Empty error == the "waiting for auth/restore"
                        // sentinel — show loading, not a blank error line.
                        message: match feed.error() {
                            Some(e) if !e.is_empty() => e,
                            _ => "読み込み中…".to_string(),
                        },
                    )
                },
            ) {
                List(
                    style: css!(flex_grow: 1.0, flex_shrink: 1.0, width: percent(100)),
                    end_reached_threshold: 320.0,
                    on_end_reached: move || load_more(()),
                    each: move || {
                        let mut all = feed.get().map(|t| t.posts).unwrap_or_default();
                        all.extend(more.get());
                        all
                    },
                    // Entry identity, not post identity: a post can appear
                    // both as the original and as a repost in one timeline,
                    // and duplicate item-keys corrupt the native list diff.
                    key: |p: &bsky_domain::FeedPost| {
                        match &p.reposted_by {
                            Some(by) => format!("{}#repost:{}", p.uri, by.did),
                            None => p.uri.clone(),
                        }
                    },
                    children: |p: ReadSignal<bsky_domain::FeedPost>| render! {
                        PostRow(post: p.get())
                    },
                )
            }
            ComposeFab {}
        }
    }
}

/// Floating compose button, anchored bottom-right above the tab bar.
#[component]
pub(super) fn compose_fab() -> Element {
    let nav = use_navigator();
    render! {
        View(
            style: css!(
                width: px(56),
                height: px(56),
                border_radius: px(28),
                background_color: theme::ACCENT,
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                position: PositionKind::Absolute,
                right: px(16),
                bottom: px(16),
            ),
            on_tap: move |_| {
                let _ = nav.navigate("/compose");
            },
        ) {
            Icon(svg: lucide::Pencil, color: "#FFFFFF", size: "24")
        }
    }
}

#[component]
pub(super) fn post_list(posts: Vec<bsky_domain::FeedPost>) -> Element {
    // Whisker's virtualized list recycles off-screen rows and scrolls
    // vertically by default. Scales to many posts without keeping every row
    // mounted (unlike scroll_view + ForEach).
    render! {
        List(
            style: css!(flex_grow: 1.0, flex_shrink: 1.0, width: percent(100)),
            each: {
                let posts = posts.clone();
                move || posts.clone()
            },
            key: |p: &bsky_domain::FeedPost| p.uri.clone(),
            children: |p: ReadSignal<bsky_domain::FeedPost>| render! { PostRow(post: p.get()) },
        )
    }
}

/// Stateful wrapper around the presentational [`PostCard`]: owns the
/// optimistic like / repost signals and drives `bsky-auth`. Tapping the
/// body opens the post detail.
#[component]
pub(super) fn post_row(post: bsky_domain::FeedPost) -> Element {
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
pub(super) fn post_detail_screen() -> Element {
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
        View(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
        )) {
            View(style: header_style) {
                View(
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
                Text(
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
                    StatusPane(
                        message: match thread.error() {
                            Some(e) if !e.is_empty() => e,
                            _ => "読み込み中…".to_string(),
                        },
                    )
                },
            ) {
                PostList(posts: {
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
pub(super) fn compose_screen() -> Element {
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
        View(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
        )) {
            View(style: header_style) {
                View(
                    style: css!(padding: px(4)),
                    on_tap: move |_| {
                        let _ = nav_cancel.back();
                    },
                ) {
                    Text(
                        style: css!(font_size: px(16), color: theme::TEXT_PRIMARY),
                        value: "キャンセル",
                    )
                }
                View(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                )) {
                    Text(
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
                    View(
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
                        Text(
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
                style: Css::new()
                    .flex_grow(1.0)
                    .padding(px(16))
                    .color(Color::hex(0xffffff))
                    .font_size(px(18)),
            )
            Show(when: move || !error.get().is_empty(), fallback: || render! { Fragment() }) {
                Text(
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
pub(super) fn status_pane(message: String) -> Element {
    let message = StoredValue::new(message.clone());
    render! {
        View(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        )) {
            Text(
                style: css!(font_size: theme::T_META, color: theme::TEXT_SECONDARY),
                value: message.get(),
            )
        }
    }
}
