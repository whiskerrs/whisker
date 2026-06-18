//! [`ModalLayout`] — slide-from-bottom modal presentation.
//!
//! On mount the sheet slides up from below the viewport via Lynx's
//! native animator. A scrim sits beneath as a fixed-opacity overlay
//! (no fade animation in v1 — Lynx's animator chokes on same-frame
//! opacity transitions in our wrapper-less setup; the sheet slide is
//! the dominant cue).
//!
//! Unlike [`StackLayout`](crate::StackLayout) the modal is driven by
//! a single route value rather than a stack — used as a leaf of an
//! [`Outlet`](crate::Outlet) or as a sibling of a stack, not as a
//! navigator in its own right.
//!
//! ```ignore
//! ModalLayout(route: AppRoute::ProfileSheet, render: |r| match r {
//!     AppRoute::ProfileSheet => render! { ProfileSheet() },
//!     _ => render! { fragment() },
//! }.into())
//! ```

use std::rc::Rc;

use whisker::css::ext::*;
use whisker::css::{AlignItems, Color, Css, PositionKind, ToCss};
use whisker::runtime::element::ElementTag;
use whisker::runtime::reactive::on_mount;
use whisker::runtime::view::apply::apply_styles;
use whisker::runtime::view::{Element, append_child, create_element};
use whisker::{AnimateOptions, animate_start, component};

use crate::route::Route;

const DEFAULT_DURATION_MS: u32 = 320;
const DEFAULT_EASING: &str = "ease-out";
const SCRIM_RGBA: (u8, u8, u8, f32) = (0, 0, 0, 0.45);

/// Function prop for [`ModalLayout`]: maps a route value to its
/// rendered element.
///
/// Same shape as [`RouteRenderFn`](crate::RouteRenderFn) but kept
/// separate so the prop type is explicit at the call site. Use the
/// [`From`] impl: `(|r: AppRoute| ...).into()`.
#[derive(Clone)]
pub struct ModalRenderFn<R: Route>(pub Rc<dyn Fn(R) -> Element + 'static>);

impl<R: Route> ModalRenderFn<R> {
    /// Invoke the renderer with `route` and return the element.
    pub fn call(&self, route: R) -> Element {
        (self.0)(route)
    }
}

impl<R, F> From<F> for ModalRenderFn<R>
where
    R: Route,
    F: Fn(R) -> Element + 'static,
{
    fn from(f: F) -> Self {
        ModalRenderFn(Rc::new(f))
    }
}

/// Slide-from-bottom modal sheet.
///
/// Renders `route` through `render` inside a sheet that animates
/// up from below the viewport on mount. A dimmed scrim sits behind
/// the sheet. The sheet does not interact with a
/// [`RouteStack`](crate::RouteStack) — callers control presentation
/// by mounting / unmounting `ModalLayout` themselves.
#[component]
pub fn modal_layout<R: Route>(route: R, render: ModalRenderFn<R>) -> Element {
    let container = create_element(ElementTag::View);
    apply_styles(container, container_css().to_css_string());

    let scrim = create_element(ElementTag::View);
    apply_styles(scrim, scrim_css().to_css_string());
    append_child(container, scrim);

    let sheet = create_element(ElementTag::View);
    // First paint at translateY(100%); the on_mount animation
    // slides it to 0.
    apply_styles(sheet, sheet_css_initial().to_css_string());
    append_child(container, sheet);

    let content = render.call(route.clone());
    append_child(sheet, content);

    on_mount(move || {
        let _ = animate_start(
            sheet,
            "whisker-modal-rise",
            &[
                ("0%", &[("transform", "translateY(100%)")]),
                ("100%", &[("transform", "translateY(0%)")]),
            ],
            &AnimateOptions::new()
                .duration_ms(DEFAULT_DURATION_MS)
                .easing(DEFAULT_EASING)
                .fill("forwards"),
        );
    });

    container
}

fn container_css() -> Css {
    Css::new()
        .position(PositionKind::Absolute)
        .top(0.px())
        .left(0.px())
        .right(0.px())
        .bottom(0.px())
        .display_flex()
        .align_items(AlignItems::FlexEnd)
}

fn scrim_css() -> Css {
    let (r, g, b, a) = SCRIM_RGBA;
    Css::new()
        .position(PositionKind::Absolute)
        .top(0.px())
        .left(0.px())
        .right(0.px())
        .bottom(0.px())
        .background_color(Color::rgba(r, g, b, a))
}

fn sheet_css_initial() -> Css {
    use whisker::css::TransformFn;
    Css::new()
        .position(PositionKind::Relative)
        .width(100.percent())
        .transform([TransformFn::TranslateY(100.percent().into())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrim_uses_configured_alpha() {
        let css = scrim_css().to_css_string();
        assert!(css.contains("0.45"), "got {css}");
    }

    #[test]
    fn sheet_initial_pose_is_offscreen_bottom() {
        let css = sheet_css_initial().to_css_string();
        assert!(css.contains("translateY(100%)"), "got {css}");
    }

    #[test]
    fn container_pins_to_viewport_bottom_aligned() {
        let css = container_css().to_css_string();
        assert!(css.contains("position: absolute"));
        assert!(css.contains("align-items: flex-end"));
    }
}
