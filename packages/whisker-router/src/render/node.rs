//! The recursive reactive renderer — the engine behind `Outlet` /
//! `Stack` / `Switch`.
//!
//! Given a [`RouterHandle`] and a [`NodePath`], [`mount_node`] returns a
//! phantom slot whose content reactively reflects the [`RouteState`]
//! subtree at that path:
//!
//! - **`Route`** → looks the leaf up in the registry and mounts it,
//!   re-mounting only when its params change.
//! - **`Switch`** → mounts **every** branch once (keep-alive) and
//!   toggles `display` on `selected`; backgrounded branches stay mounted
//!   so their state persists, matching the design's parallel-container
//!   rule.
//! - **`Stack`** → keeps every history entry mounted (lower entries
//!   frozen via [`Owner::pause`]) and runs a float-`Tween` transition on
//!   push / pop; the popped entry is unmounted when its reverse run
//!   finishes.
//!
//! Each container reads only **its own slice** (via
//! [`RouterHandle::slice_at`](crate::render::RouterHandle::slice_at)), so
//! an op that doesn't touch a given container produces an unchanged slice
//! and that container's effect does not re-run — the fine-grained
//! property. The mount/swap mechanics follow the old `outlet.rs` phantom
//! slot pattern, rebuilt against the new core + the continuous animation
//! engine.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::runtime::reactive::{Owner, effect};
use whisker::runtime::view::{
    Element, append_child, create_element, create_phantom_element, remove_child, set_attribute,
    set_inline_styles,
};
use whisker::{AnimationController, ElementTag, computed, provide_context, use_context};

use crate::core::{NodePath, RouteState, RouteTree};
use crate::render::components::OutletAnchor;
use crate::render::handle::RouterHandle;
use crate::render::registry::LayoutFn;
use crate::render::transition::{self, Direction, Pose, PoseMode, Role, RouteTransition};

/// Lynx `@LynxProp` **attribute** (not a CSS style) that makes a view clip its
/// children to `border-radius`. Drift here silently disables corner clipping,
/// so it lives in one named place. See the memory `lynx_border_radius_clip_radius_attr`.
const CLIP_RADIUS_ATTR: &str = "clip-radius";

/// Context marker: the `Layout(X)` chrome for this path is already being
/// applied. The layout's own `Outlet` re-enters [`mount_node`] at the same
/// path, and must mount the **raw** container rather than re-wrap it.
#[derive(Clone)]
struct LayoutApplied(NodePath);

/// Mount the node at `path` and return its phantom slot.
///
/// The slot is a [`create_phantom_element`] the caller appends wherever
/// the node should render; its content is managed reactively for the
/// life of the current owner.
///
/// If the node has `Layout(X)` chrome (from `routes!`), it is wrapped in the
/// layout component — unless we are already inside that layout (its `Outlet`
/// re-enters here), in which case the raw container is mounted.
pub fn mount_node(handle: &RouterHandle, path: NodePath) -> Element {
    if let Some(layout) = handle.layout_at(&path) {
        let inside = use_context::<LayoutApplied>().map(|a| a.0);
        if inside.as_ref() != Some(&path) {
            return mount_with_layout(handle, path, layout);
        }
    }
    match handle.tree().node_at(&path) {
        Some(RouteTree::Route(..)) => mount_route(handle, path),
        Some(RouteTree::Switch(_, _)) => mount_switch(handle, path),
        Some(RouteTree::Stack(_)) => mount_stack(handle, path),
        None => create_phantom_element(),
    }
}

/// Render the `Layout(X)` component for `path`. Its inner `Outlet` anchors
/// at `path` (so it draws this container) and sees [`LayoutApplied`] (so it
/// mounts the raw container instead of re-wrapping).
fn mount_with_layout(handle: &RouterHandle, path: NodePath, layout: LayoutFn) -> Element {
    let _ = handle;
    provide_context(OutletAnchor(path.clone()));
    provide_context(LayoutApplied(path));
    layout.call()
}

// =====================================================================
// Route leaf
// =====================================================================

fn mount_route(handle: &RouterHandle, path: NodePath) -> Element {
    // Route with children: mount each child (the Route is a structural
    // grouping node — its children are Stack/Switch/Route).
    if let Some(RouteTree::Route(_, children)) = handle.tree().node_at(&path) {
        if !children.is_empty() {
            let slot = create_phantom_element();
            for i in 0..children.len() {
                let child = mount_node(handle, path.child(i));
                append_child(slot, child);
            }
            return slot;
        }
    }

    let slot = create_phantom_element();
    let handle = handle.clone();

    // The route id is fixed for this static node; resolve it once.
    let route_id = handle
        .tree()
        .info_at(&path)
        .and_then(|i| i.route_id.clone())
        .unwrap_or_default();

    let render_fn = handle.render_fn(&route_id);

    // Track only this leaf's instance (params). Re-mount on param change.
    let slice = handle.slice_at(path.clone());
    let instance = computed(move || match slice.get() {
        Some(RouteState::Route(inst)) => Some(inst),
        _ => None,
    });

    type Mounted = Rc<RefCell<Option<(Owner, Element, crate::core::RouteInstance)>>>;
    let mounted: Mounted = Rc::new(RefCell::new(None));

    effect(move || {
        let inst = instance.get();

        // CRITICAL: when the instance becomes `None` — i.e. this leaf's
        // entry was just removed from `RouteState` by a `back()`/pop — DO
        // NOT tear the content down. The leaf's content lifetime is owned
        // by its enclosing `Stack` wrapper, which keeps the popped screen
        // mounted through its exit animation and disposes the whole
        // subtree (cascading to this leaf's owner) only when the animation
        // finishes. Removing here on `None` is exactly what made the
        // popped screen vanish on frame 1 while its wrapper was still
        // sliding out. So `None` is a no-op; teardown is by owner dispose.
        let Some(inst) = inst else {
            return;
        };

        // A `Some` instance: (re)mount only when the instance actually
        // changed (first mount, or a param change from a `replace`).
        let changed = mounted
            .borrow()
            .as_ref()
            .map(|(_, _, prev)| prev != &inst)
            .unwrap_or(true);
        if !changed {
            return;
        }

        if let Some((owner, el, _)) = mounted.borrow_mut().take() {
            remove_child(slot, el);
            owner.dispose();
        }
        let Some(render_fn) = render_fn.clone() else {
            return;
        };
        let owner = Owner::new(None);
        let scope_path = path.clone();
        let el = owner.with(|| {
            // Publish this leaf's path so the component's `use_param` /
            // `use_params` hooks can read ITS route params from context.
            whisker::provide_context(crate::render::handle::RouteScope(scope_path));
            let el = render_fn.call(&inst);
            append_child(slot, el);
            el
        });
        *mounted.borrow_mut() = Some((owner, el, inst));
    });

    slot
}

// =====================================================================
// Switch — all branches mounted, display toggled on `selected`
// =====================================================================

fn mount_switch(handle: &RouterHandle, path: NodePath) -> Element {
    // A real, positioned container holds the branch wrappers: it
    // `position: relative; flex-grow: 1` so the `position: absolute`
    // branches resolve against IT (filling the switch's slot in the
    // surrounding flex layout, e.g. above a tab bar) rather than against
    // some distant ancestor.
    let container = create_element(ElementTag::View);
    set_inline_styles(container, &switch_container_style());
    let handle = handle.clone();

    let branch_count = handle
        .tree()
        .node_at(&path)
        .map(|n| n.children().len())
        .unwrap_or(0);

    // Mount every branch once into its own wrapper; keep them all alive.
    // The wrapper MUST be a real `view` (not a phantom): it carries
    // `display` / `position: absolute` styles, and a phantom is a
    // style-less transparent bundler whose `set_inline_styles` never
    // reaches Lynx (so non-selected branches would not hide).
    let mut wrappers: Vec<Element> = Vec::with_capacity(branch_count);
    for i in 0..branch_count {
        let wrapper = create_element(ElementTag::View);
        // Each branch fills the switch; visibility is toggled below.
        set_inline_styles(wrapper, &branch_base_style(false));
        let child = mount_node(&handle, path.child(i));
        append_child(wrapper, child);
        append_child(container, wrapper);
        wrappers.push(wrapper);
    }

    let selected = handle.selected_at(path.clone());
    let wrappers = Rc::new(wrappers);
    effect(move || {
        let sel = selected.get().unwrap_or(0);
        for (i, w) in wrappers.iter().enumerate() {
            set_inline_styles(*w, &branch_base_style(i == sel));
        }
    });

    container
}

/// The switch's positioned container: fills its flex slot and anchors the
/// absolutely-positioned branch wrappers.
fn switch_container_style() -> String {
    "position: relative; flex-grow: 1; display: flex; flex-direction: column;".to_string()
}

/// A switch branch wrapper fills the container; only the selected one is
/// displayed.
fn branch_base_style(visible: bool) -> String {
    let display = if visible { "flex" } else { "none" };
    format!(
        "display: {display}; flex-direction: column; position: absolute; \
         left: 0; top: 0; right: 0; bottom: 0;"
    )
}

// =====================================================================
// Stack — history kept mounted, transitions on push/pop
// =====================================================================

/// One live history wrapper.
///
/// A wrapper's pose is computed by **one** persistent style effect that
/// reads two repointable reactive inputs: `pose_ctrl` (the controller
/// whose `0..1` progress currently drives this wrapper) and `pose_role`
/// (`Top` or `Under`). A push/pop points **both** the moving top and the
/// revealed/covered under wrapper at the **same** controller, so one
/// progress drives the coordinated two-screen transition (the same model
/// the swipe-back uses), instead of each wrapper animating in isolation.
struct StackWrapper {
    /// Stable key: the history index this wrapper instantiated.
    key: usize,
    /// The static child this wrapper instantiated — used to detect a
    /// `replace` (same index, different child/params) so the top can be
    /// swapped in place.
    child: NodePath,
    /// A fingerprint of the entry's nested state at mount time, so a
    /// `replace` that keeps the same child path but changes params still
    /// re-mounts.
    fingerprint: RouteState,
    /// The wrapper element (carries the transition transform).
    wrapper: Element,
    /// The child node's owner (so we can pause/resume/dispose it).
    owner: Owner,
    /// This wrapper's **own** controller — the one used to drive the
    /// transition when *this* wrapper is the moving top (push-in /
    /// pop-out). It is also what an under wrapper's pose is pointed at
    /// during a transition involving it.
    ctrl: AnimationController,
    /// The controller currently driving this wrapper's pose (repointable:
    /// during a push/pop both the top and the under wrapper point at the
    /// top's `ctrl`; at rest each points at its own).
    pose_ctrl: whisker::RwSignal<AnimationController>,
    /// The role this wrapper currently plays in its pose computation.
    pose_role: whisker::RwSignal<Role>,
    /// The pose mode — the route transition normally, flipped to
    /// `Predictive(edge)` by a back gesture for the Material preview.
    pose_mode: whisker::RwSignal<PoseMode>,
    /// The transition kind chosen for this entry's route.
    transition: RouteTransition,
}

fn mount_stack(handle: &RouterHandle, path: NodePath) -> Element {
    // A real, positioned container so the `position: absolute` entry
    // wrappers stack against IT (rather than a distant ancestor) and the
    // stack fills its flex slot.
    let slot = create_element(ElementTag::View);
    set_inline_styles(slot, &stack_container_style());
    let handle = handle.clone();

    // Backdrop-dim layer: a black absolute fill that darkens the area
    // *behind* the top card during a predictive-back gesture. Kept
    // positioned just below the top wrapper in DOM order by
    // `reconcile_stack`. Its opacity REACTIVELY follows the gesture's
    // controller (`dim_drive`) via `predictive_dim`: it rises as the finger
    // drags (up to `PB_MAX_DIM`) and fades back out on commit, 0 otherwise —
    // so it tracks both the drag and the settle animation, not a one-shot
    // per-frame write.
    let dim = create_element(ElementTag::View);
    let dim_drive: whisker::RwSignal<Option<AnimationController>> = whisker::RwSignal::new(None);
    {
        let dim_eff = dim;
        let opacity = computed(move || match dim_drive.get() {
            Some(ctrl) => transition::predictive_dim(ctrl.value().get()),
            None => 0.0,
        });
        effect(move || set_inline_styles(dim_eff, &dim_style(opacity.get())));
    }
    append_child(slot, dim);

    let slice = handle.slice_at(path.clone());
    // The history length + the top entry's child path drive diffing.
    // We read the whole StackState slice (cheap clone) and reconcile.
    let live: Rc<RefCell<Vec<StackWrapper>>> = Rc::new(RefCell::new(Vec::new()));

    let handle_eff = handle.clone();
    let path_eff = path.clone();
    effect(move || {
        let Some(RouteState::Stack(stack)) = slice.get() else {
            return;
        };
        reconcile_stack(&handle_eff, &path_eff, slot, dim, dim_drive, &live, &stack);
    });

    slot
}

/// Style for the stack's backdrop-dim layer at `opacity` (0 = invisible).
/// Always behind the top card; `pointer-events: none` so it never eats
/// touches.
fn dim_style(opacity: f32) -> String {
    format!(
        "position: absolute; left: 0; top: 0; right: 0; bottom: 0; \
         background-color: #000000; opacity: {opacity}; pointer-events: none;"
    )
}

/// Reconcile the live wrappers against `stack`'s history.
///
/// A push and a pop are both driven as a **coordinated two-screen
/// transition by one controller**: that controller's `0..1` progress
/// poses the moving top (`Role::Top`) and the screen beneath
/// (`Role::Under`) at once. A push runs it `0 → 1`; a pop runs it
/// `1 → 0` and unmounts the popped entry on finish. At rest the top sits
/// at `Role::Top` / `1.0` (translateX 0%) and lower entries are frozen.
#[allow(clippy::too_many_arguments)]
fn reconcile_stack(
    handle: &RouterHandle,
    path: &NodePath,
    slot: Element,
    dim: Element,
    dim_drive: whisker::RwSignal<Option<AnimationController>>,
    live: &Rc<RefCell<Vec<StackWrapper>>>,
    stack: &crate::core::StackState,
) {
    let new_len = stack.history.len();
    let old_len = live.borrow().len();

    // ----- Grow: a push (or initial mount).
    if new_len > old_len {
        for idx in old_len..new_len {
            let entry = &stack.history[idx];
            let w = mount_wrapper(handle, slot, idx, entry);

            if idx == 0 {
                // First entry: already present, no animation.
                w.ctrl.set_value(1.0);
                set_pose(&w, &w.ctrl.clone(), Role::Top, Direction::Push);
                live.borrow_mut().push(w);
            } else {
                // Real push: drive the new top's controller 0 → 1 and
                // point BOTH it and the entry below at that controller so
                // they animate as a coordinated pair. The per-wrapper pose
                // effect is the ONLY style writer — we set the controller
                // value first (so the effect's next run reads the start
                // pose) then repoint the bindings.
                let drive = w.ctrl.clone();
                w.ctrl.set_value(0.0);
                set_pose(&w, &drive, Role::Top, Direction::Push);
                {
                    let l = live.borrow();
                    if let Some(under) = l.last() {
                        under.owner.resume(); // animate it while covered
                        set_pose(under, &drive, Role::Under, Direction::Push);
                    }
                }
                if w.transition.is_instant() {
                    drive.set_value(1.0);
                } else {
                    drive.forward();
                }
                live.borrow_mut().push(w);
            }
        }
    }

    // ----- Shrink: a pop or a reset.
    if new_len < old_len {
        // A genuine pop leaves the revealed survivor unchanged, so we animate
        // the removed top out over it. A `reset` (full-history replacement)
        // can instead leave a *different* route at the new top index — the
        // surviving wrapper is stale. Wrappers are keyed by index, so a naive
        // shrink would keep that stale screen mounted (showing the wrong
        // screen and leaking its native views). Check the survivor: if it no
        // longer matches the history, this is a reset — swap it in place
        // (disposing the stale wrapper) with no pop animation.
        let survivor_matches = new_len == 0 || {
            let l = live.borrow();
            let w = &l[new_len - 1];
            let entry = &stack.history[new_len - 1];
            w.child == entry.child && w.fingerprint == entry.state
        };
        let popped: Vec<StackWrapper> = live.borrow_mut().split_off(new_len);
        if survivor_matches {
            // The deepest-removed (the visible top) animates out coordinated
            // with the newly-revealed survivor; any extra removed entries
            // (multi-pop) just vanish.
            let mut popped = popped.into_iter();
            if let Some(top_popped) = popped.next() {
                run_pop(slot, live, top_popped);
            }
            for w in popped {
                dispose_wrapper(slot, w);
            }
        } else {
            // Reset: dispose every removed wrapper (no pop animation), then
            // replace the stale survivor top with the route the history now
            // wants there. Disposing the old wrapper tears down its native
            // views, so nothing from the cleared screens lingers.
            for w in popped {
                dispose_wrapper(slot, w);
            }
            let top_idx = new_len - 1;
            let old = live.borrow_mut().remove(top_idx);
            dispose_wrapper(slot, old);
            let entry = &stack.history[top_idx];
            let w = mount_wrapper(handle, slot, top_idx, entry);
            w.ctrl.set_value(1.0);
            set_pose(&w, &w.ctrl.clone(), Role::Top, Direction::Push);
            live.borrow_mut().insert(top_idx, w);
        }
    }

    // ----- Same length but the top changed = `replace`. Animate it like a
    // push: the new screen slides in OVER the old top (kept mounted as a
    // transient `Under`), and the old wrapper is disposed when the slide
    // finishes. The entry *below* the old top is left untouched (it stays
    // covered), so the lower screen never flashes into view behind the
    // incoming one. (#265)
    if new_len == old_len && new_len > 0 {
        let top_idx = new_len - 1;
        let entry = &stack.history[top_idx];
        let needs_swap = {
            let l = live.borrow();
            let w = &l[top_idx];
            w.child != entry.child || w.fingerprint != entry.state
        };
        if needs_swap {
            // Detach the old top from `live` but keep it mounted — it is the
            // incoming screen's `Under` for the duration of the slide.
            let old = live.borrow_mut().remove(top_idx);
            let w = mount_wrapper(handle, slot, top_idx, entry);
            let drive = w.ctrl.clone();
            let instant = w.transition.is_instant();
            w.ctrl.set_value(0.0);
            set_pose(&w, &drive, Role::Top, Direction::Push);
            set_pose(&old, &drive, Role::Under, Direction::Push);
            live.borrow_mut().insert(top_idx, w);

            if instant {
                drive.set_value(1.0);
                dispose_wrapper(slot, old);
            } else {
                // Tear the old top down ONLY when the slide finishes (mirrors
                // `run_pop`), never mid-animation.
                let old_wrapper = old.wrapper;
                let old_owner = old.owner;
                let done = Rc::new(RefCell::new(false));
                drive.on_finish(move |finished| {
                    if !finished || *done.borrow() {
                        return;
                    }
                    *done.borrow_mut() = true;
                    remove_child(slot, old_wrapper);
                    old_owner.dispose();
                });
                drive.forward();
            }
        }
    }

    // ----- Settle the steady state: the top is active (its own ctrl,
    // Role::Top), lower entries are frozen fully-covered. We skip any
    // wrapper whose currently-assigned pose controller is still animating
    // so an in-flight push/pop (whose wrappers were pointed at the driving
    // ctrl) is not clobbered mid-flight.
    {
        let l = live.borrow();
        let top = l.len().saturating_sub(1);
        for (i, w) in l.iter().enumerate() {
            let pose_animating = w.pose_ctrl.get_untracked().is_animating();
            if i == top {
                w.owner.resume();
                if !pose_animating {
                    // Steady top: own controller, fully present (0%).
                    w.ctrl.set_value(1.0);
                    set_pose(w, &w.ctrl.clone(), Role::Top, Direction::Push);
                }
            } else if !pose_animating {
                // A buried entry not part of an in-flight transition:
                // freeze it fully covered.
                w.ctrl.set_value(1.0);
                set_pose(w, &w.ctrl.clone(), Role::Under, Direction::Push);
                w.owner.pause();
            }
            let _ = w.key;
        }
    }

    // ----- Keep the dim layer immediately below the top wrapper so it
    // darkens the under card + backdrop but is itself covered by the top.
    // Remove + re-append both so order is `[…lower, dim, top]`.
    //
    // ONLY for a push / steady state. During a **pop** the live top is the
    // revealed survivor, but the wrapper that must paint on top is the
    // *leaving* one (it slides off ABOVE the survivor) — `run_pop` keeps it
    // last. Re-appending the survivor here would put it over the leaving
    // card (Lynx ignores z-index during transform animations, so paint
    // order = DOM order), which is exactly the "previous screen on top
    // during the back slide" bug.
    if new_len >= old_len {
        let l = live.borrow();
        if let Some(top_w) = l.last() {
            remove_child(slot, dim);
            remove_child(slot, top_w.wrapper);
            append_child(slot, dim);
            append_child(slot, top_w.wrapper);
        }
    }

    // ----- Publish the gesture bridge for swipe-back / predictive-back.
    let l = live.borrow();
    let top = l.len().saturating_sub(1);
    let top_w = l.get(top);
    let under_w = if top >= 1 { l.get(top - 1) } else { None };
    let pose_of = |w: &StackWrapper| crate::render::handle::PoseBinding {
        ctrl: w.pose_ctrl,
        role: w.pose_role,
        mode: w.pose_mode,
    };
    let bridge = crate::render::handle::StackBridge {
        top_ctrl: top_w.map(|w| w.ctrl.clone()),
        top_pose: top_w.map(pose_of),
        under_pose: under_w.map(pose_of),
        dim_drive: Some(dim_drive),
        can_back: l.len() > 1,
    };
    handle.set_stack_bridge(path.clone(), bridge);
}

/// Drive a pop: reverse the popped top's controller `1 → 0`, with the
/// newly-revealed survivor pointed at the **same** controller as
/// `Role::Under` so it slides back from covered to rest in lockstep. On
/// finish, unmount the popped entry and settle the survivor to its own
/// resting controller.
fn run_pop(slot: Element, live: &Rc<RefCell<Vec<StackWrapper>>>, popped: StackWrapper) {
    let drive = popped.ctrl.clone();
    let transition = popped.transition.clone();

    // Point the popped top at the drive ctrl (Top) and the revealed
    // survivor at the same ctrl (Under). The per-wrapper pose EFFECT is the
    // single style writer — the effect runs each frame off `drive.value()`
    // and writes the coordinated pose. We `reverse()` from the controller's
    // *current* value: after a push that is 1.0; on a swipe-back commit it
    // is the scrubbed value, so the slide-out continues from the finger.
    set_pose(&popped, &drive, Role::Top, Direction::Pop);

    let survivor_handle = {
        let l = live.borrow();
        l.last().map(|w| {
            w.owner.resume();
            set_pose(w, &drive, Role::Under, Direction::Pop);
            // Capture what we need to re-settle the survivor on finish.
            (w.wrapper, w.ctrl.clone(), w.pose_ctrl, w.pose_role)
        })
    };

    if transition.is_instant() {
        // No animation: drop immediately and settle the survivor.
        dispose_wrapper(slot, popped);
        if let Some((_w, ctrl, pose_ctrl, pose_role)) = survivor_handle {
            ctrl.set_value(1.0);
            pose_ctrl.set(ctrl);
            pose_role.set(Role::Top);
        }
        return;
    }

    // The leaving card slides off ON TOP of the revealed survivor, so it
    // must paint above it. Lynx ignores z-index during transform
    // animations (paint order = DOM order), so move the popped wrapper to
    // the end (topmost) for the duration of the slide-out.
    remove_child(slot, popped.wrapper);
    append_child(slot, popped.wrapper);

    let popped_wrapper = popped.wrapper;
    let popped_owner = popped.owner;
    let done = Rc::new(RefCell::new(false));
    drive.on_finish(move |finished| {
        if !finished || *done.borrow() {
            return;
        }
        *done.borrow_mut() = true;
        // Unmount the popped entry (ONLY here — never mid-animation).
        remove_child(slot, popped_wrapper);
        popped_owner.dispose();
        // Re-settle the revealed survivor onto its own controller at the
        // active (Role::Top / 1.0 = translateX 0%) pose so no parallax
        // residue remains.
        if let Some((_w, ctrl, pose_ctrl, pose_role)) = &survivor_handle {
            ctrl.set_value(1.0);
            pose_ctrl.set(ctrl.clone());
            pose_role.set(Role::Top);
        }
    });
    drive.reverse();
}

/// Build a wrapper for `entry` at history index `idx`: choose its
/// transition, mount the child subtree under a fresh owner, wire the
/// repointable pose effect, and append it. The wrapper starts at rest
/// (`Role::Top`, its own controller); the caller drives the transition.
fn mount_wrapper(
    handle: &RouterHandle,
    slot: Element,
    idx: usize,
    entry: &crate::core::StackEntry,
) -> StackWrapper {
    let child_path = entry.child.clone();

    // Pick the transition from the leaf this entry leads to.
    let leaf_path = entry.state.current().path.clone();
    let leaf_id = handle
        .tree()
        .info_at(&leaf_path)
        .and_then(|i| i.route_id.clone())
        .unwrap_or_default();
    let transition = handle.transition(&leaf_id);

    // Everything for this wrapper lives under one owner so dispose frees
    // the wrapper element (a real `view`, registered with the current
    // owner by `create_element`), the style effect, the child subtree,
    // and deregisters the controller.
    let owner = Owner::new(None);
    let (wrapper, ctrl, pose_ctrl, pose_role, pose_mode) = owner.with(|| {
        // A real `view` (not a phantom): the wrapper carries the
        // transition `transform` / `opacity` and `position: absolute`
        // stacking — none of which a style-less phantom can apply.
        let wrapper = create_element(ElementTag::View);
        let ctrl = AnimationController::new(transition.config());

        // Repointable pose inputs. The single style effect reads the
        // *currently assigned* controller's progress + role + mode, so
        // pointing both the top and the under wrapper at one controller
        // makes one progress drive the coordinated pair, and flipping
        // `mode` to `Predictive` swaps in the Material preview.
        let pose_ctrl = whisker::RwSignal::new(ctrl.clone());
        let pose_role = whisker::RwSignal::new(Role::Top);
        let pose_mode =
            whisker::RwSignal::new(PoseMode::Transition(transition.clone(), Direction::Push));

        // The single style writer for this wrapper: read the currently
        // assigned controller's progress + role + mode, compute the pose,
        // write it.
        let style = computed(move || {
            let c = pose_ctrl.get();
            let role = pose_role.get();
            let mode = pose_mode.get();
            transition::pose_for(&mode, role, c.value().get())
        });
        let _ = idx;

        // Clip layer: `overflow: hidden; border-radius` on the *wrapper*
        // did NOT clip the user's screen view on Lynx — the opaque child
        // covers the wrapper and isn't rounded (a Lynx draw quirk, like the
        // row-default one). The proven structure is a dedicated clip view
        // BETWEEN the transform wrapper and the child: wrapper (transform /
        // opacity only) → clip_view (border-radius + overflow:hidden, sized
        // 100%) → child. The clip view is the *direct* parent of the screen
        // content, so Lynx rounds it.
        let clip = create_element(ElementTag::View);
        // `clip-radius` is a Lynx **prop** (`@LynxProp`), NOT a CSS style —
        // it must be set as an attribute, not in the inline style string.
        // It forces the view to clip its children to `border-radius` (the
        // auto overflow:hidden path is disabled in the fork:
        // `UIGroup.enableAutoClipRadius() == false`). The radius itself is
        // written reactively below so it animates with the gesture.
        set_attribute(clip, CLIP_RADIUS_ATTR, "true");

        // One style writer drives both elements from the pose: the wrapper's
        // transform/opacity AND the clip view's animated corner radius.
        effect(move || {
            let pose = style.get();
            set_inline_styles(wrapper, &wrapper_style(&pose));
            set_inline_styles(clip, &clip_view_style(pose.radius_px));
        });

        let child = mount_node(handle, child_path);
        append_child(clip, child);
        append_child(wrapper, clip);
        (wrapper, ctrl, pose_ctrl, pose_role, pose_mode)
    });
    append_child(slot, wrapper);

    StackWrapper {
        key: idx,
        child: entry.child.clone(),
        fingerprint: entry.state.clone(),
        wrapper,
        owner,
        ctrl,
        pose_ctrl,
        pose_role,
        pose_mode,
        transition,
    }
}

/// Point `w`'s pose at controller `c` playing `role` in `direction`, in the
/// normal route-transition mode (resets any predictive-back preview).
fn set_pose(w: &StackWrapper, c: &AnimationController, role: Role, direction: Direction) {
    w.pose_ctrl.set(c.clone());
    w.pose_role.set(role);
    w.pose_mode
        .set(PoseMode::Transition(w.transition.clone(), direction));
}

/// Tear down a popped wrapper immediately (no animation).
fn dispose_wrapper(slot: Element, w: StackWrapper) {
    remove_child(slot, w.wrapper);
    w.owner.dispose();
}

/// The stack's positioned container: fills its flex slot and anchors the
/// absolutely-positioned entry wrappers.
fn stack_container_style() -> String {
    "position: relative; flex-grow: 1; display: flex; flex-direction: column;".to_string()
}

/// Base style for a stack wrapper: absolutely-filled, column flow, with
/// the [`Pose`]'s transform + opacity. The corner radius / clipping live on
/// the inner [`clip_view_style`], NOT here — see `mount_wrapper`. The
/// transform origin is centred so the predictive-back scale shrinks the
/// card around its middle.
fn wrapper_style(pose: &Pose) -> String {
    format!(
        "position: absolute; left: 0; top: 0; right: 0; bottom: 0; \
         display: flex; flex-direction: column; \
         transform-origin: 50% 50%; transform: {}; opacity: {};",
        pose.transform, pose.opacity,
    )
}

/// Style for the per-screen **clip view** — the direct parent of the
/// user's screen content. Carries `overflow: hidden` and the **animated**
/// corner `radius_px`, sized to fill the transform wrapper. The radius is
/// `0` at rest (square screen) and grows to the device radius as the
/// predictive-back gesture shrinks the card — Material style. The
/// `clip-radius` Lynx attribute (set in `mount_wrapper`) is what makes the
/// rounding actually clip the child.
fn clip_view_style(radius_px: f32) -> String {
    format!(
        "position: absolute; left: 0; top: 0; width: 100%; height: 100%; \
         display: flex; flex-direction: column; overflow: hidden; \
         border-radius: {radius_px}px;"
    )
}
