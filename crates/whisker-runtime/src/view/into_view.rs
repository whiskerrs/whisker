//! [`IntoView`] — uniform return type for components.
//!
//! A component fn returns `impl IntoView`. The renderer or parent
//! component calls `.into_view()` to get either an [`Element`]
//! (for "this is one element") or a `View` (for fragments, tuples,
//! components nested inside `render!`).
//!
//! The trait is intentionally minimal: every implementor produces a
//! [`View`], which is a single element, a text snippet, a fragment of
//! children, or empty.

use super::handle::Element;

/// A renderable. Components return `impl IntoView`; the renderer
/// (called by the macro at mount time, or by the parent's
/// `render!` expansion) calls `.into_view()` to get the underlying
/// `View`.
pub trait IntoView {
    fn into_view(self) -> View;
}

/// Collector used by public builder `body` methods.
///
/// The compose macros only call [`push`](Self::push); the concrete parent
/// builder decides whether to attach the collected views immediately or keep
/// a closure for later component remounts.
#[derive(Default)]
pub struct ChildrenBuilder {
    children: Vec<View>,
}

impl ChildrenBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, child: impl IntoView) {
        self.children.push(child.into_view());
    }

    pub fn extend(&mut self, children: impl IntoIterator<Item = impl IntoView>) {
        self.children
            .extend(children.into_iter().map(IntoView::into_view));
    }

    pub fn finish(self) -> View {
        match self.children.len() {
            0 => View::Empty,
            1 => self.children.into_iter().next().unwrap_or(View::Empty),
            _ => View::Fragment(self.children),
        }
    }
}

/// Type used by `#[component]` for the conventional `children` prop.
/// The `render!` macro routes a component invocation's non-kwarg
/// children into a `move || View::Fragment(…)` closure of this type;
/// the component body invokes it to materialise the children at the
/// point in the tree where they should appear.
///
/// `Fn` (not `FnOnce`) so the closure survives being re-invoked across
/// hot-reload remounts, and `Rc` (not `Box`) so `Children` is `Clone` —
/// `#[component]` re-clones every prop on every body invocation.
pub type Children = ::std::rc::Rc<dyn ::std::ops::Fn() -> View + 'static>;

/// Authoring children whose rendered value must contain only plain-text
/// fragments.
///
/// This wrapper lets an element declaration distinguish text content from
/// ordinary scene children without changing the React-like `View` value built
/// by `render!`. The Rust renderer validates and normalizes the materialized
/// raw-text nodes before a Host sees the resulting `SetText` operation.
#[derive(Clone)]
pub struct TextChildren(pub Children);

impl TextChildren {
    /// Wraps the children closure emitted by `render!`.
    pub fn new(children: Children) -> Self {
        Self(children)
    }

    /// Materializes the wrapped authoring children.
    pub fn render(&self) -> View {
        (self.0)()
    }
}

/// Mount a `Children` prop at the current position in the tree.
///
/// Returns a phantom element (no on-screen footprint — `is_phantom`
/// is true) with the children's view attached. The caller (typically
/// framework-level control flow projecting a [`Children`] handle)
/// hands this back to the parent so the parent
/// sees a single Element handle, exactly like any other child node.
///
/// Takes `&Children` so nothing moves out of the surrounding
/// `#[component]` body and the hot-reload outer can re-call it without
/// `cannot move out of`. The children `Rc` is left untouched, so it can
/// be handed to further `mount_children` calls at other positions
/// (multi-projection a la Leptos `ChildrenFn`).
pub fn mount_children(children: &Children) -> Element {
    let ph = super::create_phantom_element();
    let view = children();
    view.attach_to(ph);
    ph
}

/// Mounts one dynamically typed `IntoView` value behind a phantom slot.
#[doc(hidden)]
pub fn mount_view(view: impl IntoView) -> Element {
    let ph = super::create_phantom_element();
    view.into_view().attach_to(ph);
    ph
}

/// Mount plain-text authoring children at their declared element.
pub fn mount_text_children(children: &TextChildren, parent: Element) -> Vec<Element> {
    children.render().attach_to(parent)
}

// Function-shaped prop types for control-flow components. These
// newtypes let `#[component]`-annotated control-flow functions accept
// closure literals via `Into` in the typed-builder `Props`, and wrap
// `Rc<dyn Fn>` for the same `Clone` reason as [`Children`].

/// `Fn() -> Vec<T>` — the "what items to render" closure for a
/// keyed-list control flow. Wrapping in a newtype gives typed-builder
/// a concrete Props field type plus an `Into` path from any matching
/// closure literal.
pub struct EachFn<T: 'static>(pub ::std::rc::Rc<dyn ::std::ops::Fn() -> Vec<T> + 'static>);

impl<T: 'static> Clone for EachFn<T> {
    fn clone(&self) -> Self {
        EachFn(::std::rc::Rc::clone(&self.0))
    }
}

impl<T: 'static, F: Fn() -> Vec<T> + 'static> From<F> for EachFn<T> {
    fn from(f: F) -> Self {
        EachFn(::std::rc::Rc::new(f))
    }
}

impl<T: 'static> EachFn<T> {
    /// Invoke the wrapped closure.
    pub fn call(&self) -> Vec<T> {
        (self.0)()
    }
}

/// `Fn(&T) -> K` — the "key extractor" closure for a keyed-list
/// control flow. Items whose keys match across reactive reruns
/// reuse their owners + per-item state.
pub struct KeyFn<T: 'static, K: 'static>(pub ::std::rc::Rc<dyn ::std::ops::Fn(&T) -> K + 'static>);

impl<T: 'static, K: 'static> Clone for KeyFn<T, K> {
    fn clone(&self) -> Self {
        KeyFn(::std::rc::Rc::clone(&self.0))
    }
}

impl<T: 'static, K: 'static, F: Fn(&T) -> K + 'static> From<F> for KeyFn<T, K> {
    fn from(f: F) -> Self {
        KeyFn(::std::rc::Rc::new(f))
    }
}

impl<T: 'static, K: 'static> KeyFn<T, K> {
    /// Invoke the wrapped closure on `item`.
    pub fn call(&self, item: &T) -> K {
        (self.0)(item)
    }
}

/// `Fn(T) -> Element` — the "render one item" closure for a
/// keyed-list control flow. The returned [`Element`] is what gets
/// attached to the surrounding fragment / list.
pub struct ItemFn<T: 'static>(pub ::std::rc::Rc<dyn ::std::ops::Fn(T) -> Element + 'static>);

impl<T: 'static> Clone for ItemFn<T> {
    fn clone(&self) -> Self {
        ItemFn(::std::rc::Rc::clone(&self.0))
    }
}

impl<T: 'static, F: Fn(T) -> Element + 'static> From<F> for ItemFn<T> {
    fn from(f: F) -> Self {
        ItemFn(::std::rc::Rc::new(f))
    }
}

impl<T: 'static> ItemFn<T> {
    /// Invoke the wrapped closure on `item`.
    pub fn call(&self, item: T) -> Element {
        (self.0)(item)
    }
}

/// `Fn() -> bool` — the predicate closure for a `show`-style
/// conditional control flow. Wrapping in a newtype gives
/// typed-builder a concrete Props field type plus an `Into` path
/// from any matching closure literal.
pub struct WhenFn(pub ::std::rc::Rc<dyn ::std::ops::Fn() -> bool + 'static>);

impl Clone for WhenFn {
    fn clone(&self) -> Self {
        WhenFn(::std::rc::Rc::clone(&self.0))
    }
}

impl<F: Fn() -> bool + 'static> From<F> for WhenFn {
    fn from(f: F) -> Self {
        WhenFn(::std::rc::Rc::new(f))
    }
}

impl WhenFn {
    /// Invoke the wrapped predicate.
    pub fn call(&self) -> bool {
        (self.0)()
    }
}

/// The fallback branch of a `show`-style conditional. Wraps an
/// optional `Fn() -> Element` closure — `None` means "render
/// nothing on false"; `Some(closure)` is what the user typed as
/// `fallback: || …`.
///
/// `From<F: Fn() -> Element + 'static>` lets a closure literal flow
/// through typed-builder's `Into<Fallback>` path; `Default` (used via
/// `#[prop(default)]`) is `None`. The closure returns an `Element`
/// rather than `Children`'s `View` because the typical fallback is a
/// single component invocation like `|| render! { status_banner(...) }`.
#[derive(Clone, Default)]
pub struct Fallback(pub Option<::std::rc::Rc<dyn ::std::ops::Fn() -> Element + 'static>>);

impl<F: Fn() -> Element + 'static> From<F> for Fallback {
    fn from(f: F) -> Self {
        Fallback(Some(::std::rc::Rc::new(f)))
    }
}

/// A rendered (or about-to-be-rendered) tree fragment.
#[derive(Debug, Clone)]
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
            View::Text(_) | View::Empty => {}
        }
    }
}

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

// Text-shaped `IntoView` impls. These are what let a `{expr}` inside
// `render!` interpolate a scalar directly. No blanket
// `impl<T: Display>`: an explicit list keeps the surface predictable
// and the orphan rules tractable, and custom types implement
// `IntoView` themselves.

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
