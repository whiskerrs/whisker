use super::{button::Button, navigation, theme};
use crate::state::{AppState, SendError, Turn};
use whisker::prelude::*;
use whisker_input::Input;
use whisker_router::use_navigator;

#[component]
pub fn chat_screen() -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let nav = use_navigator();
    let settings_nav = nav.clone();
    let retry_nav = nav.clone();
    let turns = app.turns();
    let draft = app.draft();
    let busy = app.busy();
    let connection = app.connection();
    let revision = app.revision();
    let following = signal(turns.with_untracked(Vec::is_empty));
    let list = ListHandle::<u64>::new();
    let scroll = list.clone();
    effect(move || {
        revision.get();
        turns.with(Vec::len);
        if following.get_untracked() {
            let _ = scroll.scroll_to(ListScrollTarget::End, ScrollBehavior::Instant);
        }
    });
    let settings = app.clone();
    let send = app.clone();
    let retry = app.clone();
    let save = app.clone();
    let latest = list.clone();
    render! {
        View(style: theme::screen()) {
            View(style: theme::row().padding(px(16)).gap(px(12)).flex_shrink(0.0)) {
                View(style: theme::column().flex_grow(1.0).flex_shrink(1.0)) {
                    Text(value: "Whisker Chat", style: theme::text(21.0))
                    Text(
                        value: computed(move || {
                            connection
                                .get()
                                .map(|c| format!("{} · {}", c.name, c.model))
                                .unwrap_or_else(|| "Set up a connection".into())
                        }),
                        max_lines: 1u32,
                        style: theme::muted().margin_top(px(3)),
                    )
                }
                Button(
                    label: "Settings",
                    on_press: move |()| {
                        settings.persist();
                        navigation::open_settings(&settings_nav, settings.notice());
                    },
                )
            }
            Show(when: move || turns.with(Vec::is_empty)) {
                View(style: theme::fill().padding(px(28)).padding_top(px(72))) {
                    Text(
                        value: "What would you like to explore?",
                        style: theme::text(25.0),
                    )
                    Text(
                        value: "A draft to refine, a coding question,\nor an idea taking shape.",
                        style: theme::muted().margin_top(px(16)),
                    )
                }
            }
            Show(when: move || !turns.with(Vec::is_empty)) {
                List(
                    each: move || turns.get(),
                    key: |turn: &RwSignal<Turn>| turn.with_untracked(|turn| turn.id),
                    children: |turn: ReadSignal<RwSignal<Turn>>| render! {
                        TurnRow(turn: turn.get_untracked())
                    },
                    list_ref: list.clone().r(),
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
                Button(
                    label: "↓ Latest message",
                    on_press: {
                        let latest = latest.clone();
                        move |()| {
                            following.set(true);
                            let _ = latest.scroll_to(ListScrollTarget::End, ScrollBehavior::Smooth);
                        }
                    },
                )
            }
            View(style: theme::column().padding(px(16)).gap(px(10)).flex_shrink(0.0)) {
                Text(value: "Message", style: theme::muted())
                Input(
                    text: draft,
                    multiline: true,
                    lines: 3u32,
                    placeholder: "Type a message…",
                    style: theme::field().height(px(88)),
                    on_blur: move |()| save.persist(),
                )
                View(
                    style: theme::row()
                        .justify_content(JustifyContent::SpaceBetween)
                        .gap(px(10)),
                ) {
                    Text(
                        value: "AI can make mistakes.",
                        style: theme::muted(),
                    )
                    Button(
                        label: computed(move || {
                            if busy.get() {
                                "Stop".into()
                            } else {
                                "Send ↑".into()
                            }
                        }),
                        primary: true,
                        on_press: move |()| {
                            if busy.get_untracked() {
                                send.stop();
                            } else {
                                following.set(true);
                                if let Err(SendError::MissingConnection) = send.send() {
                                    navigation::open_settings(&nav, send.notice());
                                }
                            }
                        },
                    )
                }
                Show(
                    when: move || {
                        !busy.get()
                            && turns.with(|turns| {
                                turns.last().is_some_and(|turn| {
                                    turn.with(|turn| !matches!(turn.status, crate::state::AnswerStatus::Complete))
                                })
                            })
                    },
                ) {
                    Button(
                        label: "Retry last question",
                        on_press: {
                            let retry = retry.clone();
                            let nav = retry_nav.clone();
                            move |()| {
                                if let Err(SendError::MissingConnection) = retry.retry() {
                                    navigation::open_settings(&nav, retry.notice());
                                }
                            }
                        },
                    )
                }
            }
        }
    }
}

#[component]
fn turn_row(turn: RwSignal<Turn>) -> Element {
    render! {
        View(style: theme::column().padding(px(20)).gap(px(20))) {
            View(
                style: theme::column()
                    .align_self(whisker::css::AlignSelf::FlexEnd)
                    .max_width(percent(88))
                    .background_color(Color::hex(0xe4ebe4))
                    .border_radius(px(18))
                    .padding(px(16)),
            ) {
                Text(
                    value: computed(move || turn.with(|t| t.question.clone())),
                    style: theme::text(16.0),
                )
            }
            View(style: theme::column().gap(px(8))) {
                Text(
                    value: computed(move || turn.with(|t| format!("{} · {}", t.connection.name, t.connection.model))),
                    style: theme::muted(),
                )
                Text(
                    value: computed(move || turn.with(|t| t.answer.clone())),
                    style: theme::text(16.0),
                )
                Text(value: computed(move || turn.with(Turn::status_text)), style: theme::muted())
            }
        }
    }
}
