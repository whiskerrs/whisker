//! [`StackLayout`] — back-stack-preserving stack navigator with
//! pluggable animation.
//!
//! Behaviour matches the established native stack-navigator semantics
//! (iOS `UINavigationController`, Android Fragment back stack, React
//! Navigation): every entry currently in the
//! [`RouteStack`](crate::RouteStack) stays **mounted** in the DOM and
//! keeps its reactive owner alive. Going back doesn't re-mount the
//! previous screen — it reveals the one that was already there, so
//! scroll position, form input, and in-flight resources survive a
//! push/back round-trip. Owners are only disposed for entries that
//! have been **popped off the stack**, and disposal is deferred until
//! the next navigation so the popped wrapper survives long enough to
//! animate out.
//!
//! ```ignore
//! use whisker_router::{
//!     route_stack, RouteProvider, StackLayout, IosSlide, IosSwipeBack,
//! };
//!
//! let nav = route_stack(AppRoute::Home);
//!
//! render! {
//!     RouteProvider(stack: nav.clone()) {
//!         StackLayout(
//!             transition: StackTransitionBox::new(IosSlide::default()),
//!             render: render.into(),
//!         ) {
//!             // Opt in to the iOS edge swipe gesture as a child.
//!             IosSwipeBack()
//!         }
//!     }
//! }
//! ```
//!
//! Animation is delegated to a [`StackTransition`](crate::StackTransition)
//! implementation; the layout itself handles bookkeeping: tracking
//! the entry-to-wrapper map, diffing it against the latest `entries`
//! signal, choosing which wrapper plays the incoming / outgoing role
//! on push or pop, ordering the container's child list so the
//! transition's foreground hint paints in the right z-order, and
//! deferring dispose of popped wrappers until after their animation
//! runs.
//!
//! Interactive behaviour (iOS swipe-back, Android system back) is
//! **not** part of the transition trait. The layout publishes a
//! [`StackLayoutHandle`] into context and the user composes
//! [`crate::IosSwipeBack`] / [`crate::AndroidPredictiveBack`] (or
//! custom gesture components) as children. The default transition
//! is [`IosSlide`](crate::transitions::IosSlide) — horizontal slide
//! with ~30% parallax.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use whisker::css::ext::*;
use whisker::css::{Css, Overflow, PositionKind, ToCss};
use whisker::runtime::element::ElementTag;
use whisker::runtime::reactive::{effect, on_mount, Owner};
use whisker::runtime::view::apply::apply_styles;
use whisker::runtime::view::{
    append_child, create_element, insert_child_at, remove_child, set_inline_styles, Element,
};
use whisker::{component, provide_context, Children, Style};

use crate::outlet::{router, RouteRenderFn};
use crate::route::Route;
use crate::stack::EntryId;
use crate::transitions::{Direction, Side, StackTransitionBox};

// Wrapper `view` plus the reactive owner of the rendered subtree.
// The wrapper is what the transition animates; the owner is what
// gets disposed when the entry is popped off the stack.
#[derive(Clone, Copy)]
struct MountedEntry {
    owner: Owner,
    wrapper: Element,
}

// Shared mutable state between the route-change effect and the
// gesture / back-handler closures published through the
// `StackLayoutHandle`. The two run in different reactive scopes but
// coordinate on the same maps.
#[derive(Clone)]
struct LayoutState {
    // Every entry currently mounted under `container`, keyed by its
    // stable EntryId. Insertion on push; removal on pop (dispose
    // deferred via `pending_dispose`).
    mounted: Rc<RefCell<HashMap<EntryId, MountedEntry>>>,
    // EntryId order as in `RouteStack::entries`. Last is the visible
    // top; earlier entries are the back stack at the suspended pose.
    order: Rc<RefCell<Vec<EntryId>>>,
    // Drained on the next effect run — dispose is deferred so the
    // popped wrapper survives long enough to animate out. Lynx's
    // `Element::Animate` has no completion callback, so this is a
    // memory-bounded approximation (max one popped wrapper queued
    // at a time).
    pending_dispose: Rc<RefCell<Vec<MountedEntry>>>,
    // Gesture commit suppresses the natural animation by setting
    // this to 1; the next effect run decrements and skips animation
    // + DOM reordering. Counted (not bool) for safety against rare
    // multi-fire effect paths.
    skip_animation: Rc<Cell<u32>>,
}

/// Handle a [`StackLayout`] publishes into context so child gesture
/// / back-handler components can drive it.
///
/// Retrieve it from a child via
/// [`use_context::<StackLayoutHandle>()`](whisker::use_context). The
/// handle is **erased over the route type `R`** so children that
/// don't know `R` (e.g. [`AndroidPredictiveBack`](crate::AndroidPredictiveBack))
/// can drive the layout.
///
/// For plain back navigation (in-app back button, hardware key) you
/// usually don't need this handle — `router::<R>().back()` is enough.
/// `StackLayoutHandle` is for the interactive paths that need to
/// reach the just-below-top wrapper that the back-stack model keeps
/// pre-mounted for them, and to coordinate animation suppression
/// when a gesture has already settled the wrappers by hand.
#[derive(Clone)]
pub struct StackLayoutHandle {
    /// The `StackLayout`'s root container view. Bind touch and
    /// custom listeners on this element.
    pub container: Element,

    /// Returns the wrapper for the currently-foregrounded entry
    /// (top of stack), if any. The gesture controller poses this
    /// wrapper alongside the preview during a swipe-back drag.
    pub current_wrapper: Rc<dyn Fn() -> Option<Element>>,

    /// Returns the wrapper for the entry **one step below** the top
    /// of the stack — the screen a back navigation would reveal.
    ///
    /// In the preserve-back-stack model this wrapper is already
    /// mounted at the suspended pose (parallax behind the current
    /// top), so a swipe gesture just animates it forward instead of
    /// having to mount a fresh tree on every touchstart. Returns
    /// `None` if the stack has only one entry (no back to reveal).
    pub mount_preview: Rc<dyn Fn() -> Option<Element>>,

    /// Visually cancel an in-progress swipe-back gesture: restore
    /// the just-below-top wrapper to its suspended pose so it stops
    /// tracking the finger and resumes hiding behind the top.
    /// Idempotent.
    ///
    /// Does **not** dispose anything — the just-below-top wrapper is
    /// a real back-stack entry, not a throwaway preview. The layout
    /// keeps it mounted regardless of gesture state.
    pub dispose_preview: Rc<dyn Fn()>,

    /// Commit an in-progress swipe-back gesture: call
    /// [`RouteStack::back`](crate::RouteStack::back) and suppress the
    /// next natural pop animation (the gesture has already settled
    /// the wrappers at their final pose). The popped entry's owner
    /// is disposed on the next effect run.
    pub commit_preview_and_back: Rc<dyn Fn()>,

    /// Plain back navigation — calls
    /// [`RouteStack::back`](crate::RouteStack::back). The natural
    /// route-change effect handles the pop animation.
    pub back: Rc<dyn Fn()>,
}

/// Back-stack-preserving stack navigator with pluggable animation.
///
/// Reads the in-context [`RouteStack`](crate::RouteStack) and mirrors
/// it into the DOM as a stack of wrappers, keeping every entry
/// mounted until it's popped off the stack. Animation between top
/// transitions is delegated to the configured
/// [`StackTransition`](crate::StackTransition) — defaults to
/// [`IosSlide`](crate::IosSlide).
///
/// See the [module docs](self) for the full conceptual model. Mount
/// gesture children ([`crate::IosSwipeBack`],
/// [`crate::AndroidPredictiveBack`]) inside the layout's body to
/// opt into platform-native back gestures.
#[component]
pub fn stack_layout<R: Route>(
    #[prop(default = StackTransitionBox::default())] transition: StackTransitionBox,
    render: RouteRenderFn<R>,
    children: Children,
) -> Element {
    let stack = router::<R>();
    let container = create_element(ElementTag::View);
    apply_styles(container, container_css().to_css_string());

    let state = LayoutState {
        mounted: Rc::new(RefCell::new(HashMap::new())),
        order: Rc::new(RefCell::new(Vec::new())),
        pending_dispose: Rc::new(RefCell::new(Vec::new())),
        skip_animation: Rc::new(Cell::new(0)),
    };
    let first = Rc::new(Cell::new(true));

    // Track the raw `entries` signal — derived signals each schedule
    // their own re-run, so subscribing to one signal keeps the effect
    // to one run per navigation.
    let entries_signal = stack.entries();

    {
        let render = render.clone();
        let transition = transition.clone();
        let state = state.clone();
        let first = first.clone();
        effect(move || {
            run_navigation_effect(
                &state,
                &first,
                &transition,
                container,
                &render,
                entries_signal.get(),
            );
        });
    }

    // Reserve a parent owner for gesture-triggered mounts. The touch
    // dispatcher has no active reactive owner; without this anchor,
    // owners spawned by `commit_preview_and_back` would become roots
    // and lose access to the `RouteProvider` context.
    let _handle_parent = Owner::new(None);

    let handle = build_stack_layout_handle(container, stack.clone(), state);
    provide_context(handle);

    // Children render no DOM of their own but attach handlers via
    // `on_mount`. Mounting them under the container puts their owner
    // in the layout's subtree so context lookups succeed.
    let phantom = whisker::runtime::view::mount_children(&children);
    append_child(container, phantom);

    container
}

// One pass of the route-change effect. Pulled out of the `effect`
// closure so the steps read top-to-bottom without an extra
// indentation level.
fn run_navigation_effect<R: Route>(
    state: &LayoutState,
    first: &Rc<Cell<bool>>,
    transition: &StackTransitionBox,
    container: Element,
    render: &RouteRenderFn<R>,
    entries: Vec<crate::stack::RouteEntry<R>>,
) {
    // Drain the previous navigation's deferred-dispose queue first
    // so a tight push-back-push cycle doesn't leave stale wrappers
    // in the DOM during the new transition.
    {
        let mut pending = state.pending_dispose.borrow_mut();
        for entry in pending.drain(..) {
            remove_child(container, entry.wrapper);
            entry.owner.dispose();
        }
    }

    let new_ids: Vec<EntryId> = entries.iter().map(|e| e.id).collect();
    let new_id_set: std::collections::HashSet<EntryId> = new_ids.iter().copied().collect();

    // Skip-animation guard: gesture commit settled the wrappers by
    // hand, so we skip the natural animation but still bookkeep
    // (drop popped entries from `mounted`, dispose their owners).
    let skip = state.skip_animation.get();
    if skip > 0 {
        state.skip_animation.set(skip - 1);
        let old_ids = std::mem::replace(&mut *state.order.borrow_mut(), new_ids.clone());
        let removed: Vec<EntryId> = old_ids
            .iter()
            .filter(|id| !new_id_set.contains(id))
            .copied()
            .collect();
        for id in removed {
            if let Some(entry) = state.mounted.borrow_mut().remove(&id) {
                // Gesture already animated this wrapper offscreen,
                // so dispose right away (no pending queue).
                remove_child(container, entry.wrapper);
                entry.owner.dispose();
            }
        }
        first.set(false);
        sync_owner_paused_state(state);
        return;
    }

    let old_ids = state.order.borrow().clone();
    let old_id_set: std::collections::HashSet<EntryId> = old_ids.iter().copied().collect();

    let added: Vec<EntryId> = new_ids
        .iter()
        .filter(|id| !old_id_set.contains(id))
        .copied()
        .collect();
    let removed: Vec<EntryId> = old_ids
        .iter()
        .filter(|id| !new_id_set.contains(id))
        .copied()
        .collect();

    // Direction picks the animation: only the top transition is
    // animated. `replace_all` / `back_to` / `replace` shapes either
    // don't change the top, or replace it with something not in the
    // previous stack — we then still pick Forward/Backward by
    // whether the new top was already in `old_id_set`.
    let new_top = new_ids.last().copied();
    let old_top = old_ids.last().copied();
    let dir = if first.get() {
        Direction::None
    } else if new_top == old_top {
        // Top unchanged — non-top mutation, no animation needed.
        Direction::None
    } else if new_top.is_some_and(|t| old_id_set.contains(&t)) {
        Direction::Backward
    } else {
        Direction::Forward
    };
    first.set(false);

    // Mount newly-added entries at the suspended pose; the top
    // transition step below overrides the new top's wrapper into
    // its Incoming animation pose.
    for id in &added {
        let entry = entries
            .iter()
            .find(|e| e.id == *id)
            .expect("added id must be present in new entries");
        let route = entry.route.clone();
        let new_owner = Owner::new(None);
        let wrapper = create_element(ElementTag::View);
        apply_wrapper_style(
            wrapper,
            transition.0.as_ref(),
            Side::Outgoing,
            Direction::Forward,
            // Suspended-pose mount — not the interactive top. The top
            // transition step below re-styles the new top as `is_top`.
            false,
        );
        // DOM order matches stack order — root at index 0, top at
        // the last index — so the top entry paints on top naturally.
        let position = new_ids
            .iter()
            .position(|i| *i == *id)
            .expect("just inserted");
        insert_child_at(container, wrapper, position);
        new_owner.with(|| {
            let h = render.call(route);
            append_child(wrapper, h);
        });
        state.mounted.borrow_mut().insert(
            *id,
            MountedEntry {
                owner: new_owner,
                wrapper,
            },
        );
    }

    *state.order.borrow_mut() = new_ids.clone();

    // Set the top transition's start poses, then schedule the
    // actual animation in `on_mount` so the renderer commits the
    // start frame before the animator runs.
    if dir != Direction::None {
        let incoming = new_top.and_then(|id| state.mounted.borrow().get(&id).copied());
        // `outgoing` may be `removed` (we're popping the top) but
        // hasn't been moved to `pending_dispose` yet — a single
        // `mounted` lookup works for both push and pop cases.
        let outgoing = old_top.and_then(|id| state.mounted.borrow().get(&id).copied());

        if let Some(inc) = incoming {
            // Incoming == the new top → `relative` so its children
            // hit-test.
            apply_wrapper_style(inc.wrapper, transition.0.as_ref(), Side::Incoming, dir, true);
        }
        if let Some(out) = outgoing {
            // Outgoing == the old top → back to `absolute`.
            apply_wrapper_style(out.wrapper, transition.0.as_ref(), Side::Outgoing, dir, false);
        }

        // Reorder for z-stacking from the transition's foreground
        // hint. iOS pop keeps the leaving top in front so it slides
        // off revealing the incoming behind it. Lynx animator
        // ignores explicit z-index during transform animations (see
        // memory: lynx_zindex_animation_quirk) — DOM order is the
        // only reliable knob.
        if matches!(transition.0.foreground(dir), Side::Incoming) {
            if let Some(inc) = incoming {
                remove_child(container, inc.wrapper);
                append_child(container, inc.wrapper);
            }
        } else if let Some(out) = outgoing {
            remove_child(container, out.wrapper);
            append_child(container, out.wrapper);
        }

        let transition_for_mount = transition.clone();
        on_mount(move || {
            if let Some(inc) = incoming {
                transition_for_mount
                    .0
                    .animate(inc.wrapper, Side::Incoming, dir);
            }
            if let Some(out) = outgoing {
                transition_for_mount
                    .0
                    .animate(out.wrapper, Side::Outgoing, dir);
            }
        });
    } else if let Some(top_id) = new_top {
        // No animation — pin the top wrapper to the active (centred)
        // pose. Matters for the first mount and for replace_all.
        if let Some(entry) = state.mounted.borrow().get(&top_id) {
            apply_wrapper_style(
                entry.wrapper,
                transition.0.as_ref(),
                Side::Incoming,
                Direction::None,
                // First mount / replace_all top → the interactive slot.
                true,
            );
        }
    }

    // Process removed entries: the popped top of a Backward nav is
    // mid-animation, so defer its dispose. Other removals
    // (replace_all, multi-level back_to, replace) don't animate, so
    // dispose immediately.
    for id in &removed {
        if let Some(entry) = state.mounted.borrow_mut().remove(id) {
            if dir == Direction::Backward && Some(*id) == old_top {
                state.pending_dispose.borrow_mut().push(entry);
            } else {
                remove_child(container, entry.wrapper);
                entry.owner.dispose();
            }
        }
    }

    // Sync owner pause state: only the topmost runs effects; the
    // mounted-but-hidden back stack is paused until it surfaces.
    sync_owner_paused_state(state);
}

// Pause every non-top owner; resume the top. Idempotent —
// `Owner::pause` / `Owner::resume` no-op on the matching state.
fn sync_owner_paused_state(state: &LayoutState) {
    let order = state.order.borrow();
    let mounted = state.mounted.borrow();
    let top_id = order.last().copied();
    for (id, entry) in mounted.iter() {
        if Some(*id) == top_id {
            entry.owner.resume();
        } else {
            entry.owner.pause();
        }
    }
}

fn build_stack_layout_handle<R: Route>(
    container: Element,
    stack: crate::stack::RouteStack<R>,
    state: LayoutState,
) -> StackLayoutHandle {
    let current_wrapper = {
        let state = state.clone();
        Rc::new(move || {
            let order = state.order.borrow();
            order
                .last()
                .and_then(|id| state.mounted.borrow().get(id).map(|e| e.wrapper))
        }) as Rc<dyn Fn() -> Option<Element>>
    };

    let mount_preview = {
        let state = state.clone();
        Rc::new(move || {
            let order = state.order.borrow();
            if order.len() < 2 {
                return None;
            }
            let prev_id = order[order.len() - 2];
            state.mounted.borrow().get(&prev_id).map(|e| e.wrapper)
        }) as Rc<dyn Fn() -> Option<Element>>
    };

    // The just-below-top wrapper is a real back-stack entry, never a
    // throwaway preview. The gesture re-poses it back to suspended on
    // cancel; nothing here owns the visual state, so this is a no-op
    // kept for API parity with the earlier "mount preview on demand"
    // model.
    let dispose_preview = Rc::new(|| {}) as Rc<dyn Fn()>;

    let commit_preview_and_back = {
        let stack = stack.clone();
        let skip_animation = state.skip_animation.clone();
        Rc::new(move || {
            // Suppress the natural animation; the gesture already
            // settled the wrappers. The effect still does the
            // bookkeeping (drop popped entry from `mounted`,
            // dispose its owner).
            skip_animation.set(1);
            let _ = stack.back();
        }) as Rc<dyn Fn()>
    };

    let back = {
        let stack = stack.clone();
        Rc::new(move || {
            // `back()` returns false at the stack root; the host
            // platform's natural back-when-empty behaviour takes
            // over via the gesture component's caller.
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
    // `overflow: visible` is critical — Lynx clips children's
    // `box-shadow` at the parent's bounds by default (unlike Web
    // CSS). Required all the way down for IosSlide's leading-edge
    // shadow to show through.
    Css::new()
        .position(PositionKind::Relative)
        .width(100.percent())
        .height(100.percent())
        .flex_grow(1.0)
        .overflow(Overflow::Visible)
}

// Apply the layout's slot positioning plus the transition's per-role
// decoration to a wrapper. `Style::Static` collapses to one
// `set_inline_styles` write; `Style::Dynamic` registers an effect.
pub(crate) fn apply_wrapper_style(
    wrapper: Element,
    transition: &dyn crate::transitions::StackTransition,
    side: Side,
    direction: Direction,
    is_top: bool,
) {
    let base = slot_css(is_top).to_css_string();
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

pub(crate) fn slot_css(is_top: bool) -> Css {
    // The interactive top slot must be `relative`. Lynx's hit-testing
    // does *not* descend into the children of a `position: absolute`
    // element — it stops at the absolute wrapper as the event target.
    // Whisker then replays propagation from that target, so the
    // wrapper's `on_tap` still bubbles but the children's `on_tap`s
    // silently never fire. Making the front (interactive) slot
    // `relative` (in flow, filling the container) lets the hit-test
    // descend to its children. Covered / back-stack / mid-transition
    // slots stay `absolute` so they overlay out of flow; exactly one
    // slot — the current top — is `relative` at any time.
    //
    // See container_css: `overflow: visible` keeps IosSlide's
    // leading-edge shadow visible.
    Css::new()
        .position(if is_top {
            PositionKind::Relative
        } else {
            PositionKind::Absolute
        })
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
