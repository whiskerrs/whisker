//! Hello World — a music-streaming-style home screen.
//!
//! Exercises a wide slice of Lynx CSS (flexbox / gap / gradient /
//! border-radius / box-shadow / position absolute / aspect-ratio /
//! transform) and the Whisker reactive runtime: tab selection, per-card
//! like state, and a play/pause toggle each re-render only their
//! slice of the tree.

use whisker::prelude::*;

// ---- App state (thread-local signals) ----------------------------------------

fn selected_tab() -> Signal<usize> {
    thread_local! {
        static S: std::cell::OnceCell<Signal<usize>> = const { std::cell::OnceCell::new() };
    }
    S.with(|c| *c.get_or_init(|| use_signal(|| 0_usize)))
}

fn liked_mixes() -> Signal<u8> {
    // Bitmask: 6 mixes, bit i set = liked. Cheap state, easy diff.
    thread_local! {
        static S: std::cell::OnceCell<Signal<u8>> = const { std::cell::OnceCell::new() };
    }
    S.with(|c| *c.get_or_init(|| use_signal(|| 0b000_010_u8)))
}

fn is_playing() -> Signal<bool> {
    thread_local! {
        static S: std::cell::OnceCell<Signal<bool>> = const { std::cell::OnceCell::new() };
    }
    S.with(|c| *c.get_or_init(|| use_signal(|| true)))
}

// ---- Palette / constants -----------------------------------------------------

const BG: &str = "#0f0a1e";
const SURFACE: &str = "#1a1330";
const SURFACE_2: &str = "#241946";
const TEXT_PRIMARY: &str = "#ffffff";
const TEXT_SECONDARY: &str = "rgba(255,255,255,0.65)";
const TEXT_MUTED: &str = "rgba(255,255,255,0.45)";
const ACCENT: &str = "#9b6bff";
const ACCENT_2: &str = "#ff5e9b";

// ---- Building blocks ---------------------------------------------------------

/// Square "album-art" stand-in: gradient + rounded corner. We don't ship
/// real images, but a gradient block carries enough visual structure for
/// the layout to make sense.
fn art_tile(c1: &str, c2: &str, w: &str, radius: &str) -> Element {
    view().style(format!(
        "width: {w}; aspect-ratio: 1; border-radius: {radius}; \
         background-image: linear-gradient(135deg, {c1} 0%, {c2} 100%);"
    ))
}

/// Pill-shaped chip (used as a category filter row).
fn chip(label: &str, accented: bool) -> Element {
    let (bg, fg) = if accented {
        (ACCENT, TEXT_PRIMARY)
    } else {
        ("rgba(255,255,255,0.08)", TEXT_PRIMARY)
    };
    text()
        .style(format!(
            "font-size: 13px; color: {fg}; \
             padding: 8px 16px; background-color: {bg}; \
             border-radius: 999px; margin-right: 8px;"
        ))
        .child(raw_text(label))
}

/// Section header: bold title + chevron-ish text.
fn section_header(title: &str) -> Element {
    rsx! {
        view {
            style: "display: flex; flex-direction: row; align-items: baseline; \
                    justify-content: space-between; \
                    padding: 24px 20px 12px;",
            text {
                style: "font-size: 20px; font-weight: 700; color: white;",
                { title }
            }
            text {
                style: "font-size: 13px; color: rgba(255,255,255,0.5);",
                "See all ›"
            }
        }
    }
}

/// Horizontally-scrolled "Recently played" carousel card.
fn recent_card(title: &str, sub: &str, c1: &str, c2: &str) -> Element {
    rsx! {
        view {
            style: "width: 140px; margin-right: 14px; display: flex; \
                    flex-direction: column;",
        }
    }
    .child(art_tile(c1, c2, "140px", "12px"))
    .child(
        text()
            .style(format!(
                "font-size: 14px; font-weight: 600; color: {TEXT_PRIMARY}; \
                 margin-top: 8px;"
            ))
            .child(raw_text(title)),
    )
    .child(
        text()
            .style(format!(
                "font-size: 12px; color: {TEXT_SECONDARY}; margin-top: 2px;"
            ))
            .child(raw_text(sub)),
    )
}

/// 2-column grid tile with a heart toggle.
fn grid_tile(index: usize, title: &str, c1: &str, c2: &str) -> Element {
    let liked_bit = 1u8 << index;
    let bitmask = liked_mixes();
    let liked = bitmask.get() & liked_bit != 0;
    let on_heart = move || bitmask.set(bitmask.get() ^ liked_bit);

    let heart_glyph = if liked { "♥" } else { "♡" };
    let heart_color = if liked { ACCENT_2 } else { TEXT_MUTED };

    rsx! {
        view {
            style: "width: 48%; margin-bottom: 16px; \
                    background-color: #1a1330; border-radius: 14px; \
                    padding: 12px; box-shadow: 0 4px 12px rgba(0,0,0,0.25); \
                    display: flex; flex-direction: column;",
            view {
                style: "position: relative; width: 100%;",
            }
        }
    }
    .child({
        // Build the tile body imperatively so we can pin a heart overlay
        // with `position: absolute`.
        let mut shell = view().style(
            "width: 48%; margin-bottom: 16px; \
             background-color: #1a1330; border-radius: 14px; \
             padding: 12px; box-shadow: 0 4px 12px rgba(0,0,0,0.25); \
             display: flex; flex-direction: column;",
        );
        let art_box = view()
            .style("position: relative; width: 100%;")
            .child(art_tile(c1, c2, "100%", "10px"))
            .child(
                text()
                    .style(format!(
                        "position: absolute; top: 8px; right: 8px; \
                         width: 28px; height: 28px; border-radius: 14px; \
                         background-color: rgba(0,0,0,0.45); color: {heart_color}; \
                         font-size: 16px; text-align: center; line-height: 28px;"
                    ))
                    .on("tap", on_heart)
                    .child(raw_text(heart_glyph)),
            );
        let title_el = text()
            .style(format!(
                "font-size: 14px; font-weight: 600; color: {TEXT_PRIMARY}; \
                 margin-top: 10px;"
            ))
            .child(raw_text(title));
        let sub = text()
            .style(format!(
                "font-size: 11px; color: {TEXT_SECONDARY}; margin-top: 2px;"
            ))
            .child(raw_text("Daily Mix"));
        shell = shell.child(art_box).child(title_el).child(sub);
        shell
    })
}

/// A list row with circular avatar + title + secondary text + a small
/// timestamp on the right. Written with the builder API rather than
/// rsx! so the column inside fills correctly with explicit widths
/// (Lynx's flex-shrink behaviour with `{title}` Node::Expr children
/// inside rsx was producing 0-height rows).
fn activity_row(initial: &str, c1: &str, c2: &str, title: &str, sub: &str, when: &str) -> Element {
    let avatar = view()
        .style(format!(
            "width: 44px; height: 44px; border-radius: 22px; \
             background-image: linear-gradient(135deg, {c1} 0%, {c2} 100%); \
             display: flex; align-items: center; justify-content: center; \
             margin-right: 12px;"
        ))
        .child(
            text()
                .style("font-size: 18px; color: white; font-weight: 700;")
                .child(raw_text(initial)),
        );

    let body = view()
        .style("flex-grow: 1; flex-shrink: 1; display: flex; flex-direction: column;")
        .child(
            text()
                .style(format!(
                    "font-size: 15px; color: {TEXT_PRIMARY}; font-weight: 600;"
                ))
                .child(raw_text(title)),
        )
        .child(
            text()
                .style(format!(
                    "font-size: 12px; color: {TEXT_SECONDARY}; margin-top: 2px;"
                ))
                .child(raw_text(sub)),
        );

    let stamp = text()
        .style(format!("font-size: 11px; color: {TEXT_MUTED};"))
        .child(raw_text(when));

    view()
        .style(
            "width: 100%; display: flex; flex-direction: row; align-items: center; \
             padding: 14px 20px; \
             border-bottom-width: 1px; border-bottom-style: solid; \
             border-bottom-color: rgba(255,255,255,0.06);",
        )
        .child(avatar)
        .child(body)
        .child(stamp)
}

/// Sticky bottom tab bar — 4 items, the selected one is filled.
fn tab_bar() -> Element {
    let tab = selected_tab();
    let labels = ["Home", "Search", "Library", "Profile"];
    let glyphs = ["⌂", "⌕", "♫", "○"];

    let mut bar = view().style(format!(
        "position: absolute; left: 0; right: 0; bottom: 0; \
         display: flex; flex-direction: row; justify-content: space-around; \
         padding: 12px 0 28px; background-color: {SURFACE}; \
         border-top-width: 1px; border-top-style: solid; \
         border-top-color: rgba(255,255,255,0.06);"
    ));
    for (i, (label, glyph)) in labels.iter().zip(glyphs.iter()).enumerate() {
        let selected = tab.get() == i;
        let color = if selected { ACCENT } else { TEXT_MUTED };
        let weight = if selected { 700 } else { 500 };
        let on_pick = move || tab.set(i);

        let item = view()
            .style(
                "display: flex; flex-direction: column; align-items: center; gap: 4px; \
                 padding: 4px 12px;",
            )
            .on("tap", on_pick)
            .child(
                text()
                    .style(format!("font-size: 22px; color: {color};"))
                    .child(raw_text(*glyph)),
            )
            .child(
                text()
                    .style(format!(
                        "font-size: 11px; color: {color}; font-weight: {weight};"
                    ))
                    .child(raw_text(*label)),
            );
        bar = bar.child(item);
    }
    bar
}

/// Floating "Now Playing" mini-player, just above the tab bar.
fn now_playing() -> Element {
    let playing = is_playing();
    let toggle = move || playing.set(!playing.get());
    let glyph = if playing.get() { "▌▌" } else { "▶" };

    rsx! {
        view {
            style: { format!(
                "position: absolute; left: 12px; right: 12px; bottom: 78px; \
                 display: flex; flex-direction: row; align-items: center; \
                 padding: 10px; background-color: {SURFACE_2}; \
                 border-radius: 14px; \
                 box-shadow: 0 6px 16px rgba(0,0,0,0.35);"
            ) },
        }
    }
    .child(art_tile("#ff7e5f", "#feb47b", "48px", "8px"))
    .child(
        view()
            .style("flex: 1; padding: 0 12px; display: flex; flex-direction: column;")
            .child(
                text()
                    .style(format!(
                        "font-size: 14px; color: {TEXT_PRIMARY}; font-weight: 600;"
                    ))
                    .child(raw_text("Sunset Drive")),
            )
            .child(
                text()
                    .style(format!(
                        "font-size: 11px; color: {TEXT_SECONDARY}; margin-top: 2px;"
                    ))
                    .child(raw_text(if is_playing().get() {
                        "Lo-Fi Beats · playing"
                    } else {
                        "Lo-Fi Beats · paused"
                    })),
            ),
    )
    .child(
        text()
            .style(format!(
                "width: 40px; height: 40px; border-radius: 20px; \
                 background-color: {ACCENT}; color: white; \
                 font-size: 14px; text-align: center; line-height: 40px;"
            ))
            .on("tap", toggle)
            .child(raw_text(glyph)),
    )
}

// ---- Main app ----------------------------------------------------------------

#[whisker::main]
fn app() -> Element {
    let _ = selected_tab();
    let _ = liked_mixes();
    let _ = is_playing();

    // Section 1: horizontal carousel of "recently played" cards.
    let recents = scroll_view()
        .attr("scroll-orientation", "horizontal")
        .style("padding: 4px 20px 8px; height: 200px;")
        .child(recent_card(
            "Sunset Drive",
            "Lo-Fi Beats",
            "#ff7e5f",
            "#feb47b",
        ))
        .child(recent_card("Deep Focus", "Ambient", "#4facfe", "#00f2fe"))
        .child(recent_card("Late Night", "Synthwave", "#9b6bff", "#ff5e9b"))
        .child(recent_card(
            "Coffee House",
            "Acoustic",
            "#fcb69f",
            "#ffecd2",
        ))
        .child(recent_card("Energy Boost", "Workout", "#11998e", "#38ef7d"));

    // Section 2: "Made For You" — one tall featured card with overlay text.
    let featured = view()
        .style(
            "margin: 0 20px; height: 180px; border-radius: 18px; \
             background-image: linear-gradient(135deg, #4a00e0 0%, #8e2de2 100%); \
             padding: 20px; \
             display: flex; flex-direction: column; justify-content: flex-end; \
             box-shadow: 0 10px 24px rgba(74, 0, 224, 0.4);",
        )
        .child(
            text()
                .style(format!(
                    "font-size: 12px; color: {TEXT_SECONDARY}; \
                     text-transform: uppercase; letter-spacing: 1.5px;"
                ))
                .child(raw_text("Made For You")),
        )
        .child(
            text()
                .style(format!(
                    "font-size: 26px; font-weight: 700; color: {TEXT_PRIMARY}; \
                     margin-top: 6px;"
                ))
                .child(raw_text("Discover Weekly")),
        )
        .child(
            text()
                .style(format!(
                    "font-size: 13px; color: {TEXT_SECONDARY}; margin-top: 4px;"
                ))
                .child(raw_text("30 new songs picked just for you")),
        );

    // Section 3: 2-column grid of "Your Top Mixes".
    let grid_titles = [
        "Chill Mix",
        "Happy Mix",
        "Focus Mix",
        "Workout Mix",
        "Sleep Mix",
        "Indie Mix",
    ];
    let grid_colors: [(&str, &str); 6] = [
        ("#667eea", "#764ba2"),
        ("#f093fb", "#f5576c"),
        ("#4facfe", "#00f2fe"),
        ("#43e97b", "#38f9d7"),
        ("#fa709a", "#fee140"),
        ("#30cfd0", "#330867"),
    ];
    let mut grid = view().style(
        "padding: 4px 20px 0; display: flex; flex-direction: row; \
         flex-wrap: wrap; justify-content: space-between;",
    );
    for (i, (title, (c1, c2))) in grid_titles.iter().zip(grid_colors.iter()).enumerate() {
        grid = grid.child(grid_tile(i, title, c1, c2));
    }

    // Section 4: vertical activity feed.
    let activity = view()
        .style("display: flex; flex-direction: column; padding: 0 0 8px;")
        .child(activity_row(
            "A",
            "#ff7e5f",
            "#feb47b",
            "Alice",
            "Started following you",
            "2m",
        ))
        .child(activity_row(
            "R",
            "#667eea",
            "#764ba2",
            "Riku",
            "Liked your playlist 'Late Night'",
            "1h",
        ))
        .child(activity_row(
            "M",
            "#43e97b",
            "#38f9d7",
            "Mio",
            "Shared 'Sunset Drive' with you",
            "3h",
        ))
        .child(activity_row(
            "K",
            "#fa709a",
            "#fee140",
            "Ken",
            "Added 5 songs to 'Workout'",
            "yesterday",
        ))
        .child(activity_row(
            "S",
            "#4facfe",
            "#00f2fe",
            "Sora",
            "Created a new playlist 'Focus'",
            "2d",
        ));

    // Filter chips strip just below the header.
    let chips = view()
        .style(
            "display: flex; flex-direction: row; padding: 0 20px 8px; \
             flex-wrap: nowrap;",
        )
        .child(chip("All", true))
        .child(chip("Music", false))
        .child(chip("Podcasts", false))
        .child(chip("Audiobooks", false));

    // Header: gradient strip with avatar / greeting / icon buttons.
    let header = rsx! {
        view {
            style: { format!(
                "width: 100%; padding: 60px 20px 18px; \
                 background-image: linear-gradient(180deg, #2c1860 0%, {BG} 100%); \
                 display: flex; flex-direction: row; align-items: center; \
                 justify-content: space-between;"
            ) },
            view {
                style: "display: flex; flex-direction: row; align-items: center;",
                view {
                    style: "width: 44px; height: 44px; border-radius: 22px; \
                            background-image: linear-gradient(135deg, #ff7e5f 0%, #feb47b 100%); \
                            display: flex; align-items: center; justify-content: center; \
                            margin-right: 12px;",
                    text {
                        style: "font-size: 18px; color: white; font-weight: 700;",
                        "I"
                    }
                }
                view {
                    style: "display: flex; flex-direction: column;",
                    text {
                        style: format!("font-size: 12px; color: {TEXT_SECONDARY};"),
                        "Welcome back"
                    }
                    text {
                        style: format!("font-size: 18px; color: {TEXT_PRIMARY}; font-weight: 700;"),
                        "Itome"
                    }
                }
            }
            view {
                style: "display: flex; flex-direction: row;",
                text {
                    style: "width: 40px; height: 40px; border-radius: 20px; \
                            background-color: rgba(255,255,255,0.10); \
                            color: white; font-size: 16px; text-align: center; \
                            line-height: 40px; margin-right: 8px;",
                    "♡"
                }
                text {
                    style: "width: 40px; height: 40px; border-radius: 20px; \
                            background-color: rgba(255,255,255,0.10); \
                            color: white; font-size: 16px; text-align: center; \
                            line-height: 40px;",
                    "⚙"
                }
            }
        }
    };

    let scroll_body = scroll_view()
        .attr("scroll-orientation", "vertical")
        .style(format!(
            "flex-grow: 1; flex-shrink: 1; width: 100%; background-color: {BG}; \
             display: flex; flex-direction: column;"
        ))
        .child(chips)
        .child(section_header("Recently"))
        .child(recents)
        .child(section_header("Made For You"))
        .child(featured)
        .child(section_header("Your Top Mixes"))
        .child(grid)
        .child(section_header("Activity"))
        .child(activity)
        // Bottom spacer so the Now Playing pill + tab bar don't cover content.
        .child(view().style("height: 160px;"));

    rsx! {
        page {
            style: { format!(
                "width: 100vw; height: 100vh; background-color: {BG}; \
                 display: flex; flex-direction: column; position: relative;"
            ) },
        }
    }
    .child(header)
    .child(scroll_body)
    .child(now_playing())
    .child(tab_bar())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_returns_a_page() {
        whisker::runtime::signal::__reset_runtime();
        let tree = app();
        assert_eq!(tree.tag, ElementTag::Page);
        // header + scroll_view + now_playing + tab_bar
        assert_eq!(tree.children.len(), 4);
    }

    #[test]
    fn tab_bar_has_four_items() {
        whisker::runtime::signal::__reset_runtime();
        let tree = app();
        let bar = tree.children.last().unwrap();
        assert_eq!(bar.children.len(), 4);
    }
}
