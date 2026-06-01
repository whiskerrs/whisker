//! `StackLayout` — push/pop with both screens visible during the
//! transition. Animation specifics are delegated to a
//! [`StackTransition`](crate::StackTransition) implementation; the
//! layout itself is responsible only for mounting / promoting
//! slots, deriving the [`Direction`], and ordering the DOM so the
//! transition's paint-order hint takes effect.
//!
//! The default transition is [`IosSlide`](crate::transitions::IosSlide):
//! horizontal slide with ~30% parallax.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use whisker::css::ext::*;
use whisker::css::{Css, Overflow, PositionKind, ToCss};
use whisker::runtime::element::ElementTag;
use whisker::runtime::reactive::{
    create_owner, dispose_owner, effect, on_mount, with_owner, OwnerId,
};
use whisker::runtime::view::apply::apply_styles;
use whisker::runtime::view::{
    append_child, create_element, insert_child_at, remove_child, set_inline_styles, Element,
};
use whisker::{component, Style};

use crate::outlet::{router, RouteRenderFn};
use crate::route::Route;
use crate::transitions::{Direction, Side, StackTransitionBox};

type Slot = Rc<RefCell<Option<(OwnerId, Element)>>>;

/// Animated stack: holds two slots during a transition and lets a
/// [`StackTransition`] drive their animations.
///
/// The [`RouteStack`](crate::RouteStack) for route type `R` is
/// pulled from context — wrap a [`RouteProvider`](crate::RouteProvider)
/// above the layout so `router::<R>()` finds it.
#[component]
pub fn stack_layout<R: Route>(
    #[prop(default = StackTransitionBox::default())] transition: StackTransitionBox,
    render: RouteRenderFn<R>,
) -> Element {
    let stack = router::<R>();
    let container = create_element(ElementTag::View);
    apply_styles(container, container_css().to_css_string());

    let outgoing: Slot = Rc::new(RefCell::new(None));
    let current: Slot = Rc::new(RefCell::new(None));
    let prev_len = Rc::new(Cell::new(0_usize));
    let first = Rc::new(Cell::new(true));

    let render = render.clone();
    let current_signal = stack.current();
    let stack_signal = stack.stack();
    let transition_for_effect = transition.clone();

    effect(move || {
        let route = current_signal.get();
        let new_len = stack_signal.get().len();
        let old_len = prev_len.get();

        let dir = if first.get() {
            Direction::None
        } else if new_len > old_len {
            Direction::Forward
        } else if new_len < old_len {
            Direction::Backward
        } else {
            Direction::None
        };
        prev_len.set(new_len);
        first.set(false);

        // Tear down any leftover outgoing from a still-running prior
        // transition — disposing now (rather than waiting for an
        // animation-end signal Lynx doesn't yet expose) keeps the
        // hidden-screen count bounded at one between transitions.
        if let Some((owner, wrapper)) = outgoing.borrow_mut().take() {
            remove_child(container, wrapper);
            dispose_owner(owner);
        }

        // Promote previously-current → outgoing (or dispose on snap).
        // Re-apply the wrapper's static style with its new role so
        // direction-dependent decorations (e.g. the iOS dim layer)
        // reflect the role flip before the animation kicks in.
        if let Some((owner, wrapper)) = current.borrow_mut().take() {
            if dir == Direction::None {
                remove_child(container, wrapper);
                dispose_owner(owner);
            } else {
                apply_wrapper_style(
                    wrapper,
                    transition_for_effect.0.as_ref(),
                    Side::Outgoing,
                    dir,
                );
                *outgoing.borrow_mut() = Some((owner, wrapper));
            }
        }

        // Mount new wrapper + user screen. DOM paint order = sibling
        // document order (Lynx ignores explicit `z-index` during
        // transform animations). Insert at index 0 when the
        // transition wants the outgoing on top (iOS pop semantics);
        // append otherwise.
        let new_owner = create_owner(None);
        let wrapper = create_element(ElementTag::View);
        apply_wrapper_style(
            wrapper,
            transition_for_effect.0.as_ref(),
            Side::Incoming,
            dir,
        );
        match transition_for_effect.0.foreground(dir) {
            Side::Outgoing => insert_child_at(container, wrapper, 0),
            Side::Incoming => append_child(container, wrapper),
        }
        let route_for_render = route.clone();
        let render_for_owner = render.clone();
        with_owner(new_owner, || {
            let h = render_for_owner.call(route_for_render);
            append_child(wrapper, h);
        });
        *current.borrow_mut() = Some((new_owner, wrapper));

        if dir == Direction::None {
            return;
        }

        // Hand off to the transition implementation. We wait for
        // `on_mount` so the wrapper has its Lynx `sign` assigned.
        let incoming_wrapper = wrapper;
        let outgoing_for_anim = outgoing.borrow().as_ref().map(|(_, w)| *w);
        let transition_for_mount = transition_for_effect.clone();
        on_mount(move || {
            transition_for_mount
                .0
                .animate(incoming_wrapper, Side::Incoming, dir);
            if let Some(out) = outgoing_for_anim {
                transition_for_mount.0.animate(out, Side::Outgoing, dir);
            }
        });
    });

    container
}

fn container_css() -> Css {
    // `overflow: visible` rather than the Web default — Lynx clips
    // children's `box-shadow` at the parent's bounds by default, so
    // we have to declare visibility explicitly all the way down for
    // [`IosSlide`]'s leading-edge shadow to show through.
    Css::new()
        .position(PositionKind::Relative)
        .width(100.percent())
        .height(100.percent())
        .flex_grow(1.0)
        .overflow(Overflow::Visible)
}

/// Apply the layout's slot positioning plus the transition's
/// per-role decoration to a wrapper.
///
/// `Style::Static` collapses to one `set_inline_styles` write;
/// `Style::Dynamic` registers an effect so the closure re-fires
/// (and the wrapper re-styles) whenever any signal it reads
/// changes — useful for theme-driven decoration.
fn apply_wrapper_style(
    wrapper: Element,
    transition: &dyn crate::transitions::StackTransition,
    side: Side,
    direction: Direction,
) {
    let base = slot_css().to_css_string();
    match transition.slot_style(side, direction) {
        Style::Static(extra) => {
            let combined = if extra.is_empty() {
                base
            } else {
                format!("{base}{extra}")
            };
            set_inline_styles(wrapper, &combined);
        }
        Style::Dynamic(f) => {
            effect(move || {
                let extra = f();
                let combined = if extra.is_empty() {
                    base.clone()
                } else {
                    format!("{base}{extra}")
                };
                set_inline_styles(wrapper, &combined);
            });
        }
    }
}

fn slot_css() -> Css {
    // `overflow: visible` is critical — Lynx clips a child's
    // `box-shadow` at the parent's bounds by default (unlike Web
    // CSS where overflow defaults to `visible`). Without this the
    // leading-edge shadow that `IosSlide` paints stays invisible.
    Css::new()
        .position(PositionKind::Absolute)
        .top(0.px())
        .left(0.px())
        .width(100.percent())
        .height(100.percent())
        .overflow(Overflow::Visible)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transitions::{Fade, Instant, IosSlide, StackTransition, VerticalSlide};

    #[test]
    fn ios_slide_pop_keeps_outgoing_in_front() {
        let t = IosSlide::default();
        assert_eq!(t.foreground(Direction::Backward), Side::Outgoing);
        assert_eq!(t.foreground(Direction::Forward), Side::Incoming);
    }

    #[test]
    fn fade_keeps_incoming_in_front_in_both_directions() {
        let t = Fade::default();
        assert_eq!(t.foreground(Direction::Forward), Side::Incoming);
        assert_eq!(t.foreground(Direction::Backward), Side::Incoming);
    }

    #[test]
    fn instant_is_default_no_op() {
        // Instant has no animation; the assertion is just that
        // wrapping it in a transition box compiles & is Clone.
        let t = StackTransitionBox::new(Instant);
        let _ = t.clone();
    }

    #[test]
    fn vertical_slide_inherits_default_easing() {
        let t = VerticalSlide::default();
        assert_eq!(t.easing, "ease-in-out");
        assert_eq!(t.duration_ms, 320);
    }

    #[test]
    fn container_uses_relative_positioning() {
        let css = container_css().to_css_string();
        assert!(css.contains("position: relative"), "got {css}");
    }
}
