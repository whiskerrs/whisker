//! Top-level reactive runtime — the "tie it all together" layer.
//!
//! `run_app(renderer, app_fn)`:
//!   1. Calls `app_fn()` inside a `track_dependencies` scope to build the
//!      first VDOM and record signal reads.
//!   2. [`mount`](crate::render::mount)s it on the renderer.
//!   3. Polls [`take_dirty`] each tick. When something changes, re-runs
//!      `app_fn`, diffs against the previous tree, and applies patches.
//!
//! The polling loop assumes the caller drives ticks (e.g. the iOS host
//! calls a `lyra_runtime_tick()` function on every vsync). For tests we
//! expose [`Runtime::frame`] that runs one tick synchronously.

use crate::diff::{apply, diff};
use crate::element::Element;
use crate::render::mount;
use crate::renderer::Renderer;
use crate::signal::{take_dirty, track_dependencies};

pub struct Runtime<R: Renderer, F: FnMut() -> Element> {
    renderer: R,
    app_fn: F,
    last_tree: Element,
    root_handle: R::ElementHandle,
}

impl<R: Renderer, F: FnMut() -> Element> Runtime<R, F> {
    /// Build the initial tree, mount it, and return a runtime ready to
    /// process frames.
    pub fn new(mut renderer: R, mut app_fn: F) -> Self {
        let (tree, _deps) = track_dependencies(&mut app_fn);
        let root_handle = mount(&mut renderer, &tree);
        // Clear any dirty flag set by the initial signal allocations
        // (use_signal itself doesn't dirty, but defensive).
        let _ = take_dirty();
        Self {
            renderer,
            app_fn,
            last_tree: tree,
            root_handle,
        }
    }

    /// Process one frame. If anything has marked itself dirty since the
    /// previous tick, re-run the app, diff, and apply patches. Returns
    /// the number of patches applied (0 if no re-render was needed).
    pub fn frame(&mut self) -> usize {
        if !take_dirty() {
            return 0;
        }
        let (next, _deps) = track_dependencies(&mut self.app_fn);
        let patches = diff(&self.last_tree, &next);
        let count = patches.len();
        apply(
            &mut self.renderer,
            &self.last_tree,
            self.root_handle,
            &patches,
        );
        self.renderer.flush();
        self.last_tree = next;
        count
    }

    /// Borrow the underlying renderer (mostly for tests/diagnostics).
    pub fn renderer(&self) -> &R {
        &self.renderer
    }

    pub fn renderer_mut(&mut self) -> &mut R {
        &mut self.renderer
    }
}

/// One-shot helper for tests: build, run `n` ticks, and return the
/// accumulated MockRenderer ops.
#[cfg(test)]
pub fn drain_mock(
    mut app_fn: impl FnMut() -> Element,
    apply_changes: impl FnOnce() -> (),
) -> Vec<crate::renderer::MockOp> {
    use crate::renderer::MockRenderer;
    let renderer = MockRenderer::new();
    let mut rt = Runtime::new(renderer, &mut app_fn);
    apply_changes();
    let _ = rt.frame();
    rt.renderer_mut().ops().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::*;
    use crate::renderer::{MockOp, MockRenderer};
    use crate::signal::{__reset_runtime, use_signal};

    #[test]
    fn initial_mount_creates_tree_and_flushes() {
        __reset_runtime();
        let app = || page().child(text_with("Hello"));
        let rt = Runtime::new(MockRenderer::new(), app);
        let ops = rt.renderer().ops();
        assert!(matches!(ops[0], MockOp::Create { .. }));
        assert!(ops.iter().any(|op| matches!(op, MockOp::SetRoot { .. })));
        assert!(ops.iter().any(|op| matches!(op, MockOp::Flush)));
    }

    #[test]
    fn frame_without_changes_emits_nothing() {
        __reset_runtime();
        let app = || page();
        let mut rt = Runtime::new(MockRenderer::new(), app);
        let initial_ops = rt.renderer().ops().len();
        let count = rt.frame();
        assert_eq!(count, 0);
        // No new ops appended.
        assert_eq!(rt.renderer().ops().len(), initial_ops);
    }

    #[test]
    fn signal_change_triggers_re_render() {
        __reset_runtime();
        let counter = use_signal(|| 0_i32);
        let app = move || page().child(text_with(format!("count: {}", counter.get())));
        let mut rt = Runtime::new(MockRenderer::new(), app);
        let before = rt.renderer().ops().len();

        counter.set(1);
        let patches = rt.frame();
        assert!(patches > 0, "frame must apply at least one patch");
        assert!(rt.renderer().ops().len() > before, "ops were appended");
    }

    #[test]
    fn no_dirty_flag_skips_app_fn_invocation() {
        __reset_runtime();
        // Use a mutable counter to verify app_fn isn't called when nothing
        // changed.
        let invocations = std::cell::Cell::new(0);
        let counter = use_signal(|| 0_i32);
        let app = || {
            invocations.set(invocations.get() + 1);
            page().child(text_with(format!("count: {}", counter.get())))
        };
        let mut rt = Runtime::new(MockRenderer::new(), app);

        let after_init = invocations.get();
        rt.frame();
        rt.frame();
        rt.frame();
        // Three frames with no signal changes — app_fn should have been
        // called exactly once (the initial mount) and zero additional
        // times.
        assert_eq!(invocations.get(), after_init);
    }

    #[test]
    fn multiple_signal_changes_coalesce_into_one_re_render() {
        __reset_runtime();
        let counter = use_signal(|| 0_i32);
        let app = move || page().child(text_with(format!("count: {}", counter.get())));
        let mut rt = Runtime::new(MockRenderer::new(), app);

        counter.set(1);
        counter.set(2);
        counter.set(3);
        // One frame coalesces all three sets.
        let _ = rt.frame();
        // After processing, there's no further dirty work.
        assert_eq!(rt.frame(), 0);
    }
}
