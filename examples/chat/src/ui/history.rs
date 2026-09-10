use super::{
    button::Button, history_dialog::HistoryDialog, history_row::HistoryRow, navigation, theme,
};
use crate::state::{AppState, Session};
use whisker::css::{FontWeight, PositionKind};
use whisker::prelude::*;
use whisker_icons::lucide;
use whisker_input::Input;
use whisker_router::use_navigator;

#[component]
pub fn history_screen() -> Element {
    render! {
        View(style: theme::style(move |palette| palette.screen())) {
            HistoryPanel()
        }
    }
}

#[component]
pub fn history_panel(
    #[prop(default = false)] sidebar: bool,
    #[prop(default = Callback::new(|()| {}))] closed: Callback,
) -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let nav = use_navigator();
    let notice = app.notice();
    let back = Callback::new(move |()| {
        if !sidebar {
            navigation::return_to_chat(&nav, notice);
        }
    });
    let dismiss = Callback::new(move |()| {
        if sidebar {
            closed.call();
        } else {
            back.call();
        }
    });
    let query = signal(String::new());
    let editing = signal(None::<Session>);
    let last_trashed = signal(None::<Session>);
    let sessions = app.sessions();
    let new_app = app.clone();
    let trash_app = app.clone();
    let trash = Callback::new(move |session| {
        editing.set(None);
        trash_app.trash(session);
        last_trashed.set(Some(session));
    });
    let undo = Callback::new(move |()| {
        if let Some(session) = last_trashed.get_untracked() {
            app.restore_conversation(session);
        }
        last_trashed.set(None);
    });
    let filtered = computed(move || {
        let q = query.get().to_lowercase();
        sessions.with(|sessions| {
            sessions
                .iter()
                .filter(|session| {
                    !session.trashed.get()
                        && (session.title.get().to_lowercase().contains(&q)
                            || session.turns.with(|turns| {
                                turns.iter().any(|turn| {
                                    turn.with(|turn| turn.question.to_lowercase().contains(&q))
                                })
                            }))
                })
                .copied()
                .collect::<Vec<_>>()
        })
    });
    render! {
        View(
            style: theme::style(move |palette| {
                theme::fill()
                    .position(PositionKind::Relative)
                    .height(percent(100))
                    .background_color(Color::hex(if sidebar {
                        palette.paper
                    } else {
                        palette.canvas
                    }))
                    .padding(px(12))
                    .gap(px(12))
            }),
        ) {
            View(style: theme::row().justify_content(JustifyContent::SpaceBetween)) {
                Text(
                    value: "Whisker Chat",
                    style: theme::style(move |palette| palette.text(14.0).font_weight(FontWeight::Numeric(600))),
                )
                Button(
                    label: "",
                    icon: lucide::PanelLeftClose,
                    compact: true,
                    plain: true,
                    accessible_label: if sidebar { "Hide sidebar" } else { "Back to chat" },
                    on_press: dismiss,
                )
            }
            Button(
                label: "New conversation",
                icon: lucide::Plus,
                compact: true,
                on_press: move |()| {
                    new_app.new_conversation();
                    back.call();
                },
            )
            Input(
                text: query,
                placeholder: "Search conversations",
                style: theme::style(move |palette| {
                    palette
                        .field()
                        .height(px(36))
                        .font_size(px(13))
                        .padding(px(8))
                }),
            )
            Show(when: move || filtered.with(Vec::is_empty)) {
                Text(
                    value: "No conversations found.",
                    style: theme::style(move |palette| palette.muted()),
                )
            }
            List(
                each: move || filtered.get(),
                key: |session: &Session| session.id,
                children: move |session: ReadSignal<Session>| render! {
                    HistoryRow(
                        session: session.get_untracked(),
                        opened: back,
                        edit: move |session| editing.set(Some(session)),
                    )
                },
                content_style: theme::column().row_gap(px(4)),
                style: theme::fill(),
            )
            Show(when: move || last_trashed.get().is_some()) {
                View(style: theme::row().gap(px(8))) {
                    Text(
                        value: "Moved to Trash",
                        style: theme::style(move |palette| {
                            palette
                                .text(12.0)
                                .color(Color::hex(palette.muted))
                                .flex_grow(1.0)
                        }),
                    )
                    Button(
                        label: "Undo",
                        compact: true,
                        plain: true,
                        on_press: undo,
                    )
                }
            }
            ForEach(
                each: move || editing.get().into_iter().collect::<Vec<_>>(),
                key: |session: &Session| session.id,
                children: move |session: Session| render! {
                    HistoryDialog(
                        session: session,
                        closed: move |()| editing.set(None),
                        trashed: trash,
                    )
                },
            )
        }
    }
}
