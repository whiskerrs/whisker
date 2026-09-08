use super::{
    button::Button,
    theme::{self, color, size, space},
};
use whisker::css::{AlignSelf, FontWeight};
use whisker::prelude::*;
use whisker_icons::lucide;

#[component]
pub fn welcome(draft: RwSignal<String>) -> Element {
    render! {
        ScrollView(style: theme::fill()) {
            View(
                style: theme::column()
                    .width(percent(100))
                    .max_width(px(size::READING))
                    .align_self(AlignSelf::Center)
                    .padding(px(space::XXL))
                    .padding_top(px(space::HERO))
                    .gap(px(space::XL))
                    .flex_shrink(0.0),
            ) {
                View(style: theme::row().gap(px(space::MD))) {
                    Text(
                        value: "w.",
                        style: theme::text(28.0)
                            .font_weight(FontWeight::Bold)
                            .color(Color::hex(color::ON_ACCENT))
                            .background_color(Color::hex(color::ACCENT))
                            .border_radius(px(16))
                            .padding(px(space::MD)),
                    )
                    Text(value: "WHISKER CHAT", style: theme::muted())
                }
                Text(value: "Make room\nfor a good idea.", style: theme::display())
                Text(
                    value: "A thinking partner for the things\nyou want to make, learn, and understand.",
                    style: theme::muted(),
                )
                View(style: theme::column().gap(px(space::SM)).margin_top(px(space::LG))) {
                    Button(
                        label: "Find a fresh perspective",
                        icon: lucide::Lightbulb,
                        on_press: move |()| {
                            draft.set(
                                "Help me see a problem from a new perspective. Ask me what I am working on.".into(),
                            )
                        },
                    )
                    Button(
                        label: "Make something clearer",
                        icon: lucide::PenLine,
                        on_press: move |()| {
                            draft.set("Help me improve a piece of writing. Ask me to paste my draft and tell you who it is for.".into())
                        },
                    )
                    Button(
                        label: "Work through some code",
                        icon: lucide::CodeXml,
                        on_press: move |()| {
                            draft.set("Help me work through a coding problem. Ask me about the language and what I am trying to build.".into())
                        },
                    )
                }
            }
        }
    }
}
