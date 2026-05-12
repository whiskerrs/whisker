//! Top-level reactive runtime.
//!
//! [`Runtime::new`]:
//!   1. Calls `app_fn()` inside a `track_dependencies` scope to build the
//!      first VDOM and record signal reads.
//!   2. [`mount`](crate::render::mount)s it on the renderer, capturing
//!      the [`HandleTree`].
//!   3. Returns a runtime ready to process frames.
//!
//! [`Runtime::frame`] checks the dirty flag, re-runs `app_fn`, diffs
//! against the previous tree, and applies patches against the cached
//! handle tree (so deep mutations target the right renderer handles).

use crate::diff::{apply, diff};
use crate::element::Element;
use crate::render::{mount, HandleTree};
use crate::renderer::Renderer;
use crate::signal::{take_dirty, track_dependencies};

pub struct Runtime<R: Renderer, F: FnMut() -> Element> {
    renderer: R,
    app_fn: F,
    last_tree: Element,
    handles: HandleTree<R::ElementHandle>,
}

impl<R: Renderer, F: FnMut() -> Element> Runtime<R, F> {
    /// Build the initial tree, mount it, and return a runtime ready to
    /// process frames.
    pub fn new(mut renderer: R, mut app_fn: F) -> Self {
        let (tree, _deps) = track_dependencies(&mut app_fn);
        let handles = mount(&mut renderer, &tree);
        let _ = take_dirty();
        Self {
            renderer,
            app_fn,
            last_tree: tree,
            handles,
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
        apply(&mut self.renderer, &mut self.handles, &patches);
        if count > 0 {
            self.renderer.flush();
        }
        self.last_tree = next;
        count
    }

    /// Force a re-render even if no signal is dirty. Useful after
    /// out-of-band changes (events, timer ticks the user wants to
    /// reflect even without state changes).
    pub fn force_frame(&mut self) -> usize {
        let (next, _deps) = track_dependencies(&mut self.app_fn);
        let patches = diff(&self.last_tree, &next);
        let count = patches.len();
        apply(&mut self.renderer, &mut self.handles, &patches);
        if count > 0 {
            self.renderer.flush();
        }
        self.last_tree = next;
        let _ = take_dirty();
        count
    }

    pub fn renderer(&self) -> &R {
        &self.renderer
    }

    pub fn renderer_mut(&mut self) -> &mut R {
        &mut self.renderer
    }
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
        assert_eq!(rt.renderer().ops().len(), initial_ops);
    }

    #[test]
    fn signal_change_targets_correct_handle() {
        __reset_runtime();
        let counter = use_signal(|| 0_i32);
        let app = move || page().child(text_with(format!("count: {}", counter.get())));
        let mut rt = Runtime::new(MockRenderer::new(), app);

        let before = rt.renderer().ops().len();
        counter.set(1);
        rt.frame();

        let new_ops: Vec<_> = rt.renderer().ops()[before..].to_vec();
        // Should be exactly: SetAttribute on the raw_text + Flush
        let attr_op = new_ops.iter().find_map(|op| match op {
            MockOp::SetAttribute { handle, key, value } if key == "text" => {
                Some((*handle, value.clone()))
            }
            _ => None,
        });
        assert!(attr_op.is_some(), "expected SetAttribute(text=...)");
        let (handle, value) = attr_op.unwrap();
        assert_eq!(value, "count: 1");
        assert_ne!(handle, 1, "must target raw_text handle, not page root");
    }

    #[test]
    fn no_dirty_flag_skips_app_fn_invocation() {
        __reset_runtime();
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
        let _ = rt.frame();
        assert_eq!(rt.frame(), 0);
    }

    #[test]
    fn force_frame_runs_even_without_dirty_flag() {
        __reset_runtime();
        let counter = use_signal(|| 0_i32);
        let app = move || page().child(text_with(format!("count: {}", counter.get())));
        let mut rt = Runtime::new(MockRenderer::new(), app);

        // No signal change — frame() would skip — but force_frame goes anyway.
        let count = rt.force_frame();
        // Tree is identical so 0 patches — but app_fn still ran.
        assert_eq!(count, 0);
    }

    #[test]
    fn many_re_renders_produce_no_handle_table_drift() {
        __reset_runtime();
        let counter = use_signal(|| 0_i32);
        let app = move || page().child(text_with(format!("n={}", counter.get())));
        let mut rt = Runtime::new(MockRenderer::new(), app);

        for i in 1..=20 {
            counter.set(i);
            rt.frame();
        }
        // Final SetAttribute must still target the SAME raw_text handle
        // we created at mount time (handle 3 in MockRenderer's numbering).
        let last_attr = rt
            .renderer()
            .ops()
            .iter()
            .rev()
            .find_map(|op| match op {
                MockOp::SetAttribute { handle, key, value } if key == "text" => {
                    Some((*handle, value.clone()))
                }
                _ => None,
            })
            .expect("attribute set during test");
        assert_eq!(last_attr.0, 3);
        assert_eq!(last_attr.1, "n=20");
    }
}
