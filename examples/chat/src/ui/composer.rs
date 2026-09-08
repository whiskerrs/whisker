use super::{
    button::Button,
    theme::{self, color, radius, size, space},
};
use crate::{
    hooks::ChatActions,
    state::{AppState, Session},
};
use whisker::css::{AlignSelf, TextAlign};
use whisker::prelude::*;
use whisker_input::{AutoCapitalize, Input};

#[component]
pub fn composer(session: Session, actions: ChatActions) -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let busy = app.busy();
    render! {
        View(
            style: theme::column()
                .width(percent(100))
                .max_width(px(size::READING))
                .align_self(AlignSelf::Center)
                .padding(px(space::LG))
                .gap(px(space::SM))
                .flex_shrink(0.0),
        ) {
            View(
                style: theme::card()
                    .padding(px(space::MD))
                    .gap(px(space::SM))
                    .border_radius(px(radius::CARD)),
            ) {
                Text(
                    value: "MESSAGE",
                    style: theme::text(size::CAPTION).color(Color::hex(color::MUTED)),
                )
                Input(
                    text: session.draft,
                    multiline: true,
                    lines: 3u32,
                    auto_capitalize: AutoCapitalize::Sentences,
                    placeholder: "Ask something, or explore an idea…",
                    on_blur: actions.save,
                    style: theme::text(size::BODY)
                        .height(px(68))
                        .background_color(Color::hex(color::PAPER)),
                )
                View(
                    style: theme::row()
                        .justify_content(JustifyContent::SpaceBetween)
                        .gap(px(space::SM)),
                ) {
                    Show(when: move || !session.turns.with(Vec::is_empty) && !busy.get()) {
                        Button(label: "Regenerate", on_press: actions.retry)
                    }
                    View(style: theme::fill())
                    Button(
                        label: computed(move || {
                            if busy.get() {
                                "Stop generation".into()
                            } else {
                                "Send ↑".into()
                            }
                        }),
                        primary: true,
                        disabled: computed(move || !busy.get() && session.draft.with(|s| s.trim().is_empty())),
                        on_press: actions.send,
                    )
                }
            }
            Text(
                value: "A little curiosity goes a long way. Verify important answers.",
                style: theme::text(size::CAPTION)
                    .color(Color::hex(color::MUTED))
                    .text_align(TextAlign::Center),
            )
        }
    }
}
