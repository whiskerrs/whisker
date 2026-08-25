//! Host-independent retained scene engine for Whisker
//!
//! [`Scene`] owns logical nodes and their current presentation state. Mutations
//! update that retained state and append to a coalescing change journal. At a
//! frame boundary the scene prepares either a complete snapshot or an
//! incremental [`whisker_protocol::FramePacket`], then advances its accepted revision only after
//! the renderer confirms the complete transaction.
//!
//! [`SurfaceEngine`] pairs that scene with a retained Taffy layout tree. It
//! keeps structural mutations synchronized, skips clean layout passes, and
//! journals only geometry that changed from the previous snapshot.
//!
//! [`RecordingRenderer`] is the reference Rust-only consumer. Together with
//! `whisker-protocol` it lets tree, revision, recovery, and incremental-update
//! behavior run without Android, UIKit, DOM, Lynx, FFI, or WASM.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod layout;
mod measurement;
mod paint;
mod recording;
mod scene;
mod surface;
mod text;

pub use layout::{LayoutError, LayoutOptions, MeasurementProvider};
pub use measurement::{
    DeferredMeasurementApply, LayoutProgress, MeasurementApply, MeasurementError,
};
pub use paint::{LoweredPaint, lower_color, lower_paint, lower_transform};
pub use recording::{FrameSink, RecordedFrame, RecordingRenderer};
pub use scene::{Scene, SceneError, SceneNode};
pub use surface::{LayoutUpdate, SurfaceEngine, SurfaceError, SurfacePresentError};
pub use text::{LoweredPlainText, PlainTextInput, lower_plain_text};
pub use whisker_layout;
pub use whisker_protocol;
pub use whisker_style;
