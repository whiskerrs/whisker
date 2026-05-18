//! Hello World — a music-streaming-style home screen.
//!
//! Migrated to Phase 6.5a (Leptos-style) reactivity. The visual
//! design is preserved, but state lives in `RwSignal`s, every
//! handler updates them via `.update` / `.set`, and the macro is
//! `render!` (effect-driven) rather than `rsx!` (value tree).
//!
//! Exercises a wide slice of the Whisker surface:
//!
//! - `#[whisker::main]` returning `ElementHandle`.
//! - `RwSignal<T>` shared via thread-local lazily-initialised
//!   accessors (the same module-scoped pattern users will pick
//!   up for global app state pre-context).
//! - `render!` with `{expr}` interpolation, `style:` and other
//!   attributes, `on_tap:` handlers.
//! - `Show` for the heart toggle's filled/outline glyph and the
//!   now-playing button's pause/play glyph (state-driven swap).
//! - Lynx CSS: flex, gradient backgrounds, rounded corners,
//!   shadows, `position: absolute`.

use whisker::prelude::*;
use whisker::runtime::view::ElementHandle;

// ---- App state (lazily-initialised thread-local RwSignals) ------------------

fn selected_tab() -> RwSignal<usize> {
    thread_local! {
        static S: std::cell::OnceCell<RwSignal<usize>> = const { std::cell::OnceCell::new() };
    }
    S.with(|c| *c.get_or_init(|| RwSignal::new(0_usize)))
}

fn liked_mixes() -> RwSignal<u8> {
    // Bitmask: bit i set ⇒ mix i is liked.
    thread_local! {
        static S: std::cell::OnceCell<RwSignal<u8>> = const { std::cell::OnceCell::new() };
    }
    S.with(|c| *c.get_or_init(|| RwSignal::new(0b000_010_u8)))
}

fn is_playing() -> RwSignal<bool> {
    thread_local! {
        static S: std::cell::OnceCell<RwSignal<bool>> = const { std::cell::OnceCell::new() };
    }
    S.with(|c| *c.get_or_init(|| RwSignal::new(true)))
}

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
fn art_tile(
    c1: &'static str,
    c2: &'static str,
    w: &'static str,
    radius: &'static str,
) -> ElementHandle {
    let style = format!(
        "width: {w}; aspect-ratio: 1; border-radius: {radius}; \
         background-image: linear-gradient(135deg, {c1} 0%, {c2} 100%);"
    );
    render! {
        view { style: style }
    }
}

#[component]
fn chip(label: &'static str, accented: bool) -> ElementHandle {
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
        text {
            style: style,
            {label}
        }
    }
}

#[component]
fn section_header(title: &'static str) -> ElementHandle {
    render! {
        view {
            style: "display: flex; flex-direction: row; align-items: baseline; \
                    justify-content: space-between; padding: 24px 20px 12px;",
            text {
                style: "font-size: 20px; font-weight: 700; color: white;",
                {title}
            }
            text {
                style: "font-size: 13px; color: rgba(255,255,255,0.5);",
                "See all ›"
            }
        }
    }
}

#[component]
fn recent_card(
    title: &'static str,
    sub: &'static str,
    c1: &'static str,
    c2: &'static str,
) -> ElementHandle {
    let title_style = format!(
        "font-size: 14px; font-weight: 600; color: {TEXT_PRIMARY}; margin-top: 8px;"
    );
    let sub_style = format!("font-size: 12px; color: {TEXT_SECONDARY}; margin-top: 2px;");
    render! {
        view {
            style: "width: 140px; margin-right: 14px; display: flex; flex-direction: column;",
            {art_tile(c1, c2, "140px", "12px")}
            text { style: title_style, {title} }
            text { style: sub_style, {sub} }
        }
    }
}

#[component]
fn grid_tile(
    index: usize,
    title: &'static str,
    c1: &'static str,
    c2: &'static str,
) -> ElementHandle {
    let bitmask = liked_mixes();
    let liked_bit = 1u8 << index;
    let on_heart = move || bitmask.update(|b| *b ^= liked_bit);

    // Heart appearance — driven reactively off the bitmask signal.
    let heart_glyph = move || {
        if bitmask.get() & liked_bit != 0 {
            "♥"
        } else {
            "♡"
        }
    };
    let heart_style = move || {
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
    };
    let title_style = format!(
        "font-size: 14px; font-weight: 600; color: {TEXT_PRIMARY}; margin-top: 10px;"
    );
    let sub_style = format!("font-size: 11px; color: {TEXT_SECONDARY}; margin-top: 2px;");
    render! {
        view {
            style: "width: 48%; margin-bottom: 16px; \
                    background-color: #1a1330; border-radius: 14px; \
                    padding: 12px; box-shadow: 0 4px 12px rgba(0,0,0,0.25); \
                    display: flex; flex-direction: column;",
            view {
                style: "position: relative; width: 100%;",
                {art_tile(c1, c2, "100%", "10px")}
                text {
                    style: { heart_style() },
                    on_tap: on_heart,
                    {heart_glyph()}
                }
            }
            text { style: title_style, {title} }
            text { style: sub_style, "Daily Mix" }
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
) -> ElementHandle {
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
        view {
            style: "width: 100%; display: flex; flex-direction: row; align-items: center; \
                    padding: 14px 20px; border-bottom-width: 1px; border-bottom-style: solid; \
                    border-bottom-color: rgba(255,255,255,0.06);",
            view {
                style: avatar_style,
                text {
                    style: "font-size: 18px; color: white; font-weight: 700;",
                    {initial}
                }
            }
            view {
                style: "flex-grow: 1; flex-shrink: 1; display: flex; flex-direction: column;",
                text { style: title_style, {title} }
                text { style: sub_style, {sub} }
            }
            text { style: stamp_style, {when} }
        }
    }
}

#[component]
fn tab_item(index: usize, label: &'static str, glyph: &'static str) -> ElementHandle {
    let tab = selected_tab();
    let on_pick = move || tab.set(index);
    let glyph_style = move || {
        let color = if tab.get() == index { ACCENT } else { TEXT_MUTED };
        format!("font-size: 22px; color: {color};")
    };
    let label_style = move || {
        let selected = tab.get() == index;
        let color = if selected { ACCENT } else { TEXT_MUTED };
        let weight = if selected { 700 } else { 500 };
        format!("font-size: 11px; color: {color}; font-weight: {weight};")
    };
    render! {
        view {
            style: "display: flex; flex-direction: column; align-items: center; \
                    gap: 4px; padding: 4px 12px;",
            on_tap: on_pick,
            text { style: { glyph_style() }, {glyph} }
            text { style: { label_style() }, {label} }
        }
    }
}

#[component]
fn tab_bar() -> ElementHandle {
    let style = format!(
        "position: absolute; left: 0; right: 0; bottom: 0; \
         display: flex; flex-direction: row; justify-content: space-around; \
         padding: 12px 0 28px; background-color: {SURFACE}; \
         border-top-width: 1px; border-top-style: solid; \
         border-top-color: rgba(255,255,255,0.06);"
    );
    render! {
        view {
            style: style,
            {tab_item(0, "Home", "⌂")}
            {tab_item(1, "Search", "⌕")}
            {tab_item(2, "Library", "♫")}
            {tab_item(3, "Profile", "○")}
        }
    }
}

#[component]
fn now_playing() -> ElementHandle {
    let playing = is_playing();
    let toggle = move || playing.update(|p| *p = !*p);
    let glyph = move || if playing.get() { "▌▌" } else { "▶" };
    let status = move || {
        if playing.get() {
            "Lo-Fi Beats · playing"
        } else {
            "Lo-Fi Beats · paused"
        }
    };
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
        view {
            style: container_style,
            {art_tile("#ff7e5f", "#feb47b", "48px", "8px")}
            view {
                style: "flex: 1; padding: 0 12px; display: flex; flex-direction: column;",
                text { style: title_style, "Sunset Drive" }
                text { style: sub_style, {status()} }
            }
            text {
                style: btn_style,
                on_tap: toggle,
                {glyph()}
            }
        }
    }
}

#[component]
fn header() -> ElementHandle {
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
        view {
            style: bg_style,
            view {
                style: "display: flex; flex-direction: row; align-items: center;",
                view {
                    style: "width: 44px; height: 44px; border-radius: 22px; \
                            background-image: linear-gradient(135deg, #ff7e5f 0%, #feb47b 100%); \
                            display: flex; align-items: center; justify-content: center; \
                            margin-right: 12px;",
                    text { style: "font-size: 18px; color: white; font-weight: 700;", "I" }
                }
                view {
                    style: "display: flex; flex-direction: column;",
                    text { style: small, "Welcome back" }
                    text { style: big, "Itome" }
                }
            }
            view {
                style: "display: flex; flex-direction: row;",
                text {
                    style: format!("{icon} margin-right: 8px;"),
                    "♡"
                }
                text { style: icon, "⚙" }
            }
        }
    }
}

#[component]
fn chips() -> ElementHandle {
    render! {
        view {
            style: "display: flex; flex-direction: row; padding: 0 20px 8px; flex-wrap: nowrap;",
            {chip("All", true)}
            {chip("Music", false)}
            {chip("Podcasts", false)}
            {chip("Audiobooks", false)}
        }
    }
}

#[component]
fn recents() -> ElementHandle {
    render! {
        scroll_view {
            scroll_orientation: "horizontal",
            style: "padding: 4px 20px 8px; height: 200px;",
            {recent_card("Sunset Drive", "Lo-Fi Beats", "#ff7e5f", "#feb47b")}
            {recent_card("Deep Focus", "Ambient", "#4facfe", "#00f2fe")}
            {recent_card("Late Night", "Synthwave", "#9b6bff", "#ff5e9b")}
            {recent_card("Coffee House", "Acoustic", "#fcb69f", "#ffecd2")}
            {recent_card("Energy Boost", "Workout", "#11998e", "#38ef7d")}
        }
    }
}

#[component]
fn featured() -> ElementHandle {
    let cap = format!(
        "font-size: 12px; color: {TEXT_SECONDARY}; \
         text-transform: uppercase; letter-spacing: 1.5px;"
    );
    let title = format!(
        "font-size: 26px; font-weight: 700; color: {TEXT_PRIMARY}; margin-top: 6px;"
    );
    let sub = format!("font-size: 13px; color: {TEXT_SECONDARY}; margin-top: 4px;");
    render! {
        view {
            style: "margin: 0 20px; height: 180px; border-radius: 18px; \
                    background-image: linear-gradient(135deg, #4a00e0 0%, #8e2de2 100%); \
                    padding: 20px; \
                    display: flex; flex-direction: column; justify-content: flex-end; \
                    box-shadow: 0 10px 24px rgba(74, 0, 224, 0.4);",
            text { style: cap, "Made For You" }
            text { style: title, "Discover Weekly" }
            text { style: sub, "30 new songs picked just for you" }
        }
    }
}

#[component]
fn grid() -> ElementHandle {
    render! {
        view {
            style: "padding: 4px 20px 0; display: flex; flex-direction: row; \
                    flex-wrap: wrap; justify-content: space-between;",
            {grid_tile(0, "Chill Mix",   "#667eea", "#764ba2")}
            {grid_tile(1, "Happy Mix",   "#f093fb", "#f5576c")}
            {grid_tile(2, "Focus Mix",   "#4facfe", "#00f2fe")}
            {grid_tile(3, "Workout Mix", "#43e97b", "#38f9d7")}
            {grid_tile(4, "Sleep Mix",   "#fa709a", "#fee140")}
            {grid_tile(5, "Indie Mix",   "#30cfd0", "#330867")}
        }
    }
}

#[component]
fn activity_feed() -> ElementHandle {
    render! {
        view {
            style: "display: flex; flex-direction: column; padding: 0 0 8px;",
            {activity_row("A", "#ff7e5f", "#feb47b", "Alice", "Started following you",            "2m")}
            {activity_row("R", "#667eea", "#764ba2", "Riku",  "Liked your playlist 'Late Night'", "1h")}
            {activity_row("M", "#43e97b", "#38f9d7", "Mio",   "Shared 'Sunset Drive' with you",   "3h")}
            {activity_row("K", "#fa709a", "#fee140", "Ken",   "Added 5 songs to 'Workout'",       "yesterday")}
            {activity_row("S", "#4facfe", "#00f2fe", "Sora",  "Created a new playlist 'Focus'",   "2d")}
        }
    }
}

#[component]
fn scroll_body() -> ElementHandle {
    let style = format!(
        "flex-grow: 1; flex-shrink: 1; width: 100%; background-color: {BG}; \
         display: flex; flex-direction: column;"
    );
    render! {
        scroll_view {
            scroll_orientation: "vertical",
            style: style,
            {chips()}
            {section_header("Recently")}
            {recents()}
            {section_header("Made For You")}
            {featured()}
            {section_header("Your Top Mixes")}
            {grid()}
            {section_header("Activity")}
            {activity_feed()}
            view { style: "height: 160px;" }
        }
    }
}

// ---- Main app ---------------------------------------------------------------

#[whisker::main]
fn app() -> ElementHandle {
    // Pre-touch the signals so their thread-local OnceCells are populated
    // under the bootstrap-time detached owner before any reactive read
    // tries to use them. Without this, the first call inside an effect
    // would init the signal under that effect's owner and tie its
    // lifetime to a transient scope.
    let _ = selected_tab();
    let _ = liked_mixes();
    let _ = is_playing();

    let page_style = format!(
        "width: 100vw; height: 100vh; background-color: {BG}; \
         display: flex; flex-direction: column; position: relative;"
    );
    render! {
        page {
            style: page_style,
            {header()}
            {scroll_body()}
            {now_playing()}
            {tab_bar()}
        }
    }
}
