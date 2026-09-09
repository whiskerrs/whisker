use super::{button::Button, theme};
use crate::state::{AppState, Session};
use whisker::css::{Cursor, FontWeight, PointerEvents};
use whisker::prelude::*;
use whisker_icons::lucide;

#[component]
pub fn history_row(session: Session, opened: Callback, edit: Callback<Session>) -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let active = app.active_id();
    render! {
        View(
            style: theme::style(move |palette| {
                theme::row()
                    .min_height(px(40))
                    .padding_left(px(10))
                    .padding_right(px(4))
                    .gap(px(4))
                    .border_radius(px(8))
                    .background_color(if active.get() == session.id {
                        Color::hex(palette.tint)
                    } else {
                        Color::Transparent
                    })
            }),
        ) {
            View(
                accessibility: computed(move || {
                    Accessibility::new()
                        .label(session.title.get())
                        .role(AccessibilityRole::Button)
                }),
                on_tap: move |_| {
                    app.select(session.id);
                    opened.call();
                },
                style: theme::row()
                    .flex_grow(1.0)
                    .min_width(px(0))
                    .height(px(40))
                    .cursor(Cursor::Pointer),
            ) {
                Text(
                    value: session.title,
                    max_lines: 1u32,
                    style: theme::style(move |palette| {
                        palette
                            .text(13.0)
                            .font_weight(FontWeight::Numeric(500))
                            .pointer_events(PointerEvents::None)
                    }),
                )
            }
            Button(
                label: "",
                icon: lucide::Ellipsis,
                accessible_label: "Conversation actions",
                compact: true,
                plain: true,
                on_press: move |()| edit.run(session),
            )
        }
    }
}
