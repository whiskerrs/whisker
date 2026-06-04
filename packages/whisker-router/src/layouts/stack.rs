//! `StackLayout` — back-stack-preserving stack navigator.
//!
//! Behaviour matches the established native stack-navigator semantics
//! (iOS `UINavigationController`, Android Fragment back stack, React
//! Navigation): every entry currently in the
//! [`RouteStack`](crate::RouteStack) stays **mounted** in the DOM and
//! keeps its reactive owner alive. Going back doesn't re-mount the
//! previous screen; it reveals the one that was already there.
//! Owners are only disposed for entries that have been **popped off
//! the stack** (and dispose is deferred until the next navigation so
//! the popped wrapper survives long enough to animate out).
//!
//! This is a change from the earlier model that held a single
//! `current` / `outgoing` slot pair and disposed the previous route
//! on every transition. The earlier model lost component state
//! (scroll position, in-flight resources, child component
//! lifecycles) on back-navigation, and forced module authors who
//! cached signals into route owners to fight an extra owner-tree
//! complication. The preserve-back-stack model fixes both: scroll
//! position survives a push/back round-trip, and per-route owners
//! survive disposal of *other* routes.
//!
//! Animation specifics are still delegated to a
//! [`StackTransition`](crate::StackTransition) implementation; the
//! layout is responsible for: tracking the entry-to-wrapper map,
//! diffing it against the latest `entries` signal, choosing which
//! wrapper plays the incoming / outgoing role on push or pop,
//! ordering the container's child list so the transition's
//! foreground hint paints in the right z-order, and deferring
//! dispose of popped wrappers until after their animation runs.
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
use std::collections::HashMap;
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
use crate::stack::EntryId;
use crate::transitions::{Direction, Side, StackTransitionBox};

/// One mounted entry's bookkeeping: the reactive owner that holds the
/// rendered tree, plus the wrapper `view` it lives inside. The wrapper
/// is what the transition animates; the owner is what gets disposed
/// when the entry is popped off the stack.
#[derive(Clone, Copy)]
struct MountedEntry {
    owner: OwnerId,
    wrapper: Element,
}

/// Mutable state shared between the route-change effect and the
/// gesture / back-handler closures published through the
/// [`StackLayoutHandle`]. Held in `Rc<RefCell<...>>` because the
/// closures live in different reactive scopes (the effect's, the
/// gesture component's) but coordinate on the same data.
#[derive(Clone)]
struct LayoutState {
    /// Every entry currently mounted under [`container`], keyed by its
    /// stable [`EntryId`]. Insertion happens when an entry is pushed
    /// onto the stack; removal happens when an entry is popped off
    /// (with dispose deferred via [`Self::pending_dispose`]).
    ///
    /// The `last` of [`Self::order`] is the active (visible) entry.
    mounted: Rc<RefCell<HashMap<EntryId, MountedEntry>>>,
    /// IDs in the order they appear in [`RouteStack::entries`]. Last
    /// element is the current visible route; everything before is
    /// the back stack, kept mounted at the suspended pose.
    order: Rc<RefCell<Vec<EntryId>>>,
    /// Entries whose owners are scheduled for disposal **on the
    /// next effect run**. Dispose is deferred so the wrapper stays
    /// alive long enough to animate out — Lynx's `Element::Animate`
    /// has no completion callback, so we can't dispose at "end of
    /// animation" precisely; the next-nav drain is a memory-bounded
    /// approximation (max one popped wrapper queued at a time).
    pending_dispose: Rc<RefCell<Vec<MountedEntry>>>,
    /// Counter the gesture commit closures use to suppress the
    /// route-change effect's natural animation. The gesture has
    /// already moved wrappers to their final pose by hand, so the
    /// next effect run reads `> 0`, decrements, and skips animation
    /// + DOM reordering.
    skip_animation: Rc<Cell<u32>>,
}

/// Handle a [`StackLayout`] publishes to context so child components
/// (gestures, back-handlers, anything else that needs to coordinate
/// with the layout's wrapper bookkeeping) can drive it.
///
/// Read this from a child via
/// `use_context::<StackLayoutHandle>().expect("inside StackLayout")`,
/// then call the closures as needed. For plain back navigation
/// (Android system back, hardware key, in-app back UI) you usually
/// don't need this handle — `router::<R>().back()` is enough.
/// This is for the interactive paths that need to reach the
/// just-below-top wrapper that the back-stack model now keeps
/// pre-mounted for them.
#[derive(Clone)]
pub struct StackLayoutHandle {
    /// The `StackLayout`'s root container view. Bind touch /
    /// animation / custom listeners on this element.
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
    /// [`RouteStack::back`] and tell the route-change effect to
    /// skip its natural animation (the gesture has already settled
    /// the wrappers at their final pose). The popped entry's owner
    /// is disposed on the next effect run.
    pub commit_preview_and_back: Rc<dyn Fn()>,

    /// Plain back navigation — calls the in-context
    /// [`RouteStack::back`](crate::RouteStack::back). The natural
    /// route-change effect handles the pop animation. Erased over
    /// the route type `R` so children that don't know `R`
    /// (e.g. [`AndroidPredictiveBack`](crate::gestures::AndroidPredictiveBack))
    /// can drive it.
    pub back: Rc<dyn Fn()>,
}

/// Back-stack-preserving stack navigator.
///
/// Reads the in-context [`RouteStack`](crate::RouteStack) and mirrors
/// it into the DOM as a stack of wrappers, keeping every entry
/// mounted until it's popped off the stack. Animation between top
/// transitions is delegated to the configured
/// [`StackTransition`](crate::StackTransition).
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

    // Tracking a single `entries` signal (rather than the derived
    // `current()` + `stack()` signals) so the effect re-runs once
    // per navigation — separate computeds would each schedule a
    // distinct re-run.
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

    // Reserve an owner whose parent is the layout's own owner so
    // children that mount via `commit_preview_and_back` get reachable
    // context (`RouteProvider` etc.). The gesture handlers fire from
    // the touch dispatcher, which has no active reactive owner;
    // without this anchor, owners spawned for entries the gesture
    // interacts with would become roots and lose access to the
    // `RouteProvider` context their components rely on.
    let _handle_parent = create_owner(None);

    let handle = build_stack_layout_handle(container, stack.clone(), state);
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

/// One pass of the route-change effect. Pulled out of the `effect`
/// closure so the steps read top-to-bottom without an extra
/// indentation level.
fn run_navigation_effect<R: Route>(
    state: &LayoutState,
    first: &Rc<Cell<bool>>,
    transition: &StackTransitionBox,
    container: Element,
    render: &RouteRenderFn<R>,
    entries: Vec<crate::stack::RouteEntry<R>>,
) {
    // Step 1: drain the previous navigation's pending dispose —
    // wrappers that were popped off the stack last time and need
    // their owners freed now that their animation has had time to
    // play. Done first so a tight push-back-push cycle doesn't
    // leave stale wrappers in the DOM during the new transition.
    {
        let mut pending = state.pending_dispose.borrow_mut();
        for entry in pending.drain(..) {
            remove_child(container, entry.wrapper);
            dispose_owner(entry.owner);
        }
    }

    let new_ids: Vec<EntryId> = entries.iter().map(|e| e.id).collect();
    let new_id_set: std::collections::HashSet<EntryId> = new_ids.iter().copied().collect();

    // Step 2: skip-animation guard. The gesture commit path
    // (`commit_preview_and_back`) sets `skip_animation` so the
    // natural pop animation doesn't fire — the gesture already
    // settled the wrappers at their final pose. We still need to
    // *bookkeep* the stack change: the popped entry has to leave
    // `mounted`, and any owner it held has to be disposed.
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
                // Gesture already animated this wrapper to its
                // offscreen pose, so dispose right away — no
                // pending queue.
                remove_child(container, entry.wrapper);
                dispose_owner(entry.owner);
            }
        }
        first.set(false);
        return;
    }

    let old_ids = state.order.borrow().clone();
    let old_id_set: std::collections::HashSet<EntryId> = old_ids.iter().copied().collect();

    // Step 3: compute the diff.
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

    // Step 4: determine direction. We only animate the top transition
    // — replace_all / back_to / replace shapes either don't change
    // the top, or change it from / to something not in the previous
    // stack, in which case we still pick Forward or Backward by
    // whether the new top was already in `old_id_set`.
    let new_top = new_ids.last().copied();
    let old_top = old_ids.last().copied();
    let dir = if first.get() {
        Direction::None
    } else if new_top == old_top {
        // Top didn't change — maybe a non-top mutation (rare).
        Direction::None
    } else if new_top.is_some_and(|t| old_id_set.contains(&t)) {
        Direction::Backward
    } else {
        Direction::Forward
    };
    first.set(false);

    // Step 5: mount any newly-added entries. They start at the
    // "below top" suspended pose; the top-transition step below
    // overrides the wrapper for the new top into its Incoming
    // animation pose.
    for id in &added {
        let entry = entries
            .iter()
            .find(|e| e.id == *id)
            .expect("added id must be present in new entries");
        let route = entry.route.clone();
        let new_owner = create_owner(None);
        let wrapper = create_element(ElementTag::View);
        apply_wrapper_style(
            wrapper,
            transition.0.as_ref(),
            Side::Outgoing,
            Direction::Forward,
        );
        // Insert at the position the entry occupies in the new
        // stack. DOM order matches stack order — root at index 0,
        // current top at the last index — so z-stacking naturally
        // puts the top entry on top.
        let position = new_ids
            .iter()
            .position(|i| *i == *id)
            .expect("just inserted");
        insert_child_at(container, wrapper, position);
        with_owner(new_owner, || {
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

    // Step 6: persist the new order so the next run can diff.
    *state.order.borrow_mut() = new_ids.clone();

    // Step 7: set the top transition's animation start poses, then
    // schedule the actual animation in `on_mount` so the renderer
    // has a chance to commit the start frame.
    if dir != Direction::None {
        let incoming = new_top.and_then(|id| state.mounted.borrow().get(&id).copied());
        // `outgoing` might be in `removed` (when we're popping the
        // top), but at this point in the effect run we haven't moved
        // anything to `pending_dispose` yet — the wrapper is still
        // in `mounted` until step 8 below. So a single lookup works
        // for both push and pop cases.
        let outgoing = old_top.and_then(|id| state.mounted.borrow().get(&id).copied());

        if let Some(inc) = incoming {
            apply_wrapper_style(inc.wrapper, transition.0.as_ref(), Side::Incoming, dir);
        }
        if let Some(out) = outgoing {
            apply_wrapper_style(out.wrapper, transition.0.as_ref(), Side::Outgoing, dir);
        }

        // Reorder for z-stacking based on the transition's
        // foreground hint. iOS slide's Backward keeps `Outgoing`
        // (= the leaving top) in front so it visibly slides off
        // the screen revealing the incoming behind it; the default
        // child order at this point has the incoming below already,
        // so we only have to act for the Incoming foreground case.
        if matches!(transition.0.foreground(dir), Side::Incoming) {
            if let Some(inc) = incoming {
                // Move incoming to last child so it paints on top.
                // (No-op for Forward since we already inserted the
                // newly-mounted incoming at the last index.)
                remove_child(container, inc.wrapper);
                append_child(container, inc.wrapper);
            }
        } else if let Some(out) = outgoing {
            // Outgoing foreground: ensure outgoing paints last.
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
        // No animation — just make sure the top wrapper sits at the
        // active (centred) pose. Important for the very first run
        // and for replace_all-style transitions.
        if let Some(entry) = state.mounted.borrow().get(&top_id) {
            apply_wrapper_style(
                entry.wrapper,
                transition.0.as_ref(),
                Side::Incoming,
                Direction::None,
            );
        }
    }

    // Step 8: process removed entries.
    //   - The popped *top* (in a Backward navigation) is mid-
    //     animation — its wrapper has to stay alive long enough to
    //     play out. Move it to `pending_dispose`; the next effect
    //     run drains the queue.
    //   - Any other removed entries (replace_all, back_to multiple
    //     levels, replace) don't animate — dispose right away.
    for id in &removed {
        if let Some(entry) = state.mounted.borrow_mut().remove(id) {
            if dir == Direction::Backward && Some(*id) == old_top {
                state.pending_dispose.borrow_mut().push(entry);
            } else {
                remove_child(container, entry.wrapper);
                dispose_owner(entry.owner);
            }
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
            // In the preserve-back-stack model the entry one step
            // below the top is already mounted at the suspended
            // pose — return its wrapper so the gesture can drive
            // it. Returns None if there's no entry below the top
            // (i.e. the stack is at the root and back is invalid).
            let order = state.order.borrow();
            if order.len() < 2 {
                return None;
            }
            let prev_id = order[order.len() - 2];
            state.mounted.borrow().get(&prev_id).map(|e| e.wrapper)
        }) as Rc<dyn Fn() -> Option<Element>>
    };

    let dispose_preview = {
        // The just-below-top wrapper is a real back-stack entry, so
        // we don't dispose it. This closure exists to let the
        // gesture signal "cancel the drag" — the gesture component
        // itself is responsible for animating the wrapper back to
        // its suspended pose (it owns the touch progress and the
        // re-pose animation). We only need to keep the closure for
        // API compatibility; no-op is the right semantic now.
        Rc::new(|| {}) as Rc<dyn Fn()>
    };

    let commit_preview_and_back = {
        let stack = stack.clone();
        let skip_animation = state.skip_animation.clone();
        Rc::new(move || {
            // The gesture has already animated the just-below-top
            // wrapper to centre and the previous top wrapper to its
            // offscreen pose. Tell the route-change effect to skip
            // its natural animation so the next `stack.back()` doesn't
            // re-fire the transition; the effect still does the
            // bookkeeping (dispose the popped entry's owner, update
            // mounted_order). Counted because rare-but-possible
            // multi-fire signal paths could land more than one
            // effect run on a single back.
            skip_animation.set(1);
            let _ = stack.back();
        }) as Rc<dyn Fn()>
    };

    let back = {
        let stack = stack.clone();
        Rc::new(move || {
            // `back()` returns false if already at the stack root —
            // plain back handlers don't surface that to the host,
            // the platform's natural back-when-empty behaviour
            // takes over.
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
