use crate::ElementTag;
use whisker_runtime::event::{AnimationEvent, ScrollEvent, TouchEvent, bind_typed};
use whisker_runtime::reactive::{Signal, effect};
use whisker_runtime::view::{
    BindType, Element, append_child, apply_accessibility, apply_attr, apply_attr_bool,
    apply_dataset, apply_element_id, apply_text_max_lines, create_element, create_phantom_element,
    set_attribute_object,
};

// A trait, not `macro_rules!`: RA's method-completion does NOT
// surface methods produced by a `macro_rules!` expansion inside an
// `impl` block, whereas trait methods are first-class items it
// indexes — provided the trait is in scope. `render!` imports it through
// `__tags` for built-ins and through the common `__element_builder`
// re-export for module components. End-to-end guard:
// `crates/whisker-macros/tests/ra_completion.rs`.

/// Shared builder methods for built-in and module element tags.
///
/// Each method consumes `self` and returns it, so calls chain:
/// `view().style(…).on_tap(…).child(…)`. Reactive-capable
/// attributes accept any `Into<Signal<T>>` (a static value, a
/// `ReadSignal`, an `RwSignal`, …) and re-apply on change.
pub trait ElementBuilder: Sized {
    /// The underlying runtime element handle. Implemented by each
    /// tag struct as `self.handle`.
    #[doc(hidden)]
    fn __element(&self) -> Element;

    // ---- Styling ----------------------------------------------------

    /// Structured CSS declarations.
    ///
    /// Accepts any value that converts into [`crate::Style`] — a
    /// [`whisker_css::Css`] builder, or a reactive
    /// [`ReadSignal`](crate::ReadSignal) / [`RwSignal`](crate::RwSignal)
    /// carrying `Css`. Reactive variants re-apply the declarations via the
    /// element's internal `effect` whenever the underlying
    /// signal changes.
    ///
    /// ```ignore
    /// view(style: css!(padding: px(8), background_color: Color::hex(0xff0000)))
    /// view(style: Css::new().padding(px(8)).background_color(Color::hex(0xff0000)))
    /// view(style: computed(move || Css::new().opacity(alpha.get())))
    /// ```
    fn style<V>(self, v: V) -> Self
    where
        V: Into<crate::Style>,
    {
        crate::apply_style(self.__element(), v);
        self
    }

    // ---- Common semantics (shared by all elements) ------------------

    /// Stable identifier surfaced through event target metadata.
    fn id<V>(self, v: V) -> Self
    where
        V: Into<Signal<String>>,
    {
        apply_element_id(self.__element(), v);
        self
    }

    /// Structured metadata surfaced through event targets.
    fn dataset<V>(self, v: V) -> Self
    where
        V: Into<Signal<crate::Dataset>>,
    {
        apply_dataset(self.__element(), v);
        self
    }

    /// Common accessibility semantics for built-in and module elements.
    fn accessibility<V>(self, v: V) -> Self
    where
        V: Into<Signal<crate::Accessibility>>,
    {
        apply_accessibility(self.__element(), v);
        self
    }

    // Capture listeners fire root-to-target and bubble listeners fire
    // target-to-root. `catch` stops propagation at that listener. The
    // retained Rust scene performs hit testing and event propagation, so
    // these semantics are identical on every Host.

    /// `tap` — single tap (won't fire if the finger moved far).
    /// Bubble phase, lets the event continue up the chain.
    ///
    /// The closure receives a [`TouchEvent`] with the tap
    /// coordinates and target metadata. For "stop propagation"
    /// semantics use [`on_tap_catch`](Self::on_tap_catch);
    /// for the down-pass capture phase, the `on_capture_tap*`
    /// variants.
    ///
    /// ```ignore
    /// view(on_tap: move |e| println!("tap at {:?}", e.detail))
    /// ```
    fn on_tap<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "tap", BindType::Bind, f);
        self
    }
    /// `tap`, bubble phase — **stops** propagation at this element.
    fn on_tap_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "tap", BindType::Catch, f);
        self
    }
    /// `tap`, capture phase (fires before descendants) — doesn't stop.
    fn on_capture_tap<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "tap", BindType::CaptureBind, f);
        self
    }
    /// `tap`, capture phase — **stops** propagation before it reaches
    /// the target.
    fn on_capture_tap_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "tap", BindType::CaptureCatch, f);
        self
    }

    /// `click` — click on the nearest listening node.
    fn on_click<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "click", BindType::Bind, f);
        self
    }
    /// `click`, bubble phase — **stops** propagation here.
    fn on_click_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "click", BindType::Catch, f);
        self
    }
    /// `click`, capture phase — doesn't stop.
    fn on_capture_click<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "click", BindType::CaptureBind, f);
        self
    }
    /// `click`, capture phase — **stops** propagation.
    fn on_capture_click_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "click", BindType::CaptureCatch, f);
        self
    }

    /// `touchstart` — finger touches the surface.
    fn on_touchstart<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchstart", BindType::Bind, f);
        self
    }
    /// `touchstart`, bubble phase — **stops** propagation here.
    fn on_touchstart_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchstart", BindType::Catch, f);
        self
    }
    /// `touchstart`, capture phase — doesn't stop.
    fn on_capture_touchstart<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchstart", BindType::CaptureBind, f);
        self
    }
    /// `touchstart`, capture phase — **stops** propagation.
    fn on_capture_touchstart_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchstart", BindType::CaptureCatch, f);
        self
    }

    /// `touchmove` — finger moves on the surface.
    fn on_touchmove<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchmove", BindType::Bind, f);
        self
    }
    /// `touchmove`, bubble phase — **stops** propagation here.
    fn on_touchmove_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchmove", BindType::Catch, f);
        self
    }
    /// `touchmove`, capture phase — doesn't stop.
    fn on_capture_touchmove<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchmove", BindType::CaptureBind, f);
        self
    }
    /// `touchmove`, capture phase — **stops** propagation.
    fn on_capture_touchmove_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchmove", BindType::CaptureCatch, f);
        self
    }

    /// `touchend` — finger leaves the surface.
    fn on_touchend<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchend", BindType::Bind, f);
        self
    }
    /// `touchend`, bubble phase — **stops** propagation here.
    fn on_touchend_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchend", BindType::Catch, f);
        self
    }
    /// `touchend`, capture phase — doesn't stop.
    fn on_capture_touchend<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchend", BindType::CaptureBind, f);
        self
    }
    /// `touchend`, capture phase — **stops** propagation.
    fn on_capture_touchend_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchend", BindType::CaptureCatch, f);
        self
    }

    /// `touchcancel` — touch interrupted by the system / a gesture.
    fn on_touchcancel<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchcancel", BindType::Bind, f);
        self
    }
    /// `touchcancel`, bubble phase — **stops** propagation here.
    fn on_touchcancel_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchcancel", BindType::Catch, f);
        self
    }
    /// `touchcancel`, capture phase — doesn't stop.
    fn on_capture_touchcancel<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchcancel", BindType::CaptureBind, f);
        self
    }
    /// `touchcancel`, capture phase — **stops** propagation.
    fn on_capture_touchcancel_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "touchcancel", BindType::CaptureCatch, f);
        self
    }

    // ---- Events: animation / transition → `AnimationEvent` ----------

    /// `animationstart` — keyframe animation began.
    fn on_animationstart<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "animationstart", BindType::Bind, f);
        self
    }

    /// `animationend` — keyframe animation completed.
    fn on_animationend<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "animationend", BindType::Bind, f);
        self
    }

    /// `animationcancel` — keyframe animation interrupted.
    fn on_animationcancel<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "animationcancel", BindType::Bind, f);
        self
    }

    /// `animationiteration` — keyframe animation cycle boundary.
    fn on_animationiteration<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "animationiteration", BindType::Bind, f);
        self
    }

    /// `transitionstart` — transition animation began.
    fn on_transitionstart<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "transitionstart", BindType::Bind, f);
        self
    }

    /// `transitionend` — transition animation completed.
    fn on_transitionend<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "transitionend", BindType::Bind, f);
        self
    }

    /// `transitioncancel` — transition animation interrupted.
    fn on_transitioncancel<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.__element(), "transitioncancel", BindType::Bind, f);
        self
    }

    // ---- Children ---------------------------------------------------

    /// Append a child handle.
    fn child(self, child: Element) -> Self {
        append_child(self.__element(), child);
        self
    }

    // ---- Ref --------------------------------------------------------

    /// Bind an [`ElementRef`](crate::ElementRef) to this element so
    /// its commands can be invoked after mount. `render!` routes the `ref:`
    /// kwarg here (`view(ref: my_ref) { … }`).
    fn bind_ref(self, r: crate::ElementRef) -> Self {
        r.__bind(self.__element());
        self
    }

    /// Finish building and return the underlying handle.
    #[doc(hidden)]
    fn __h(self) -> Element {
        self.__element()
    }
}

/// `<view>` — Whisker's basic layout primitive:
/// a rectangular box that lays out children with CSS flexbox
/// (`<View>` in React Native, `<div>` on the web).
///
/// Use `view` for any non-text grouping or layout. `view` is also the
/// right tag for touch targets; attach `on_tap` or `on_click` there.
///
/// ```ignore
/// render! {
///     view(
///         style: css!(flex_direction: FlexDirection::Column, padding: px(16)),
///         on_tap: move |_| println!("tapped"),
///     ) {
///         text(value: "Title")
///         text(value: "Subtitle")
///     }
/// }
/// ```
#[allow(non_camel_case_types)]
pub struct view {
    handle: Element,
}
#[allow(non_snake_case)]
pub fn __view_ctor() -> view {
    view {
        handle: create_element(ElementTag::View),
    }
}
impl ElementBuilder for view {
    fn __element(&self) -> Element {
        self.handle
    }
}

/// `<text>` — plain-text leaf. The raw-text node used to lower `value`
/// is an internal runtime detail.
///
/// `text` is the only element that renders text on screen. Set
/// the content through the [`value`](Self::value) attribute
/// (which takes any `Into<Signal<String>>`, so static strings,
/// `ReadSignal<String>`, and computed signals all work). Font /
/// color / size live in the `style` attribute as ordinary CSS.
///
/// ```ignore
/// let count = signal(0_i32);
/// render! {
///     text {
///         style: css!(font_size: px(18), color: Color::hex(0x000000)),
///         value: computed(move || format!("count: {}", count.get())),
///     }
/// }
/// ```
#[allow(non_camel_case_types)]
pub struct text {
    handle: Element,
}
#[allow(non_snake_case)]
pub fn __text_ctor() -> text {
    text {
        handle: create_element(ElementTag::Text),
    }
}
impl ElementBuilder for text {
    fn __element(&self) -> Element {
        self.handle
    }
}
impl text {
    /// `value` — the text string (reactive-capable).
    pub fn value<V>(self, v: V) -> Self
    where
        V: ::std::convert::Into<Signal<::std::string::String>>,
    {
        let raw = create_element(ElementTag::RawText);
        append_child(self.handle, raw);
        apply_attr(raw, "text", v);
        self
    }

    /// Maximum displayed line count. `0` restores the unlimited default.
    pub fn max_lines<V>(self, value: V) -> Self
    where
        V: ::std::convert::Into<Signal<u32>>,
    {
        apply_text_max_lines(self.handle, value);
        self
    }
}

/// `<scroll-view>` — scrollable container.
///
/// Use `scroll_view` for content the user should be able to pan
/// past the viewport. For long, *virtualised* lists where only
/// the visible items should hold platform views, reach for
/// [`list`] instead — `scroll_view` keeps every child mounted.
/// Direction defaults to `Vertical`; flip with
/// [`axis`](Self::axis).
///
/// ```ignore
/// render! {
///     scroll_view {
///         style: css!(flex_grow: 1.0),
///         axis: ScrollAxis::Vertical,
///         on_scroll: |e| println!("y = {}", e.detail.scroll_top),
///         view { /* ... long content ... */ }
///     }
/// }
/// ```
#[allow(non_camel_case_types)]
pub struct scroll_view {
    handle: Element,
}
#[allow(non_snake_case)]
pub fn __scroll_view_ctor() -> scroll_view {
    scroll_view {
        handle: create_element(ElementTag::ScrollView),
    }
}
impl ElementBuilder for scroll_view {
    fn __element(&self) -> Element {
        self.handle
    }
}
impl scroll_view {
    /// Logical scroll axis (vertical by default).
    pub fn axis<V>(self, v: V) -> Self
    where
        V: ::std::convert::Into<Signal<crate::ScrollAxis>>,
    {
        apply_attr(self.handle, "scroll-orientation", v);
        self
    }

    /// Enables item-aligned settling for carousels and pagers.
    pub fn snap<V>(self, snap: V) -> Self
    where
        V: ::std::convert::Into<Signal<crate::ScrollSnap>>,
    {
        let apply = |snap: crate::ScrollSnap| {
            set_attribute_object(
                self.handle,
                "item-snap",
                &[
                    ("factor".to_string(), snap.factor()),
                    ("offset".to_string(), snap.offset()),
                ],
            );
        };
        match snap.into() {
            Signal::Stored(value) => value.with(|value| apply(*value)),
            Signal::Dynamic(value) => {
                let handle = self.handle;
                effect(move || {
                    let snap = value.get();
                    set_attribute_object(
                        handle,
                        "item-snap",
                        &[
                            ("factor".to_string(), snap.factor()),
                            ("offset".to_string(), snap.offset()),
                        ],
                    );
                });
            }
        }
        self
    }

    /// Controls whether one scroll gesture may pass intermediate snap
    /// points. [`ScrollSnapStop::Always`](crate::attrs::ScrollSnapStop::Always)
    /// is useful for pagers and manga readers that advance one page at a time.
    pub fn scroll_snap_stop<V>(self, value: V) -> Self
    where
        V: ::std::convert::Into<Signal<crate::attrs::ScrollSnapStop>>,
    {
        apply_attr(self.handle, "scroll-snap-stop", value);
        self
    }

    /// Enables or disables user-driven scrolling.
    pub fn scroll_enabled<V>(self, v: V) -> Self
    where
        V: ::std::convert::Into<Signal<bool>>,
    {
        apply_attr_bool(self.handle, "enable-scroll", v);
        self
    }

    // ---- scroll events (CustomEvent → bind only) ----------------

    /// `scroll` — fired continuously while scrolling. The
    /// [`ScrollEvent`] `detail` carries the current offset, content
    /// size, per-event delta, and drag state.
    pub fn on_scroll<F: Fn(ScrollEvent) + 'static>(self, f: F) -> Self {
        bind_typed(self.handle, "scroll", BindType::Bind, f);
        self
    }
}

#[path = "list.rs"]
mod virtual_list;
pub use virtual_list::*;

/// `<fragment>` — *transparent grouping container*. Mounts as a
/// phantom element ([`create_phantom_element`]) the runtime
/// tracks in its mirror but never forwards to Host. Children
/// appended under a fragment are hoisted to the fragment's
/// nearest non-phantom ancestor in the Host tree, in source
/// order — so on screen the fragment is *invisible*, while in
/// user code it serves as a stable grouping point for reactive
/// children.
///
/// **What it's for**: Whisker's `For` / `Show` control flow
/// (`for_each` / `show`) both `return` a fragment. Any
/// user-defined control flow follows the same pattern — a
/// function that allocates a fragment, installs an effect, and
/// mutates the fragment's children — so a custom control flow
/// looks and feels exactly like the built-in `For` / `Show`.
///
/// **Restrictions**: a fragment carries no styling, attributes,
/// or event listeners — those would have no Host element to
/// attach to. The builder exposes only `.child(...)`. Fragments
/// inside a `<list>` are not supported (use the list builder's
/// `each` / `key` / `children` render-props instead).
#[allow(non_camel_case_types)]
pub struct fragment {
    handle: Element,
}
#[allow(non_snake_case)]
pub fn __fragment_ctor() -> fragment {
    fragment {
        handle: create_phantom_element(),
    }
}
impl ElementBuilder for fragment {
    fn __element(&self) -> Element {
        self.handle
    }
}
