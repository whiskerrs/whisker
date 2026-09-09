use super::{button::Button, navigation, theme};
use crate::state::AppState;
use whisker::css::{AlignSelf, FlexWrap, FontWeight};
use whisker::prelude::*;
use whisker_icons::lucide;
use whisker_router::use_navigator;

#[derive(Clone, Copy, PartialEq)]
pub enum SettingsSection {
    Appearance,
    Connection,
}

#[component]
pub fn settings_shell(section: SettingsSection, children: Children) -> Element {
    let notice = use_context::<AppState>()
        .expect("AppState context")
        .notice();
    let projected = children.clone();
    let nav = use_navigator();
    let appearance_nav = nav.clone();
    let connection_nav = nav.clone();
    render! {
        ScrollView(style: theme::style(move |palette| palette.screen())) {
            View(
                style: theme::row()
                    .width(percent(100))
                    .max_width(px(1200))
                    .align_self(AlignSelf::Center)
                    .align_items(AlignItems::FlexStart)
                    .flex_wrap(FlexWrap::Wrap)
                    .padding(px(24))
                    .gap(px(40))
                    .flex_shrink(0.0),
            ) {
                View(style: theme::column().width(px(192)).flex_shrink(0.0).gap(px(24))) {
                    View(style: theme::column().gap(px(12))) {
                        Button(
                            label: "Back to chat",
                            icon: lucide::ArrowLeft,
                            compact: true,
                            plain: true,
                            on_press: move |()| {
                                if section == SettingsSection::Connection {
                                    navigation::return_to_settings(&nav, notice);
                                }
                                navigation::return_to_chat(&nav, notice);
                            },
                        )
                        Text(
                            value: "Settings",
                            style: theme::style(move |palette| {
                                palette
                                    .text(20.0)
                                    .font_weight(FontWeight::Numeric(600))
                                    .padding_left(px(12))
                            }),
                        )
                    }
                    View(style: theme::column().gap(px(4))) {
                        Button(
                            label: "Appearance",
                            icon: lucide::SlidersHorizontal,
                            compact: true,
                            plain: section != SettingsSection::Appearance,
                            on_press: move |()| {
                                if section != SettingsSection::Appearance {
                                    navigation::return_to_settings(&appearance_nav, notice);
                                }
                            },
                        )
                        Button(
                            label: "API connection",
                            icon: lucide::KeyRound,
                            compact: true,
                            plain: section != SettingsSection::Connection,
                            on_press: move |()| {
                                if section != SettingsSection::Connection {
                                    navigation::open_connection(&connection_nav, notice);
                                }
                            },
                        )
                    }
                }
                View(
                    style: theme::column()
                        .flex_basis(px(560))
                        .flex_grow(1.0)
                        .min_width(px(0))
                        .max_width(percent(100))
                        .gap(px(28))
                        .padding_top(px(8)),
                ) {
                    Fragment {
                        {projected()}
                    }
                }
            }
        }
    }
}

pub fn heading(palette: theme::Palette) -> Css {
    palette.text(22.0).font_weight(FontWeight::Numeric(600))
}

pub fn label(palette: theme::Palette) -> Css {
    palette.text(14.0).font_weight(FontWeight::Numeric(500))
}

pub fn description(palette: theme::Palette) -> Css {
    palette.text(13.0).color(Color::hex(palette.muted))
}

pub fn field(palette: theme::Palette) -> Css {
    palette
        .field()
        .font_size(px(14))
        .height(px(36))
        .padding(px(8))
}
