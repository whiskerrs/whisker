use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    People,
    Posts,
}

#[component]
pub(super) fn search_screen() -> Element {
    let insets = safe_area_insets();
    // `draft` is the live field text; `query` is the committed term (set on
    // Return). Separating them means we only hit the network on submit, not
    // on every keystroke.
    let draft = RwSignal::new(String::new());
    let query = RwSignal::new(String::new());
    let mode = RwSignal::new(SearchMode::People);
    // Drives the swipeable pager: keyed `scroll_to` on a tab tap, bound
    // via `ref:` on the horizontal `<list>` below.
    let pager = ListHandle::<&'static str>::new();

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

    let people_pager = pager.clone();
    let posts_pager = pager.clone();
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
                    style: Css::new()
                        .width(percent(100))
                        .height(px(40))
                        .border_radius(px(10))
                        .background_color(Color::hex(0x16191f))
                        .color(Color::hex(0xffffff))
                        .font_size(px(16))
                        .padding_left(px(14))
                        .padding_right(px(14)),
                )
            }
            view(style: css!(
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                border_bottom_width: px(1),
                border_bottom_color: theme::BORDER,
            )) {
                search_tab(label: "ユーザー", active: computed(move || mode.get() == SearchMode::People), on_tap: std::rc::Rc::new(move || { mode.set(SearchMode::People); let _ = people_pager.scroll_to(ListScrollTarget::key("people", ScrollAlignment::Start), ScrollBehavior::Smooth); }) as std::rc::Rc<dyn Fn()>)
                search_tab(label: "投稿", active: computed(move || mode.get() == SearchMode::Posts), on_tap: std::rc::Rc::new(move || { mode.set(SearchMode::Posts); let _ = posts_pager.scroll_to(ListScrollTarget::key("posts", ScrollAlignment::Start), ScrollBehavior::Smooth); }) as std::rc::Rc<dyn Fn()>)
            }
            // Swipeable pager: a horizontal `<list>` of two full-viewport-width
            // pages (People / Posts) with item snapping for ViewPager-style
            // paging. Always mounted — swipeable even before a query, with each
            // page showing its own empty state. Swiping snaps to a page →
            // `on_snap` syncs the tab highlight; tapping a tab calls
            // keyed `scroll_to` to page over.
            list(
                ref: pager.r(),
                style: css!(flex_grow: 1.0, width: percent(100)),
                axis: ScrollAxis::Horizontal,
                on_scroll: move |e| {
                    let m = if e.detail.scroll_left < e.detail.viewport_width * 0.5 {
                        SearchMode::People
                    } else {
                        SearchMode::Posts
                    };
                    if mode.get() != m {
                        mode.set(m);
                    }
                },
                each: move || vec![SearchMode::People, SearchMode::Posts],
                key: |m: &SearchMode| match m {
                    SearchMode::People => "people",
                    SearchMode::Posts => "posts",
                },
                children: move |m: ReadSignal<SearchMode>| match m.get() {
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
pub(super) fn search_tab(
    label: &'static str,
    active: Signal<bool>,
    on_tap: std::rc::Rc<dyn Fn()>,
) -> Element {
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
pub(super) fn actor_list(actors: Vec<bsky_domain::ActorView>) -> Element {
    render! {
        list(
            style: css!(flex_grow: 1.0, flex_shrink: 1.0, width: percent(100)),
            each: {
                let actors = actors.clone();
                move || actors.clone()
            },
            key: |a: &bsky_domain::ActorView| a.did.clone(),
            children: |a: ReadSignal<bsky_domain::ActorView>| render! { actor_row(actor: a.get()) },
        )
    }
}

/// One people-search row: avatar + name / handle + bio snippet. Tapping
/// opens the account's profile.
#[component]
pub(super) fn actor_row(actor: bsky_domain::ActorView) -> Element {
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
pub(super) fn row_avatar(src: String) -> Element {
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
