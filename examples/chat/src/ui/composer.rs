use super::{
    composer_action::ComposerAction,
    theme::{self, radius, size, space},
};
use crate::{
    hooks::ChatActions,
    state::{AppState, Session},
};
use whisker::css::AlignSelf;
use whisker::prelude::*;
use whisker_input::{AutoCapitalize, Input};

#[component]
pub fn composer(session: Session, actions: ChatActions, input_height: RwSignal<f32>) -> Element {
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
                .flex_shrink(0.0),
        ) {
            View(
                style: theme::style(move |palette| {
                    palette
                        .card()
                        .flex_direction(FlexDirection::Row)
                        .align_items(AlignItems::FlexEnd)
                        .padding(px(space::SM))
                        .padding_left(px(space::LG))
                        .gap(px(space::SM))
                        .border_radius(px(radius::CARD))
                }),
            ) {
                Input(
                    text: session.draft,
                    multiline: true,
                    auto_size: true,
                    auto_capitalize: AutoCapitalize::Sentences,
                    placeholder: "Message Whisker Chat…",
                    on_size_change: move |rect: whisker_input::InputSize| {
                        if (input_height.get_untracked() - rect.height).abs() > 0.5 {
                            input_height.set(rect.height);
                        }
                    },
                    style: theme::style(move |palette| {
                        palette
                            .text(size::BODY)
                            .line_height(px(24))
                            .min_height(px(size::TOUCH))
                            .max_height(px(size::COMPOSER_INPUT_MAX))
                            .padding_top(px(10))
                            .padding_bottom(px(10))
                            .flex_grow(1.0)
                            .flex_basis(px(0))
                            .min_width(px(0))
                            .background_color(Color::hex(palette.paper))
                    }),
                    on_blur: actions.save,
                    on_submit: move |_: String| {
                        if !busy.get_untracked() {
                            actions.send.call();
                        }
                    },
                )
                ComposerAction(busy: busy.read_only(), draft: session.draft, on_press: actions.send)
            }
        }
    }
}
