use super::{
    composer_action::ComposerAction,
    composer_surface::{ComposerSurface, INPUT_LEFT, INPUT_RIGHT, INPUT_VERTICAL},
    theme::{self, size, space},
};
use crate::{
    hooks::{ChatActions, use_composer_motion},
    state::{AppState, Session},
};
use whisker::css::{AlignSelf, PositionKind};
use whisker::prelude::*;
use whisker_input::{AutoCapitalize, Input};

#[component]
pub fn composer(session: Session, actions: ChatActions, input_height: RwSignal<f32>) -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let busy = app.busy();
    let motion = use_composer_motion();
    let input_size = signal(whisker_input::InputSize {
        width: 0.0,
        height: size::TOUCH,
    });
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
            View(style: theme::column().position(PositionKind::Relative)) {
                ComposerSurface(progress: motion.progress, input_size: input_size.read_only())
                View(
                    style: theme::row()
                        .padding_top(px(INPUT_VERTICAL))
                        .padding_bottom(px(INPUT_VERTICAL))
                        .padding_left(px(INPUT_LEFT))
                        .padding_right(px(INPUT_RIGHT)),
                ) {
                    Input(
                        text: session.draft,
                        multiline: true,
                        auto_size: true,
                        auto_capitalize: AutoCapitalize::Sentences,
                        placeholder: "Message Whisker Chat…",
                        on_size_change: move |rect: whisker_input::InputSize| {
                            if input_size.get_untracked() != rect {
                                input_size.set(rect);
                            }
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
                                .background_color(Color::Transparent)
                        }),
                        on_focus: move |()| motion.focused.set(true),
                        on_blur: move |()| {
                            motion.focused.set(false);
                            actions.save.call();
                        },
                        on_submit: move |_: String| {
                            if !busy.get_untracked() {
                                actions.send.call();
                            }
                        },
                    )
                }
                View(
                    style: theme::column()
                        .position(PositionKind::Absolute)
                        .right(px(0))
                        .bottom(px(0))
                        .width(px(64))
                        .height(px(64))
                        .align_items(AlignItems::Center)
                        .justify_content(JustifyContent::Center),
                ) {
                    ComposerAction(
                        progress: motion.progress,
                        busy: busy.read_only(),
                        draft: session.draft,
                        on_press: actions.send,
                    )
                }
            }
        }
    }
}
