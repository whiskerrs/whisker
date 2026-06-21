//! # New router core — `RouteTree` + `RouteState` (phase 1)
//!
//! This module is the **pure-logic** model of the redesigned router
//! described in [`docs/router-design.md`]. It is deliberately free of
//! the macro, of rendering (`Outlet` / `Stack` components), of signals
//! and `Element` wiring, and of any device/gesture concern. It is just
//! the in-memory navigation graph and the operations over it, so the
//! model can be exhaustively unit-tested without a reactive runtime.
//!
//! The old `whisker-router` files (the signal-backed stack) are
//! untouched and still compile; this `core` module is the new system
//! and will grow the macro / rendering / device layers in later phases.
//!
//! ## The two graphs
//!
//! - [`RouteTree`] — the **static** structure: a tree of [`RouteTree`]
//!   nodes ([`RouteTree::Route`] leaves, [`RouteTree::Stack`] ordered
//!   containers, [`RouteTree::Switch`] parallel containers). It
//!   determines URLs and the set of legal targets. Built by hand here
//!   via the constructor helpers ([`RouteTree::route`],
//!   [`RouteTree::stack`], [`RouteTree::switch`]) since there is no
//!   `routes!` macro yet.
//! - [`RouteState`] — the **dynamic** state: the tree *instantiated*
//!   with `Stack.history` and `Switch.selected`. The shown screen
//!   ([`RouteState::current`]) is **derived** by walking the tree — it
//!   is never stored, so there is no marker that can drift.
//!
//! ## Addressing
//!
//! Every node has a stable [`NodeId`] (assigned in a pre-order walk at
//! [`CompiledTree`] build time) and can be addressed positionally by a
//! [`NodePath`] (the child-index chain from the root). Resolution and
//! `current` both speak `NodePath`.
//!
//! ## Operations
//!
//! The five verbs ([`navigate`], [`back`], [`replace`], [`pop_to`],
//! [`reset`]) live on [`Navigator`], a thin handle wrapping
//! `&mut RouteState` + `&CompiledTree`.
//!
//! [`navigate`]: Navigator::navigate
//! [`back`]: Navigator::back
//! [`replace`]: Navigator::replace
//! [`pop_to`]: Navigator::pop_to
//! [`reset`]: Navigator::reset
//! [`docs/router-design.md`]: https://github.com/whiskerrs/whisker/blob/main/docs/router-design.md

pub mod nav;
pub mod resolve;
pub mod state;
pub mod tree;

pub use nav::{NavError, Navigator};
pub use resolve::{Scope, Target, resolve, resolve_within};
pub use state::{RouteInstance, RouteState, StackEntry, StackState, SwitchState};
pub use tree::{CompiledTree, NodeId, NodeInfo, NodePath, RouteDef, RouteTree, SwitchDef};
