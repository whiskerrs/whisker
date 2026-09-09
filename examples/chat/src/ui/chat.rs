use super::{
    button::Button, chat_layout, composer::Composer, history_sidebar::HistorySidebar,
    messages::TurnRow, theme, welcome::Welcome,
};
use crate::{
    hooks::{SidebarState, use_chat, use_sidebar},
    state::{AppState, Session, Turn},
};
use whisker::css::{FontWeight, PositionKind};
use whisker::prelude::*;
use whisker_icons::lucide;
use whisker_router::use_navigator;

#[component]
pub fn chat_screen() -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let selected = app.clone();
    let sidebar = use_sidebar(cfg!(not(any(
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32"
    ))));
    render! {
        View(
            style: theme::style(move |palette| palette.screen().flex_direction(FlexDirection::Row)),
        ) {
            HistorySidebar(state: sidebar)
            ForEach(
                each: move || selected.active().into_iter().collect::<Vec<_>>(),
                key: |session: &Session| session.id,
                children: move |session: Session| render! {
                    ConversationView(session: session, sidebar: sidebar)
                },
            )
        }
    }
}

#[component]
fn conversation_view(session: Session, sidebar: SidebarState) -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let actions = use_chat(session);
    let nav = use_navigator();
    let notice = app.notice();
    let connection = app.connection();
    let scroll = actions.scroll;
    let list_ref = actions.list.r();
    let retry = actions.retry;
    let open_history = Callback::new(move |()| {
        if cfg!(any(target_os = "ios", target_os = "android")) {
            if nav.navigate("/history").is_err() {
                notice.set("Could not open conversations.".into());
            }
        } else {
            sidebar.open.update(|open| *open = !*open);
        }
    });
    render! {
        View(style: theme::fill().position(PositionKind::Relative)) {
            Show(when: move || session.turns.with(Vec::is_empty)) {
                Welcome(draft: session.draft)
            }
            Show(when: move || !session.turns.with(Vec::is_empty)) {
                List(
                    each: move || session.turns.get(),
                    key: |turn: &RwSignal<Turn>| turn.with_untracked(|t| t.id),
                    children: move |turn: ReadSignal<RwSignal<Turn>>| render! {
                        TurnRow(turn: turn.get_untracked(), session: session, retry: retry)
                    },
                    list_ref: list_ref.clone(),
                    header: || chat_layout::spacer(chat_layout::CONTENT_TOP),
                    footer: || chat_layout::spacer(chat_layout::CONTENT_BOTTOM),
                    on_scroll: move |event| scroll.changed.run(event),
                    style: theme::fill(),
                )
            }
            View(
                style: theme::style(move |palette| {
                    chat_layout::header()
                        .background_color(Color::hex(palette.paper))
                        .border_radius(px(theme::radius::CARD))
                        .border(
                            whisker::css::Border::new()
                                .width(px(1))
                                .color(Color::hex(palette.border))
                                .style(whisker::css::BorderStyle::Solid),
                        )
                }),
            ) {
                Show(when: move || !sidebar.visible.get()) {
                    Button(
                        label: "Chats",
                        icon: lucide::PanelLeft,
                        on_press: open_history,
                    )
                }
                View(style: theme::fill()) {
                    Text(
                        value: session.title,
                        max_lines: 1u32,
                        style: theme::style(move |palette| palette.text(15.0).font_weight(FontWeight::Bold)),
                    )
                    Text(
                        value: computed(move || {
                            connection
                                .get()
                                .map(|c| c.model)
                                .unwrap_or_else(|| "Connect a model".into())
                        }),
                        max_lines: 1u32,
                        style: theme::style(move |palette| palette.muted()),
                    )
                }
                Button(
                    label: "",
                    icon: lucide::Settings2,
                    accessible_label: "Settings",
                    on_press: actions.settings,
                )
            }
            Show(when: move || !scroll.at_end.get() && !session.turns.with(Vec::is_empty)) {
                View(
                    style: chat_layout::latest(),
                ) {
                    Button(label: "Latest", icon: lucide::ArrowDown, on_press: actions.latest)
                }
            }
            View(style: chat_layout::composer()) {
                Composer(session: session, actions: actions)
            }
        }
    }
}
