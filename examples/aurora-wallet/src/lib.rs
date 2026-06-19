//! Aurora Wallet — a single-file Whisker demo.
//!
//! A finance dashboard built entirely in this one `lib.rs`: a gradient
//! balance card, a Day/Week/Month segmented control, a spending bar
//! chart, and a recent-transactions list. No native modules, no bundled
//! assets — just the core runtime.
//!
//! ## Try the hot-reload loop
//!
//! Run it (`whisker run ios` / `whisker run android`), then edit one of
//! the `Tweak me` constants below — the accent colour, the currency, a
//! label — and save. The running app updates in under a second, and the
//! state you'd built up (the selected segment, the hidden/shown balance,
//! your scroll position) survives the patch.

use whisker::css::{AlignItems, Color, Display, FlexDirection, FontWeight, JustifyContent, ToCss};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{Icon, lucide};

// ── Tweak me (great hot-reload targets) ──────────────────────────────
const ACCENT: Color = Color::hex(0x7C5CFF); // brand / interactive accent
const BG: Color = Color::hex(0x0B0B0F); // app background
const SURFACE: Color = Color::hex(0x16161D); // cards & controls
const TEXT: Color = Color::hex(0xF5F5F7); // primary text
const POSITIVE: Color = Color::hex(0x32D74B); // income
const NEGATIVE: Color = Color::hex(0xFF6B6B); // expense
const CURRENCY: &str = "$"; // try "€" / "£" / "¥"
const USER: &str = "Alex"; // greeting name
const CARD_RADIUS: i32 = 24; // balance-card corner radius
const CARD_GRADIENT: &str = "linear-gradient(135deg, #7C5CFF 0%, #4E9BFF 100%)";
// ─────────────────────────────────────────────────────────────────────

fn muted() -> Color {
    Color::rgba(235, 235, 245, 0.55)
}

#[whisker::main]
pub fn app() -> Element {
    // Selected segment (0 = Day, 1 = Week, 2 = Month) and whether the
    // balance figure is masked. Both survive a hot patch.
    let selected = RwSignal::new(1usize);
    let hidden = RwSignal::new(false);

    let balance_text = computed(move || {
        if hidden.get() {
            "••••••••".to_string()
        } else {
            format!("{CURRENCY}12,480.55")
        }
    });
    let eye_icon = computed(move || {
        if hidden.get() {
            lucide::EyeOff
        } else {
            lucide::Eye
        }
        .to_string()
    });
    let delta_text = computed(move || {
        match selected.get() {
            0 => "▲ 0.4%  ·  today",
            2 => "▲ 9.1%  ·  this month",
            _ => "▲ 2.4%  ·  this week",
        }
        .to_string()
    });

    render! {
        // Whisker provides the root `page`; this root `view` fills it
        // (`flex_grow: 1`) and carries the app background + layout.
        view(style: css!(
            flex_grow: 1.0,
            background_color: BG,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
        scroll_view(
            style: css!(
                flex_grow: 1.0,
                width: percent(100),
            ),
            scroll_orientation: ScrollOrientation::Vertical,
            scroll_bar_enable: false,
            bounces: true,
        ) {
            view(style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                padding: px(20),
                padding_top: px(64),
                padding_bottom: px(40),
            )) {
                // ── Header ───────────────────────────────────────────
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    margin_bottom: px(20),
                )) {
                    view(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
                        text(
                            style: css!(color: muted(), font_size: px(13), margin_bottom: px(2)),
                            value: "Good morning",
                        )
                        text(
                            style: css!(color: TEXT, font_size: px(22), font_weight: FontWeight::Bold),
                            value: USER,
                        )
                    }
                    view(style: css!(
                        width: px(40),
                        height: px(40),
                        border_radius: px(20),
                        background_color: SURFACE,
                        display: Display::Flex,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                    )) {
                        Icon(svg: lucide::Settings, color: "#9A9AA2", size: "20")
                    }
                }

                // ── Balance card ─────────────────────────────────────
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    border_radius: px(CARD_RADIUS),
                    padding: px(22),
                    margin_bottom: px(22),
                ).raw("background", CARD_GRADIENT)) {
                    view(style: css!(
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                    )) {
                        text(
                            style: css!(color: Color::rgba(255, 255, 255, 0.85), font_size: px(14)),
                            value: "Total balance",
                        )
                        view(on_tap: move |_| hidden.set(!hidden.get())) {
                            Icon(svg: eye_icon, color: "#FFFFFF", size: "18")
                        }
                    }
                    text(
                        style: css!(
                            color: Color::hex(0xFFFFFF),
                            font_size: px(38),
                            font_weight: FontWeight::Bold,
                            margin_top: px(10),
                        ),
                        value: balance_text,
                    )
                    text(
                        style: css!(color: Color::rgba(255, 255, 255, 0.9), font_size: px(13), margin_top: px(6)),
                        value: delta_text,
                    )
                }

                // ── Segmented control ────────────────────────────────
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    background_color: SURFACE,
                    border_radius: px(14),
                    padding: px(4),
                    margin_bottom: px(24),
                )) {
                    Segment(label: "Day", index: 0usize, selected: selected)
                    Segment(label: "Week", index: 1usize, selected: selected)
                    Segment(label: "Month", index: 2usize, selected: selected)
                }

                // ── Spending bar chart ───────────────────────────────
                text(
                    style: css!(color: TEXT, font_size: px(17), font_weight: FontWeight::Bold, margin_bottom: px(14)),
                    value: "Spending",
                )
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::FlexEnd,
                    justify_content: JustifyContent::SpaceBetween,
                    height: px(76),
                    margin_bottom: px(28),
                )) {
                    Bar(height: 30, active: false)
                    Bar(height: 18, active: false)
                    Bar(height: 58, active: false)
                    Bar(height: 24, active: false)
                    Bar(height: 44, active: false)
                    Bar(height: 70, active: true)
                    Bar(height: 14, active: false)
                }

                // ── Recent transactions ──────────────────────────────
                text(
                    style: css!(color: TEXT, font_size: px(17), font_weight: FontWeight::Bold, margin_bottom: px(10)),
                    value: "Recent",
                )
                Tx(icon: lucide::ShoppingCart, name: "Groceries", sub: "Whole Foods", amount: "42.10", positive: false)
                Tx(icon: lucide::Coffee, name: "Coffee", sub: "Blue Bottle", amount: "5.20", positive: false)
                Tx(icon: lucide::Banknote, name: "Salary", sub: "Acme Inc", amount: "3,200.00", positive: true)
                Tx(icon: lucide::Music, name: "Spotify", sub: "Subscription", amount: "10.99", positive: false)
                Tx(icon: lucide::Car, name: "Ride", sub: "Uber", amount: "18.40", positive: false)
            }
        }
        }
    }
}

/// One pill in the Day/Week/Month segmented control. The selected pill
/// is filled with `ACCENT`; tapping it writes `selected`, which the
/// reactive style below re-reads — so the highlight follows your tap
/// with no manual diffing.
#[component]
fn segment(label: &'static str, index: usize, selected: RwSignal<usize>) -> Element {
    let pill_style = computed(move || {
        let on = selected.get() == index;
        css!(
            flex_grow: 1.0,
            border_radius: px(10),
            padding_top: px(9),
            padding_bottom: px(9),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            background_color: if on { Color::hex(0xFF5CFF) } else { Color::rgba(0, 0, 0, 0.0) },
        )
        .to_css_string()
    });
    let label_style = computed(move || {
        let on = selected.get() == index;
        css!(
            font_size: px(14),
            font_weight: FontWeight::Numeric(600),
            color: if on { Color::hex(0xFFFFFF) } else { muted() },
        )
        .to_css_string()
    });

    render! {
        view(style: pill_style, on_tap: move |_| selected.set(index)) {
            text(style: label_style, value: label)
        }
    }
}

/// A single spending bar. `active` tints it with the accent; the rest
/// are a muted surface tone.
#[component]
fn bar(height: i32, active: bool) -> Element {
    let color = if active {
        ACCENT
    } else {
        Color::rgba(124, 92, 255, 0.25)
    };
    render! {
        view(style: css!(
            width: px(26),
            height: px(height),
            border_radius: px(7),
            background_color: color,
        )) {}
    }
}

/// A recent-transaction row: a coloured icon chip, the merchant name +
/// category, and the signed amount (green for income, red for expense).
#[component]
fn tx(
    icon: &'static str,
    name: &'static str,
    sub: &'static str,
    amount: &'static str,
    positive: bool,
) -> Element {
    let amount_color = if positive { POSITIVE } else { NEGATIVE };
    let amount_text = format!("{}{CURRENCY}{}", if positive { "+" } else { "-" }, amount);
    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            background_color: SURFACE,
            border_radius: px(16),
            padding: px(12),
            margin_bottom: px(10),
        )) {
            view(style: css!(
                width: px(40),
                height: px(40),
                border_radius: px(12),
                background_color: Color::rgba(124, 92, 255, 0.18),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                margin_right: px(12),
            )) {
                Icon(svg: icon, color: "#B9A3FF", size: "20")
            }
            view(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column, flex_grow: 1.0)) {
                text(style: css!(color: TEXT, font_size: px(15), font_weight: FontWeight::Numeric(600)), value: name)
                text(style: css!(color: muted(), font_size: px(12), margin_top: px(2)), value: sub)
            }
            text(
                style: css!(color: amount_color, font_size: px(15), font_weight: FontWeight::Numeric(600)),
                value: amount_text,
            )
        }
    }
}
