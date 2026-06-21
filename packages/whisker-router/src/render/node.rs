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
    Element, append_child, create_element, create_phantom_element, remove_child, set_inline_styles,
};
use whisker::{AnimationController, ElementTag, computed};

use crate::core::{NodePath, RouteState, RouteTree};
use crate::render::handle::RouterHandle;
use crate::render::registry::Transition;
use crate::render::transition::{self, Role};

/// **Temporary diagnostic logging** for the Stack transition path.
///
/// Forwarded to the dev-server via `eprintln!` + whisker's log_capture so
/// the iOS-sim trace of a push/pop can be inspected (the frame-1 vanish
/// could only be pinned down on a device). Defaults **on** for this
/// diagnostic build; set `WHISKER_ROUTER_DEBUG=0` to silence. Remove once
/// the cause is confirmed.
//
// TODO(phase-2 cleanup): delete this gate + its call sites once the
// transition is verified on device.
fn router_debug() -> bool {
    std::env::var("WHISKER_ROUTER_DEBUG").as_deref() != Ok("0")
}

/// Mount the node at `path` and return its phantom slot.
///
/// The slot is a [`create_phantom_element`] the caller appends wherever
/// the node should render; its content is managed reactively for the
/// life of the current owner.
pub fn mount_node(handle: &RouterHandle, path: NodePath) -> Element {
    match handle.tree().node_at(&path) {
        Some(RouteTree::Route(_)) => mount_route(handle, path),
        Some(RouteTree::Switch(_, _)) => mount_switch(handle, path),
        Some(RouteTree::Stack(_)) => mount_stack(handle, path),
        None => create_phantom_element(),
    }
}

// =====================================================================
// Route leaf
// =====================================================================

fn mount_route(handle: &RouterHandle, path: NodePath) -> Element {
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

    type Mounted = Rc<RefCell<Option<(Owner, Element)>>>;
    let mounted: Mounted = Rc::new(RefCell::new(None));

    effect(move || {
        let inst = instance.get();
        // Tear down the previous mount.
        if let Some((owner, el)) = mounted.borrow_mut().take() {
            remove_child(slot, el);
            owner.dispose();
        }
        let Some(inst) = inst else { return };
        let Some(render_fn) = render_fn.clone() else {
            return;
        };
        let owner = Owner::new(None);
        let el = owner.with(|| {
            let el = render_fn.call(&inst);
            append_child(slot, el);
            el
        });
        *mounted.borrow_mut() = Some((owner, el));
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
    /// The transition kind chosen for this entry's route.
    transition: Transition,
}

fn mount_stack(handle: &RouterHandle, path: NodePath) -> Element {
    // A real, positioned container so the `position: absolute` entry
    // wrappers stack against IT (rather than a distant ancestor) and the
    // stack fills its flex slot.
    let slot = create_element(ElementTag::View);
    set_inline_styles(slot, &stack_container_style());
    let handle = handle.clone();

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
        reconcile_stack(&handle_eff, &path_eff, slot, &live, &stack);
    });

    slot
}

/// Reconcile the live wrappers against `stack`'s history.
///
/// A push and a pop are both driven as a **coordinated two-screen
/// transition by one controller**: that controller's `0..1` progress
/// poses the moving top (`Role::Top`) and the screen beneath
/// (`Role::Under`) at once. A push runs it `0 → 1`; a pop runs it
/// `1 → 0` and unmounts the popped entry on finish. At rest the top sits
/// at `Role::Top` / `1.0` (translateX 0%) and lower entries are frozen.
fn reconcile_stack(
    handle: &RouterHandle,
    path: &NodePath,
    slot: Element,
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
                set_pose(&w, &w.ctrl.clone(), Role::Top);
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
                set_pose(&w, &drive, Role::Top);
                {
                    let l = live.borrow();
                    if let Some(under) = l.last() {
                        under.owner.resume(); // animate it while covered
                        set_pose(under, &drive, Role::Under);
                    }
                }
                if w.transition == Transition::None {
                    drive.set_value(1.0);
                } else {
                    drive.forward();
                }
                live.borrow_mut().push(w);
            }
        }
    }

    // ----- Shrink: a pop / reset.
    if new_len < old_len {
        let popped: Vec<StackWrapper> = {
            let mut l = live.borrow_mut();
            l.split_off(new_len)
        };
        // The deepest-removed (the visible top) animates out coordinated
        // with the newly-revealed survivor; any extra removed entries
        // (multi-pop / reset) just vanish.
        let mut popped = popped.into_iter();
        if let Some(top_popped) = popped.next() {
            run_pop(slot, live, top_popped);
        }
        for w in popped {
            dispose_wrapper(slot, w);
        }
    }

    // ----- Same length but the top changed = `replace`. Swap in place,
    // no animation (the new top is already present).
    if new_len == old_len && new_len > 0 {
        let top_idx = new_len - 1;
        let entry = &stack.history[top_idx];
        let needs_swap = {
            let l = live.borrow();
            let w = &l[top_idx];
            w.child != entry.child || w.fingerprint != entry.state
        };
        if needs_swap {
            let old = live.borrow_mut().remove(top_idx);
            dispose_wrapper(slot, old);
            let w = mount_wrapper(handle, slot, top_idx, entry);
            w.ctrl.set_value(1.0);
            set_pose(&w, &w.ctrl.clone(), Role::Top);
            live.borrow_mut().insert(top_idx, w);
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
                    set_pose(w, &w.ctrl.clone(), Role::Top);
                }
            } else if !pose_animating {
                // A buried entry not part of an in-flight transition:
                // freeze it fully covered.
                w.ctrl.set_value(1.0);
                set_pose(w, &w.ctrl.clone(), Role::Under);
                w.owner.pause();
            }
            let _ = w.key;
        }
    }

    // ----- Publish the gesture bridge for swipe-back.
    let l = live.borrow();
    let top = l.len().saturating_sub(1);
    let top_w = l.get(top);
    let under_w = if top >= 1 { l.get(top - 1) } else { None };
    let pose_of = |w: &StackWrapper| crate::render::handle::PoseBinding {
        ctrl: w.pose_ctrl,
        role: w.pose_role,
    };
    let bridge = crate::render::handle::StackBridge {
        top_ctrl: top_w.map(|w| w.ctrl.clone()),
        top_pose: top_w.map(pose_of),
        under_pose: under_w.map(pose_of),
        transition: top_w.map(|w| w.transition).unwrap_or_default(),
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
    let transition = popped.transition;

    // Guarantee the reverse animates over the full range: if the top's
    // progress is anything other than 1.0 (e.g. an interrupted run, or a
    // device frame that left it short), `reverse()`'s target would already
    // be reached and `on_finish(true)` would fire on the first frame —
    // popping the screen with no visible slide-out. Anchoring at 1.0 first
    // forces a real 1 → 0 animation. (A controller op, not a style write.)
    drive.set_value(1.0);

    // Point the popped top at the drive ctrl (Top) and the revealed
    // survivor at the same ctrl (Under). The per-wrapper pose EFFECT is the
    // single style writer — we never call `set_inline_styles` by hand here
    // (a second writer racing the effect, plus a re-append disturbing the
    // effect's element, was the suspected frame-1 vanish). The effect runs
    // each frame off `drive.value()` and writes the coordinated pose.
    set_pose(&popped, &drive, Role::Top);

    if router_debug() {
        eprintln!(
            "[router] run_pop: drive.value={} popped.role=Top transition={transition:?}",
            drive.value().get_untracked()
        );
    }

    let survivor_handle = {
        let l = live.borrow();
        l.last().map(|w| {
            w.owner.resume();
            set_pose(w, &drive, Role::Under);
            // Capture what we need to re-settle the survivor on finish.
            (w.wrapper, w.ctrl.clone(), w.pose_ctrl, w.pose_role)
        })
    };

    if transition == Transition::None {
        // No animation: drop immediately and settle the survivor.
        dispose_wrapper(slot, popped);
        if let Some((_w, ctrl, pose_ctrl, pose_role)) = survivor_handle {
            ctrl.set_value(1.0);
            pose_ctrl.set(ctrl);
            pose_role.set(Role::Top);
        }
        return;
    }

    let popped_wrapper = popped.wrapper;
    let popped_owner = popped.owner;
    let done = Rc::new(RefCell::new(false));
    drive.on_finish(move |finished| {
        if router_debug() {
            eprintln!("[router] run_pop on_finish: finished={finished}");
        }
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
    let (wrapper, ctrl, pose_ctrl, pose_role) = owner.with(|| {
        // A real `view` (not a phantom): the wrapper carries the
        // transition `transform` / `opacity` and `position: absolute`
        // stacking — none of which a style-less phantom can apply.
        let wrapper = create_element(ElementTag::View);
        let ctrl = AnimationController::new(transition::config(transition));

        // Repointable pose inputs. The single style effect reads the
        // *currently assigned* controller's progress + role, so pointing
        // both the top and the under wrapper at one controller makes one
        // progress drive the coordinated pair.
        let pose_ctrl = whisker::RwSignal::new(ctrl.clone());
        let pose_role = whisker::RwSignal::new(Role::Top);

        // The pose `computed` also carries the (role, transform, opacity)
        // for logging — the single source of truth for what gets written.
        let style = computed(move || {
            let c = pose_ctrl.get();
            let role = pose_role.get();
            let (transform, opacity) = transition::pose(transition, role, c.value().get());
            (
                wrapper_style(transform.clone(), opacity),
                role,
                transform,
                opacity,
            )
        });
        effect(move || {
            let (css, role, transform, opacity) = style.get();
            if router_debug() {
                eprintln!(
                    "[router] pose idx={idx} role={role:?} transform={transform} opacity={opacity}"
                );
            }
            set_inline_styles(wrapper, &css);
        });

        let child = mount_node(handle, child_path);
        append_child(wrapper, child);
        (wrapper, ctrl, pose_ctrl, pose_role)
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
        transition,
    }
}

/// Point `w`'s pose at controller `c` playing `role`.
fn set_pose(w: &StackWrapper, c: &AnimationController, role: Role) {
    w.pose_ctrl.set(c.clone());
    w.pose_role.set(role);
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
/// the transition's transform + opacity applied.
fn wrapper_style(transform: String, opacity: f32) -> String {
    format!(
        "position: absolute; left: 0; top: 0; right: 0; bottom: 0; \
         display: flex; flex-direction: column; transform: {transform}; \
         opacity: {opacity};"
    )
}
