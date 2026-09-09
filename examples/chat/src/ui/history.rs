use super::{
    button::Button,
    navigation,
    theme::{self, size, space},
};
use crate::state::{AppState, Session};
use whisker::css::{Cursor, FontWeight, PointerEvents};
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
    let query = signal(String::new());
    let trash = signal(false);
    let sessions = app.sessions();
    let new_app = app.clone();
    let filtered = computed(move || {
        let q = query.get().to_lowercase();
        sessions.with(|sessions| {
            sessions
                .iter()
                .filter(|session| {
                    session.trashed.get() == trash.get()
                        && (session.title.get().to_lowercase().contains(&q)
                            || session.turns.with(|turns| {
                                turns
                                    .iter()
                                    .any(|t| t.with(|t| t.question.to_lowercase().contains(&q)))
                            }))
                })
                .copied()
                .collect::<Vec<_>>()
        })
    });
    render! {
        View(
            style: theme::style(move |palette| {
                if sidebar {
                    theme::column()
                        .width(px(size::SIDEBAR))
                        .height(percent(100))
                        .flex_shrink(0.0)
                        .background_color(Color::hex(palette.tint))
                        .padding(px(space::LG))
                        .gap(px(space::LG))
                } else {
                    theme::fill().padding(px(space::XL)).gap(px(space::LG))
                }
            }),
        ) {
            View(style: theme::row().justify_content(JustifyContent::SpaceBetween)) {
                Text(value: "WHISKER CHAT", style: theme::style(move |palette| palette.muted()))
                Show(when: move || sidebar) {
                    Button(
                        label: "",
                        icon: lucide::PanelLeftClose,
                        accessible_label: "Hide sidebar",
                        on_press: closed,
                    )
                }
            }
            Text(value: "Your conversations", style: theme::style(move |palette| palette.title()))
            Button(
                label: "New conversation",
                icon: lucide::Plus,
                primary: true,
                on_press: move |()| {
                    new_app.new_conversation();
                    back.call();
                },
            )
            Input(
                text: query,
                placeholder: "Search conversations",
                style: theme::style(move |palette| palette.field()),
            )
            Button(
                label: computed(move || {
                    if trash.get() {
                        "← All conversations".into()
                    } else {
                        "Trash".into()
                    }
                }),
                on_press: move |()| trash.update(|value| *value = !*value),
            )
            Show(when: move || filtered.with(Vec::is_empty)) {
                Text(
                    value: computed(move || {
                        if trash.get() {
                            "No conversations in Trash.".into()
                        } else {
                            "No conversations found.".into()
                        }
                    }),
                    style: theme::style(move |palette| palette.muted()),
                )
            }
            List(
                each: move || filtered.get(),
                key: |session: &Session| session.id,
                children: move |session: ReadSignal<Session>| render! {
                    HistoryRow(session: session.get_untracked(), opened: back)
                },
                style: theme::fill(),
            )
            Show(when: move || !sidebar) {
                Button(label: "Back to chat", on_press: back)
            }
            Text(value: "Only on this device.", style: theme::style(move |palette| palette.muted()))
        }
    }
}

#[component]
fn history_row(session: Session, opened: Callback) -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let active = app.active_id();
    let edit = signal(false);
    let title = signal(session.title.get_untracked());
    let open_app = app.clone();
    let rename_app = app.clone();
    let trash_app = app.clone();
    render! {
        View(
            style: theme::style(move |palette| {
                theme::column()
                    .padding(px(space::MD))
                    .margin_bottom(px(space::SM))
                    .gap(px(space::SM))
                    .border_radius(px(12))
                    .background_color(Color::hex(if active.get() == session.id {
                        palette.paper
                    } else {
                        palette.canvas
                    }))
            }),
        ) {
            View(
                on_tap: move |_| {
                    if !session.trashed.get_untracked() {
                        open_app.select(session.id);
                        opened.call();
                    }
                },
                style: theme::column()
                    .cursor(Cursor::Pointer)
                    .min_height(px(size::TOUCH))
                    .gap(px(space::XS)),
            ) {
                Text(
                    value: session.title,
                    max_lines: 2u32,
                    style: theme::style(move |palette| {
                        palette
                            .text(size::BODY)
                            .pointer_events(PointerEvents::None)
                            .font_weight(FontWeight::Numeric(600))
                    }),
                )
                Text(
                    value: computed(move || format!("{} messages", session.turns.with(Vec::len) * 2)),
                    style: theme::style(move |palette| palette.muted().pointer_events(PointerEvents::None)),
                )
            }
            Show(when: move || edit.get()) {
                Input(text: title, style: theme::style(move |palette| palette.field()))
            }
            View(style: theme::row().gap(px(space::SM))) {
                Button(
                    label: computed(move || {
                        if edit.get() {
                            "Save".into()
                        } else {
                            "Rename".into()
                        }
                    }),
                    on_press: move |()| {
                        if edit.get_untracked() {
                            rename_app.rename(session, &title.get_untracked());
                        }
                        edit.update(|value| *value = !*value);
                    },
                )
                Button(
                    label: computed(move || {
                        if session.trashed.get() {
                            "Restore".into()
                        } else {
                            "Trash".into()
                        }
                    }),
                    on_press: move |()| {
                        if session.trashed.get_untracked() {
                            trash_app.restore_conversation(session);
                        } else {
                            trash_app.trash(session);
                        }
                    },
                )
            }
        }
    }
}
