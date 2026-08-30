use super::*;

// ---- list (Rust-owned virtualized control primitive) ----------------

/// `list` keeps a bounded window of ordinary item subtrees mounted below
/// the standard `ScrollView`. It is control flow like [`ForEach`], not a
/// Host element: FramePacket contains the ScrollView, spacer Views, and
/// visible item nodes only.
///
/// Use `list` when the data set is large enough that
/// `scroll_view` + a [`ForEach`](crate::ForEach) inside would
/// hold too many off-screen platform views. For short,
/// fully-mounted content prefer the simpler combo.
///
/// ```ignore
/// let items = signal(vec!["alpha".to_string(), "beta".to_string()]);
/// render! {
///     list(
///         each: move || items.get(),
///         key: |s: &String| s.clone(),
///         children: |s: ReadSignal<String>| render! { view { text(value: s) } },
///     )
/// }
/// ```
///
/// # Trade-offs
///
/// The builder takes its items source as three kwargs (`each`,
/// `key`, `children`) and **does not accept a body** — the macro
/// rejects `list { … }` invocations because items can only come
/// through the reactive props. The three setters are
/// **type-stated**: `__h()` is only callable when all three have
/// been supplied, so a missing prop is a compile-time error at
/// the close of the builder chain rather than a runtime panic.
///
/// `__h()` installs one reactive keyed reconciler and one ordinary
/// ScrollView `scroll` listener. The Host reports geometry with the same
/// node event path used by custom elements; no list-specific bridge call
/// exists.
struct ListOptions {
    content: Element,
    content_style: Option<crate::Style>,
    axis: ::whisker_runtime::view::ScrollAxis,
    start_reached_threshold: f32,
    end_reached_threshold: f32,
    on_start_reached: Option<::std::rc::Rc<dyn Fn()>>,
    on_end_reached: Option<::std::rc::Rc<dyn Fn()>>,
    header: Option<::std::rc::Rc<dyn Fn() -> Element>>,
    footer: Option<::std::rc::Rc<dyn Fn() -> Element>>,
    empty: Option<::std::rc::Rc<dyn Fn() -> Element>>,
}

fn configure_list_presentation(
    scroll_view: Element,
    options: &ListOptions,
) -> ::whisker_runtime::view::VirtualListLayout {
    apply_attr(scroll_view, "scroll-orientation", options.axis.to_string());
    crate::style::apply_list_content_style(
        options.content,
        options.axis,
        options.content_style.clone(),
    )
}

#[allow(non_camel_case_types)]
pub struct list<EachF = (), KeyF = (), ChildF = (), RefF = (), InitialF = ()> {
    handle: Element,
    options: ListOptions,
    each: EachF,
    key: KeyF,
    children: ChildF,
    list_ref: RefF,
    initial_scroll: InitialF,
}
#[allow(non_snake_case)]
pub fn __list_ctor() -> list<(), (), ()> {
    // `list` is a Rust control primitive, not a Host element. Its only
    // Host-visible container is the same built-in ScrollView that an app
    // can author directly; the Rust virtualizer mounts ordinary children
    // into a bounded window below it.
    let handle = create_element(ElementTag::ScrollView);
    let content = create_element(ElementTag::View);
    list {
        handle,
        options: ListOptions {
            content,
            content_style: None,
            axis: ::whisker_runtime::view::ScrollAxis::Vertical,
            start_reached_threshold: 0.0,
            end_reached_threshold: 0.0,
            on_start_reached: None,
            on_end_reached: None,
            header: None,
            footer: None,
            empty: None,
        },
        each: (),
        key: (),
        children: (),
        list_ref: (),
        initial_scroll: (),
    }
}
impl<EachF, KeyF, ChildF, RefF, InitialF> ElementBuilder
    for list<EachF, KeyF, ChildF, RefF, InitialF>
{
    fn __element(&self) -> Element {
        self.handle
    }
    // `list` takes its items through the `each`/`key`/`children`
    // render props, never body children.
}
impl<EachF, KeyF, ChildF, RefF, InitialF> list<EachF, KeyF, ChildF, RefF, InitialF> {
    /// Styles the internal content View while `style:` styles the outer
    /// ScrollView viewport. A static typed style may select the constrained
    /// virtualized Grid subset documented in `docs/list-design.md`.
    pub fn content_style<V>(mut self, value: V) -> Self
    where
        V: ::std::convert::Into<crate::Style>,
    {
        self.options.content_style = Some(value.into());
        self
    }

    /// Selects the virtualized main axis. The default is vertical.
    pub fn axis(mut self, axis: ::whisker_runtime::view::ScrollAxis) -> Self {
        self.options.axis = axis;
        self
    }

    /// Enables or disables user-driven scrolling without disabling
    /// imperative ListHandle operations.
    pub fn scroll_enabled<V>(self, value: V) -> Self
    where
        V: ::std::convert::Into<Signal<bool>>,
    {
        apply_attr_bool(self.handle, "enable-scroll", value);
        self
    }

    /// Logical-pixel distance from the start edge at which
    /// `on_start_reached` becomes active.
    pub fn start_reached_threshold(mut self, value: f32) -> Self {
        self.options.start_reached_threshold = value.max(0.0);
        self
    }

    /// Logical-pixel distance from the end edge at which
    /// `on_end_reached` becomes active.
    pub fn end_reached_threshold(mut self, value: f32) -> Self {
        self.options.end_reached_threshold = value.max(0.0);
        self
    }

    /// Fires once when scrolling enters the configured start threshold.
    pub fn on_start_reached<F: Fn() + 'static>(mut self, callback: F) -> Self {
        self.options.on_start_reached = Some(::std::rc::Rc::new(callback));
        self
    }

    /// Fires once when scrolling enters the configured end threshold.
    pub fn on_end_reached<F: Fn() + 'static>(mut self, callback: F) -> Self {
        self.options.on_end_reached = Some(::std::rc::Rc::new(callback));
        self
    }

    /// Builds persistent content before the virtualized item range.
    pub fn header<F: Fn() -> Element + 'static>(mut self, content: F) -> Self {
        self.options.header = Some(::std::rc::Rc::new(content));
        self
    }

    /// Builds persistent content after the virtualized item range.
    pub fn footer<F: Fn() -> Element + 'static>(mut self, content: F) -> Self {
        self.options.footer = Some(::std::rc::Rc::new(content));
        self
    }

    /// Builds the content shown while the item source is empty.
    pub fn empty<F: Fn() -> Element + 'static>(mut self, content: F) -> Self {
        self.options.empty = Some(::std::rc::Rc::new(content));
        self
    }

    /// Fired continuously while scrolling. Geometry is normalized by the
    /// standard ScrollView event contract on every Host.
    pub fn on_scroll<F: Fn(ScrollEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.handle, "scroll", BindType::Bind, f);
        self
    }
}
// ---- Type-stated render-props setters ----
//
// Each setter advances one type parameter from `()` to the
// function-shaped newtype; the `__h()` finaliser is only impl'd
// on the fully-populated state. The user can call the three in
// any order — the render! macro emits them in whatever order
// they appear in the source.
impl<EachF, KeyF, ChildF, InitialF> list<EachF, KeyF, ChildF, (), InitialF> {
    /// Binds the typed Rust List controller. Unlike ordinary element refs,
    /// this also exposes key/index resolution and cached snapshots.
    pub fn bind_ref<K: 'static>(
        self,
        list_ref: ::whisker_runtime::view::ListRef<K>,
    ) -> list<EachF, KeyF, ChildF, ::whisker_runtime::view::ListRef<K>, InitialF> {
        list {
            handle: self.handle,
            options: self.options,
            each: self.each,
            key: self.key,
            children: self.children,
            list_ref,
            initial_scroll: self.initial_scroll,
        }
    }
}

impl<KeyF, ChildF, RefF, InitialF> list<(), KeyF, ChildF, RefF, InitialF> {
    pub fn each<T: 'static, F>(
        self,
        f: F,
    ) -> list<::whisker_runtime::view::EachFn<T>, KeyF, ChildF, RefF, InitialF>
    where
        F: ::std::convert::Into<::whisker_runtime::view::EachFn<T>>,
    {
        list {
            handle: self.handle,
            options: self.options,
            each: f.into(),
            key: self.key,
            children: self.children,
            list_ref: self.list_ref,
            initial_scroll: self.initial_scroll,
        }
    }
}
impl<EachF, ChildF, RefF, InitialF> list<EachF, (), ChildF, RefF, InitialF> {
    /// Stable logical identity extractor, matching [`ForEach`](crate::ForEach).
    pub fn key<T: 'static, K: 'static, F>(
        self,
        f: F,
    ) -> list<EachF, ::whisker_runtime::view::KeyFn<T, K>, ChildF, RefF, InitialF>
    where
        F: ::std::convert::Into<::whisker_runtime::view::KeyFn<T, K>>,
    {
        list {
            handle: self.handle,
            options: self.options,
            each: self.each,
            key: f.into(),
            children: self.children,
            list_ref: self.list_ref,
            initial_scroll: self.initial_scroll,
        }
    }
}
impl<EachF, KeyF, RefF, InitialF> list<EachF, KeyF, (), RefF, InitialF> {
    /// Builds one keyed row. The signal is updated when data for the same
    /// key changes; leaving the mounted window disposes its owner.
    pub fn children<T: 'static, F>(
        self,
        f: F,
    ) -> list<
        EachF,
        KeyF,
        ::whisker_runtime::view::ItemFn<::whisker_runtime::reactive::ReadSignal<T>>,
        RefF,
        InitialF,
    >
    where
        F: ::std::convert::Into<
                ::whisker_runtime::view::ItemFn<::whisker_runtime::reactive::ReadSignal<T>>,
            >,
    {
        list {
            handle: self.handle,
            options: self.options,
            each: self.each,
            key: self.key,
            children: f.into(),
            list_ref: self.list_ref,
            initial_scroll: self.initial_scroll,
        }
    }
}

impl<EachF, KeyF, ChildF, RefF> list<EachF, KeyF, ChildF, RefF, ()> {
    /// Applies one logical target after the initial source snapshot is
    /// indexed. Key targets are checked against the List's key type at
    /// compile time.
    pub fn initial_scroll<K: 'static>(
        self,
        target: ::whisker_runtime::view::ListScrollTarget<K>,
    ) -> list<EachF, KeyF, ChildF, RefF, ::whisker_runtime::view::ListScrollTarget<K>> {
        list {
            handle: self.handle,
            options: self.options,
            each: self.each,
            key: self.key,
            children: self.children,
            list_ref: self.list_ref,
            initial_scroll: target,
        }
    }
}

#[doc(hidden)]
pub trait ListInitialScroll<K> {
    fn into_target(self) -> Option<::whisker_runtime::view::ListScrollTarget<K>>;
}

impl<K> ListInitialScroll<K> for () {
    fn into_target(self) -> Option<::whisker_runtime::view::ListScrollTarget<K>> {
        None
    }
}

impl<K> ListInitialScroll<K> for ::whisker_runtime::view::ListScrollTarget<K> {
    fn into_target(self) -> Option<::whisker_runtime::view::ListScrollTarget<K>> {
        Some(self)
    }
}
// ---- Finaliser, only on fully-populated state ----
impl<T, K, InitialF>
    list<
        ::whisker_runtime::view::EachFn<T>,
        ::whisker_runtime::view::KeyFn<T, K>,
        ::whisker_runtime::view::ItemFn<::whisker_runtime::reactive::ReadSignal<T>>,
        (),
        InitialF,
    >
where
    T: ::std::clone::Clone + 'static,
    K: ::std::cmp::Eq + ::std::hash::Hash + ::std::clone::Clone + 'static,
    InitialF: ListInitialScroll<K>,
{
    /// Finalises the Rust-owned keyed windowing core.
    #[allow(non_snake_case)]
    pub fn __h(self) -> Element {
        let virtual_layout = configure_list_presentation(self.handle, &self.options);
        let handle = self.handle;
        let content = self.options.content;
        let axis = self.options.axis;
        let each = self.each;
        let key = self.key;
        let children = self.children;
        let header = self.options.header.map(|content| content());
        let footer = self.options.footer.map(|content| content());
        let empty = self.options.empty.map(|content| content());

        ::whisker_runtime::view::virtualize(
            handle,
            content,
            move || each.call(),
            move |t: &T| key.call(t),
            move |item| children.call(item),
            ::whisker_runtime::view::VirtualListOptions {
                axis,
                layout: virtual_layout,
                list_ref: None,
                initial_scroll: self.initial_scroll.into_target(),
                start_reached_threshold: self.options.start_reached_threshold,
                end_reached_threshold: self.options.end_reached_threshold,
                on_start_reached: self.options.on_start_reached,
                on_end_reached: self.options.on_end_reached,
                header,
                footer,
                empty,
            },
        );

        handle
    }
}

impl<T, K, InitialF>
    list<
        ::whisker_runtime::view::EachFn<T>,
        ::whisker_runtime::view::KeyFn<T, K>,
        ::whisker_runtime::view::ItemFn<::whisker_runtime::reactive::ReadSignal<T>>,
        ::whisker_runtime::view::ListRef<K>,
        InitialF,
    >
where
    T: ::std::clone::Clone + 'static,
    K: ::std::cmp::Eq + ::std::hash::Hash + ::std::clone::Clone + 'static,
    InitialF: ListInitialScroll<K>,
{
    #[allow(non_snake_case)]
    pub fn __h(self) -> Element {
        let virtual_layout = configure_list_presentation(self.handle, &self.options);
        let handle = self.handle;
        let content = self.options.content;
        let axis = self.options.axis;
        let each = self.each;
        let key = self.key;
        let children = self.children;
        let header = self.options.header.map(|content| content());
        let footer = self.options.footer.map(|content| content());
        let empty = self.options.empty.map(|content| content());

        ::whisker_runtime::view::virtualize(
            handle,
            content,
            move || each.call(),
            move |t: &T| key.call(t),
            move |item| children.call(item),
            ::whisker_runtime::view::VirtualListOptions {
                axis,
                layout: virtual_layout,
                list_ref: Some(self.list_ref),
                initial_scroll: self.initial_scroll.into_target(),
                start_reached_threshold: self.options.start_reached_threshold,
                end_reached_threshold: self.options.end_reached_threshold,
                on_start_reached: self.options.on_start_reached,
                on_end_reached: self.options.on_end_reached,
                header,
                footer,
                empty,
            },
        );

        handle
    }
}
