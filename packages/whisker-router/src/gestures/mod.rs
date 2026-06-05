//! Gesture / back-handler components for
//! [`StackLayout`](crate::StackLayout).
//!
//! Interactive back behaviour — iOS edge swipe-back, Android system
//! back — is implemented as **composable components** rather than
//! baked into [`StackTransition`](crate::StackTransition). Mix and
//! match them as children of [`StackLayout`](crate::StackLayout):
//!
//! ```ignore
//! StackLayout(transition: IosSlide::default(), render: render.into()) {
//!     IosSwipeBack()
//!     AndroidPredictiveBack()
//! }
//! ```
//!
//! Each component reads the [`StackLayoutHandle`](crate::StackLayoutHandle)
//! from context and drives the stack through it — they render no DOM
//! of their own. Pairing both is safe: each component is a no-op on
//! the platform it doesn't target.

pub mod android_predictive_back;
pub mod ios_swipe_back;

pub use android_predictive_back::{AndroidPredictiveBack, AndroidPredictiveBackProps};
pub use ios_swipe_back::{IosSwipeBack, IosSwipeBackProps};
