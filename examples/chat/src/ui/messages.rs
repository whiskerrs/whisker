use super::{
    button::Button,
    markdown::Markdown,
    theme::{self, radius, size, space},
};
use crate::state::{AnswerStatus, AppState, Session, Turn};
use whisker::css::{AlignSelf, FontWeight};
use whisker::prelude::*;

#[component]
pub fn turn_row(turn: RwSignal<Turn>, session: Session, retry: Callback) -> Element {
    let busy = use_context::<AppState>().expect("AppState context").busy();
    let can_retry = computed(move || {
        !busy.get()
            && session
                .turns
                .with(|turns| turns.last().copied() == Some(turn))
    });
    let alternative = signal(None::<usize>);
    let answer = computed(move || {
        turn.with(|turn| {
            alternative
                .get()
                .and_then(|index| turn.alternatives.get(index))
                .map(|answer| answer.text.clone())
                .unwrap_or_else(|| turn.answer.clone())
        })
    });
    let status = computed(move || {
        turn.with(|t| {
            alternative
                .get()
                .and_then(|i| t.alternatives.get(i))
                .map(|a| a.status.clone())
                .unwrap_or_else(|| t.status.clone())
        })
    });
    let model = computed(move || {
        turn.with(|t| {
            alternative
                .get()
                .and_then(|i| t.alternatives.get(i))
                .map(|a| a.connection.model.clone())
                .unwrap_or_else(|| t.connection.model.clone())
        })
    });
    let running = computed(move || status.get() == AnswerStatus::Running);
    render! {
        View(
            style: theme::column()
                .width(percent(100))
                .max_width(px(size::READING))
                .align_self(AlignSelf::Center)
                .padding(px(space::XL))
                .gap(px(space::XL)),
        ) {
            View(
                style: theme::style(move |palette| {
                    theme::column()
                        .align_self(AlignSelf::FlexEnd)
                        .max_width(percent(88))
                        .background_color(Color::hex(palette.accent_soft))
                        .border_radius(px(radius::CARD))
                        .padding(px(space::LG))
                }),
            ) {
                Text(
                    value: computed(move || turn.with(|t| t.question.clone())),
                    style: theme::style(move |palette| palette.text(size::BODY)),
                )
            }
            View(style: theme::column().gap(px(space::LG))) {
                View(style: theme::row().gap(px(space::SM))) {
                    Text(
                        value: "w.",
                        style: theme::style(move |palette| {
                            palette
                                .text(19.0)
                                .font_weight(FontWeight::Bold)
                                .color(Color::hex(palette.accent))
                        }),
                    )
                    Text(
                        value: model,
                        style: theme::style(move |palette| palette.muted()),
                    )
                }
                Show(when: move || running.get()) {
                    Text(
                        value: answer,
                        style: theme::style(move |palette| palette.text(size::BODY)),
                    )
                }
                Show(when: move || !running.get()) {
                    Markdown(text: answer)
                }
                Show(when: move || status.get() != AnswerStatus::Complete) {
                    Text(
                        value: computed(move || status.get().label(answer.get().is_empty())),
                        style: theme::style(move |palette| palette.muted()),
                    )
                }
                Show(when: move || can_retry.get()) {
                    View(style: theme::row()) {
                        Button(
                            label: "Regenerate",
                            icon: whisker_icons::lucide::RotateCcw,
                            compact: true,
                            plain: true,
                            on_press: move |()| {
                                alternative.set(None);
                                retry.call();
                            },
                        )
                    }
                }
                Show(when: move || turn.with(|t| !t.alternatives.is_empty())) {
                    View(style: theme::row().gap(px(space::SM))) {
                        Button(
                            label: "Previous answer",
                            on_press: move |()| {
                                let count = turn.with_untracked(|t| t.alternatives.len());
                                alternative.update(|index| {
                                    *index = match *index {
                                        None if count > 0 => Some(count - 1),
                                        Some(i) if i > 0 => Some(i - 1),
                                        _ => None,
                                    }
                                });
                            },
                        )
                        Text(
                            value: computed(move || {
                                turn.with(|t| {
                                    format!(
                                        "{} / {}",
                                        alternative
                                            .get()
                                            .map(|i| i + 1)
                                            .unwrap_or(t.alternatives.len() + 1),
                                        t.alternatives.len() + 1
                                    )
                                })
                            }),
                            style: theme::style(move |palette| palette.muted()),
                        )
                        Button(label: "Latest", on_press: move |()| alternative.set(None))
                    }
                }
            }
        }
    }
}
