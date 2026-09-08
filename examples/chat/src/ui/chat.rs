use super::{
    button::Button,
    composer::Composer,
    history::HistoryPanel,
    messages::TurnRow,
    theme::{self, color, space},
    welcome::Welcome,
};
use crate::{
    hooks::use_chat,
    state::{AppState, Session, Turn},
};
use whisker::css::FontWeight;
use whisker::prelude::*;
use whisker_icons::lucide;
use whisker_router::use_navigator;

#[component]
pub fn chat_screen() -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let selected = app.clone();
    let sidebar = signal(cfg!(not(any(
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32"
    ))));
    render! {
        View(style: theme::screen().flex_direction(FlexDirection::Row)) {
            Show(when: move || sidebar.get()) {
                HistoryPanel(sidebar: true, closed: move |()| sidebar.set(false))
            }
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
fn conversation_view(session: Session, sidebar: RwSignal<bool>) -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let actions = use_chat(session);
    let nav = use_navigator();
    let notice = app.notice();
    let connection = app.connection();
    let following = actions.following;
    let list_ref = actions.list.r();
    render! {
        View(style: theme::fill()) {
            View(
                style: theme::row()
                    .padding(px(space::LG))
                    .gap(px(space::SM))
                    .flex_shrink(0.0)
                    .border_bottom_width(px(1))
                    .border_bottom_color(Color::hex(color::BORDER)),
            ) {
                Button(
                    label: "Chats",
                    icon: lucide::PanelLeft,
                    on_press: move |()| {
                        if cfg!(any(target_os = "ios", target_os = "android")) {
                            if nav.navigate("/history").is_err() {
                                notice.set("Could not open conversations.".into());
                            }
                        } else {
                            sidebar.update(|open| *open = !*open);
                        }
                    },
                )
                View(style: theme::fill()) {
                    Text(
                        value: session.title,
                        max_lines: 1u32,
                        style: theme::text(15.0).font_weight(FontWeight::Bold),
                    )
                    Text(
                        value: computed(move || {
                            connection
                                .get()
                                .map(|c| c.model)
                                .unwrap_or_else(|| "Connect a model".into())
                        }),
                        max_lines: 1u32,
                        style: theme::muted(),
                    )
                }
                Button(
                    label: "",
                    icon: lucide::Settings2,
                    accessible_label: "Settings",
                    on_press: actions.settings,
                )
            }
            Show(when: move || session.turns.with(Vec::is_empty)) {
                Welcome(draft: session.draft)
            }
            Show(when: move || !session.turns.with(Vec::is_empty)) {
                List(
                    each: move || session.turns.get(),
                    key: |turn: &RwSignal<Turn>| turn.with_untracked(|t| t.id),
                    children: |turn: ReadSignal<RwSignal<Turn>>| render! {
                        TurnRow(turn: turn.get_untracked())
                    },
                    list_ref: list_ref.clone(),
                    on_scroll: move |event| {
                        let d = event.detail;
                        if d.is_dragging || d.delta_y < 0.0 {
                            following.set(d.scroll_height - d.viewport_height - d.scroll_top < 48.0);
                        }
                    },
                    style: theme::fill(),
                )
            }
            Show(when: move || !following.get()) {
                View(
                    style: theme::row()
                        .justify_content(JustifyContent::Center)
                        .padding(px(space::XS)),
                ) {
                    Button(label: "Latest", icon: lucide::ArrowDown, on_press: actions.latest)
                }
            }
            Composer(session: session, actions: actions)
        }
    }
}
