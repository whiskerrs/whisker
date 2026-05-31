//! Hello World — a music-streaming-style home screen.
//!
//! Migrated to Phase 6.5a (Leptos-style) reactivity. The visual
//! design is preserved, but state lives in `RwSignal`s and every
//! handler updates them via `.update` / `.set`.
//!
//! ## State management — props drilling
//!
//! App-wide reactive state is created at the [`app`] root in a single
//! `Copy` [`AppState`] struct and threaded down to the components
//! that need it via `#[component]` props. There is no global
//! accessor / no `thread_local!` cell. The visual / structural
//! components that don't touch state (`header`, `chips`,
//! `section_header`, …) keep their signatures unchanged.
//!
//! Bigger apps usually graduate from props drilling to
//! `provide_context` / `use_context` once the prop chain gets too
//! noisy. For a one-screen demo this is direct and the type signature
//! of every component documents exactly which state it depends on.
//!
//! Exercises a wide slice of the Whisker surface:
//!
//! - `#[whisker::main]` returning `Element`.
//! - `RwSignal<T>` shared by passing a `Copy` `AppState` through
//!   the component tree.
//! - `render!` with `text(value: …)` kwargs, `style:` and other
//!   attributes, `on_tap:` handlers.
//! - `whisker_css::Css` builder (replaces all CSS string literals).

use whisker::prelude::*;
use whisker::runtime::view::Element;
// Pull in every Css helper without per-name imports. `Css` and
// the common keyword enums already come in via `whisker::prelude`,
// but the long tail (`BackgroundRepeat`, `Gradient`, `ColorStop`,
// `BorderRadius`, `MarginValue`, `Padding`, …) lives here.
use whisker::css::keyword::{BorderStyle, FontWeight, TextAlign, TextTransform};
use whisker::css::{ColorStop, Gradient, ImageRef, LinearDirection, PositionKind, Size};

// ---- App state --------------------------------------------------------------

/// App-wide reactive state. All fields are `RwSignal<T>` (i.e. `Copy`
/// handles into the reactive arena), so the struct itself is `Copy`
/// and free to thread through `#[component]` props without `move ||`
/// boilerplate.
#[derive(Copy, Clone)]
struct AppState {
    /// Index of the bottom-tab the user has selected (0..=3).
    selected_tab: RwSignal<usize>,
    /// Bitmask: bit `i` set ⇒ mix `i` in the grid is liked.
    liked_mixes: RwSignal<u8>,
    /// Whether the now-playing widget is showing the "playing" state.
    is_playing: RwSignal<bool>,
}

impl AppState {
    fn new() -> Self {
        let initial_liked = whisker_local_store::WhiskerLocalStore::load(LIKED_MIXES_KEY.into())
            .ok()
            .flatten()
            .and_then(|s| s.parse::<u8>().ok())
            .unwrap_or(0b000_010_u8);
        Self {
            selected_tab: RwSignal::new(0_usize),
            liked_mixes: RwSignal::new(initial_liked),
            is_playing: RwSignal::new(true),
        }
    }
}

/// Storage key for the heart-bitmask.
const LIKED_MIXES_KEY: &str = "hello_world.liked_mixes";

// ---- Palette ----------------------------------------------------------------

const BG: Color = Color::hex(0x0F0A1E);
const SURFACE: Color = Color::hex(0x1A1330);
const SURFACE_2: Color = Color::hex(0x241946);
const TEXT_PRIMARY: Color = Color::hex(0xFFFFFF);
// `rgba(255,255,255,0.65)` etc. — pre-computed at compile time.
const TEXT_SECONDARY: Color = Color::rgba(255, 255, 255, 0.65);
const TEXT_MUTED: Color = Color::rgba(255, 255, 255, 0.45);
const ACCENT: Color = Color::hex(0x9B6BFF);
const ACCENT_2: Color = Color::hex(0xFF5E9B);

// Shorthand for `linear-gradient(angle, c1, c2)`. Returns an
// `ImageRef` suitable for `.background_image(...)`.
fn linear_gradient_135(c1: Color, c2: Color) -> ImageRef {
    Gradient::Linear {
        direction: LinearDirection::Angle(135.deg()),
        stops: vec![
            ColorStop::at(c1, 0.percent()),
            ColorStop::at(c2, 100.percent()),
        ],
    }
    .into()
}

fn linear_gradient_180(c1: Color, c2: Color) -> ImageRef {
    Gradient::Linear {
        direction: LinearDirection::Angle(180.deg()),
        stops: vec![
            ColorStop::at(c1, 0.percent()),
            ColorStop::at(c2, 100.percent()),
        ],
    }
    .into()
}

// ---- Building blocks --------------------------------------------------------

#[component]
fn art_tile(c1: Color, c2: Color, width: Size, radius: Length) -> Element {
    render! {
        view(style: Css::new()
            .width(width.clone())
            .aspect_ratio(1.0, 1.0)
            .border_radius(radius)
            .background_image(linear_gradient_135(c1, c2)))
    }
}

#[component]
fn chip(label: &'static str, accented: bool) -> Element {
    let bg = if accented {
        ACCENT
    } else {
        Color::rgba(255, 255, 255, 0.08)
    };
    render! {
        text(
            value: label,
            style: Css::new()
                .font_size(13.px())
                .color(TEXT_PRIMARY)
                .padding((8.px(), 16.px()))
                .background_color(bg)
                .border_radius(999.px())
                .margin_right(8.px()),
        )
    }
}

#[component]
fn section_header(title: &'static str) -> Element {
    render! {
        view {
            text(
                value: title,
                style: Css::new()
                    .font_size(20.px())
                    .font_weight(FontWeight::Numeric(700))
                    .color(TEXT_PRIMARY),
            )
            text(
                value: "See all ›",
                style: Css::new()
                    .font_size(13.px())
                    .color(Color::rgba(255, 255, 255, 0.5)),
            )
        }
    }
}

#[component]
fn recent_card(title: &'static str, sub: &'static str, c1: Color, c2: Color) -> Element {
    render! {
        view(
            style: Css::new()
                .width(140.px())
                .margin_right(14.px())
                .display_flex()
                .flex_direction(FlexDirection::Column),
        ) {
            ArtTile(c1: c1, c2: c2, width: 140.px(), radius: 12.px())
            text(
                value: title,
                style: Css::new()
                    .font_size(14.px())
                    .font_weight(FontWeight::Numeric(600))
                    .color(TEXT_PRIMARY)
                    .margin_top(8.px()),
            )
            text(
                value: sub,
                style: Css::new()
                    .font_size(12.px())
                    .color(TEXT_SECONDARY)
                    .margin_top(2.px()),
            )
        }
    }
}

#[component]
fn grid_tile(
    index: usize,
    title: &'static str,
    c1: Color,
    c2: Color,
    state: AppState,
) -> Element {
    let bitmask = state.liked_mixes;
    let liked_bit = 1u8 << index;
    let on_heart = move |_| bitmask.update(|b| *b ^= liked_bit);

    let heart_glyph = computed(move || {
        if bitmask.get() & liked_bit != 0 {
            "♥".to_string()
        } else {
            "♡".to_string()
        }
    });
    let heart_style = computed(move || {
        let heart_color = if bitmask.get() & liked_bit != 0 {
            ACCENT_2
        } else {
            TEXT_MUTED
        };
        Css::new()
            .position(PositionKind::Absolute)
            .top(8.px())
            .right(8.px())
            .width(28.px())
            .height(28.px())
            .border_radius(14.px())
            .background_color(Color::rgba(0, 0, 0, 0.45))
            .color(heart_color)
            .font_size(16.px())
            .text_align(TextAlign::Center)
            .line_height(28.px())
    });

    render! {
        view(
            style: Css::new()
                .width(48.percent())
                .margin_bottom(16.px())
                .background_color(SURFACE)
                .border_radius(14.px())
                .padding(12.px())
                .box_shadow(0.px(), 4.px(), 12.px(), 0.px(), Color::rgba(0, 0, 0, 0.25))
                .display_flex()
                .flex_direction(FlexDirection::Column),
        ) {
            view(
                style: Css::new()
                    .position(PositionKind::Relative)
                    .width(100.percent()),
            ) {
                ArtTile(c1: c1, c2: c2, width: 100.percent(), radius: 10.px())
                text(style: heart_style, on_tap: on_heart, value: heart_glyph)
            }
            text(
                value: title,
                style: Css::new()
                    .font_size(14.px())
                    .font_weight(FontWeight::Numeric(600))
                    .color(TEXT_PRIMARY)
                    .margin_top(10.px()),
            )
            text(
                value: "Daily Mix",
                style: Css::new()
                    .font_size(11.px())
                    .color(TEXT_SECONDARY)
                    .margin_top(2.px()),
            )
        }
    }
}

#[component]
fn activity_row(
    initial: &'static str,
    c1: Color,
    c2: Color,
    title: &'static str,
    sub: &'static str,
    when: &'static str,
) -> Element {
    render! {
        view(
            style: Css::new()
                .width(100.percent())
                .display_flex()
                .flex_direction(FlexDirection::Row)
                .align_items(AlignItems::Center)
                .padding((14.px(), 20.px()))
                .border_bottom(
                    Border::new()
                        .width(1.px())
                        .style(BorderStyle::Solid)
                        .color(Color::rgba(255, 255, 255, 0.06)),
                ),
        ) {
            view(
                style: Css::new()
                    .width(44.px())
                    .height(44.px())
                    .border_radius(22.px())
                    .background_image(linear_gradient_135(c1, c2))
                    .display_flex()
                    .align_items(AlignItems::Center)
                    .justify_content(JustifyContent::Center)
                    .margin_right(12.px()),
            ) {
                text(
                    value: initial,
                    style: Css::new()
                        .font_size(18.px())
                        .color(Color::Named(NamedColor::White))
                        .font_weight(FontWeight::Numeric(700)),
                )
            }
            view(
                style: Css::new()
                    .flex_grow(1.0)
                    .flex_shrink(1.0)
                    .display_flex()
                    .flex_direction(FlexDirection::Column),
            ) {
                text(
                    value: title,
                    style: Css::new()
                        .font_size(15.px())
                        .color(TEXT_PRIMARY)
                        .font_weight(FontWeight::Numeric(600)),
                )
                text(
                    value: sub,
                    style: Css::new()
                        .font_size(12.px())
                        .color(TEXT_SECONDARY)
                        .margin_top(2.px()),
                )
            }
            text(
                value: when,
                style: Css::new().font_size(11.px()).color(TEXT_MUTED),
            )
        }
    }
}

#[component]
fn tab_item(index: usize, label: &'static str, glyph: &'static str, state: AppState) -> Element {
    let tab = state.selected_tab;
    let on_pick = move |_| tab.set(index);
    let glyph_style = computed(move || {
        let tab_color = if tab.get() == index { ACCENT } else { TEXT_MUTED };
        Css::new().font_size(22.px()).color(tab_color)
    });
    let label_style = computed(move || {
        let selected = tab.get() == index;
        let tab_color = if selected { ACCENT } else { TEXT_MUTED };
        let weight = if selected { 700 } else { 500 };
        Css::new()
            .font_size(11.px())
            .color(tab_color)
            .font_weight(FontWeight::Numeric(weight))
    });
    render! {
        view(
            on_tap: on_pick,
            style: Css::new()
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .align_items(AlignItems::Center)
                .gap(4.px())
                .padding((4.px(), 12.px())),
        ) {
            text(style: glyph_style, value: glyph)
            text(style: label_style, value: label)
        }
    }
}

#[component]
fn tab_bar(state: AppState) -> Element {
    render! {
        view(
            style: Css::new()
                .position(PositionKind::Absolute)
                .left(0.px())
                .right(0.px())
                .bottom(0.px())
                .display_flex()
                .flex_direction(FlexDirection::Row)
                .justify_content(JustifyContent::SpaceAround)
                .padding((12.px(), 0.px(), 28.px()))
                .background_color(SURFACE)
                .border_top(
                    Border::new()
                        .width(1.px())
                        .style(BorderStyle::Solid)
                        .color(Color::rgba(255, 255, 255, 0.06)),
                ),
        ) {
            TabItem(index: 0_usize, label: "Home",    glyph: "⌂", state: state)
            TabItem(index: 1_usize, label: "Search",  glyph: "⌕", state: state)
            TabItem(index: 2_usize, label: "Library", glyph: "♫", state: state)
            TabItem(index: 3_usize, label: "Profile", glyph: "○", state: state)
        }
    }
}

#[component]
fn now_playing(state: AppState) -> Element {
    let playing = state.is_playing;
    let toggle = move |_| playing.update(|p| *p = !*p);
    let glyph = computed(move || {
        if playing.get() {
            "▌▌".to_string()
        } else {
            "▶".to_string()
        }
    });
    let status = computed(move || {
        if playing.get() {
            "Lo-Fi Beats · playing".to_string()
        } else {
            "Lo-Fi Beats · paused".to_string()
        }
    });
    render! {
        view(
            style: Css::new()
                .position(PositionKind::Absolute)
                .left(12.px())
                .right(12.px())
                .bottom(78.px())
                .display_flex()
                .flex_direction(FlexDirection::Row)
                .align_items(AlignItems::Center)
                .padding(10.px())
                .background_color(SURFACE_2)
                .border_radius(14.px())
                .box_shadow(0.px(), 6.px(), 16.px(), 0.px(), Color::rgba(0, 0, 0, 0.35)),
        ) {
            ArtTile(c1: Color::hex(0xFF7E5F), c2: Color::hex(0xFEB47B), width: 48.px(), radius: 8.px())
            view(
                style: Css::new()
                    .flex(Flex::Number(1.0))
                    .padding((0.px(), 12.px()))
                    .display_flex()
                    .flex_direction(FlexDirection::Column),
            ) {
                text(
                    value: "Sunset Drive",
                    style: Css::new()
                        .font_size(14.px())
                        .color(TEXT_PRIMARY)
                        .font_weight(FontWeight::Numeric(600)),
                )
                text(
                    value: status,
                    style: Css::new()
                        .font_size(11.px())
                        .color(TEXT_SECONDARY)
                        .margin_top(2.px()),
                )
            }
            text(
                value: glyph,
                on_tap: toggle,
                style: Css::new()
                    .width(40.px())
                    .height(40.px())
                    .border_radius(20.px())
                    .background_color(ACCENT)
                    .color(Color::Named(NamedColor::White))
                    .font_size(14.px())
                    .text_align(TextAlign::Center)
                    .line_height(40.px()),
            )
        }
    }
}

#[component]
fn header() -> Element {
    // Two icon buttons share the same base; the closure-built
    // style is kept so each call can append its own margin.
    let icon = || {
        Css::new()
            .width(40.px())
            .height(40.px())
            .border_radius(20.px())
            .background_color(Color::rgba(255, 255, 255, 0.10))
            .color(Color::Named(NamedColor::White))
            .font_size(16.px())
            .text_align(TextAlign::Center)
            .line_height(40.px())
    };
    render! {
        view(
            style: Css::new()
                .width(100.percent())
                .padding((60.px(), 20.px(), 18.px()))
                .background_image(linear_gradient_180(Color::hex(0x2C1860), BG))
                .display_flex()
                .flex_direction(FlexDirection::Row)
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::SpaceBetween),
        ) {
            view(
                style: Css::new()
                    .display_flex()
                    .flex_direction(FlexDirection::Row)
                    .align_items(AlignItems::Center),
            ) {
                view(
                    style: Css::new()
                        .width(44.px())
                        .height(44.px())
                        .border_radius(22.px())
                        .background_image(linear_gradient_135(
                            Color::hex(0xFF7E5F),
                            Color::hex(0xFEB47B),
                        ))
                        .display_flex()
                        .align_items(AlignItems::Center)
                        .justify_content(JustifyContent::Center)
                        .margin_right(12.px()),
                ) {
                    text(
                        value: "I",
                        style: Css::new()
                            .font_size(18.px())
                            .color(Color::Named(NamedColor::White))
                            .font_weight(FontWeight::Numeric(700)),
                    )
                }
                view(
                    style: Css::new()
                        .display_flex()
                        .flex_direction(FlexDirection::Column),
                ) {
                    text(
                        value: "Welcome back",
                        style: Css::new().font_size(12.px()).color(TEXT_SECONDARY),
                    )
                    text(
                        value: "Itome",
                        style: Css::new()
                            .font_size(18.px())
                            .color(TEXT_PRIMARY)
                            .font_weight(FontWeight::Numeric(700)),
                    )
                }
            }
            view(
                style: Css::new()
                    .display_flex()
                    .flex_direction(FlexDirection::Row),
            ) {
                text(value: "♡", style: icon().margin_right(8.px()))
                text(value: "⚙", style: icon())
            }
        }
    }
}

#[component]
fn chips() -> Element {
    render! {
        view(
            style: Css::new()
                .display_flex()
                .flex_direction(FlexDirection::Row)
                .padding((0.px(), 20.px(), 8.px()))
                .flex_wrap(whisker::css::FlexWrap::Nowrap),
        ) {
            Chip(label: "All",        accented: true)
            Chip(label: "Music",      accented: false)
            Chip(label: "Podcasts",   accented: false)
            Chip(label: "Audiobooks", accented: false)
        }
    }
}

#[component]
fn recents() -> Element {
    render! {
        scroll_view(
            scroll_orientation: "horizontal",
            style: Css::new().padding((4.px(), 20.px(), 8.px())).height(200.px()),
        ) {
            RecentCard(title: "Sunset Drive",  sub: "Lo-Fi Beats", c1: Color::hex(0xFF7E5F), c2: Color::hex(0xFEB47B))
            RecentCard(title: "Deep Focus",    sub: "Ambient",     c1: Color::hex(0x4FACFE), c2: Color::hex(0x00F2FE))
            RecentCard(title: "Late Night",    sub: "Synthwave",   c1: Color::hex(0x9B6BFF), c2: Color::hex(0xFF5E9B))
            RecentCard(title: "Coffee House",  sub: "Acoustic",    c1: Color::hex(0xFCB69F), c2: Color::hex(0xFFECD2))
            RecentCard(title: "Energy Boost",  sub: "Workout",     c1: Color::hex(0x11998E), c2: Color::hex(0x38EF7D))
        }
    }
}

#[component]
fn featured() -> Element {
    render! {
        view(
            style: Css::new()
                .margin((0.px(), 20.px()))
                .height(180.px())
                .border_radius(18.px())
                .background_image(linear_gradient_135(Color::hex(0x4A00E0), Color::hex(0x8E2DE2)))
                .padding(20.px())
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .justify_content(JustifyContent::FlexEnd)
                .box_shadow(0.px(), 10.px(), 24.px(), 0.px(), Color::rgba(74, 0, 224, 0.4)),
        ) {
            text(
                value: "Made For You",
                style: Css::new()
                    .font_size(12.px())
                    .color(TEXT_SECONDARY)
                    .text_transform(TextTransform::Uppercase)
                    .letter_spacing(1.5.px()),
            )
            text(
                value: "Discover Weekly",
                style: Css::new()
                    .font_size(26.px())
                    .font_weight(FontWeight::Numeric(700))
                    .color(TEXT_PRIMARY)
                    .margin_top(6.px()),
            )
            text(
                value: "30 new songs picked just for you",
                style: Css::new()
                    .font_size(13.px())
                    .color(TEXT_SECONDARY)
                    .margin_top(4.px()),
            )
        }
    }
}

#[component]
fn grid(state: AppState) -> Element {
    render! {
        view(
            style: Css::new()
                .padding((4.px(), 20.px(), 0.px()))
                .display_flex()
                .flex_direction(FlexDirection::Row)
                .flex_wrap(whisker::css::FlexWrap::Wrap)
                .justify_content(JustifyContent::SpaceBetween),
        ) {
            GridTile(index: 0_usize, title: "Chill Mix",   c1: Color::hex(0x667EEA), c2: Color::hex(0x764BA2), state: state)
            GridTile(index: 1_usize, title: "Happy Mix",   c1: Color::hex(0xF093FB), c2: Color::hex(0xF5576C), state: state)
            GridTile(index: 2_usize, title: "Focus Mix",   c1: Color::hex(0x4FACFE), c2: Color::hex(0x00F2FE), state: state)
            GridTile(index: 3_usize, title: "Workout Mix", c1: Color::hex(0x43E97B), c2: Color::hex(0x38F9D7), state: state)
            GridTile(index: 4_usize, title: "Sleep Mix",   c1: Color::hex(0xFA709A), c2: Color::hex(0xFEE140), state: state)
            GridTile(index: 5_usize, title: "Indie Mix",   c1: Color::hex(0x30CFD0), c2: Color::hex(0x330867), state: state)
        }
    }
}

#[component]
fn activity_feed() -> Element {
    render! {
        view(
            style: Css::new()
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .padding((0.px(), 0.px(), 8.px())),
        ) {
            ActivityRow(initial: "A", c1: Color::hex(0xFF7E5F), c2: Color::hex(0xFEB47B), title: "Alice", sub: "Started following you",            when: "2m")
            ActivityRow(initial: "R", c1: Color::hex(0x667EEA), c2: Color::hex(0x764BA2), title: "Riku",  sub: "Liked your playlist 'Late Night'", when: "1h")
            ActivityRow(initial: "M", c1: Color::hex(0x43E97B), c2: Color::hex(0x38F9D7), title: "Mio",   sub: "Shared 'Sunset Drive' with you",   when: "3h")
            ActivityRow(initial: "K", c1: Color::hex(0xFA709A), c2: Color::hex(0xFEE140), title: "Ken",   sub: "Added 5 songs to 'Workout'",       when: "yesterday")
            ActivityRow(initial: "S", c1: Color::hex(0x4FACFE), c2: Color::hex(0x00F2FE), title: "Sora",  sub: "Created a new playlist 'Focus'",   when: "2d")
        }
    }
}

#[component]
fn scroll_card(n: i32, color: Color) -> Element {
    render! {
        view(
            style: Css::new()
                .width(96.px())
                .height(56.px())
                .flex_shrink(0.0)
                .margin_right(8.px())
                .border_radius(10.px())
                .background_color(color)
                .display_flex()
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::Center),
        ) {
            text(
                value: format!("{n}"),
                style: Css::new()
                    .color(Color::Named(NamedColor::White))
                    .font_size(18.px())
                    .font_weight(FontWeight::Numeric(700)),
            )
        }
    }
}

#[component]
fn scroll_demo() -> Element {
    let info = RwSignal::new(String::new());
    let row = ScrollViewHandle::new();
    let label = computed(move || {
        let s = info.get();
        if s.is_empty() {
            "← swipe, or use the buttons →".to_string()
        } else {
            s
        }
    });
    // Shared by 6 buttons — kept as a closure so each call site
    // gets a fresh `Css` value.
    let btn_style = || {
        Css::new()
            .padding((6.px(), 10.px()))
            .background_color(Color::hex(0x6C5CE7))
            .border_radius(6.px())
            .color(Color::hex(0xFFFFFF))
            .font_size(12.px())
            .font_weight(FontWeight::Numeric(600))
    };
    render! {
        view(
            style: Css::new()
                .margin((4.px(), 20.px(), 8.px()))
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .gap(6.px()),
        ) {
            text(
                value: label,
                style: Css::new()
                    .color(Color::hex(0xB9A9FF))
                    .font_size(12.px())
                    .font_family("monospace"),
            )
            scroll_view(
                ref: row.r(),
                scroll_orientation: "horizontal",
                on_scroll: move |e| {
                    info.set(format!(
                        "left={:.0}  width={:.0}  dx={:.0}  drag={}",
                        e.detail.scroll_left,
                        e.detail.scroll_width,
                        e.detail.delta_x,
                        e.detail.is_dragging,
                    ))
                },
                style: Css::new()
                    .height(64.px())
                    .display_flex()
                    .flex_direction(FlexDirection::Row)
                    .background_color(SURFACE)
                    .border_radius(12.px())
                    .padding(4.px()),
            ) {
                ScrollCard(n: 1_i32, color: Color::hex(0x667EEA))
                ScrollCard(n: 2_i32, color: Color::hex(0xF093FB))
                ScrollCard(n: 3_i32, color: Color::hex(0x4FACFE))
                ScrollCard(n: 4_i32, color: Color::hex(0x43E97B))
                ScrollCard(n: 5_i32, color: Color::hex(0xFA709A))
                ScrollCard(n: 6_i32, color: Color::hex(0x30CFD0))
                ScrollCard(n: 7_i32, color: Color::hex(0xFF7E5F))
                ScrollCard(n: 8_i32, color: Color::hex(0x9B6BFF))
            }
            view(
                style: Css::new()
                    .display_flex()
                    .flex_direction(FlexDirection::Row)
                    .flex_wrap(whisker::css::FlexWrap::Wrap)
                    .gap(8.px()),
            ) {
                text(value: "→ 300",  style: btn_style(), on_tap: move |_| { row.scroll_to(300.0, true); })
                text(value: "⇤ start", style: btn_style(), on_tap: move |_| { row.scroll_to(0.0, true); })
                text(value: "+120",    style: btn_style(), on_tap: move |_| { row.scroll_by(120.0); })
                text(value: "▶ auto",  style: btn_style(), on_tap: move |_| { row.auto_scroll(120.0); })
                text(value: "■ stop",  style: btn_style(), on_tap: move |_| { row.stop_auto_scroll(); })
                text(value: "ℹ info",  style: btn_style(), on_tap: move |_| {
                    spawn_local(async move {
                        match row.get_scroll_info().await {
                            Ok(i) => info.set(format!(
                                "getScrollInfo  x={:.0}  range={:.0}",
                                i.scroll_x, i.scroll_range,
                            )),
                            Err(e) => info.set(format!("err: {e}")),
                        }
                    });
                })
            }
        }
    }
}

#[component]
fn scroll_body(state: AppState) -> Element {
    render! {
        scroll_view(
            scroll_orientation: "vertical",
            style: Css::new()
                .flex_grow(1.0)
                .flex_shrink(1.0)
                .width(100.percent())
                .background_color(BG)
                .display_flex()
                .flex_direction(FlexDirection::Column),
        ) {
            ScrollDemo()
            Chips()
            SectionHeader(title: "Recently Played")
            Recents()
            SectionHeader(title: "Made For You")
            Featured()
            SectionHeader(title: "Your Top Mixes")
            Grid(state: state)
            SectionHeader(title: "Activity")
            ActivityFeed()
            view(style: Css::new().height(160.px()))
        }
    }
}

// ---- Main app ---------------------------------------------------------------

use whisker_hello_element::*;
use whisker_video::{Video, VideoHandle, VideoProps};

const BIG_BUCK_BUNNY_URL: &str =
    "https://test-videos.co.uk/vids/bigbuckbunny/mp4/h264/360/Big_Buck_Bunny_360_10s_1MB.mp4";

#[component]
pub fn video_demo() -> Element {
    let video = VideoHandle::new();
    // Shared by 3 buttons.
    let btn_style = || {
        Css::new()
            .padding((8.px(), 16.px()))
            .background_color(Color::hex(0x6C5CE7))
            .border_radius(6.px())
            .color(Color::hex(0xFFFFFF))
            .font_size(14.px())
    };
    render! {
        view(style: Css::new().flex_direction(FlexDirection::Column)) {
            // `Video` is a module component (separate crate); its
            // `style: Signal<String>` prop doesn't accept `Css`
            // directly, so the explicit `.to_string()` stays.
            Video(
                ref: video.r(),
                src: BIG_BUCK_BUNNY_URL,
                style: Css::new().width(100.percent()).height(220.px()).to_string(),
            )
            view(
                style: Css::new()
                    .flex_direction(FlexDirection::Row)
                    .align_items(AlignItems::Center)
                    .padding(8.px())
                    .background_color(Color::hex(0x1A1A1A))
                    .gap(12.px()),
            ) {
                text(value: "▶ Play",  style: btn_style(), on_tap: move |_| { video.play(); })
                text(value: "⏸ Pause", style: btn_style(), on_tap: move |_| { video.pause(); })
                text(value: "+10s",    style: btn_style(), on_tap: move |_| { video.seek(10.0); })
            }
        }
    }
}

#[component]
pub fn measure_demo() -> Element {
    let card = ElementHandle::new();
    let dims = RwSignal::new(String::new());
    let label = computed(move || {
        let d = dims.get();
        if d.is_empty() {
            "tap to measure".to_string()
        } else {
            d
        }
    });
    let on_measure = move |_| {
        spawn_local(async move {
            match card.bounding_client_rect().await {
                Ok(r) => dims.set(format!("{}×{} px", r.width as i32, r.height as i32)),
                Err(e) => dims.set(format!("err: {e}")),
            }
        });
    };
    render! {
        view(
            ref: card.r(),
            on_tap: on_measure,
            style: Css::new()
                .width(200.px())
                .height(56.px())
                .margin((8.px(), 16.px()))
                .background_color(SURFACE)
                .border_radius(8.px())
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::Center),
        ) {
            text(
                value: label,
                style: Css::new()
                    .color(Color::hex(0xB9A9FF))
                    .font_size(14.px())
                    .font_weight(FontWeight::Numeric(600)),
            )
        }
    }
}

#[component]
fn text_methods_demo() -> Element {
    let out = RwSignal::new(String::from("tap the text to measure “Hello” →"));
    let txt = TextHandle::new();
    let display = computed(move || out.get());
    let measure = move |_| {
        spawn_local(async move {
            match txt.get_text_bounding_rect(0, 5).await {
                Ok(r) => out.set(format!(
                    "getTextBoundingRect[0..5] → {:.0}×{:.0} @({:.0},{:.0})  boxes={}",
                    r.bounding_rect.width,
                    r.bounding_rect.height,
                    r.bounding_rect.left,
                    r.bounding_rect.top,
                    r.boxes.len(),
                )),
                Err(e) => out.set(format!("err: {e}")),
            }
        });
    };
    render! {
        view(
            style: Css::new()
                .margin((4.px(), 16.px(), 8.px()))
                .flex_shrink(0.0)
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .gap(4.px()),
        ) {
            text(
                ref: txt.r(),
                on_tap: measure,
                flatten: false,
                value: "Hello Whisker text methods",
                style: Css::new()
                    .color(Color::hex(0xE8E3FF))
                    .font_size(15.px())
                    .font_weight(FontWeight::Numeric(600)),
            )
            text(
                value: display,
                style: Css::new()
                    .color(Color::hex(0xB9A9FF))
                    .font_size(12.px())
                    .font_family("monospace"),
            )
        }
    }
}

#[component]
fn list_demo() -> Element {
    // Used inside the render-props `children` closure on every
    // list item; kept as variables so each row can `.clone()`.
    let card = Css::new()
        .height(52.px())
        .margin((4.px(), 16.px()))
        .border_radius(8.px())
        .background_color(SURFACE)
        .display_flex()
        .flex_direction(FlexDirection::Row)
        .align_items(AlignItems::Center)
        .padding_left(16.px());
    let txt = Css::new()
        .color(Color::hex(0xE8E3FF))
        .font_size(15.px())
        .font_weight(FontWeight::Numeric(600));
    render! {
        view(
            style: Css::new()
                .flex_shrink(0.0)
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .gap(4.px()),
        ) {
            text(
                value: "list (decoupled · virtualized)",
                style: Css::new()
                    .color(Color::hex(0xB9A9FF))
                    .font_size(12.px())
                    .margin((4.px(), 16.px())),
            )
            list(
                each: || (0_i32..10).collect::<::std::vec::Vec<i32>>(),
                key: |i: &i32| *i,
                children: move |i: i32| render! {
                    view(style: card.clone()) {
                        text(value: format!("List item {}", i), style: txt.clone())
                    }
                },
                list_type: "single",
                column_count: 1_i32,
                style: Css::new().height(220.px()),
            )
        }
    }
}

#[component]
fn show_demo() -> Element {
    let visible = RwSignal::new(true);
    let toggle = move |_| visible.update(|v| *v = !*v);
    // The two label styles are used inside the `Show`'s body /
    // fallback closures, so they need to be captured by value
    // (hence `let` + `.clone()` per branch).
    let hidden_lbl = Css::new()
        .color(Color::hex(0x888888))
        .font_size(14.px())
        .padding(12.px())
        .background_color(SURFACE)
        .border_radius(8.px());
    let visible_lbl = Css::new()
        .color(Color::hex(0xE8E3FF))
        .font_size(14.px())
        .padding(12.px())
        .background_color(Color::hex(0x2A1F4A))
        .border_radius(8.px());
    render! {
        view(
            style: Css::new()
                .margin((8.px(), 16.px()))
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .gap(8.px()),
        ) {
            text(
                value: "Show (toggle the condition)",
                style: Css::new().color(Color::hex(0xB9A9FF)).font_size(12.px()),
            )
            text(
                value: "Toggle",
                on_tap: toggle,
                style: Css::new()
                    .padding((8.px(), 16.px()))
                    .background_color(Color::hex(0x6C5CE7))
                    .border_radius(6.px())
                    .color(Color::hex(0xFFFFFF))
                    .font_size(14.px())
                    .font_weight(FontWeight::Numeric(600)),
            )
            Show(
                when: move || visible.get(),
                fallback: move || render! {
                    text(value: "Hidden — flip the toggle", style: hidden_lbl.clone())
                },
            ) {
                text(value: "Visible — the truthy branch is mounted", style: visible_lbl.clone())
            }
        }
    }
}

#[component]
fn for_each_demo() -> Element {
    let count = RwSignal::new(3_usize);
    // Used inside the ForEach `children` closure → kept as a
    // variable so each row can `.clone()`.
    let card = Css::new()
        .padding(10.px())
        .background_color(SURFACE)
        .border_radius(8.px())
        .color(Color::hex(0xE8E3FF))
        .font_size(14.px());
    let item_color = Css::new().color(Color::hex(0xE8E3FF));
    // Used by two buttons (+/-) → closure.
    let btn = || {
        Css::new()
            .padding((8.px(), 16.px()))
            .background_color(Color::hex(0x6C5CE7))
            .border_radius(6.px())
            .color(Color::hex(0xFFFFFF))
            .font_size(14.px())
            .font_weight(FontWeight::Numeric(600))
    };
    render! {
        view(
            style: Css::new()
                .margin((8.px(), 16.px()))
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .gap(8.px()),
        ) {
            text(
                value: "ForEach (reactive item count)",
                style: Css::new().color(Color::hex(0xB9A9FF)).font_size(12.px()),
            )
            view(
                style: Css::new()
                    .display_flex()
                    .flex_direction(FlexDirection::Row)
                    .gap(8.px()),
            ) {
                text(value: "+", style: btn(), on_tap: move |_| count.update(|n| *n += 1))
                text(value: "-", style: btn(), on_tap: move |_| count.update(|n| *n = n.saturating_sub(1)))
            }
            view(
                style: Css::new()
                    .display_flex()
                    .flex_direction(FlexDirection::Column)
                    .gap(6.px()),
            ) {
                ForEach(
                    each: move || (0_usize..count.get()).collect::<::std::vec::Vec<usize>>(),
                    key: |i: &usize| *i,
                    children: move |i: usize| render! {
                        view(style: card.clone()) {
                            text(value: format!("Item {}", i), style: item_color.clone())
                        }
                    },
                )
            }
        }
    }
}

#[component]
fn fragment_demo() -> Element {
    // 3 pills share the same style → closure.
    let pill = || {
        Css::new()
            .padding((6.px(), 12.px()))
            .border_radius(999.px())
            .color(Color::hex(0xFFFFFF))
            .font_size(12.px())
            .font_weight(FontWeight::Numeric(600))
            .background_color(Color::hex(0x6C5CE7))
    };
    render! {
        view(
            style: Css::new()
                .margin((8.px(), 16.px()))
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .gap(8.px()),
        ) {
            text(
                value: "fragment (transparent grouping, no DOM element)",
                style: Css::new().color(Color::hex(0xB9A9FF)).font_size(12.px()),
            )
            view(
                style: Css::new()
                    .display_flex()
                    .flex_direction(FlexDirection::Row)
                    .gap(6.px())
                    .flex_wrap(whisker::css::FlexWrap::Wrap),
            ) {
                fragment {
                    text(value: "A", style: pill())
                    text(value: "B", style: pill())
                    text(value: "C", style: pill())
                }
            }
        }
    }
}

#[component]
fn pill(label: &'static str) -> Element {
    render! {
        text(
            value: label,
            style: Css::new()
                .padding((6.px(), 12.px()))
                .border_radius(999.px())
                .color(Color::hex(0xFFFFFF))
                .font_size(12.px())
                .font_weight(FontWeight::Numeric(600))
                .background_color(Color::hex(0x00B894)),
        )
    }
}

#[component]
fn pill_group(children: Children) -> Element {
    render! {
        view(
            style: Css::new()
                .display_flex()
                .flex_direction(FlexDirection::Row)
                .gap(6.px())
                .flex_wrap(whisker::css::FlexWrap::Wrap)
                .align_items(AlignItems::Center),
        ) {
            text(
                value: "tags:",
                style: Css::new()
                    .color(Color::hex(0xB9A9FF))
                    .font_size(11.px())
                    .margin_right(4.px()),
            )
            children()
        }
    }
}

#[component]
fn children_demo() -> Element {
    render! {
        view(
            style: Css::new()
                .margin((8.px(), 16.px()))
                .padding(12.px())
                .background_color(Color::hex(0x1A1A2E))
                .border_radius(10.px())
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .gap(8.px())
                .flex_shrink(0.0)
                .min_height(130.px()),
        ) {
            text(
                value: "children() slot (user component with a Children prop)",
                style: Css::new()
                    .color(Color::hex(0x00B894))
                    .font_size(13.px())
                    .font_weight(FontWeight::Numeric(600)),
            )
            pill_group {
                Pill(label: "rust")
                Pill(label: "lynx")
                Pill(label: "ios")
            }
            pill_group {
                Pill(label: "android")
                Pill(label: "hot-reload")
            }
        }
    }
}

#[component]
pub fn propagation_demo() -> Element {
    let log = RwSignal::new(String::new());
    let push = move |tag: &'static str| {
        log.update(|s| {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(tag);
        });
    };
    let label = computed(move || {
        let s = log.get();
        if s.is_empty() {
            "tap the inner box →".to_string()
        } else {
            s
        }
    });
    // 3 nested boxes share this layout — closure form.
    let box_style = |c: Color, pad: Length| {
        Css::new()
            .background_color(c)
            .padding(pad)
            .border_radius(10.px())
            .display_flex()
            .flex_direction(FlexDirection::Column)
            .align_items(AlignItems::Center)
            .justify_content(JustifyContent::Center)
    };
    render! {
        view(
            style: Css::new()
                .margin((8.px(), 16.px()))
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .gap(8.px()),
        ) {
            text(
                value: label,
                on_tap: move |_| log.set(String::new()),
                style: Css::new()
                    .color(Color::hex(0xB9A9FF))
                    .font_size(13.px())
                    .font_weight(FontWeight::Numeric(600))
                    .font_family("monospace")
                    .padding(6.px()),
            )
            view(
                style: box_style(Color::hex(0x241946), 20.px()),
                on_capture_tap: move |_| push("\u{2193}outer"),
                on_tap: move |_| push("\u{2191}outer"),
            ) {
                text(
                    value: "outer",
                    style: Css::new()
                        .color(Color::rgba(255, 255, 255, 0.5))
                        .font_size(11.px()),
                )
                view(
                    style: box_style(Color::hex(0x3A2A6B), 20.px()),
                    on_capture_tap: move |_| push("\u{2193}middle"),
                    on_tap: move |_| push("\u{2191}middle"),
                ) {
                    text(
                        value: "middle",
                        style: Css::new()
                            .color(Color::rgba(255, 255, 255, 0.6))
                            .font_size(11.px()),
                    )
                    view(
                        style: box_style(Color::hex(0x5B43A8), 18.px()),
                        on_capture_tap: move |_| push("\u{2193}inner"),
                        on_tap: move |_| push("\u{2191}inner"),
                    ) {
                        text(
                            value: "inner",
                            style: Css::new()
                                .color(Color::Named(NamedColor::White))
                                .font_size(12.px())
                                .font_weight(FontWeight::Numeric(700)),
                        )
                    }
                }
            }
        }
    }
}

#[whisker::main]
fn app() -> Element {
    let state = AppState::new();

    effect(move || {
        let bits = state.liked_mixes.get();
        let _ =
            whisker_local_store::WhiskerLocalStore::save(LIKED_MIXES_KEY.into(), bits.to_string());
    });

    render! {
        page(
            style: Css::new()
                .width(100.vw())
                .height(100.vh())
                .background_color(BG)
                .display_flex()
                .flex_direction(FlexDirection::Column)
                .position(PositionKind::Relative),
        ) {
            // `Hello` is a module component (separate crate); see Video above.
            Hello(style: Css::new().width(100.percent()).height(8.px()).to_string())
            ChildrenDemo()
            VideoDemo()
            MeasureDemo()
            TextMethodsDemo()
            ShowDemo()
            ForEachDemo()
            FragmentDemo()
            ListDemo()
            PropagationDemo()
            Header()
            ScrollBody(state: state)
            NowPlaying(state: state)
            TabBar(state: state)
        }
    }
}
