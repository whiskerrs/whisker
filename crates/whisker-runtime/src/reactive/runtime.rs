//! Core data structures for the reactive runtime.
//!
//! The runtime is a single thread-local `ReactiveRuntime` holding two
//! generational slot maps (`owners` and `nodes`) plus a bit of
//! transient bookkeeping for the currently-running effect and the
//! pending-effects queue.
//!
//! All public reactive primitives (`ReadSignal`, `WriteSignal`,
//! `RwSignal`) are `Copy` newtypes around a `NodeId`. They look their
//! value up through the runtime on every operation. Cloning a handle
//! is just an integer copy; the lifetime of the underlying state is
//! bounded by its owning `Owner`, not by the handle. `memo()` returns
//! a `ReadSignal<T>` that happens to be backed by a `NodeData::Memo`
//! node — externally indistinguishable from a primitive signal.
//!
//! This module defines the types only. The thread-local instance and
//! the orchestration logic live in `mod.rs` and the sibling files.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use slotmap::{new_key_type, SlotMap};

new_key_type! {
    /// Identifier for an [`Owner`] slot in the runtime's owner map.
    /// Generational — disposing an owner invalidates outstanding
    /// `OwnerId`s pointing at the same slot index.
    pub struct OwnerId;

    /// Identifier for a [`ReactiveNode`] slot. Generational like
    /// [`OwnerId`].
    pub struct NodeId;
}

/// Kind discriminator for [`ReactiveNode`]. Carried separately from
/// [`NodeData`] so dependency-graph walks can branch on kind without
/// matching the data variant (the variants carry mutable state that we
/// generally don't want to touch during graph walks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// Mutable reactive value. Subscribers re-run when it changes.
    Signal,
    /// Side-effecting reactive computation. Has no return value
    /// observable to other nodes; runs once on registration and again
    /// whenever a tracked source changes.
    Effect,
    /// Derived value. Like an effect, but caches its return so
    /// downstream readers can observe it through the same dependency
    /// mechanism as a signal.
    Memo,
}

/// The mutable payload of a [`ReactiveNode`]. Signals carry a value,
/// effects carry a compute closure, memos carry both.
///
/// The compute closure is wrapped in `Rc<RefCell<…>>` so the
/// scheduler can grab a clone of the handle before invoking it,
/// keeping the runtime borrow short-lived. User code inside the
/// closure can then re-borrow the runtime freely.
pub enum NodeData {
    Signal {
        value: Rc<RefCell<dyn Any>>,
    },
    Effect {
        compute: Rc<RefCell<dyn FnMut()>>,
    },
    Memo {
        value: Rc<RefCell<dyn Any>>,
        compute: Rc<RefCell<dyn FnMut()>>,
    },
}

impl NodeData {
    pub fn kind(&self) -> NodeKind {
        match self {
            NodeData::Signal { .. } => NodeKind::Signal,
            NodeData::Effect { .. } => NodeKind::Effect,
            NodeData::Memo { .. } => NodeKind::Memo,
        }
    }

    /// Borrow the stored value if this node carries one. `None` for
    /// pure effects (which have no observable value).
    pub fn value(&self) -> Option<&Rc<RefCell<dyn Any>>> {
        match self {
            NodeData::Signal { value } => Some(value),
            NodeData::Memo { value, .. } => Some(value),
            NodeData::Effect { .. } => None,
        }
    }
}

/// One node in the reactive graph.
///
/// `sources` records what this node read in its last run (downstream
/// dependencies); `subscribers` records who reads us. Both sets are
/// kept in sync by the effect/memo runner — on each re-run, the runner
/// re-derives `sources` by tracking signal reads during the closure,
/// then sets `subscribers` on the new sources symmetrically.
pub struct ReactiveNode {
    pub owner: OwnerId,
    pub data: NodeData,
    pub sources: HashSet<NodeId>,
    pub subscribers: HashSet<NodeId>,
}

impl ReactiveNode {
    pub fn kind(&self) -> NodeKind {
        self.data.kind()
    }
}

/// A scope in the reactive tree. Created when a component mounts,
/// disposed when the component unmounts. Tracks the reactive nodes
/// allocated inside it (so they can be freed on disposal) and the
/// child owners (so disposal cascades).
///
/// `contexts` is the per-owner context bag for `provide_context` /
/// `use_context`. `cleanups` is the LIFO callback queue from
/// `on_cleanup`.
pub struct Owner {
    pub parent: Option<OwnerId>,
    pub children: Vec<OwnerId>,
    pub nodes: Vec<NodeId>,
    pub contexts: HashMap<TypeId, Box<dyn Any>>,
    pub cleanups: Vec<Box<dyn FnOnce()>>,
    /// Function-pointer fingerprint of the component fn that created
    /// this owner. Used by Strategy C hot reload (A6) to map
    /// subsecond-patched fn pointers back to live owners. `None` for
    /// non-component owners (e.g. the root, or manually-created
    /// scopes in tests).
    pub mount_fn: Option<*const ()>,
    /// Element handles created via `view::create_element` while this
    /// owner was at the top of the owner stack. Released through
    /// `view::release_element` when the owner is disposed (or its
    /// ancestor disposes via cascade), preventing the renderer-side
    /// `BridgeRenderer::elements` map from accumulating dangling
    /// `WhiskerElement*` pointers across `<Show>` flips, `<For>`
    /// item removals, and per-component remounts.
    pub elements: Vec<crate::view::ElementHandle>,
}

impl Owner {
    pub fn new(parent: Option<OwnerId>) -> Self {
        Self {
            parent,
            children: Vec::new(),
            nodes: Vec::new(),
            contexts: HashMap::new(),
            cleanups: Vec::new(),
            mount_fn: None,
            elements: Vec::new(),
        }
    }
}

/// The reactive runtime itself. One per thread (held in a
/// `thread_local!` slot in `mod.rs`).
///
/// All public reactive operations route through here. The pattern is
/// always:
///
/// 1. Open a short `with_borrow_mut` to read or mutate `owners` /
///    `nodes` / `current_*`.
/// 2. If user code needs to run (effect / memo closure), drop the
///    borrow first by cloning the necessary handles out, then call
///    the closure.
/// 3. Re-open a short borrow to restore book-keeping.
///
/// This keeps the `RefCell` borrow window narrow enough that user code
/// running inside a closure can re-enter the runtime (read signals,
/// write signals, register new effects) without panicking.
pub struct ReactiveRuntime {
    pub owners: SlotMap<OwnerId, Owner>,
    pub nodes: SlotMap<NodeId, ReactiveNode>,
    /// Owner stack: the topmost is the "current" owner — new signals,
    /// effects, memos, and lifecycle hooks register against it. Push
    /// when entering a `with_owner` (or `#[component]`) scope, pop on
    /// exit.
    pub owner_stack: Vec<OwnerId>,
    /// The effect/memo currently being computed, if any. Signal reads
    /// inside this effect register a `sources`/`subscribers` link
    /// against it.
    pub current_tracker: Option<NodeId>,
    /// Queue of effect/memo nodes scheduled to re-run on the next flush.
    /// Populated by signal writes; drained by [`flush_pending`].
    pub pending: Vec<NodeId>,
    /// True while [`flush_pending`] is actively draining `pending`.
    /// Used to avoid recursive flushes (signal writes inside a running
    /// effect just enqueue; we keep draining the queue until empty
    /// rather than recursing).
    pub flushing: bool,
    /// Component-fn-pointer → list of live owners that ran that fn.
    /// Populated by `register_component`; consulted by the A6 hot-
    /// reload path to find which owners to dispose when a fn body
    /// gets subsecond-patched.
    pub component_owners: HashMap<*const (), Vec<OwnerId>>,
    /// Side table of remountable component mount sites
    /// (`#[component]` with all-`Clone` props), keyed by a stable
    /// `MountId`. Hot-reload remount walks this table on every
    /// patch; ordinary `mount_component` (FnOnce body) does not
    /// register here.
    pub(crate) mount_sites: HashMap<super::component::MountId, super::component::MountSite>,
    /// Component-fn-pointer → list of remountable mount sites that
    /// ran that fn. Mirror of `component_owners` indexed by
    /// `MountId` instead of `OwnerId` so it survives the dispose +
    /// re-create cycle on each hot-reload remount (the owner is
    /// fresh every time, the mount id is stable).
    pub fn_ptr_mounts: HashMap<*const (), Vec<super::component::MountId>>,
    /// Monotonic counter for fresh `MountId`s.
    pub mount_id_counter: u64,
    /// Pending on_mount callbacks, in the order they were registered.
    /// Drained by [`super::flush_mounts`] — which the renderer (A3)
    /// will call after appending a component's view to its parent.
    pub pending_mounts: Vec<Box<dyn FnOnce()>>,
}

impl ReactiveRuntime {
    pub fn new() -> Self {
        Self {
            owners: SlotMap::with_key(),
            nodes: SlotMap::with_key(),
            owner_stack: Vec::new(),
            current_tracker: None,
            pending: Vec::new(),
            flushing: false,
            component_owners: HashMap::new(),
            pending_mounts: Vec::new(),
            mount_sites: HashMap::new(),
            fn_ptr_mounts: HashMap::new(),
            mount_id_counter: 0,
        }
    }

    /// Current top-of-stack owner. `None` outside any owner scope (the
    /// pre-mount state, basically only relevant for tests).
    pub fn current_owner(&self) -> Option<OwnerId> {
        self.owner_stack.last().copied()
    }
}

impl Default for ReactiveRuntime {
    fn default() -> Self {
        Self::new()
    }
}
