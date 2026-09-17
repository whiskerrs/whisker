//! An offline, image-rich editorial app with a virtualized feed and animated routes.

mod data;
#[cfg(feature = "perf")]
mod perf;

use data::{CATEGORIES, STORY_COUNT, Story, photo, story};
#[cfg(not(feature = "perf-no-blur"))]
use whisker::css::BackdropFilter;
use whisker::css::{
    Border, BorderStyle, FontStyle, FontWeight, Overflow, PositionKind, TextDecorationLine,
    TextDecorationStyle,
};
use whisker::prelude::*;
use whisker_image::{Image, ImageEvent, ImageMode};
use whisker_router::{Outlet, Router, routes, use_navigator, use_param};
use whisker_safe_area::safe_area_insets;

const PAPER: u32 = 0xf5f2eb;
const INK: u32 = 0x202b29;
const MUTED: u32 = 0x758079;
const ACCENT: u32 = 0xc75035;

#[derive(Clone, Copy)]
struct NewsState {
    category: RwSignal<usize>,
    saved: RwSignal<Vec<usize>>,
}

fn column() -> Css {
    Css::new().flex_direction(FlexDirection::Column)
}
fn row() -> Css {
    Css::new()
        .flex_direction(FlexDirection::Row)
        .align_items(AlignItems::Center)
}
fn fill() -> Css {
    column()
        .flex_grow(1.0)
        .flex_shrink(1.0)
        .min_height(px(0))
        .background_color(Color::hex(PAPER))
}
fn text(size: f32) -> Css {
    Css::new().font_size(px(size)).color(Color::hex(INK))
}
fn muted(size: f32) -> Css {
    text(size).color(Color::hex(MUTED))
}
fn rule() -> Border {
    Border::new()
        .width(px(1))
        .style(BorderStyle::Solid)
        .color(Color::hex(0xdcded5))
}
fn spacer(height: f32) -> Element {
    View::builder()
        .style(column().height(px(height)).flex_shrink(0.0))
        .build()
}

fn glass() -> Css {
    let style = column().background_color(Color::rgba(245, 242, 235, 0.76));
    #[cfg(not(feature = "perf-no-blur"))]
    let style = style.backdrop_filter(BackdropFilter::blur(px(18)));
    style
}

fn toggle_saved(state: NewsState, id: usize) {
    state.saved.update(|saved| {
        if saved.contains(&id) {
            saved.retain(|item| *item != id);
        } else {
            saved.push(id);
        }
    });
}

#[whisker::main]
pub fn app() -> Element {
    provide_context(NewsState {
        category: signal(0),
        saved: signal(vec![1, 4, 7]),
    });
    render! {
        Router(routes: routes! {
            Stack {
                Route(path: "", component: Feed)
                Route(path: "article/:id", component: Article)
                Route(path: "saved", component: Saved)
            }
        }) { Outlet {} }
    }
}

#[component]
fn feed() -> Element {
    let state = use_context::<NewsState>().unwrap();
    let nav = use_navigator();
    let insets = safe_area_insets();
    let list = ListHandle::<usize>::new();
    #[cfg(feature = "perf")]
    perf::start(list.clone(), nav.clone(), state);
    render! {
        View(style: fill()) {
            List(
                each: move || (1..STORY_COUNT).filter(|id| state.category.get() == 0 || story(*id).category == CATEGORIES[state.category.get()]).collect::<Vec<_>>(),
                key: |id: &usize| *id,
                children: |id: ReadSignal<usize>| render! { StoryCard(item: story(id.get_untracked())) },
                list_ref: list.r(),
                header: move || render! {
                    View(style: column()) {
                        View(style: computed(move || column().height(px(insets.get().top as f32 + 126.0))))
                        LeadStory {}
                        View(style: column().padding(px(22)).padding_bottom(px(4))) {
                            Text(value: "THE LATEST", style: text(12.0).font_weight(FontWeight::Bold).letter_spacing(px(2)))
                        }
                    }
                },
                footer: || spacer(110.0),
                style: fill(),
            )
            View(style: computed(move || glass().position(PositionKind::Absolute).top(px(0)).left(px(0)).right(px(0)).padding_top(px(insets.get().top as f32)).z_index(3))) {
                View(style: row().padding(px(22)).padding_top(px(12)).padding_bottom(px(12)).justify_content(JustifyContent::SpaceBetween)) {
                    Text(value: "fieldnotes.", style: text(32.0).font_family("Georgia").font_weight(FontWeight::Bold).letter_spacing(px(-1)))
                    View(on_tap: move |_| { let _ = nav.push("/saved"); }, style: row().padding(px(10))) {
                        Text(value: "Saved ↗", style: text(13.0).font_weight(FontWeight::Bold))
                    }
                }
                ScrollView(axis: ScrollAxis::Horizontal, style: Css::new().height(px(48)).flex_shrink(0.0)) {
                    View(style: row().width(px(456)).gap(px(8)).padding_left(px(22)).padding_right(px(22))) {
                        ForEach(each: || (0..CATEGORIES.len()).collect::<Vec<_>>(), key: |i: &usize| *i, children: move |i: usize| render! {
                            View(on_tap: move |_| state.category.set(i), style: computed(move || row().width(px(76)).flex_shrink(0.0).justify_content(JustifyContent::Center).height(px(32)).border_radius(px(18)).background_color(Color::hex(if state.category.get() == i { INK } else { 0xe8e8df })))) {
                                Text(value: CATEGORIES[i], max_lines: 1u32, style: computed(move || text(12.0).font_weight(FontWeight::Bold).color(Color::hex(if state.category.get() == i { PAPER } else { INK }))))
                            }
                        })
                    }
                }
            }
        }
    }
}

#[component]
fn lead_story() -> Element {
    let nav = use_navigator();
    render! {
        View(style: column().padding(px(22)).gap(px(12)), on_tap: move |_| { let _ = nav.push("/article/0"); }) {
            Text(value: "WEDNESDAY, SEPTEMBER 16   /   ISSUE 024", style: muted(10.0).letter_spacing(px(1)))
            View(style: column().height(px(292)).border_radius(px(18)).overflow(Overflow::Hidden)) {
                Image(on_load: photo_loaded, on_error: photo_error, src: photo(10), mode: ImageMode::AspectFill, style: Css::new().width(percent(100)).height(percent(100)))
                View(style: glass().position(PositionKind::Absolute).bottom(px(14)).left(px(14)).right(px(14)).border_radius(px(12)).padding(px(14))) {
                    Text(value: "THE BIG READ  /  8 MIN", style: text(10.0).font_weight(FontWeight::Bold).letter_spacing(px(1.5)))
                    Text(value: "A quieter kind\nof progress.", style: text(30.0).font_family("Georgia").font_weight(FontWeight::Bold).margin_top(px(6)))
                }
            }
            Text(style: text(15.0).line_height(1.55)) {
                Text(value: "今、読みたい物語。", style: text(15.0).font_weight(FontWeight::Bold))
                Text(value: " 世界を変える小さなアイデアを、日々の暮らしから。")
            }
            View(style: row().justify_content(JustifyContent::SpaceBetween).padding_bottom(px(14)).border_bottom(rule())) {
                Text(value: "FIELDNOTES EDITORS", style: muted(10.0).letter_spacing(px(1)))
                Text(value: "Explore the story ↗", style: text(12.0).color(Color::hex(ACCENT)))
            }
        }
    }
}

#[component]
fn story_card(item: Story) -> Element {
    let nav = use_navigator();
    render! {
        View(style: column().padding(px(22)).padding_top(px(16)).padding_bottom(px(16)).gap(px(10)), on_tap: move |_| { let _ = nav.push(&format!("/article/{}", item.id)); }) {
            View(style: row().gap(px(16)).align_items(AlignItems::FlexStart)) {
                View(style: column().flex_grow(1.0).flex_shrink(1.0).gap(px(7))) {
                    Text(value: format!("{}  /  {:03}", item.category.to_uppercase(), item.id + 1), style: text(10.0).color(Color::hex(ACCENT)).font_weight(FontWeight::Bold).letter_spacing(px(1)))
                    Text(value: item.title, style: text(21.0).font_family("Georgia").font_weight(FontWeight::Bold).line_height(1.15))
                    Text(value: item.summary, style: muted(12.0).line_height(1.5))
                }
                Image(on_load: photo_loaded, on_error: photo_error, src: photo(item.photo), mode: ImageMode::AspectFill, style: Css::new().width(px(105)).height(px(124)).border_radius(px(12)).flex_shrink(0.0))
            }
            View(style: row().justify_content(JustifyContent::SpaceBetween).padding_bottom(px(16)).border_bottom(rule())) {
                Text(value: format!("{} MIN READ  ·  FIELDNOTES", 4 + item.id % 7), style: muted(9.0).letter_spacing(px(0.6)))
                Text(value: "Read ↗", style: text(12.0))
            }
        }
    }
}

#[component]
fn article() -> Element {
    let id = use_param("id")
        .get_untracked()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let item = story(id);
    let nav = use_navigator();
    let state = use_context::<NewsState>().unwrap();
    let insets = safe_area_insets();
    let list = ListHandle::<usize>::new();
    #[cfg(feature = "perf")]
    perf::article_scroll(list.clone());
    render! {
        View(style: fill()) {
            List(
                each: || (0..20usize).collect::<Vec<_>>(),
                key: |i: &usize| *i,
                list_ref: list.r(),
                header: move || render! {
                    View(style: column()) {
                        Image(on_load: photo_loaded, on_error: photo_error, src: photo(item.photo), mode: ImageMode::AspectFill, style: Css::new().width(percent(100)).height(px(350)))
                        View(style: column().padding(px(26)).padding_bottom(px(16)).gap(px(16))) {
                            Text(value: format!("{}  /  LONG READ", item.category.to_uppercase()), style: text(11.0).color(Color::hex(ACCENT)).letter_spacing(px(2)))
                            Text(value: item.title, style: text(36.0).font_family("Georgia").font_weight(FontWeight::Bold).line_height(1.12))
                            Text(value: item.summary, style: text(17.0).line_height(1.6))
                            View(style: row().gap(px(10)).padding_top(px(12)).padding_bottom(px(18)).border_bottom(rule())) {
                                Text(value: "FN", style: text(14.0).font_weight(FontWeight::Bold).color(Color::hex(ACCENT)))
                                Text(value: "By the Fieldnotes editors\nSeptember 16, 2026 · 8 min read", style: muted(11.0).line_height(1.5))
                            }
                        }
                    }
                },
                children: move |index: ReadSignal<usize>| {
                    let i = index.get_untracked();
                    render! {
                        View(style: column().padding_left(px(26)).padding_right(px(26)).padding_bottom(px(16)).gap(px(18))) {
                            Show(when: move || i == 3 || i == 11) {
                                Text(value: "“The future is something\nwe make together.”", style: text(28.0).font_family("Georgia").font_style(FontStyle::Italic).color(Color::hex(ACCENT)).padding_top(px(18)).padding_bottom(px(18)))
                                Image(on_load: photo_loaded, on_error: photo_error, src: photo(story(id + i).photo), mode: ImageMode::AspectFill, style: Css::new().width(percent(100)).height(px(220)).border_radius(px(12)))
                            }
                            Text(style: text(16.0).line_height(1.8)) {
                                Text(value: if i % 4 == 0 { "A closer look. " } else { "" }, style: text(16.0).font_weight(FontWeight::Bold))
                                Text(value: data::PARAGRAPHS[i % 4])
                                Text(value: "  Field notes", style: text(14.0).color(Color::hex(ACCENT)).text_decoration(TextDecorationLine::Underline, TextDecorationStyle::Solid, Color::hex(ACCENT)))
                            }
                        }
                    }
                },
                footer: || render! {
                    View(style: column().padding(px(26)).padding_top(px(24)).gap(px(16))) {
                        Text(value: "This is a fictional editorial fixture. Photos are credited in assets/credits.json.", style: muted(11.0))
                        View(style: column().height(px(90)))
                    }
                },
                style: fill(),
            )
            View(style: computed(move || glass().position(PositionKind::Absolute).top(px(0)).left(px(0)).right(px(0)).padding_top(px(insets.get().top as f32)).z_index(3))) {
                View(style: row().height(px(54)).padding_left(px(22)).padding_right(px(22)).justify_content(JustifyContent::SpaceBetween)) {
                    View(on_tap: move |_| { let _ = nav.back(); }, style: row().padding(px(8))) { Text(value: "← Back", style: text(14.0)) }
                    Text(value: "fieldnotes.", style: text(20.0).font_family("Georgia").font_weight(FontWeight::Bold))
                    View(on_tap: move |_| toggle_saved(state, id), style: row().padding(px(8))) {
                        Text(value: computed(move || if state.saved.with(|saved| saved.contains(&id)) { "Saved ✓".to_owned() } else { "Save +".to_owned() }), style: text(13.0).color(Color::hex(ACCENT)))
                    }
                }
            }
        }
    }
}

#[component]
fn saved() -> Element {
    let state = use_context::<NewsState>().unwrap();
    let nav = use_navigator();
    let insets = safe_area_insets();
    render! {
        View(style: computed(move || fill().padding_top(px(insets.get().top as f32)))) {
            View(style: column().padding(px(22)).gap(px(14))) {
                View(on_tap: move |_| { let _ = nav.back(); }) { Text(value: "← Back to your edition", style: text(14.0)) }
                Text(value: "Your reading list.", style: text(32.0).font_family("Georgia").font_weight(FontWeight::Bold))
                Text(value: computed(move || format!("{} stories, for a slower moment.", state.saved.with(Vec::len))), style: muted(14.0))
            }
            List(each: move || state.saved.get(), key: |id: &usize| *id, children: |id: ReadSignal<usize>| render! { StoryCard(item: story(id.get_untracked())) }, footer: || spacer(60.0), style: fill())
        }
    }
}

fn photo_loaded(_event: ImageEvent) {
    #[cfg(feature = "perf")]
    perf::first_image_loaded();
}

fn photo_error(event: ImageEvent) {
    eprintln!("NEWS_IMAGE_ERROR {}", event.error());
}
