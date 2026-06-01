//! Layout components: `StackLayout`, `TabsLayout`, `ModalLayout`.
//!
//! Each layout consumes one variant of a parent route enum and
//! decides how to render its slice (stack push/pop semantics, tab
//! switching, modal presentation). They are intentionally
//! standalone components — users can implement their own layout
//! by following the same pattern (route variant in, element out).

pub mod modal;
pub mod pane;
pub mod stack;
pub mod tabs;

pub use modal::{ModalLayout, ModalLayoutProps, ModalRenderFn};
pub use pane::{Pane, PaneProps};
pub use stack::{StackLayout, StackLayoutProps};
pub use tabs::{TabSpec, TabsLayout, TabsLayoutProps};
