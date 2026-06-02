//! `StackLayout` — push/pop with both screens visible during the
//! transition. Animation specifics are delegated to a
//! [`StackTransition`](crate::StackTransition) implementation; the
//! layout itself is responsible only for mounting / promoting
//! slots, deriving the [`Direction`], and ordering the DOM so the
//! transition's paint-order hint takes effect.
//!
//! Interactive behaviour (iOS swipe-back, Android system back) is
//! **not** part of the transition trait. Instead, the layout
//! publishes a [`StackLayoutHandle`] into context and the user
//! composes gesture / back-handler components as children of
//! [`StackLayout`]. See [`crate::IosSwipeBack`] for the iOS edge
//! swipe-back component.
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
use whisker::{component, provide_context, Children, Style};

use crate::outlet::{router, RouteRenderFn};
use crate::route::Route;
use crate::transitions::{Direction, Side, StackTransitionBox};

type Slot = Rc<RefCell<Option<(OwnerId, Element)>>>;

/// Handle a [`StackLayout`] publishes to context so child components
/// (gestures, back-handlers, anything else that needs to coordinate
/// with the layout's wrapper bookkeeping) can drive it.
///
/// Read this from a child via
/// `use_context::<StackLayoutHandle>().expect("inside StackLayout")`,
/// then call the closures as needed. For plain back navigation
/// (Android system back, hardware key, in-app back UI) you usually
/// don't need this handle — `router::<R>().back()` is enough.
/// This is for the interactive paths that need to mount a preview
/// of the destination screen and promote it atomically.
#[derive(Clone)]
pub struct StackLayoutHandle {
    /// The `StackLayout`'s root container view. Bind touch /
    /// animation / custom listeners on this element.
    pub container: Element,

    /// Handle to the currently-foregrounded wrapper, if any. The
    /// gesture controller poses this wrapper alongside the
    /// preview.
    pub current_wrapper: Rc<dyn Fn() -> Option<Element>>,

    /// Build a wrapper element for the screen one step below the
    /// top of the stack, mount the rendered screen into it, and
    /// insert it at DOM index 0 of the container. Returns the
    /// wrapper handle.
    ///
    /// The layout retains ownership; release via
    /// [`dispose_preview`](Self::dispose_preview) or
    /// [`commit_preview_and_back`](Self::commit_preview_and_back).
    pub mount_preview: Rc<dyn Fn() -> Element>,

    /// Tear down the preview wrapper (remove from DOM, dispose the
    /// reactive owner). Idempotent.
    pub dispose_preview: Rc<dyn Fn()>,

    /// Promote the preview wrapper into the `current` slot, dispose
    /// both the old current and any stale outgoing wrapper, then
    /// `stack.back()` with `skip_animation` set so the route-change
    /// effect doesn't re-animate the navigation the gesture
    /// already finished.
    pub commit_preview_and_back: Rc<dyn Fn()>,

    /// Plain back navigation — calls the in-context
    /// [`RouteStack::back`](crate::RouteStack::back). The natural
    /// route-change effect handles the pop animation, so non-
    /// interactive back handlers (Android system back, hardware key,
    /// in-app "Back" button) don't need to touch the preview slot at
    /// all. Erased over the route type `R` so children that don't
    /// know `R` (e.g. [`AndroidPredictiveBack`](crate::gestures::AndroidPredictiveBack))
    /// can drive it.
    pub back: Rc<dyn Fn()>,
}

/// Animated stack: holds two slots during a transition and renders
/// the current entry of the in-context
/// [`RouteStack`](crate::RouteStack).
///
/// The route stack for route type `R` is pulled from context —
/// wrap a [`RouteProvider`](crate::RouteProvider) above the layout
/// so `router::<R>()` finds it. Children of the layout (gestures,
/// back-handlers) receive a [`StackLayoutHandle`] via context.
#[component]
pub fn stack_layout<R: Route>(
    #[prop(default = StackTransitionBox::default())] transition: StackTransitionBox,
    render: RouteRenderFn<R>,
    children: Children,
) -> Element {
    let stack = router::<R>();
    let container = create_element(ElementTag::View);
    apply_styles(container, container_css().to_css_string());

    let outgoing: Slot = Rc::new(RefCell::new(None));
    let current: Slot = Rc::new(RefCell::new(None));
    let preview: Slot = Rc::new(RefCell::new(None));
    // When a gesture commit fires `stack.back()`, the resulting
    // route-change effect must NOT animate — the wrappers already
    // sit at the final pose. The commit closure sets this to 1;
    // the effect decrements and bails. Counted (rather than bool)
    // because a single signal update can fan out into more than
    // one effect run on some runtime paths.
    let skip_animation: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    let prev_len = Rc::new(Cell::new(0_usize));
    let first = Rc::new(Cell::new(true));

    // Tracking a single `entries` signal (rather than the derived
    // `current()` + `stack()` signals) so the effect re-runs once
    // per navigation — separate computeds would each schedule a
    // distinct re-run.
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

            // Gesture-driven commit already advanced the visual
            // state. Update bookkeeping and bail.
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

    // Reserve an owner whose parent is the layout's own owner so
    // children that mount via `commit_preview_and_back` /
    // `mount_preview` get reachable context (`RouteProvider` etc.).
    // The gesture handlers fire from the touch dispatcher, which
    // has no active reactive owner; without this anchor, owners
    // spawned for the preview screen would become roots and lose
    // access to the `RouteProvider` context their components rely
    // on.
    let handle_parent = create_owner(None);

    // Publish the handle so child gesture/back components can pick
    // it up via `use_context::<StackLayoutHandle>()`.
    let handle = build_stack_layout_handle(
        container,
        transition.clone(),
        stack.clone(),
        render.clone(),
        current,
        outgoing,
        preview,
        skip_animation,
        handle_parent,
    );
    provide_context(handle);

    // Mount the user's children (gestures / back-handlers / etc.).
    // They render no DOM of their own but attach handlers via
    // `on_mount`; the phantom returned by `mount_children` is
    // attached under the container so the children's owner is a
    // descendant of the layout's owner.
    let phantom = whisker::runtime::view::mount_children(&children);
    append_child(container, phantom);

    container
}

/// Build the closure surface a child component pulls out of context
/// to drive the stack's preview / commit primitives.
///
/// Lives here in the layout because all of these operations touch
/// the slots that the route-change effect also manipulates — the
/// gesture / back-handler components stay free of `OwnerId` / `Slot`
/// plumbing.
#[allow(clippy::too_many_arguments)]
fn build_stack_layout_handle<R: Route>(
    container: Element,
    transition: StackTransitionBox,
    stack: crate::stack::RouteStack<R>,
    render: RouteRenderFn<R>,
    current: Slot,
    outgoing: Slot,
    preview: Slot,
    skip_animation: Rc<Cell<u32>>,
    // `handle_parent`: anchor owner used as the parent of any owner
    // spawned via the handle's closures. Touch / system back
    // dispatchers fire with no active reactive owner; without this
    // anchor screens mounted inside a preview wrapper would become
    // roots and lose access to the `RouteProvider`'s `RouteStack`
    // context.
    handle_parent: OwnerId,
) -> StackLayoutHandle {
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

            let preview_owner = create_owner(Some(handle_parent));
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

    let back = {
        let stack = stack.clone();
        Rc::new(move || {
            // `back()` returns false if already at the stack root.
            // Plain back handlers don't surface that to the host —
            // the platform's natural back-when-empty behaviour (e.g.
            // finishing the Activity) takes over if the route stays
            // unchanged. Drop the bool here so the closure matches
            // `Fn()` for the type-erased handle.
            let _ = stack.back();
        }) as Rc<dyn Fn()>
    };

    StackLayoutHandle {
        container,
        current_wrapper,
        mount_preview,
        dispose_preview,
        commit_preview_and_back,
        back,
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
pub(crate) fn apply_wrapper_style(
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

pub(crate) fn slot_css() -> Css {
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
}
