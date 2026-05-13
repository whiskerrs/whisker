//! Diff two [`Element`] trees and emit a list of [`Patch`] ops.
//!
//! The algorithm:
//!
//! 1. If the two roots are not reconcilable (different tag or key), emit
//!    a single `Replace`.
//! 2. Otherwise compare attribute / style / event maps and emit
//!    `SetAttribute` / `RemoveAttribute` / `SetInlineStyles` /
//!    `ReplaceEvents` as needed.
//! 3. Recurse into children.
//!    - Unkeyed: pair `prev[i]` with `next[i]`. Tail extras become
//!      append/remove.
//!    - Keyed: build a map of `key -> prev_index`, walk `next`
//!      reconciling matched, inserting unmatched, removing leftovers.
//!
//! Apply ([`apply`]) walks each patch and dispatches to a Renderer,
//! using a [`HandleTree`] to look up handles by [`Path`].

use crate::element::{Element, EventHandler};
use crate::patch::{can_reconcile, Patch, Path};
use crate::render::{build_subtree, HandleTree};
use crate::renderer::Renderer;

/// Compute a list of [`Patch`]es that transforms `prev` into `next`.
pub fn diff(prev: &Element, next: &Element) -> Vec<Patch> {
    let mut out = Vec::new();
    diff_into(prev, next, Vec::new(), &mut out);
    out
}

fn diff_into(prev: &Element, next: &Element, path: Path, out: &mut Vec<Patch>) {
    if can_reconcile(prev, next).is_some() {
        out.push(Patch::Replace {
            path,
            new: next.clone(),
        });
        return;
    }

    // Attributes diff (sorted-merge).
    let mut i = 0;
    let mut j = 0;
    while i < prev.attrs.len() && j < next.attrs.len() {
        match prev.attrs[i].name.cmp(&next.attrs[j].name) {
            std::cmp::Ordering::Equal => {
                if prev.attrs[i].value != next.attrs[j].value {
                    out.push(Patch::SetAttribute {
                        path: path.clone(),
                        name: next.attrs[j].name.clone(),
                        value: next.attrs[j].value.clone(),
                    });
                }
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => {
                out.push(Patch::RemoveAttribute {
                    path: path.clone(),
                    name: prev.attrs[i].name.clone(),
                });
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(Patch::SetAttribute {
                    path: path.clone(),
                    name: next.attrs[j].name.clone(),
                    value: next.attrs[j].value.clone(),
                });
                j += 1;
            }
        }
    }
    while i < prev.attrs.len() {
        out.push(Patch::RemoveAttribute {
            path: path.clone(),
            name: prev.attrs[i].name.clone(),
        });
        i += 1;
    }
    while j < next.attrs.len() {
        out.push(Patch::SetAttribute {
            path: path.clone(),
            name: next.attrs[j].name.clone(),
            value: next.attrs[j].value.clone(),
        });
        j += 1;
    }

    if prev.styles != next.styles {
        out.push(Patch::SetInlineStyles {
            path: path.clone(),
            css: next.styles.clone(),
        });
    }

    if events_differ(&prev.events, &next.events) {
        out.push(Patch::ReplaceEvents {
            path: path.clone(),
            events: next.events.clone(),
        });
    }

    let any_keyed = prev.children.iter().any(|c| c.key.is_some())
        || next.children.iter().any(|c| c.key.is_some());
    if any_keyed {
        diff_keyed_children(&prev.children, &next.children, &path, out);
    } else {
        diff_indexed_children(&prev.children, &next.children, &path, out);
    }
}

fn events_differ(a: &[EventHandler], b: &[EventHandler]) -> bool {
    if a.len() != b.len() {
        return true;
    }
    let names_a: Vec<&str> = a.iter().map(|h| h.name.as_str()).collect();
    let names_b: Vec<&str> = b.iter().map(|h| h.name.as_str()).collect();
    names_a != names_b
}

fn diff_indexed_children(
    prev: &[Element],
    next: &[Element],
    parent_path: &[usize],
    out: &mut Vec<Patch>,
) {
    let common = prev.len().min(next.len());
    for i in 0..common {
        let mut child_path: Path = parent_path.to_vec();
        child_path.push(i);
        diff_into(&prev[i], &next[i], child_path, out);
    }
    // Removes go from the end so the indices stay valid as we strip.
    for _ in next.len()..prev.len() {
        out.push(Patch::RemoveChild {
            parent: parent_path.to_vec(),
            child_index: prev.len() - 1 - (prev.len() - next.len() - 1),
        });
    }
    for child in next.iter().skip(prev.len()) {
        out.push(Patch::AppendChild {
            parent: parent_path.to_vec(),
            node: child.clone(),
        });
    }
}

fn diff_keyed_children(
    prev: &[Element],
    next: &[Element],
    parent_path: &[usize],
    out: &mut Vec<Patch>,
) {
    use std::collections::HashMap;

    let mut prev_index: HashMap<&str, usize> = HashMap::new();
    for (i, child) in prev.iter().enumerate() {
        if let Some(k) = child.key.as_deref() {
            prev_index.insert(k, i);
        }
    }
    let mut consumed = vec![false; prev.len()];

    for (next_idx, child) in next.iter().enumerate() {
        let mut child_path = parent_path.to_vec();
        child_path.push(next_idx);
        match child.key.as_deref().and_then(|k| prev_index.get(k)) {
            Some(&prev_idx) => {
                consumed[prev_idx] = true;
                diff_into(&prev[prev_idx], child, child_path, out);
            }
            None => {
                let pair = (0..prev.len()).find(|&i| {
                    !consumed[i] && prev[i].key.is_none() && i == next_idx
                });
                if let Some(prev_idx) = pair {
                    consumed[prev_idx] = true;
                    diff_into(&prev[prev_idx], child, child_path, out);
                } else {
                    out.push(Patch::InsertChildBefore {
                        parent: parent_path.to_vec(),
                        child_index: next_idx,
                        node: child.clone(),
                    });
                }
            }
        }
    }

    let mut removed_offset = 0;
    for (i, used) in consumed.iter().enumerate() {
        if !used {
            out.push(Patch::RemoveChild {
                parent: parent_path.to_vec(),
                child_index: i - removed_offset,
            });
            removed_offset += 1;
        }
    }
}

// ----------------------------------------------------------------------------
// Apply
// ----------------------------------------------------------------------------

/// Apply `patches` to a renderer, updating `handles` in place so it stays
/// in sync with the (now-current) tree.
///
/// `handles` is the [`HandleTree`] returned by the most recent
/// [`crate::render::mount`] / [`apply`] call.
pub fn apply<R: Renderer>(
    renderer: &mut R,
    handles: &mut HandleTree<R::ElementHandle>,
    patches: &[Patch],
) {
    for patch in patches {
        apply_one(renderer, handles, patch);
    }
}

fn apply_one<R: Renderer>(
    renderer: &mut R,
    handles: &mut HandleTree<R::ElementHandle>,
    patch: &Patch,
) {
    match patch {
        Patch::SetAttribute { path, name, value } => {
            if let Some(h) = handles.at_path(path) {
                renderer.set_attribute(h, name, value);
            }
        }
        Patch::RemoveAttribute { path, name } => {
            // The Renderer trait has no `remove_attribute` (Lynx accepts
            // empty-string SetAttribute to clear). Future iteration:
            // grow the trait.
            if let Some(h) = handles.at_path(path) {
                renderer.set_attribute(h, name, "");
            }
        }
        Patch::SetInlineStyles { path, css } => {
            if let Some(h) = handles.at_path(path) {
                renderer.set_inline_styles(h, css);
            }
        }
        Patch::AppendChild { parent, node } => {
            let parent_handle = match handles.at_path(parent) {
                Some(h) => h,
                None => return,
            };
            let new_subtree = build_subtree(renderer, node);
            renderer.append_child(parent_handle, new_subtree.handle);
            if let Some(parent_node) = handles.subtree_mut(parent) {
                parent_node.children.push(new_subtree);
            }
        }
        Patch::RemoveChild { parent, child_index } => {
            let parent_handle = match handles.at_path(parent) {
                Some(h) => h,
                None => return,
            };
            let parent_node = match handles.subtree_mut(parent) {
                Some(n) => n,
                None => return,
            };
            if *child_index >= parent_node.children.len() {
                return;
            }
            let removed = parent_node.children.remove(*child_index);
            release_recursive(renderer, &removed, parent_handle);
        }
        Patch::InsertChildBefore { parent, child_index, node } => {
            // No native insert_before in the Renderer trait yet — append
            // and then move-by-removing-and-re-inserting on the C++ side
            // would be ideal, but we don't have that primitive either.
            // For now, append. Real reorder support is a follow-up.
            let parent_handle = match handles.at_path(parent) {
                Some(h) => h,
                None => return,
            };
            let new_subtree = build_subtree(renderer, node);
            renderer.append_child(parent_handle, new_subtree.handle);
            if let Some(parent_node) = handles.subtree_mut(parent) {
                let idx = (*child_index).min(parent_node.children.len());
                parent_node.children.insert(idx, new_subtree);
            }
        }
        Patch::Replace { path, new } => {
            // Build the new subtree first, then swap. Replace at root has
            // no parent, which the bridge can handle via a fresh
            // SetRoot. For non-root nodes, we'd need an "insert at index"
            // primitive on the parent — left as a follow-up.
            let new_subtree = build_subtree(renderer, new);
            if path.is_empty() {
                renderer.set_root(new_subtree.handle);
                let old_root = std::mem::replace(handles, new_subtree);
                renderer.release_element(old_root.handle);
            }
            // For non-root replace we leak the old subtree's handle for
            // now. That's a known limitation of this iteration.
        }
        Patch::ReplaceEvents { .. } => {
            // Event wiring is a runtime concern (see signal/runtime).
            // Patch is observed; renderer trait doesn't expose listener
            // mutation yet.
        }
    }
}

fn release_recursive<R: Renderer>(
    renderer: &mut R,
    tree: &HandleTree<R::ElementHandle>,
    parent_handle: R::ElementHandle,
) {
    renderer.remove_child(parent_handle, tree.handle);
    fn drop_subtree<R: Renderer>(renderer: &mut R, tree: &HandleTree<R::ElementHandle>) {
        for child in &tree.children {
            drop_subtree(renderer, child);
        }
        renderer.release_element(tree.handle);
    }
    drop_subtree(renderer, tree);
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::*;
    use crate::render::mount;
    use crate::renderer::{MockOp, MockRenderer};

    fn kinds(patches: &[Patch]) -> Vec<&'static str> {
        patches.iter().map(Patch::kind).collect()
    }

    #[test]
    fn identical_trees_produce_no_patches() {
        let prev = view().style("color: red").child(text_with("hi"));
        let next = view().style("color: red").child(text_with("hi"));
        assert!(diff(&prev, &next).is_empty());
    }

    #[test]
    fn tag_change_produces_replace_at_root() {
        let prev = view();
        let next = text();
        let patches = diff(&prev, &next);
        assert_eq!(kinds(&patches), vec!["Replace"]);
    }

    #[test]
    fn key_change_produces_replace() {
        let prev = view().key("a");
        let next = view().key("b");
        let patches = diff(&prev, &next);
        assert_eq!(kinds(&patches), vec!["Replace"]);
    }

    #[test]
    fn style_change_produces_set_inline_styles() {
        let prev = view().style("color: red");
        let next = view().style("color: blue");
        let patches = diff(&prev, &next);
        assert_eq!(patches.len(), 1);
        match &patches[0] {
            Patch::SetInlineStyles { css, path } => {
                assert_eq!(css, "color: blue");
                assert!(path.is_empty(), "root path");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn attribute_added() {
        let prev = view();
        let next = view().attr("class", "x");
        let patches = diff(&prev, &next);
        assert_eq!(kinds(&patches), vec!["SetAttribute"]);
    }

    #[test]
    fn attribute_removed() {
        let prev = view().attr("class", "x");
        let next = view();
        let patches = diff(&prev, &next);
        assert_eq!(kinds(&patches), vec!["RemoveAttribute"]);
    }

    #[test]
    fn attribute_changed() {
        let prev = view().attr("class", "old");
        let next = view().attr("class", "new");
        let patches = diff(&prev, &next);
        assert_eq!(kinds(&patches), vec!["SetAttribute"]);
        match &patches[0] {
            Patch::SetAttribute { value, .. } => assert_eq!(value, "new"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn child_added_to_indexed_list() {
        let prev = view();
        let next = view().child(text_with("x"));
        let patches = diff(&prev, &next);
        assert_eq!(kinds(&patches), vec!["AppendChild"]);
    }

    #[test]
    fn child_removed_from_indexed_list() {
        let prev = view().child(text_with("x"));
        let next = view();
        let patches = diff(&prev, &next);
        assert_eq!(kinds(&patches), vec!["RemoveChild"]);
    }

    #[test]
    fn deep_text_change_produces_attribute_patch_at_correct_path() {
        let prev = page().child(text_with("hello"));
        let next = page().child(text_with("hi"));
        let patches = diff(&prev, &next);
        assert_eq!(patches.len(), 1);
        match &patches[0] {
            Patch::SetAttribute { name, value, path } => {
                assert_eq!(name, "text");
                assert_eq!(value, "hi");
                assert_eq!(path, &vec![0, 0], "page > text > raw_text");
            }
            other => panic!("unexpected patch: {other:?}"),
        }
    }

    // ---- apply integration tests --------------------------------------

    #[test]
    fn apply_set_attribute_uses_path_handle() {
        let initial = page().child(text_with("hello"));
        let updated = page().child(text_with("world"));

        let mut r = MockRenderer::new();
        let mut handles = mount(&mut r, &initial);
        let patches = diff(&initial, &updated);
        let initial_op_count = r.ops().len();

        apply(&mut r, &mut handles, &patches);

        // The update should hit raw_text (the deepest handle, id 3).
        let new_ops = &r.ops()[initial_op_count..];
        assert_eq!(new_ops.len(), 1);
        match &new_ops[0] {
            MockOp::SetAttribute { handle, key, value } => {
                assert_eq!(*handle, 3, "should target the raw_text handle, not root");
                assert_eq!(key, "text");
                assert_eq!(value, "world");
            }
            other => panic!("unexpected op: {other:?}"),
        }
    }

    #[test]
    fn apply_set_inline_styles_uses_path_handle() {
        let initial = page().child(view().style("color: red"));
        let updated = page().child(view().style("color: blue"));

        let mut r = MockRenderer::new();
        let mut handles = mount(&mut r, &initial);
        let patches = diff(&initial, &updated);
        let before = r.ops().len();
        apply(&mut r, &mut handles, &patches);

        let new_ops = &r.ops()[before..];
        assert_eq!(new_ops.len(), 1);
        match &new_ops[0] {
            MockOp::SetInlineStyles { handle, css } => {
                assert_eq!(*handle, 2, "should target the inner view");
                assert_eq!(css, "color: blue");
            }
            other => panic!("unexpected op: {other:?}"),
        }
    }

    #[test]
    fn apply_append_child_grows_handle_tree() {
        let initial = view();
        let updated = view().child(text_with("new"));

        let mut r = MockRenderer::new();
        let mut handles = mount(&mut r, &initial);
        assert_eq!(handles.children.len(), 0);

        let patches = diff(&initial, &updated);
        apply(&mut r, &mut handles, &patches);

        assert_eq!(handles.children.len(), 1, "append must grow handle tree");
    }

    #[test]
    fn apply_remove_child_shrinks_handle_tree_and_releases() {
        let initial = view().child(text_with("first"));
        let updated = view();

        let mut r = MockRenderer::new();
        let mut handles = mount(&mut r, &initial);
        assert_eq!(handles.children.len(), 1);

        let before = r.ops().len();
        let patches = diff(&initial, &updated);
        apply(&mut r, &mut handles, &patches);

        assert_eq!(handles.children.len(), 0);
        let new_ops = &r.ops()[before..];
        assert!(new_ops.iter().any(|op| matches!(op, MockOp::RemoveChild { .. })));
        assert!(new_ops.iter().any(|op| matches!(op, MockOp::Release { .. })));
    }

    // ---- events_differ -------------------------------------------------

    #[test]
    fn events_differ_when_count_changes() {
        let prev = view();
        let next = view().on("tap", || {});
        assert_eq!(kinds(&diff(&prev, &next)), vec!["ReplaceEvents"]);
    }

    #[test]
    fn events_differ_when_a_handler_is_removed() {
        let prev = view().on("tap", || {});
        let next = view();
        assert_eq!(kinds(&diff(&prev, &next)), vec!["ReplaceEvents"]);
    }

    #[test]
    fn events_differ_when_handler_name_changes() {
        let prev = view().on("tap", || {});
        let next = view().on("longpress", || {});
        assert_eq!(kinds(&diff(&prev, &next)), vec!["ReplaceEvents"]);
    }

    #[test]
    fn events_with_same_names_produce_no_replaceevents_patch() {
        // Different closures but identical names → no patch. We diff
        // by *name*, not by closure identity (Box<dyn Fn> can't be
        // compared structurally).
        let prev = view().on("tap", || {});
        let next = view().on("tap", || {});
        let patches = diff(&prev, &next);
        assert!(
            !patches.iter().any(|p| matches!(p, Patch::ReplaceEvents { .. })),
            "same event names should not trigger ReplaceEvents, got {patches:?}",
        );
    }

    // ---- RemoveAttribute apply ----------------------------------------

    #[test]
    fn apply_remove_attribute_clears_via_empty_set_attribute() {
        // RemoveAttribute → renderer.set_attribute(handle, name, "")
        // because the Renderer trait has no remove_attribute primitive.
        let initial = view().attr("class", "x");
        let updated = view();

        let mut r = MockRenderer::new();
        let mut handles = mount(&mut r, &initial);
        let before = r.ops().len();
        let patches = diff(&initial, &updated);
        apply(&mut r, &mut handles, &patches);

        let new_ops = &r.ops()[before..];
        assert_eq!(new_ops.len(), 1);
        match &new_ops[0] {
            MockOp::SetAttribute { handle, key, value } => {
                assert_eq!(*handle, 1, "root view");
                assert_eq!(key, "class");
                assert_eq!(value, "");
            }
            other => panic!("unexpected op: {other:?}"),
        }
    }

    // ---- Replace at root ----------------------------------------------

    #[test]
    fn apply_replace_at_root_resets_root_and_releases_old() {
        let initial = view();
        let updated = text();

        let mut r = MockRenderer::new();
        let mut handles = mount(&mut r, &initial);
        let old_handle = handles.handle;
        let before = r.ops().len();

        let patches = diff(&initial, &updated);
        assert_eq!(kinds(&patches), vec!["Replace"]);
        apply(&mut r, &mut handles, &patches);

        let new_ops = &r.ops()[before..];
        // Expect: Create new (text) → SetRoot(new) → Release(old).
        assert!(
            new_ops.iter().any(|op| matches!(op, MockOp::SetRoot { .. })),
            "Replace at root must call set_root, got {new_ops:?}",
        );
        assert!(
            new_ops
                .iter()
                .any(|op| matches!(op, MockOp::Release { handle } if *handle == old_handle)),
            "Replace at root must release the old root handle ({old_handle}), got {new_ops:?}",
        );
    }

    // ---- Keyed children -----------------------------------------------

    #[test]
    fn keyed_child_reorder_diffs_in_place_no_replace() {
        // a, b → b, a. Both keys exist on both sides, so no
        // Insert/Remove — the diff just recurses into each match and
        // (since payloads are identical) produces zero patches.
        let prev = view()
            .child(text_with("A").key("a"))
            .child(text_with("B").key("b"));
        let next = view()
            .child(text_with("B").key("b"))
            .child(text_with("A").key("a"));
        let patches = diff(&prev, &next);
        assert!(
            patches.is_empty(),
            "keyed reorder of identical payloads should be a no-op, got {patches:?}",
        );
    }

    #[test]
    fn keyed_child_removed_produces_remove_patch() {
        let prev = view()
            .child(text_with("A").key("a"))
            .child(text_with("B").key("b"));
        let next = view().child(text_with("A").key("a"));
        let kinds_v = kinds(&diff(&prev, &next));
        assert!(
            kinds_v.contains(&"RemoveChild"),
            "expected a RemoveChild, got {kinds_v:?}",
        );
    }

    #[test]
    fn keyed_child_added_produces_insert_patch() {
        let prev = view().child(text_with("A").key("a"));
        let next = view()
            .child(text_with("A").key("a"))
            .child(text_with("B").key("b"));
        let kinds_v = kinds(&diff(&prev, &next));
        assert!(
            kinds_v.contains(&"InsertChildBefore"),
            "expected an InsertChildBefore for the new key, got {kinds_v:?}",
        );
    }

    // ---- InsertChildBefore apply --------------------------------------

    #[test]
    fn apply_insert_child_before_grows_handle_tree() {
        let initial = view().child(text_with("A").key("a"));
        let updated = view()
            .child(text_with("A").key("a"))
            .child(text_with("B").key("b"));

        let mut r = MockRenderer::new();
        let mut handles = mount(&mut r, &initial);
        assert_eq!(handles.children.len(), 1);

        let patches = diff(&initial, &updated);
        apply(&mut r, &mut handles, &patches);

        assert_eq!(handles.children.len(), 2, "insert must grow the tree");
    }
}
