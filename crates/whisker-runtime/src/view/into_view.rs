//! [`IntoView`] — uniform return type for components.
//!
//! A component fn returns `impl IntoView`. The renderer or parent
//! component calls `.into_view()` to get either an [`ElementHandle`]
//! (for "this is one element") or a `View` (for fragments, tuples,
//! components nested inside `render!`).
//!
//! The trait is intentionally minimal: every implementor produces a
//! `View`. A `View` is either a single element, a fragment of
//! children, or a "marker" view (used by `Show`/`For` to mark
//! reactive boundaries — Phase 6.5a A3 Step 4).

use super::handle::ElementHandle;

/// A renderable. Components return `impl IntoView`; the renderer
/// (called by the macro at mount time, or by the parent's
/// `render!` expansion) calls `.into_view()` to get the underlying
/// `View`.
pub trait IntoView {
    fn into_view(self) -> View;
}

/// A rendered (or about-to-be-rendered) tree fragment.
#[derive(Debug, Clone)]
pub enum View {
    /// A single element. The most common shape.
    Element(ElementHandle),
    /// Zero-or-more children, in order. Used for tuples / option-some
    /// / iterator-flatten / fragments.
    Fragment(Vec<View>),
    /// A view with no on-screen footprint. Used for `Show { when: false }`
    /// and similar conditional skips; carries no children to mount.
    Empty,
}

impl View {
    /// Attach this view as a child of `parent`, calling
    /// [`append_child`](super::append_child) for every leaf element it
    /// contains. Fragments are spread in order; `Empty` is a no-op.
    pub fn attach_to(&self, parent: ElementHandle) {
        match self {
            View::Element(h) => super::append_child(parent, *h),
            View::Fragment(children) => {
                for child in children {
                    child.attach_to(parent);
                }
            }
            View::Empty => {}
        }
    }

    /// Collect the leaf element handles this view contributes, in
    /// child-order. Useful for renderers that want to assign sibling
    /// indices, and for unit tests inspecting the result.
    pub fn elements(&self) -> Vec<ElementHandle> {
        let mut out = Vec::new();
        self.collect_into(&mut out);
        out
    }

    fn collect_into(&self, out: &mut Vec<ElementHandle>) {
        match self {
            View::Element(h) => out.push(*h),
            View::Fragment(children) => {
                for c in children {
                    c.collect_into(out);
                }
            }
            View::Empty => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Stock IntoView impls
// ---------------------------------------------------------------------------

impl IntoView for View {
    fn into_view(self) -> View {
        self
    }
}

impl IntoView for ElementHandle {
    fn into_view(self) -> View {
        View::Element(self)
    }
}

impl IntoView for () {
    fn into_view(self) -> View {
        View::Empty
    }
}

impl<T: IntoView> IntoView for Option<T> {
    fn into_view(self) -> View {
        match self {
            Some(v) => v.into_view(),
            None => View::Empty,
        }
    }
}

// Tuple impls for 1–8 elements. Tuples render as fragments —
// children mount in declaration order.
macro_rules! impl_into_view_tuple {
    ($($name:ident),+) => {
        impl<$($name: IntoView),+> IntoView for ($($name,)+) {
            #[allow(non_snake_case)]
            fn into_view(self) -> View {
                let ($($name,)+) = self;
                View::Fragment(vec![$($name.into_view()),+])
            }
        }
    };
}

impl_into_view_tuple!(A);
impl_into_view_tuple!(A, B);
impl_into_view_tuple!(A, B, C);
impl_into_view_tuple!(A, B, C, D);
impl_into_view_tuple!(A, B, C, D, E);
impl_into_view_tuple!(A, B, C, D, E, F);
impl_into_view_tuple!(A, B, C, D, E, F, G);
impl_into_view_tuple!(A, B, C, D, E, F, G, H);
