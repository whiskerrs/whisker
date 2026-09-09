use super::{button::Button, theme};
use crate::state::{AppState, Session};
use whisker::css::{FontWeight, PositionKind};
use whisker::prelude::*;
use whisker_icons::lucide;
use whisker_input::Input;

#[component]
pub fn history_dialog(session: Session, closed: Callback, trashed: Callback<Session>) -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let editing = signal(false);
    let title = signal(session.title.get_untracked());
    let save = Callback::new(move |()| {
        if !title.with_untracked(|title| title.trim().is_empty()) {
            app.rename(session, &title.get_untracked());
            closed.call();
        }
    });
    render! {
        View(
            style: theme::column()
                .position(PositionKind::Absolute)
                .top(px(0))
                .bottom(px(0))
                .left(px(0))
                .right(px(0))
                .z_index(10)
                .padding(px(12))
                .justify_content(JustifyContent::Center)
                .align_items(AlignItems::Center),
        ) {
            View(
                on_tap: move |_| closed.call(),
                style: Css::new()
                    .position(PositionKind::Absolute)
                    .top(px(0))
                    .bottom(px(0))
                    .left(px(0))
                    .right(px(0))
                    .background_color(Color::hex(0))
                    .opacity(0.45),
            )
            View(
                style: theme::style(move |palette| {
                    palette
                        .card()
                        .width(percent(100))
                        .max_width(px(360))
                        .padding(px(16))
                        .gap(px(12))
                        .z_index(1)
                }),
            ) {
                View(style: theme::row().gap(px(8))) {
                    Text(
                        value: "Conversation",
                        style: theme::style(move |palette| {
                            palette
                                .text(14.0)
                                .font_weight(FontWeight::Numeric(600))
                                .flex_grow(1.0)
                        }),
                    )
                    Button(
                        label: "",
                        icon: lucide::X,
                        compact: true,
                        plain: true,
                        accessible_label: "Close conversation actions",
                        on_press: closed,
                    )
                }
                Text(
                    value: session.title,
                    max_lines: 2u32,
                    style: theme::style(move |palette| palette.muted()),
                )
                Show(when: move || !editing.get()) {
                    Button(
                        label: "Rename",
                        icon: lucide::Pencil,
                        compact: true,
                        on_press: move |()| editing.set(true),
                    )
                    Button(
                        label: "Trash",
                        icon: lucide::Trash2,
                        compact: true,
                        plain: true,
                        on_press: move |()| trashed.run(session),
                    )
                }
                Show(when: move || editing.get()) {
                    Input(
                        text: title,
                        auto_focus: true,
                        on_submit: move |_: String| save.call(),
                        style: theme::style(move |palette| {
                            palette
                                .field()
                                .height(px(36))
                                .font_size(px(13))
                                .padding(px(8))
                        }),
                    )
                    View(
                        style: theme::row()
                            .gap(px(8))
                            .justify_content(JustifyContent::FlexEnd),
                    ) {
                        Button(label: "Cancel", compact: true, plain: true, on_press: closed)
                        Button(
                            label: "Save",
                            compact: true,
                            primary: true,
                            disabled: computed(move || title.with(|title| title.trim().is_empty())),
                            on_press: save,
                        )
                    }
                }
            }
        }
    }
}
