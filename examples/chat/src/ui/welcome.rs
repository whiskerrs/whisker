use super::{
    button::Button,
    chat_layout,
    theme::{self, size, space},
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
                    .padding_top(px(chat_layout::CONTENT_TOP + space::HERO))
                    .padding_bottom(px(chat_layout::CONTENT_BOTTOM))
                    .gap(px(space::XL))
                    .flex_shrink(0.0),
            ) {
                View(style: theme::row().gap(px(space::MD))) {
                    Text(
                        value: "w.",
                        style: theme::style(move |palette| {
                            palette
                                .text(28.0)
                                .font_weight(FontWeight::Bold)
                                .color(Color::hex(palette.on_accent))
                                .background_color(Color::hex(palette.accent))
                                .border_radius(px(8))
                                .padding(px(space::MD))
                        }),
                    )
                    Text(value: "WHISKER CHAT", style: theme::style(move |palette| palette.muted()))
                }
                Text(
                    value: "What are you working on?",
                    style: theme::style(move |palette| palette.display()),
                )
                Text(
                    value: "Write, explore, and solve problems with your models.",
                    style: theme::style(move |palette| palette.muted()),
                )
                View(style: theme::column().gap(px(space::SM)).margin_top(px(space::LG))) {
                    Button(
                        label: "Explore a problem",
                        icon: lucide::Lightbulb,
                        on_press: move |()| {
                            draft.set(
                                "Help me see a problem from a new perspective. Ask me what I am working on.".into(),
                            )
                        },
                    )
                    Button(
                        label: "Refine a draft",
                        icon: lucide::PenLine,
                        on_press: move |()| {
                            draft.set("Help me improve a piece of writing. Ask me to paste my draft and tell you who it is for.".into())
                        },
                    )
                    Button(
                        label: "Review some code",
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
