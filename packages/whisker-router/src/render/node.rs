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
    Element, append_child, create_phantom_element, remove_child, set_inline_styles,
};
use whisker::{AnimationController, computed};

use crate::core::{NodePath, RouteState, RouteTree};
use crate::render::handle::RouterHandle;
use crate::render::registry::Transition;
use crate::render::transition::{self, Role};

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
    let slot = create_phantom_element();
    let handle = handle.clone();

    let branch_count = handle
        .tree()
        .node_at(&path)
        .map(|n| n.children().len())
        .unwrap_or(0);

    // Mount every branch once into its own wrapper; keep them all alive.
    let mut wrappers: Vec<Element> = Vec::with_capacity(branch_count);
    for i in 0..branch_count {
        let wrapper = create_phantom_element();
        // Each branch fills the switch; visibility is toggled below.
        set_inline_styles(wrapper, &branch_base_style(false));
        let child = mount_node(&handle, path.child(i));
        append_child(wrapper, child);
        append_child(slot, wrapper);
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

    slot
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
    /// The transition controller driving this wrapper's pose.
    ctrl: AnimationController,
    /// The transition kind chosen for this entry's route.
    transition: Transition,
}

fn mount_stack(handle: &RouterHandle, path: NodePath) -> Element {
    let slot = create_phantom_element();
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

/// Reconcile the live wrappers against `stack`'s history: add wrappers
/// for new entries (animate in), drop wrappers for popped entries
/// (animate out, then unmount), and re-pose / freeze the rest.
fn reconcile_stack(
    handle: &RouterHandle,
    path: &NodePath,
    slot: Element,
    live: &Rc<RefCell<Vec<StackWrapper>>>,
    stack: &crate::core::StackState,
) {
    let new_len = stack.history.len();
    let old_len = live.borrow().len();

    // ----- Grow: a push (or initial mount). Add the new top wrappers.
    if new_len > old_len {
        for idx in old_len..new_len {
            let entry = &stack.history[idx];
            // The very first entry mounts already-present; a real push
            // animates in from off-screen.
            let animate_in = idx > 0;
            let w = mount_wrapper(handle, slot, idx, entry, animate_in);
            live.borrow_mut().push(w);
        }
    }

    // ----- Shrink: a pop / reset. Animate out + unmount the surplus.
    if new_len < old_len {
        let popped: Vec<StackWrapper> = {
            let mut l = live.borrow_mut();
            l.split_off(new_len)
        };
        // Animate the top-most popped wrapper out; the rest (a multi-pop /
        // reset) just vanish.
        for (i, w) in popped.into_iter().enumerate() {
            unmount_wrapper(slot, w, /* animate_out = */ i == 0);
        }
    }

    // ----- Same length but the top changed = `replace`. Swap it in
    // place with no animation (the new top is already "present").
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
            unmount_wrapper(slot, old, /* animate_out = */ false);
            let w = mount_wrapper(handle, slot, top_idx, entry, /* animate_in = */ false);
            live.borrow_mut().insert(top_idx, w);
        }
    }

    // ----- Re-pose + freeze the survivors: the top is active, the rest
    // are covered (parallaxed under the top) and paused.
    let l = live.borrow();
    let top = l.len().saturating_sub(1);
    for (i, w) in l.iter().enumerate() {
        if i == top {
            w.owner.resume();
            // Ensure the top is fully present (covers replace/reset where
            // no enter animation ran).
            if !w.ctrl.is_animating() {
                w.ctrl.set_value(1.0);
            }
        } else {
            // Covered screen: pose "under" and freeze its effects.
            let (transform, opacity) = transition::pose(w.transition, Role::Under, 1.0);
            set_inline_styles(w.wrapper, &wrapper_style(transform, opacity));
            w.owner.pause();
        }
        let _ = w.key;
    }

    // ----- Publish the gesture bridge for this stack (top + under
    // wrappers + the top controller) so swipe-back can scrub the pop.
    let top_w = l.get(top);
    let under_w = if top >= 1 { l.get(top - 1) } else { None };
    let bridge = crate::render::handle::StackBridge {
        top_wrapper: top_w.map(|w| w.wrapper),
        under_wrapper: under_w.map(|w| w.wrapper),
        top_ctrl: top_w.map(|w| w.ctrl.clone()),
        transition: top_w.map(|w| w.transition).unwrap_or_default(),
        can_back: l.len() > 1,
    };
    handle.set_stack_bridge(path.clone(), bridge);
}

/// Build a wrapper for `entry` at history index `idx`: choose its
/// transition, mount the child subtree under a fresh owner, wire the
/// pose `computed`, append it, and (optionally) animate it in.
fn mount_wrapper(
    handle: &RouterHandle,
    slot: Element,
    idx: usize,
    entry: &crate::core::StackEntry,
    animate_in: bool,
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

    let wrapper = create_phantom_element();
    let ctrl = AnimationController::new(transition::config(transition));

    // Compose the wrapper's pose from the controller progress.
    let ctrl_for_style = ctrl.clone();
    let style = computed(move || {
        let (transform, opacity) =
            transition::pose(transition, Role::Top, ctrl_for_style.value().get());
        wrapper_style(transform, opacity)
    });
    effect(move || set_inline_styles(wrapper, &style.get()));

    // Mount the child subtree under its own owner.
    let owner = Owner::new(None);
    let child = owner.with(|| mount_node(handle, child_path));
    append_child(wrapper, child);
    append_child(slot, wrapper);

    if animate_in {
        ctrl.set_value(0.0);
        ctrl.forward();
    } else {
        ctrl.set_value(1.0);
    }

    StackWrapper {
        key: idx,
        child: entry.child.clone(),
        fingerprint: entry.state.clone(),
        wrapper,
        owner,
        ctrl,
        transition,
    }
}

/// Remove `w` from the slot, optionally animating it out first (the
/// popped top); otherwise tear it down immediately.
fn unmount_wrapper(slot: Element, w: StackWrapper, animate_out: bool) {
    let wrapper = w.wrapper;
    let owner = w.owner;
    if animate_out && w.transition != Transition::None {
        let done = Rc::new(RefCell::new(false));
        w.ctrl.on_finish(move |finished| {
            if finished && !*done.borrow() {
                *done.borrow_mut() = true;
                remove_child(slot, wrapper);
                owner.dispose();
            }
        });
        w.ctrl.reverse();
    } else {
        remove_child(slot, wrapper);
        owner.dispose();
    }
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
