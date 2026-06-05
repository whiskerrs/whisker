//! Layout components — render the current
//! [`RouteStack`](crate::RouteStack) in a particular shape.
//!
//! Each layout consumes the in-context stack via
//! [`router::<R>()`](crate::router) and decides how to display its
//! contents:
//!
//! - [`StackLayout`] — back-stack-preserving stack navigator with
//!   pluggable [`StackTransition`](crate::StackTransition) animation.
//!   The default for screen-stack navigation.
//! - [`TabsLayout`] — keep-alive tab switcher driven by per-tab
//!   predicates; combine with nested [`StackLayout`]s for
//!   tab-per-stack patterns.
//! - [`ModalLayout`] — slide-from-bottom modal sheet with scrim.
//! - [`Pane`] — display-toggleable container; the building block
//!   that powers [`TabsLayout`] internally and is useful for custom
//!   keep-alive layouts.
//!
//! Layouts are intentionally standalone components. Users can
//! implement their own (route value in, element out) by following
//! the same pattern.

pub mod modal;
pub mod pane;
pub mod stack;
pub mod tabs;

pub use modal::{ModalLayout, ModalLayoutProps, ModalRenderFn};
pub use pane::{Pane, PaneProps};
pub use stack::{StackLayout, StackLayoutProps};
pub use tabs::{TabSpec, TabsLayout, TabsLayoutProps};
