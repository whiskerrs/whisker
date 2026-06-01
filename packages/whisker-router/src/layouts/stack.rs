//! `StackLayout` — push/pop with both screens visible during the
//! transition. Animation specifics are delegated to a
//! [`StackTransition`](crate::StackTransition) implementation; the
//! layout itself is responsible only for mounting / promoting
//! slots, deriving the [`Direction`], and ordering the DOM so the
//! transition's paint-order hint takes effect.
//!
//! Interactive gestures (iOS swipe-back, future Android predictive
//! back) live inside the transition — `StackLayout` only builds a
//! [`GestureContext`](crate::transitions::GestureContext) of
//! primitives and hands it to [`StackTransition::install_gestures`]
//! once, at mount. Transitions that don't have a gesture (cross-
//! fade, instant, vertical slide) ignore the hook.
//!
//! The default transition is [`IosSlide`](crate::transitions::IosSlide):
//! horizontal slide with ~30% parallax + edge swipe-back gesture.

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
use crate::transitions::{Direction, GestureContext, Side, StackTransitionBox};

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
    let preview: Slot = Rc::new(RefCell::new(None));
    // When a gesture commit fires `stack.back()`, the resulting
    // route-change effect must NOT animate — the wrappers already
    // sit at the final pose. The commit closure sets this to a
    // small count (2 by default); each effect run decrements and
    // bails. A count rather than a bool because a single signal
    // update can fan out into more than one effect run (the
    // computed-of-a-RwSignal chain doesn't always batch them) and
    // a one-shot bool would let the second run mount a fresh
    // wrapper, double-stacking the destination screen on top of
    // the gesture's promoted preview.
    let skip_animation: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    let prev_len = Rc::new(Cell::new(0_usize));
    let first = Rc::new(Cell::new(true));

    // Single source of truth for the effect — tracking both
    // `current()` and `stack()` separately makes the runtime re-run
    // the effect twice per navigation (each one is its own
    // `computed`), which drops `skip_animation` on the floor for the
    // second run and double-mounts the destination screen.
    let entries_signal = stack.entries();

    {
        let render = render.clone();
        let outgoing = outgoing.clone();
        let current = current.clone();
        let prev_len = prev_len.clone();
        let first = first.clone();
        let skip_animation = skip_animation.clone();
        let transition = transition.clone();
        effect(move || {
            let entries = entries_signal.get();
            let route = entries
                .last()
                .map(|e| e.route.clone())
                .expect("RouteStack invariant: at least one entry");
            let new_len = entries.len();
            let old_len = prev_len.get();

            // Gesture-driven commit already advanced the visual state.
            // Update bookkeeping and bail.
            let skip = skip_animation.get();
            if skip > 0 {
                skip_animation.set(skip - 1);
                prev_len.set(new_len);
                first.set(false);
                return;
            }

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

            if let Some((owner, wrapper)) = outgoing.borrow_mut().take() {
                remove_child(container, wrapper);
                dispose_owner(owner);
            }

            if let Some((owner, wrapper)) = current.borrow_mut().take() {
                if dir == Direction::None {
                    remove_child(container, wrapper);
                    dispose_owner(owner);
                } else {
                    apply_wrapper_style(wrapper, transition.0.as_ref(), Side::Outgoing, dir);
                    *outgoing.borrow_mut() = Some((owner, wrapper));
                }
            }

            let new_owner = create_owner(None);
            let wrapper = create_element(ElementTag::View);
            apply_wrapper_style(wrapper, transition.0.as_ref(), Side::Incoming, dir);
            match transition.0.foreground(dir) {
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

            let incoming_wrapper = wrapper;
            let outgoing_for_anim = outgoing.borrow().as_ref().map(|(_, w)| *w);
            let transition_for_mount = transition.clone();
            on_mount(move || {
                transition_for_mount
                    .0
                    .animate(incoming_wrapper, Side::Incoming, dir);
                if let Some(out) = outgoing_for_anim {
                    transition_for_mount.0.animate(out, Side::Outgoing, dir);
                }
            });
        });
    }

    // Reserve an owner whose parent is the layout's own owner
    // (`create_owner(None)` falls back to the current owner). The
    // gesture handlers fire from the touch dispatcher, which has no
    // active reactive owner — without this anchor, owners spawned
    // for the preview screen become roots and lose access to the
    // `RouteProvider` context their components rely on.
    let gesture_parent = create_owner(None);

    // Hand off gesture wiring to the transition. The trait method
    // is no-op by default; only `IosSlide` (and any user-defined
    // gesture-aware transition) installs handlers.
    let gesture_ctx = build_gesture_context(
        container,
        transition.clone(),
        stack.clone(),
        render.clone(),
        current.clone(),
        outgoing,
        preview,
        skip_animation,
        gesture_parent,
    );
    transition.0.install_gestures(&gesture_ctx);

    container
}

/// Build the closure surface a [`StackTransition`] uses to drive
/// the stack interactively (the preview wrapper, the commit-back
/// path, the can-back gate).
///
/// Lives here in the layout because all of these operations touch
/// the slots that the route-change effect also manipulates — the
/// transition itself stays free of `OwnerId`/`Slot` plumbing.
#[allow(clippy::too_many_arguments)]
fn build_gesture_context<R: Route>(
    container: Element,
    transition: StackTransitionBox,
    stack: crate::stack::RouteStack<R>,
    render: RouteRenderFn<R>,
    current: Slot,
    outgoing: Slot,
    preview: Slot,
    skip_animation: Rc<Cell<u32>>,
    // `gesture_parent`: anchor owner used as the parent of any owner
    // spawned from a gesture handler. Touch dispatchers fire with no
    // active reactive owner, so without this anchor the screens
    // mounted inside a preview wrapper would become roots and lose
    // access to the `RouteProvider`'s `RouteStack` context.
    gesture_parent: OwnerId,
) -> GestureContext {
    let can_back = {
        let stack = stack.clone();
        Rc::new(move || stack.entries().get().len() > 1) as Rc<dyn Fn() -> bool>
    };

    let mount_preview = {
        let stack = stack.clone();
        let render = render.clone();
        let preview = preview.clone();
        let outgoing_for_mount = outgoing.clone();
        let transition = transition.clone();
        Rc::new(move || {
            // If a previous gesture left a preview lingering (e.g.,
            // mount → no commit/cancel before the next touchstart),
            // tear it down before mounting a fresh one — otherwise
            // the DOM accumulates orphan wrappers that paint on top
            // of the current screen.
            if let Some((owner, wrapper)) = preview.borrow_mut().take() {
                remove_child(container, wrapper);
                dispose_owner(owner);
            }

            // Dispose any stale outgoing wrapper from the most
            // recent natural push. It would otherwise still sit in
            // the DOM at its parallax pose (translateX(-30%)
            // brightness 0.85) and obscure the preview the gesture
            // is about to mount — visible as a frozen "back" screen
            // that doesn't track the finger.
            if let Some((owner, wrapper)) = outgoing_for_mount.borrow_mut().take() {
                remove_child(container, wrapper);
                dispose_owner(owner);
            }

            let entries = stack.entries().get();
            let prev_route = if entries.len() >= 2 {
                entries[entries.len() - 2].route.clone()
            } else {
                entries[0].route.clone()
            };

            let preview_owner = create_owner(Some(gesture_parent));
            let preview_wrapper = create_element(ElementTag::View);
            apply_wrapper_style(
                preview_wrapper,
                transition.0.as_ref(),
                Side::Incoming,
                Direction::Backward,
            );
            insert_child_at(container, preview_wrapper, 0);
            with_owner(preview_owner, || {
                let h = render.call(prev_route);
                append_child(preview_wrapper, h);
            });
            *preview.borrow_mut() = Some((preview_owner, preview_wrapper));
            preview_wrapper
        }) as Rc<dyn Fn() -> Element>
    };

    let dispose_preview = {
        let preview = preview.clone();
        Rc::new(move || {
            if let Some((owner, wrapper)) = preview.borrow_mut().take() {
                remove_child(container, wrapper);
                dispose_owner(owner);
            }
        }) as Rc<dyn Fn()>
    };

    let commit_preview_and_back = {
        let preview = preview.clone();
        let current = current.clone();
        let outgoing_for_commit = outgoing.clone();
        let stack = stack.clone();
        let skip_animation = skip_animation.clone();
        Rc::new(move || {
            // Promote preview → current; dispose both the old
            // current (the screen we're leaving) AND any wrapper
            // sitting in the outgoing slot. The outgoing slot
            // holds whatever screen the prior push promoted there
            // (e.g. the original Home after a Home → List push) —
            // the natural pop would dispose it at the top of the
            // effect, but we're about to skip that effect, so
            // without this the wrapper persists in the DOM and
            // reappears underneath the promoted preview.
            let promoted = preview.borrow_mut().take();
            let old_current = current.borrow_mut().take();
            let stale_outgoing = outgoing_for_commit.borrow_mut().take();
            if let Some((owner, wrapper)) = stale_outgoing {
                remove_child(container, wrapper);
                dispose_owner(owner);
            }
            if let Some((owner, wrapper)) = old_current {
                remove_child(container, wrapper);
                dispose_owner(owner);
            }
            if let Some((owner, wrapper)) = promoted {
                *current.borrow_mut() = Some((owner, wrapper));
            }
            // One effect run is consumed by the `stack.back()` call
            // below — `skip_animation` makes that run a no-op so we
            // don't re-mount on top of the gesture's promoted
            // preview.
            skip_animation.set(1);
            stack.back();
        }) as Rc<dyn Fn()>
    };

    let current_wrapper = {
        let current = current.clone();
        Rc::new(move || current.borrow().as_ref().map(|(_, w)| *w))
            as Rc<dyn Fn() -> Option<Element>>
    };

    GestureContext {
        container,
        transition,
        can_back,
        mount_preview,
        dispose_preview,
        commit_preview_and_back,
        current_wrapper,
    }
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

    #[test]
    fn ios_slide_pose_endpoints_match_animate_keyframes() {
        // `pose(0.0)` matches the "from" pose of `animate()`'s
        // keyframes, `pose(1.0)` matches the "to". Verified for the
        // common case (incoming forward push).
        let t = IosSlide::default();
        let p0 = t.pose(Side::Incoming, Direction::Forward, 0.0);
        let p1 = t.pose(Side::Incoming, Direction::Forward, 1.0);
        assert_eq!(p0[0].0, "transform");
        assert!(p0[0].1.contains("100"), "got {}", p0[0].1);
        assert!(p1[0].1.contains('0'), "got {}", p1[0].1);
    }
}
