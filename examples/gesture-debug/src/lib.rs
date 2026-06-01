//! Touch-event diagnostic app.
//!
//! Each probe card isolates a single variable in the event-binding
//! matrix so it's possible to see, on a real device, exactly which
//! shapes route `on_tap` / `on_touchstart` / `on_touchmove` /
//! `on_touchend` through Lynx into our handlers, and what payload
//! each event carries.
//!
//! Background: the router PR ([#96](https://github.com/whiskerrs/whisker/pull/96))
//! shipped without iOS swipe-back. After patching the iOS bridge to
//! splice `touches` / `changedTouches` / `detail` onto the touch
//! event body (the LynxTouchEvent class doesn't override
//! `generateEventBody`), the cards below validate every shape we
//! might use to drive a gesture: leaf tap, leaf touch lifecycle,
//! mixed handlers, bubble to ancestor, capture on ancestor.

use whisker::event::{Touch, TouchEvent};
use whisker::prelude::*;
use whisker::runtime::view::Element;

fn screen_style() -> &'static str {
    // `padding-top: 60px` clears the iOS notch / status bar so the
    // "GestureDebug" title isn't hidden behind the system clock.
    // Individual padding props (Lynx drops the four-value shorthand).
    "display: flex; flex-direction: column; gap: 12px; \
     padding-top: 60px; padding-bottom: 16px; \
     padding-left: 16px; padding-right: 16px; \
     width: 100%; background-color: white;"
}

fn card_style() -> &'static str {
    "display: flex; flex-direction: column; gap: 4px; padding: 10px; \
     background-color: #f3f4f6; border-radius: 8px;"
}

/// The tap target itself is a `text` element with a button-like
/// style — the shape that reliably routes `on_tap`/`on_touch*` to a
/// handler. (A wrapping `view` works but its child glyphs come out
/// at sub-pixel sizes when the screen content overflows.)
fn zone_text_style() -> &'static str {
    "padding: 18px 24px; background: #fde68a; \
     border-radius: 6px; font-size: 16px; color: black;"
}

/// Inline style for a single details row.
fn detail_row_style() -> &'static str {
    "font-size: 11px;"
}

/// Pull the touch that originated the event — `changedTouches[0]`
/// preferentially, falling back to `touches[0]`, then `Touch::default`.
fn primary_touch(e: &TouchEvent) -> Touch {
    e.changed_touches
        .first()
        .copied()
        .or_else(|| e.touches.first().copied())
        .unwrap_or_default()
}

/// Build the five detail rows for one captured event. The strings
/// are returned in render order: counts, arrays, detail point,
/// target signs, primary touch details.
fn fmt_event_lines(counts: &str, e: &TouchEvent) -> [String; 5] {
    let t = primary_touch(e);
    [
        counts.into(),
        format!(
            "touches: {}  changed: {}",
            e.touches.len(),
            e.changed_touches.len()
        ),
        format!("detail ({:.1}, {:.1})", e.detail.x, e.detail.y),
        format!(
            "target.uid={}  currentTarget.uid={}",
            e.target.uid, e.current_target.uid
        ),
        format!(
            "primary id={}  client=({:.1},{:.1})  page=({:.1},{:.1})",
            t.identifier, t.client_x, t.client_y, t.page_x, t.page_y
        ),
    ]
}

/// Card 1 — `on_tap` only on a leaf `text`.
///
/// Control: matches `hello-world`'s tap-text pattern. Expected:
/// `taps` increments on every tap; the payload shows the tap
/// location.
#[component]
pub fn card_tap_only() -> Element {
    let taps = RwSignal::new(0_u32);
    let lines = RwSignal::new([
        String::from("taps: 0"),
        String::from("touches: 0  changed: 0"),
        String::from("detail (0.0, 0.0)"),
        String::from("target.uid=0  currentTarget.uid=0"),
        String::from("primary id=0  client=(0.0,0.0)  page=(0.0,0.0)"),
    ]);
    let lines_r = lines.read_only();
    let line0 = computed(move || lines_r.get()[0].clone());
    let line1 = computed(move || lines_r.get()[1].clone());
    let line2 = computed(move || lines_r.get()[2].clone());
    let line3 = computed(move || lines_r.get()[3].clone());
    let line4 = computed(move || lines_r.get()[4].clone());
    render! {
        view(style: card_style()) {
            text(style: "font-size: 14px; font-weight: 700;") {
                text(value: "1. on_tap only")
            }
            text(
                style: zone_text_style(),
                value: "tap me",
                on_tap: move |e: TouchEvent| {
                    let next = taps.get() + 1;
                    taps.set(next);
                    lines.set(fmt_event_lines(&format!("taps: {next}"), &e));
                },
            )
            text(style: detail_row_style()) { text(value: line0) }
            text(style: detail_row_style()) { text(value: line1) }
            text(style: detail_row_style()) { text(value: line2) }
            text(style: detail_row_style()) { text(value: line3) }
            text(style: detail_row_style()) { text(value: line4) }
        }
    }
}

/// Card 2 — `on_touchstart` only.
#[component]
pub fn card_touchstart_only() -> Element {
    let starts = RwSignal::new(0_u32);
    let lines = RwSignal::new([
        String::from("starts: 0"),
        String::from("touches: 0  changed: 0"),
        String::from("detail (0.0, 0.0)"),
        String::from("target.uid=0  currentTarget.uid=0"),
        String::from("primary id=0  client=(0.0,0.0)  page=(0.0,0.0)"),
    ]);
    let lines_r = lines.read_only();
    let line0 = computed(move || lines_r.get()[0].clone());
    let line1 = computed(move || lines_r.get()[1].clone());
    let line2 = computed(move || lines_r.get()[2].clone());
    let line3 = computed(move || lines_r.get()[3].clone());
    let line4 = computed(move || lines_r.get()[4].clone());
    render! {
        view(style: card_style()) {
            text(style: "font-size: 14px; font-weight: 700;") {
                text(value: "2. on_touchstart only")
            }
            text(
                style: zone_text_style(),
                value: "touch me",
                on_touchstart: move |e: TouchEvent| {
                    let next = starts.get() + 1;
                    starts.set(next);
                    lines.set(fmt_event_lines(&format!("starts: {next}"), &e));
                },
            )
            text(style: detail_row_style()) { text(value: line0) }
            text(style: detail_row_style()) { text(value: line1) }
            text(style: detail_row_style()) { text(value: line2) }
            text(style: detail_row_style()) { text(value: line3) }
            text(style: detail_row_style()) { text(value: line4) }
        }
    }
}

/// Card 3 — `on_tap` AND `on_touchstart` on the same leaf.
///
/// Each handler tracks its own counter; the details panel reflects
/// whichever event fired most recently (its prefix names the
/// event).
#[component]
pub fn card_tap_plus_touchstart() -> Element {
    let taps = RwSignal::new(0_u32);
    let starts = RwSignal::new(0_u32);
    let lines = RwSignal::new([
        String::from("tap=0  touchstart=0"),
        String::from("touches: 0  changed: 0"),
        String::from("detail (0.0, 0.0)"),
        String::from("target.uid=0  currentTarget.uid=0"),
        String::from("primary id=0  client=(0.0,0.0)  page=(0.0,0.0)"),
    ]);
    let lines_r = lines.read_only();
    let line0 = computed(move || lines_r.get()[0].clone());
    let line1 = computed(move || lines_r.get()[1].clone());
    let line2 = computed(move || lines_r.get()[2].clone());
    let line3 = computed(move || lines_r.get()[3].clone());
    let line4 = computed(move || lines_r.get()[4].clone());
    render! {
        view(style: card_style()) {
            text(style: "font-size: 14px; font-weight: 700;") {
                text(value: "3. on_tap + on_touchstart")
            }
            text(
                style: zone_text_style(),
                value: "tap and touch",
                on_tap: move |e: TouchEvent| {
                    let nt = taps.get() + 1;
                    taps.set(nt);
                    lines.set(fmt_event_lines(
                        &format!("tap=#{nt}  (tap fired)  touchstart={}", starts.get()),
                        &e,
                    ));
                },
                on_touchstart: move |e: TouchEvent| {
                    let ns = starts.get() + 1;
                    starts.set(ns);
                    lines.set(fmt_event_lines(
                        &format!("tap={}  touchstart=#{ns}  (touchstart fired)", taps.get()),
                        &e,
                    ));
                },
            )
            text(style: detail_row_style()) { text(value: line0) }
            text(style: detail_row_style()) { text(value: line1) }
            text(style: detail_row_style()) { text(value: line2) }
            text(style: detail_row_style()) { text(value: line3) }
            text(style: detail_row_style()) { text(value: line4) }
        }
    }
}

/// Card 4 — full touch lifecycle with all detail fields + derived
/// drag metrics (start point, current point, delta, dominant
/// direction). The richest probe; everything else is a subset of
/// this view.
#[component]
pub fn card_touch_lifecycle() -> Element {
    let counts = RwSignal::new((0_u32, 0_u32, 0_u32)); // (start, move, end)
    let lines = RwSignal::new([
        String::from("start: 0  move: 0  end: 0"),
        String::from("touches: 0  changed: 0"),
        String::from("detail (0.0, 0.0)"),
        String::from("target.uid=0  currentTarget.uid=0"),
        String::from("primary id=0  client=(0.0,0.0)  page=(0.0,0.0)"),
    ]);
    // Drag-derived signals: track start point, current point, and
    // delta separately so the dominant direction follows the finger
    // even when touchmove fires many times per second.
    let start_pt = RwSignal::new((0.0_f64, 0.0_f64));
    let cur_pt = RwSignal::new((0.0_f64, 0.0_f64));
    let delta = RwSignal::new((0.0_f64, 0.0_f64));
    let direction = RwSignal::new(String::from("—"));

    let lines_r = lines.read_only();
    let start_pt_r = start_pt.read_only();
    let cur_pt_r = cur_pt.read_only();
    let delta_r = delta.read_only();
    let direction_r = direction.read_only();

    let line0 = computed(move || lines_r.get()[0].clone());
    let line1 = computed(move || lines_r.get()[1].clone());
    let line2 = computed(move || lines_r.get()[2].clone());
    let line3 = computed(move || lines_r.get()[3].clone());
    let line4 = computed(move || lines_r.get()[4].clone());
    let start_lbl = computed(move || {
        let (x, y) = start_pt_r.get();
        format!("start ({x:.1}, {y:.1})")
    });
    let cur_lbl = computed(move || {
        let (x, y) = cur_pt_r.get();
        format!("current ({x:.1}, {y:.1})")
    });
    let delta_lbl = computed(move || {
        let (dx, dy) = delta_r.get();
        format!(
            "delta dx={dx:.1} dy={dy:.1} |Δ|={:.1}",
            (dx * dx + dy * dy).sqrt()
        )
    });
    let dir_lbl = computed(move || format!("dir   {}", direction_r.get()));

    fn classify(dx: f64, dy: f64) -> &'static str {
        // Dead zone of 2pt — below this we keep showing `—` so the
        // direction doesn't flicker on micro-jitter at the start of
        // a touch.
        if dx.abs() < 2.0 && dy.abs() < 2.0 {
            "—"
        } else if dx.abs() >= dy.abs() {
            if dx >= 0.0 {
                "→ right"
            } else {
                "← left"
            }
        } else if dy >= 0.0 {
            "↓ down"
        } else {
            "↑ up"
        }
    }

    render! {
        view(style: card_style()) {
            text(style: "font-size: 14px; font-weight: 700;") {
                text(value: "4. touchstart + touchmove + touchend (detailed)")
            }
            text(
                style: zone_text_style(),
                value: "drag inside this box",
                on_touchstart: move |e: TouchEvent| {
                    let t = primary_touch(&e);
                    let (s, m, en) = counts.get();
                    counts.set((s + 1, m, en));
                    lines.set(fmt_event_lines(
                        &format!("start: {}  move: {}  end: {}  (start fired)", s + 1, m, en),
                        &e,
                    ));
                    start_pt.set((t.client_x, t.client_y));
                    cur_pt.set((t.client_x, t.client_y));
                    delta.set((0.0, 0.0));
                    direction.set(String::from("—"));
                },
                on_touchmove: move |e: TouchEvent| {
                    let t = primary_touch(&e);
                    let (sx, sy) = start_pt.get();
                    let dx = t.client_x - sx;
                    let dy = t.client_y - sy;
                    let (s, m, en) = counts.get();
                    counts.set((s, m + 1, en));
                    lines.set(fmt_event_lines(
                        &format!("start: {}  move: {}  end: {}  (move fired)", s, m + 1, en),
                        &e,
                    ));
                    cur_pt.set((t.client_x, t.client_y));
                    delta.set((dx, dy));
                    direction.set(String::from(classify(dx, dy)));
                },
                on_touchend: move |e: TouchEvent| {
                    let (s, m, en) = counts.get();
                    counts.set((s, m, en + 1));
                    lines.set(fmt_event_lines(
                        &format!("start: {}  move: {}  end: {}  (end fired)", s, m, en + 1),
                        &e,
                    ));
                },
            )
            text(style: detail_row_style()) { text(value: line0) }
            text(style: detail_row_style()) { text(value: line1) }
            text(style: detail_row_style()) { text(value: line2) }
            text(style: detail_row_style()) { text(value: line3) }
            text(style: detail_row_style()) { text(value: line4) }
            text(style: detail_row_style()) { text(value: start_lbl) }
            text(style: detail_row_style()) { text(value: cur_lbl) }
            text(style: detail_row_style()) { text(value: delta_lbl) }
            text(style: detail_row_style()) { text(value: dir_lbl) }
        }
    }
}

/// Card 5 — `on_touchstart` on an ANCESTOR view; touch the inner
/// `text` descendant. Confirms event bubbling.
///
/// `target.uid` should differ from `currentTarget.uid` — the
/// touched element vs the element whose handler is firing.
#[component]
pub fn card_touchstart_on_parent() -> Element {
    let starts = RwSignal::new(0_u32);
    let lines = RwSignal::new([
        String::from("parent starts: 0"),
        String::from("touches: 0  changed: 0"),
        String::from("detail (0.0, 0.0)"),
        String::from("target.uid=0  currentTarget.uid=0"),
        String::from("primary id=0  client=(0.0,0.0)  page=(0.0,0.0)"),
    ]);
    let lines_r = lines.read_only();
    let line0 = computed(move || lines_r.get()[0].clone());
    let line1 = computed(move || lines_r.get()[1].clone());
    let line2 = computed(move || lines_r.get()[2].clone());
    let line3 = computed(move || lines_r.get()[3].clone());
    let line4 = computed(move || lines_r.get()[4].clone());
    render! {
        view(style: card_style()) {
            text(style: "font-size: 14px; font-weight: 700;") {
                text(value: "5. on_touchstart on parent view")
            }
            view(
                style: "padding: 18px 24px; background: #fde68a; border-radius: 6px;",
                on_touchstart: move |e: TouchEvent| {
                    let next = starts.get() + 1;
                    starts.set(next);
                    lines.set(fmt_event_lines(&format!("parent starts: {next}"), &e));
                },
            ) {
                text(style: "font-size: 16px; color: black;", value: "touch the inner text")
            }
            text(style: detail_row_style()) { text(value: line0) }
            text(style: detail_row_style()) { text(value: line1) }
            text(style: detail_row_style()) { text(value: line2) }
            text(style: detail_row_style()) { text(value: line3) }
            text(style: detail_row_style()) { text(value: line4) }
        }
    }
}

/// Card 6 — `on_capture_touchstart` on the parent (capture phase
/// instead of bubble).
///
/// If bubble (card 5) doesn't reach the ancestor but capture does,
/// the bug is in the bubble-phase walk over `parent_sign`.
#[component]
pub fn card_capture_touchstart_on_parent() -> Element {
    let starts = RwSignal::new(0_u32);
    let lines = RwSignal::new([
        String::from("capture starts: 0"),
        String::from("touches: 0  changed: 0"),
        String::from("detail (0.0, 0.0)"),
        String::from("target.uid=0  currentTarget.uid=0"),
        String::from("primary id=0  client=(0.0,0.0)  page=(0.0,0.0)"),
    ]);
    let lines_r = lines.read_only();
    let line0 = computed(move || lines_r.get()[0].clone());
    let line1 = computed(move || lines_r.get()[1].clone());
    let line2 = computed(move || lines_r.get()[2].clone());
    let line3 = computed(move || lines_r.get()[3].clone());
    let line4 = computed(move || lines_r.get()[4].clone());
    render! {
        view(style: card_style()) {
            text(style: "font-size: 14px; font-weight: 700;") {
                text(value: "6. on_capture_touchstart on parent view")
            }
            view(
                style: "padding: 18px 24px; background: #fde68a; border-radius: 6px;",
                on_capture_touchstart: move |e: TouchEvent| {
                    let next = starts.get() + 1;
                    starts.set(next);
                    lines.set(fmt_event_lines(&format!("capture starts: {next}"), &e));
                },
            ) {
                text(style: "font-size: 16px; color: black;", value: "touch the inner text")
            }
            text(style: detail_row_style()) { text(value: line0) }
            text(style: detail_row_style()) { text(value: line1) }
            text(style: detail_row_style()) { text(value: line2) }
            text(style: detail_row_style()) { text(value: line3) }
            text(style: detail_row_style()) { text(value: line4) }
        }
    }
}

/// Top-level layout — stack the cards vertically inside a
/// `scroll_view` so they all fit (each card is now much taller
/// because every event prints its full payload).
#[whisker::main]
pub fn render_app() -> Element {
    render! {
        page(
            style: "width: 100vw; height: 100vh; background-color: white; \
                    display: flex; flex-direction: column;",
        ) {
            scroll_view(
                scroll_orientation: "vertical",
                style: "width: 100%; height: 100%;",
            ) {
                view(style: screen_style()) {
                    text(style: "font-size: 22px; font-weight: 700;") {
                        text(value: "GestureDebug")
                    }
                    CardTapOnly()
                    CardTouchstartOnly()
                    CardTapPlusTouchstart()
                    CardTouchLifecycle()
                    CardTouchstartOnParent()
                    CardCaptureTouchstartOnParent()
                }
            }
        }
    }
}
