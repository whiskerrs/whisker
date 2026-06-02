//! Gesture / back-handler components for [`StackLayout`](crate::StackLayout).
//!
//! Interactive behaviour — iOS edge swipe-back, Android system back,
//! predictive back, hardware keys — is implemented as **composable
//! components** rather than baked into the transition trait.
//!
//! Mount them as children of [`StackLayout`](crate::StackLayout):
//!
//! ```ignore
//! StackLayout(transition: IosSlide::default(), render: render) {
//!     IosSwipeBack()
//! }
//! ```
//!
//! Each component reads the [`StackLayoutHandle`](crate::StackLayoutHandle)
//! from context and uses [`router::<R>()`](crate::router) to drive
//! the stack.

pub mod ios_swipe_back;

pub use ios_swipe_back::{IosSwipeBack, IosSwipeBackProps};
