use super::{
    button::Button,
    chat_layout,
    theme::{self, radius, size, space},
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
                .padding_left(px(space::LG))
                .padding_right(px(space::LG))
                .padding_bottom(px(space::LG))
                .height(px(chat_layout::COMPOSER_HEIGHT))
                .gap(px(space::SM))
                .flex_shrink(0.0),
        ) {
            View(
                style: theme::style(move |palette| {
                    palette
                        .card()
                        .padding(px(space::MD))
                        .gap(px(space::SM))
                        .border_radius(px(radius::CARD))
                }),
            ) {
                Input(
                    text: session.draft,
                    multiline: true,
                    lines: 3u32,
                    auto_capitalize: AutoCapitalize::Sentences,
                    placeholder: "Message Whisker Chat…",
                    on_blur: actions.save,
                    style: theme::style(move |palette| {
                        palette
                            .text(size::BODY)
                            .height(px(64))
                            .flex_shrink(0.0)
                            .background_color(Color::hex(palette.paper))
                    }),
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
                value: "AI can make mistakes. Verify important details.",
                style: theme::style(move |palette| {
                    palette
                        .text(size::CAPTION)
                        .color(Color::hex(palette.muted))
                        .background_color(Color::hex(palette.canvas))
                        .border_radius(px(4))
                        .padding(px(space::XS))
                        .align_self(AlignSelf::Center)
                        .text_align(TextAlign::Center)
                }),
            )
        }
    }
}
