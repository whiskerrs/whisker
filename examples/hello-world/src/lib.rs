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
//! - Lynx CSS: flex, gradient backgrounds, rounded corners,
//!   shadows, `position: absolute`.

use whisker::prelude::*;
use whisker::runtime::view::Element;

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
        // Phase 7-Φ.E.8 demo: hydrate the heart-bitmask from
        // `WhiskerLocalStore` so liked mixes survive app restarts.
        // The store is registered automatically through the
        // `@WhiskerModule("WhiskerLocalStore")` annotation
        // discovery — no manual registration needed in the user
        // app.
        //
        // The bridge stub on host targets (e.g. `cargo test`)
        // returns `WhiskerValue::Error`; the proxy lifts that into
        // `Err`, which we silently fall back to the default for.
        // Mobile launches go through the real platform-side
        // dispatch so this returns the persisted value.
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

/// Storage key for the heart-bitmask. Single source of truth so a
/// future schema change (versioning, e.g. `liked_mixes_v2`) lands
/// in one place.
const LIKED_MIXES_KEY: &str = "hello_world.liked_mixes";

// ---- Palette / constants ----------------------------------------------------

const BG: &str = "#0f0a1e";
const SURFACE: &str = "#1a1330";
const SURFACE_2: &str = "#241946";
const TEXT_PRIMARY: &str = "#ffffff";
const TEXT_SECONDARY: &str = "rgba(255,255,255,0.65)";
const TEXT_MUTED: &str = "rgba(255,255,255,0.45)";
const ACCENT: &str = "#9b6bff";
const ACCENT_2: &str = "#ff5e9b";

// ---- Building blocks --------------------------------------------------------

#[component]
fn art_tile(c1: &'static str, c2: &'static str, w: &'static str, radius: &'static str) -> Element {
    let style = format!(
        "width: {w}; aspect-ratio: 1; border-radius: {radius}; \
         background-image: linear-gradient(135deg, {c1} 0%, {c2} 100%);"
    );
    render! {
        view(style: style)
    }
}

#[component]
fn chip(label: &'static str, accented: bool) -> Element {
    let (bg, fg) = if accented {
        (ACCENT, TEXT_PRIMARY)
    } else {
        ("rgba(255,255,255,0.08)", TEXT_PRIMARY)
    };
    let style = format!(
        "font-size: 13px; color: {fg}; \
         padding: 8px 16px; background-color: {bg}; \
         border-radius: 999px; margin-right: 8px;"
    );
    render! {
        text(style: style, value: label)
    }
}

#[component]
fn section_header(title: &'static str) -> Element {
    render! {
        view {
            text(
                style: "font-size: 20px; font-weight: 700; color: white;",
                value: title,
            )
            text(
                style: "font-size: 13px; color: rgba(255,255,255,0.5);",
                value: "See all ›",
            )
        }
    }
}

#[component]
fn recent_card(
    title: &'static str,
    sub: &'static str,
    c1: &'static str,
    c2: &'static str,
) -> Element {
    let title_style =
        format!("font-size: 14px; font-weight: 600; color: {TEXT_PRIMARY}; margin-top: 8px;");
    let sub_style = format!("font-size: 12px; color: {TEXT_SECONDARY}; margin-top: 2px;");
    render! {
        view(style: "width: 140px; margin-right: 14px; display: flex; flex-direction: column;") {
            ArtTile(c1: c1, c2: c2, w: "140px", radius: "12px")
            text(style: title_style, value: title)
            text(style: sub_style, value: sub)
        }
    }
}

#[component]
fn grid_tile(
    index: usize,
    title: &'static str,
    c1: &'static str,
    c2: &'static str,
    state: AppState,
) -> Element {
    let bitmask = state.liked_mixes;
    let liked_bit = 1u8 << index;
    let on_heart = move |_| bitmask.update(|b| *b ^= liked_bit);

    // Heart appearance — driven reactively off the bitmask signal.
    // Φ.B removed the render! macro's `move ||` auto-wrap, so a bare
    // call like `value: heart_glyph()` becomes a one-shot snapshot.
    // We route through `computed` so the read sits inside an effect
    // and re-fires when bitmask changes.
    let heart_glyph = computed(move || {
        if bitmask.get() & liked_bit != 0 {
            "♥".to_string()
        } else {
            "♡".to_string()
        }
    });
    let heart_style = computed(move || {
        let color = if bitmask.get() & liked_bit != 0 {
            ACCENT_2
        } else {
            TEXT_MUTED
        };
        format!(
            "position: absolute; top: 8px; right: 8px; \
             width: 28px; height: 28px; border-radius: 14px; \
             background-color: rgba(0,0,0,0.45); color: {color}; \
             font-size: 16px; text-align: center; line-height: 28px;"
        )
    });
    let title_style =
        format!("font-size: 14px; font-weight: 600; color: {TEXT_PRIMARY}; margin-top: 10px;");
    let sub_style = format!("font-size: 11px; color: {TEXT_SECONDARY}; margin-top: 2px;");
    render! {
        view(style: "width: 48%; margin-bottom: 16px; \
                     background-color: #1a1330; border-radius: 14px; \
                     padding: 12px; box-shadow: 0 4px 12px rgba(0,0,0,0.25); \
                     display: flex; flex-direction: column;") {
            view(style: "position: relative; width: 100%;") {
                ArtTile(c1: c1, c2: c2, w: "100%", radius: "10px")
                text(style: heart_style, on_tap: on_heart, value: heart_glyph)
            }
            text(style: title_style, value: title)
            text(style: sub_style, value: "Daily Mix")
        }
    }
}

#[component]
fn activity_row(
    initial: &'static str,
    c1: &'static str,
    c2: &'static str,
    title: &'static str,
    sub: &'static str,
    when: &'static str,
) -> Element {
    let avatar_style = format!(
        "width: 44px; height: 44px; border-radius: 22px; \
         background-image: linear-gradient(135deg, {c1} 0%, {c2} 100%); \
         display: flex; align-items: center; justify-content: center; \
         margin-right: 12px;"
    );
    let title_style = format!("font-size: 15px; color: {TEXT_PRIMARY}; font-weight: 600;");
    let sub_style = format!("font-size: 12px; color: {TEXT_SECONDARY}; margin-top: 2px;");
    let stamp_style = format!("font-size: 11px; color: {TEXT_MUTED};");
    render! {
        view(style: "width: 100%; display: flex; flex-direction: row; align-items: center; \
                     padding: 14px 20px; border-bottom-width: 1px; border-bottom-style: solid; \
                     border-bottom-color: rgba(255,255,255,0.06);") {
            view(style: avatar_style) {
                text(
                    style: "font-size: 18px; color: white; font-weight: 700;",
                    value: initial,
                )
            }
            view(style: "flex-grow: 1; flex-shrink: 1; display: flex; flex-direction: column;") {
                text(style: title_style, value: title)
                text(style: sub_style, value: sub)
            }
            text(style: stamp_style, value: when)
        }
    }
}

#[component]
fn tab_item(index: usize, label: &'static str, glyph: &'static str, state: AppState) -> Element {
    let tab = state.selected_tab;
    let on_pick = move |_| tab.set(index);
    let glyph_style = computed(move || {
        let color = if tab.get() == index {
            ACCENT
        } else {
            TEXT_MUTED
        };
        format!("font-size: 22px; color: {color};")
    });
    let label_style = computed(move || {
        let selected = tab.get() == index;
        let color = if selected { ACCENT } else { TEXT_MUTED };
        let weight = if selected { 700 } else { 500 };
        format!("font-size: 11px; color: {color}; font-weight: {weight};")
    });
    render! {
        view(
            style: "display: flex; flex-direction: column; align-items: center; \
                    gap: 4px; padding: 4px 12px;",
            on_tap: on_pick,
        ) {
            text(style: glyph_style, value: glyph)
            text(style: label_style, value: label)
        }
    }
}

#[component]
fn tab_bar(state: AppState) -> Element {
    let style = format!(
        "position: absolute; left: 0; right: 0; bottom: 0; \
         display: flex; flex-direction: row; justify-content: space-around; \
         padding: 12px 0 28px; background-color: {SURFACE}; \
         border-top-width: 1px; border-top-style: solid; \
         border-top-color: rgba(255,255,255,0.06);"
    );
    render! {
        view(style: style) {
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
    let container_style = format!(
        "position: absolute; left: 12px; right: 12px; bottom: 78px; \
         display: flex; flex-direction: row; align-items: center; \
         padding: 10px; background-color: {SURFACE_2}; \
         border-radius: 14px; box-shadow: 0 6px 16px rgba(0,0,0,0.35);"
    );
    let title_style = format!("font-size: 14px; color: {TEXT_PRIMARY}; font-weight: 600;");
    let sub_style = format!("font-size: 11px; color: {TEXT_SECONDARY}; margin-top: 2px;");
    let btn_style = format!(
        "width: 40px; height: 40px; border-radius: 20px; \
         background-color: {ACCENT}; color: white; \
         font-size: 14px; text-align: center; line-height: 40px;"
    );
    render! {
        view(style: container_style) {
            ArtTile(c1: "#ff7e5f", c2: "#feb47b", w: "48px", radius: "8px")
            view(style: "flex: 1; padding: 0 12px; display: flex; flex-direction: column;") {
                text(style: title_style, value: "Sunset Drive")
                text(style: sub_style, value: status)
            }
            text(style: btn_style, on_tap: toggle, value: glyph)
        }
    }
}

#[component]
fn header() -> Element {
    let bg_style = format!(
        "width: 100%; padding: 60px 20px 18px; \
         background-image: linear-gradient(180deg, #2c1860 0%, {BG} 100%); \
         display: flex; flex-direction: row; align-items: center; \
         justify-content: space-between;"
    );
    let small = format!("font-size: 12px; color: {TEXT_SECONDARY};");
    let big = format!("font-size: 18px; color: {TEXT_PRIMARY}; font-weight: 700;");
    let icon = "width: 40px; height: 40px; border-radius: 20px; \
                background-color: rgba(255,255,255,0.10); \
                color: white; font-size: 16px; text-align: center; line-height: 40px;";
    render! {
        view(style: bg_style) {
            view(style: "display: flex; flex-direction: row; align-items: center;") {
                view(style: "width: 44px; height: 44px; border-radius: 22px; \
                             background-image: linear-gradient(135deg, #ff7e5f 0%, #feb47b 100%); \
                             display: flex; align-items: center; justify-content: center; \
                             margin-right: 12px;") {
                    text(
                        style: "font-size: 18px; color: white; font-weight: 700;",
                        value: "I",
                    )
                }
                view(style: "display: flex; flex-direction: column;") {
                    text(style: small, value: "Welcome back")
                    text(style: big, value: "Itome")
                }
            }
            view(style: "display: flex; flex-direction: row;") {
                text(style: format!("{icon} margin-right: 8px;"), value: "♡")
                text(style: icon, value: "⚙")
            }
        }
    }
}

#[component]
fn chips() -> Element {
    render! {
        view(style: "display: flex; flex-direction: row; padding: 0 20px 8px; flex-wrap: nowrap;") {
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
            style: "padding: 4px 20px 8px; height: 200px;",
        ) {
            RecentCard(title: "Sunset Drive",  sub: "Lo-Fi Beats", c1: "#ff7e5f", c2: "#feb47b")
            RecentCard(title: "Deep Focus",    sub: "Ambient",     c1: "#4facfe", c2: "#00f2fe")
            RecentCard(title: "Late Night",    sub: "Synthwave",   c1: "#9b6bff", c2: "#ff5e9b")
            RecentCard(title: "Coffee House",  sub: "Acoustic",    c1: "#fcb69f", c2: "#ffecd2")
            RecentCard(title: "Energy Boost",  sub: "Workout",     c1: "#11998e", c2: "#38ef7d")
        }
    }
}

#[component]
fn featured() -> Element {
    let cap = format!(
        "font-size: 12px; color: {TEXT_SECONDARY}; \
         text-transform: uppercase; letter-spacing: 1.5px;"
    );
    let title =
        format!("font-size: 26px; font-weight: 700; color: {TEXT_PRIMARY}; margin-top: 6px;");
    let sub = format!("font-size: 13px; color: {TEXT_SECONDARY}; margin-top: 4px;");
    render! {
        view(style: "margin: 0 20px; height: 180px; border-radius: 18px; \
                     background-image: linear-gradient(135deg, #4a00e0 0%, #8e2de2 100%); \
                     padding: 20px; \
                     display: flex; flex-direction: column; justify-content: flex-end; \
                     box-shadow: 0 10px 24px rgba(74, 0, 224, 0.4);") {
            text(style: cap, value: "Made For You")
            text(style: title, value: "Discover Weekly")
            text(style: sub, value: "30 new songs picked just for you")
        }
    }
}

#[component]
fn grid(state: AppState) -> Element {
    render! {
        view(style: "padding: 4px 20px 0; display: flex; flex-direction: row; \
                     flex-wrap: wrap; justify-content: space-between;") {
            GridTile(index: 0_usize, title: "Chill Mix",   c1: "#667eea", c2: "#764ba2", state: state)
            GridTile(index: 1_usize, title: "Happy Mix",   c1: "#f093fb", c2: "#f5576c", state: state)
            GridTile(index: 2_usize, title: "Focus Mix",   c1: "#4facfe", c2: "#00f2fe", state: state)
            GridTile(index: 3_usize, title: "Workout Mix", c1: "#43e97b", c2: "#38f9d7", state: state)
            GridTile(index: 4_usize, title: "Sleep Mix",   c1: "#fa709a", c2: "#fee140", state: state)
            GridTile(index: 5_usize, title: "Indie Mix",   c1: "#30cfd0", c2: "#330867", state: state)
        }
    }
}

#[component]
fn activity_feed() -> Element {
    render! {
        view(style: "display: flex; flex-direction: column; padding: 0 0 8px;") {
            ActivityRow(initial: "A", c1: "#ff7e5f", c2: "#feb47b", title: "Alice", sub: "Started following you",            when: "2m")
            ActivityRow(initial: "R", c1: "#667eea", c2: "#764ba2", title: "Riku",  sub: "Liked your playlist 'Late Night'", when: "1h")
            ActivityRow(initial: "M", c1: "#43e97b", c2: "#38f9d7", title: "Mio",   sub: "Shared 'Sunset Drive' with you",   when: "3h")
            ActivityRow(initial: "K", c1: "#fa709a", c2: "#fee140", title: "Ken",   sub: "Added 5 songs to 'Workout'",       when: "yesterday")
            ActivityRow(initial: "S", c1: "#4facfe", c2: "#00f2fe", title: "Sora",  sub: "Created a new playlist 'Focus'",   when: "2d")
        }
    }
}

/// Phase 6 demo — scroll event payload. A horizontal `scroll_view`
/// whose `on_scroll` reads the typed [`ScrollEvent`] and prints the
/// `detail` fields live, so we can confirm the payload (scroll offset,
/// content width, per-event delta, drag state) actually arrives across
/// the bridge. Lives inside the vertical page scroll — the orthogonal
/// directions don't conflict.
#[component]
fn scroll_card(n: i32, color: &'static str) -> Element {
    let style = format!(
        "width: 96px; height: 56px; flex-shrink: 0; margin-right: 8px; \
         border-radius: 10px; background-color: {color}; \
         display: flex; align-items: center; justify-content: center;"
    );
    render! {
        view(style: style) {
            text(value: format!("{n}"), style: "color: white; font-size: 18px; font-weight: 700;")
        }
    }
}

/// Phase 6 scroll-event readout + the imperative `ScrollViewHandle`
/// methods. The buttons drive the same horizontal `scroll_view` the
/// `on_scroll` reads: `scrollTo` / `scrollBy` (Phase B — params-map
/// dispatch through the bridge) move it programmatically, and
/// `getScrollInfo` (Phase A — async result) reads the offset / range
/// back. Watching the row jump on tap (and the label update) confirms
/// both dispatch paths end-to-end.
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
    let btn = "padding: 6px 10px; background-color: #6c5ce7; border-radius: 6px; \
               color: #fff; font-size: 12px; font-weight: 600;";
    render! {
        view(style: "margin: 4px 20px 8px; display: flex; flex-direction: column; gap: 6px;") {
            text(
                value: label,
                style: "color: #b9a9ff; font-size: 12px; font-family: monospace;",
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
                style: "height: 64px; display: flex; flex-direction: row; \
                        background-color: #1a1330; border-radius: 12px; padding: 4px;",
            ) {
                ScrollCard(n: 1_i32, color: "#667eea")
                ScrollCard(n: 2_i32, color: "#f093fb")
                ScrollCard(n: 3_i32, color: "#4facfe")
                ScrollCard(n: 4_i32, color: "#43e97b")
                ScrollCard(n: 5_i32, color: "#fa709a")
                ScrollCard(n: 6_i32, color: "#30cfd0")
                ScrollCard(n: 7_i32, color: "#ff7e5f")
                ScrollCard(n: 8_i32, color: "#9b6bff")
            }
            view(style: "display: flex; flex-direction: row; flex-wrap: wrap; gap: 8px;") {
                text(value: "→ 300", style: btn, on_tap: move |_| { row.scroll_to(300.0, true); })
                text(value: "⇤ start", style: btn, on_tap: move |_| { row.scroll_to(0.0, true); })
                text(value: "+120", style: btn, on_tap: move |_| { row.scroll_by(120.0); })
                text(value: "▶ auto", style: btn, on_tap: move |_| { row.auto_scroll(120.0); })
                text(value: "■ stop", style: btn, on_tap: move |_| { row.stop_auto_scroll(); })
                text(value: "ℹ info", style: btn, on_tap: move |_| {
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
    let style = format!(
        "flex-grow: 1; flex-shrink: 1; width: 100%; background-color: {BG}; \
         display: flex; flex-direction: column;"
    );
    render! {
        scroll_view(scroll_orientation: "vertical", style: style) {
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
            view(style: "height: 160px;")
        }
    }
}

// ---- Main app ---------------------------------------------------------------

// Phase 7-Φ.F: the `Hello` platform component is sourced from the
// external `whisker-hello-element` module crate (see
// `packages/whisker-hello-element/`). The Whisker module-system
// machinery discovers the crate via cargo metadata; per-package
// SwiftPM target / Gradle subproject builds the platform-side
// registration. The pink bar at the top of this screen is wired
// in through the same path a third-party native-element library
// would use.
//
// Phase 7-Φ.H.2: the actual Lynx tag string is namespaced as
// `whisker-hello-element:Hello` — the `#[whisker::module_component]`
// proc macro auto-prepends `env!("CARGO_PKG_NAME")` on the call
// site, and the SwiftPM build plugin / KSP processor do the same
// on the platform side. From the author's perspective the name
// is just `Hello`; the namespacing prevents collisions between
// unrelated module packages.
//
// `Hello` is the call-site alias; `HelloProps` is what `render!`
// emits via `HelloProps::builder()...build()` for every native-
// element invocation. Both must be in scope at the macro's
// emission site — wildcard import keeps the line short and
// matches the pattern third-party module crates will follow.
use whisker_hello_element::*;
// `Video` (the element for `render!`) + `VideoProps` (the builder
// Props struct `render!` emits) + `VideoHandle` (the typed
// imperative API — `handle.play()`, `handle.seek(10.0)`). The handle
// wraps an `ElementRef`; pass `handle.r()` to the element's `ref:`.
use whisker_video::{Video, VideoHandle, VideoProps};

// Phase 7-Φ.H.2.7 demo — Big Buck Bunny in a Whisker Video
// element, with imperative play/pause/seek dispatched from Rust
// via `ElementRef<VideoProps>`. The video sits at the top of the
// page; the existing hello-world UI sits below it untouched.
//
// A tiny 10s 360p mp4 (~1MB) is enough to see frames inside the
// initial network-fetch window. Larger / longer clips work too —
// AVPlayer's progressive download starts as soon as it has the
// moov atom.
const BIG_BUCK_BUNNY_URL: &str =
    "https://test-videos.co.uk/vids/bigbuckbunny/mp4/h264/360/Big_Buck_Bunny_360_10s_1MB.mp4";

#[component]
pub fn video_demo() -> Element {
    let video = VideoHandle::new();
    let row_style = "flex-direction: row; align-items: center; padding: 8px; \
         background-color: #1a1a1a; gap: 12px;";
    let btn_style = "padding: 8px 16px; background-color: #6c5ce7; \
         border-radius: 6px; color: #fff; font-size: 14px;";
    render! {
        view(style: "flex-direction: column;") {
            Video(
                ref: video.r(),
                src: BIG_BUCK_BUNNY_URL,
                style: "width: 100%; height: 220px;"
            )
            view(style: row_style) {
                text(value: "▶ Play",  style: btn_style, on_tap: move |_| { video.play(); })
                text(value: "⏸ Pause", style: btn_style, on_tap: move |_| { video.pause(); })
                text(value: "+10s",    style: btn_style, on_tap: move |_| { video.seek(10.0); })
            }
        }
    }
}

/// Phase 4 demo — built-in element method invocation. Binds an
/// `ElementRef` to a sized box, then measures it via the async
/// `boundingClientRect` UI method once the box appears on screen
/// (`on_uiappear` fires post-layout, so no tap needed). The result
/// flows back through the binary `WhiskerValueRaw` wire and updates
/// the label reactively.
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
    // Measure on tap — `boundingClientRect` is async (the result
    // arrives via Lynx's UI-method callback after layout), so we
    // `spawn_local` it and update `dims` reactively when it resolves.
    // The result crosses back through the binary `WhiskerValueRaw`
    // wire. Tap-triggered (rather than on-mount) so the platform UI
    // is reliably laid out before we measure on both platforms.
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
            style: "width: 200px; height: 56px; margin: 8px 16px; \
                    background-color: #1a1330; border-radius: 8px; \
                    display: flex; flex-direction: column; \
                    align-items: center; justify-content: center;",
        ) {
            text(
                value: label,
                style: "color: #b9a9ff; font-size: 14px; font-weight: 600;",
            )
        }
    }
}

/// Method-coverage demo — `TextHandle` over the unified `invoke` path.
/// Tap the text to measure its substring `[0, 5)` ("Hello") via
/// `get_text_bounding_rect`, which rides the unified params-map +
/// async-result dispatch (`whisker.8`) — the same path
/// `get_scroll_info` / `get_selected_text` / `bounding_client_rect` now
/// use. The result lands in the readout below.
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
        view(style: "margin: 4px 16px 8px; flex-shrink: 0; display: flex; flex-direction: column; gap: 4px;") {
            text(
                ref: txt.r(),
                on_tap: measure,
                // `getTextBoundingRect` needs a real text Layout on Android
                // (`mTextLayout`); a flattened text has none, so the boxes
                // come back empty. `flatten: false` keeps the text as its
                // own UI so the boxes are extractable on both platforms.
                flatten: false,
                value: "Hello Whisker text methods",
                style: "color: #e8e3ff; font-size: 15px; font-weight: 600;",
            )
            text(
                value: display,
                style: "color: #b9a9ff; font-size: 12px; font-family: monospace;",
            )
        }
    }
}

/// `list` demo — Lynx's virtualized list driven from a static element
/// tree via the *decoupled* native list (`enable-decoupled-list`, set by
/// the `list` builder). The `<list-item>` children are written directly
/// (no JS data source); the native list virtualizes / recycles them.
/// The fixed height makes it scroll internally.
#[component]
fn list_demo() -> Element {
    let card = "height: 52px; margin: 4px 16px; border-radius: 8px; \
                background-color: #1a1330; display: flex; flex-direction: row; \
                align-items: center; padding-left: 16px;";
    let txt = "color: #e8e3ff; font-size: 15px; font-weight: 600;";
    render! {
        view(style: "flex-shrink: 0; display: flex; flex-direction: column; gap: 4px;") {
            text(
                value: "list (decoupled · virtualized)",
                style: "color: #b9a9ff; font-size: 12px; margin: 4px 16px;",
            )
            list(list_type: "single", column_count: 1_i32, style: "height: 220px;") {
                list_item(item_key: "i0", style: card) { text(value: "List item 0", style: txt) }
                list_item(item_key: "i1", style: card) { text(value: "List item 1", style: txt) }
                list_item(item_key: "i2", style: card) { text(value: "List item 2", style: txt) }
                list_item(item_key: "i3", style: card) { text(value: "List item 3", style: txt) }
                list_item(item_key: "i4", style: card) { text(value: "List item 4", style: txt) }
                list_item(item_key: "i5", style: card) { text(value: "List item 5", style: txt) }
                list_item(item_key: "i6", style: card) { text(value: "List item 6", style: txt) }
                list_item(item_key: "i7", style: card) { text(value: "List item 7", style: txt) }
                list_item(item_key: "i8", style: card) { text(value: "List item 8", style: txt) }
                list_item(item_key: "i9", style: card) { text(value: "List item 9", style: txt) }
            }
        }
    }
}

/// Phase 5 demo — event propagation (capture / bubble / catch).
///
/// Three nested boxes (outer → middle → inner) each register **both**
/// a capture-phase and a bubble-phase `tap` handler. Tapping the inner
/// box drives Whisker's reconstructed chain and appends each handler's
/// tag to a reactive log, so the displayed order makes the phases
/// concrete:
///
/// ```text
/// ↓outer ↓middle ↓inner   ↑inner ↑middle ↑outer
/// └────── capture ──────┘ └────── bubble ──────┘
/// ```
///
/// Capture runs root→target, bubble runs target→root — exactly Lynx's
/// model, reconstructed in Rust (`on_capture_tap` ↔ `capture-bindtap`,
/// `on_tap` ↔ `bindtap`). Tapping the log line resets it. Swapping an
/// `on_tap` for `on_tap_catch` on the middle box would stop the bubble
/// at "middle" (no `↑outer`); `on_capture_tap_catch` on the outer box
/// would swallow everything after `↓outer`.
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

    let box_style = |c: &str, pad: &str| {
        format!(
            "background-color: {c}; padding: {pad}; border-radius: 10px; \
             display: flex; flex-direction: column; align-items: center; \
             justify-content: center;"
        )
    };
    render! {
        view(style: "margin: 8px 16px; display: flex; flex-direction: column; gap: 8px;") {
            text(
                value: label,
                on_tap: move |_| log.set(String::new()),
                style: "color: #b9a9ff; font-size: 13px; font-weight: 600; \
                        font-family: monospace; padding: 6px;",
            )
            view(
                style: box_style("#241946", "20px"),
                on_capture_tap: move |_| push("\u{2193}outer"),
                on_tap: move |_| push("\u{2191}outer"),
            ) {
                text(value: "outer", style: "color: rgba(255,255,255,0.5); font-size: 11px;")
                view(
                    style: box_style("#3a2a6b", "20px"),
                    on_capture_tap: move |_| push("\u{2193}middle"),
                    on_tap: move |_| push("\u{2191}middle"),
                ) {
                    text(value: "middle", style: "color: rgba(255,255,255,0.6); font-size: 11px;")
                    view(
                        style: box_style("#5b43a8", "18px"),
                        on_capture_tap: move |_| push("\u{2193}inner"),
                        on_tap: move |_| push("\u{2191}inner"),
                    ) {
                        text(value: "inner", style: "color: white; font-size: 12px; font-weight: 700;")
                    }
                }
            }
        }
    }
}

#[whisker::main]
fn app() -> Element {
    // Allocate every app-wide signal in the bootstrap owner. `AppState`
    // is `Copy`, so threading it through `#[component]` props below
    // doesn't introduce any `move ||` boilerplate.
    let state = AppState::new();

    // Persist the heart bitmask on every change. The effect reads
    // `state.liked_mixes` (subscribing) and writes to the local
    // store so an app restart restores the same toggles.
    effect(move || {
        let bits = state.liked_mixes.get();
        let _ =
            whisker_local_store::WhiskerLocalStore::save(LIKED_MIXES_KEY.into(), bits.to_string());
    });

    let page_style = format!(
        "width: 100vw; height: 100vh; background-color: {BG}; \
         display: flex; flex-direction: column; position: relative;"
    );
    render! {
        page(style: page_style) {
            Hello(style: "width: 100%; height: 8px;")
            VideoDemo()
            MeasureDemo()
            TextMethodsDemo()
            ListDemo()
            PropagationDemo()
            Header()
            ScrollBody(state: state)
            NowPlaying(state: state)
            TabBar(state: state)
        }
    }
}
