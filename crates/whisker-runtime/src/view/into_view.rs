//! [`IntoView`] — uniform return type for components.
//!
//! A component fn returns `impl IntoView`. The renderer or parent
//! component calls `.into_view()` to get either an [`Element`]
//! (for "this is one element") or a `View` (for fragments, tuples,
//! components nested inside `render!`).
//!
//! The trait is intentionally minimal: every implementor produces a
//! `View`. A `View` is either a single element, a fragment of
//! children, a control-flow node (`Show`/`For` reactive boundary,
//! attaches itself via [`ControlFlow`]), or empty.

use std::cell::RefCell;
use std::rc::Rc;

use super::handle::Element;

/// A renderable. Components return `impl IntoView`; the renderer
/// (called by the macro at mount time, or by the parent's
/// `render!` expansion) calls `.into_view()` to get the underlying
/// `View`.
pub trait IntoView {
    fn into_view(self) -> View;
}

/// Type used by `#[component]` for the conventional `children` prop.
/// The `render!` macro routes a component invocation's non-kwarg
/// children into a `move || View::Fragment(…)` closure of this type;
/// the component body invokes it to materialise the children at the
/// point in the tree where they should appear.
///
/// Two design points:
///
/// - `Fn` (not `FnOnce`) so the closure can be re-invoked across
///   hot-reload remounts and similar "re-run the body" paths.
/// - `Rc` (not `Box`) so `Children` itself implements `Clone`. The
///   `#[component]` macro re-clones every prop on every body
///   invocation, so a `Children` prop has to be a cheaply-cloneable
///   handle — `Rc<dyn Fn>` is one machine word.
pub type Children = ::std::rc::Rc<dyn ::std::ops::Fn() -> View + 'static>;

/// Shared list-item state owned by the [`list`](crate::view) builder
/// and handed to every [`ControlFlow::attach_to_list`] call. The
/// native-item-provider closure installed in `list.__h()` reads
/// from the same `Rc<RefCell<…>>`, so a control flow's effect can
/// mutate the items on every reactive update and the next
/// `componentAtIndex` lookup sees the new state.
///
/// Each entry is `(child element handle, Lynx `impl_id` sign)`.
/// The sign is captured eagerly at insert time because the provider
/// closure runs from inside Lynx's layout C++ which is itself
/// already inside [`with_renderer`](super::renderer)'s borrow — a
/// reentrant `element_sign` call would panic on the `RefCell::borrow_mut`.
pub type ListItemsHandle = Rc<RefCell<Vec<(Element, i32)>>>;

/// A view that takes responsibility for its own attachment instead
/// of being `append_child`-ed directly by the parent builder.
///
/// Used by the wrapper-less `For` / `Show` control flow — see
/// [`docs/wrapper-less-control-flow-design.md`](https://github.com/whiskerrs/whisker/blob/main/docs/wrapper-less-control-flow-design.md).
/// Today there are exactly two implementors; the trait is
/// extension-ready for other reactive-children primitives.
///
/// `Box<dyn ControlFlow>` is the carrier inside [`View::ControlFlow`].
/// Container builders dispatch via [`View::materialise_into`] or, for
/// `<list>`, by pattern-matching `View::ControlFlow` and routing to
/// [`ControlFlow::attach_to_list`] explicitly.
pub trait ControlFlow {
    /// Attach into a generic container (`view`, `scroll_view`, …).
    /// The implementor inserts a *phantom* anchor at `parent`'s
    /// current end (via [`super::create_phantom_element`]) to mark
    /// its range, then mounts reactive children as siblings *before*
    /// the anchor on every effect run.
    fn attach_generic(self: Box<Self>, parent: Element);

    /// Attach into a `<list>` container. The implementor writes
    /// `(handle, sign)` tuples directly into the list's shared
    /// [`ListItemsHandle`] and broadcasts the count via
    /// [`super::set_update_list_info`] on every update. Items go
    /// straight into the list's children (no anchor needed — the
    /// list's native-item-provider indexes into the items Vec).
    fn attach_to_list(self: Box<Self>, list_handle: Element, items: ListItemsHandle);
}

/// A rendered (or about-to-be-rendered) tree fragment.
pub enum View {
    /// A single element handle the caller has already created.
    Element(Element),
    /// A text snippet — `materialize` creates a `raw_text` element
    /// with `text=<value>`. The `IntoView` impls for `&str` /
    /// `String` / primitive numeric types route through here so
    /// `{count.get()}` inside `render!` can interpolate scalar
    /// values without the caller having to manually wrap them in a
    /// `raw_text { … }` element.
    Text(String),
    /// Zero-or-more child views in order. Tuples, option-some /
    /// option-none → empty, iterator flattening, and the macro's
    /// multi-child `Show` children all use this.
    Fragment(Vec<View>),
    /// A control-flow node (`For` / `Show`) that takes
    /// responsibility for its own attachment. Container builders
    /// hand it the parent via [`ControlFlow::attach_generic`]; the
    /// `<list>` builder calls [`ControlFlow::attach_to_list`]
    /// instead. The wrapped value is `Box<dyn ControlFlow>` — not
    /// `Clone` and not `Debug`, which is why this enum dropped its
    /// previous `Debug + Clone` derives.
    ControlFlow(Box<dyn ControlFlow>),
    /// A view with no on-screen footprint — `Show { when: false }`
    /// and `Option::None`.
    Empty,
}

impl View {
    /// Realise the view: create whatever element handles the text /
    /// fragment variants require, append them to `parent`, and return
    /// the resulting flat list of leaf handles in attach order. The
    /// returned list is what the `{expr}` macro path stashes so the
    /// next effect re-run can detach the previous children before
    /// attaching the new ones.
    pub fn attach_to(self, parent: Element) -> Vec<Element> {
        let mut out = Vec::new();
        self.materialise_into(parent, &mut out);
        out
    }

    fn materialise_into(self, parent: Element, out: &mut Vec<Element>) {
        match self {
            View::Element(h) => {
                super::append_child(parent, h);
                out.push(h);
            }
            View::Text(s) => {
                let h = super::create_element(crate::element::ElementTag::RawText);
                super::set_attribute(h, "text", &s);
                super::append_child(parent, h);
                out.push(h);
            }
            View::Fragment(children) => {
                for child in children {
                    child.materialise_into(parent, out);
                }
            }
            View::ControlFlow(cf) => {
                // Control flow owns its own range — it allocates an
                // anchor, installs an effect, and inserts/updates
                // children itself. It contributes no leaf handles
                // to the caller's `out` because none exist yet
                // (and the ones the effect creates later live
                // under the control flow's internal owners, not
                // the surrounding `{expr}` effect that would have
                // used `out` to detach old children).
                cf.attach_generic(parent);
            }
            View::Empty => {}
        }
    }

    /// Collect the (already-realised) leaf element handles this view
    /// contributes, in child-order. **Only `Element` and `Fragment`
    /// contribute.** `Text` returns nothing here because its element
    /// only exists once `attach_to` has run.
    pub fn elements(&self) -> Vec<Element> {
        let mut out = Vec::new();
        self.collect_into(&mut out);
        out
    }

    fn collect_into(&self, out: &mut Vec<Element>) {
        match self {
            View::Element(h) => out.push(*h),
            View::Fragment(children) => {
                for c in children {
                    c.collect_into(out);
                }
            }
            // Control-flow children aren't yet realised when this
            // is called (no anchor, no effect run, no items). The
            // caller is asking for the "already-materialised leaf
            // handles I contribute"; that set is empty for control
            // flow.
            View::Text(_) | View::ControlFlow(_) | View::Empty => {}
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

impl IntoView for Element {
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

// Text-shaped `IntoView` impls.
//
// Inside `render!`, any `{expr}` that evaluates to a string- or
// number-shaped value is routed through one of these into a
// `View::Text`, which becomes a `raw_text` element when the surrounding
// effect's `attach_to` runs. This is what lets the user write
// `text { {count.get()} }` and `text { {label} }` interchangeably.
//
// We intentionally avoid a blanket `impl<T: Display>` to keep the
// surface predictable and the orphan rules tractable — primitives
// list explicitly, custom types implement `IntoView` themselves.

impl IntoView for String {
    fn into_view(self) -> View {
        View::Text(self)
    }
}

impl IntoView for &str {
    fn into_view(self) -> View {
        View::Text(self.to_owned())
    }
}

impl IntoView for &String {
    fn into_view(self) -> View {
        View::Text(self.clone())
    }
}

macro_rules! impl_into_view_via_display {
    ($($t:ty),+) => {
        $(
            impl IntoView for $t {
                fn into_view(self) -> View {
                    View::Text(self.to_string())
                }
            }
        )+
    };
}

impl_into_view_via_display!(i8, i16, i32, i64, i128, isize);
impl_into_view_via_display!(u8, u16, u32, u64, u128, usize);
impl_into_view_via_display!(f32, f64, bool, char);

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
