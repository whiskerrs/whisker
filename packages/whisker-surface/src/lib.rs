//! Native GPU render targets for Whisker elements.
//!
//! `whisker-surface` does not draw. It hands a [`SurfaceDelegate`] the platform's native render
//! target, its size, and display-synchronized frames, so any graphics API (wgpu, Vulkan, Metal,
//! WebGPU) can draw into a Whisker element:
//!
//! ```ignore
//! let surface = Surface::new(MyRenderer::default());
//! surface.set_continuous(true);
//! render! { SurfaceView(surface: surface.clone(), style: css!(width: percent(100), height: px(300))) }
//! ```
//!
//! | Host | [`NativeSurface`] |
//! |---|---|
//! | iOS, macOS | [`NativeSurface::CoreAnimationLayer`] (a `CAMetalLayer`) |
//! | Android | [`NativeSurface::AndroidNativeWindow`] (an `ANativeWindow` backed by a `TextureView`) |
//! | Web | `NativeSurface::WebCanvas` |
//! | Windows | [`NativeSurface::DCompSurfaceHandle`] |
//! | Linux | `NativeSurface::Dmabuf`, which takes finished frames instead of giving a window |

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::time::Duration;

use whisker::runtime::view::Element;
use whisker::runtime::{WeakRuntimeContext, current_runtime_context};
use whisker::{RwSignal, Signal, Style};

#[cfg(target_os = "linux")]
mod dmabuf;
#[cfg(not(target_arch = "wasm32"))]
mod ffi;

#[cfg(target_os = "linux")]
#[doc(hidden)]
pub use dmabuf::DmabufPresenter;
#[cfg(target_os = "linux")]
pub use dmabuf::{DmabufFrame, DmabufPlane, DmabufSink};

/// Physical pixel size of a surface and its pixels per logical pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceSize {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

/// One display-synchronized frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameInfo {
    /// Monotonic Host time of the frame.
    pub timestamp: Duration,
}

/// The platform's render target for one mounted [`SurfaceView`].
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum NativeSurface {
    /// A `CAMetalLayer`.
    CoreAnimationLayer(NonNull<c_void>),
    /// An `ANativeWindow`.
    AndroidNativeWindow(NonNull<c_void>),
    #[cfg(target_arch = "wasm32")]
    WebCanvas(web_sys::HtmlCanvasElement),
    /// A DirectComposition surface handle from `DCompositionCreateSurfaceHandle`.
    DCompSurfaceHandle(NonNull<c_void>),
    #[cfg(target_os = "linux")]
    Dmabuf(DmabufSink),
}

/// Receives a [`Surface`]'s native target and frames, always on the main thread.
pub trait SurfaceDelegate: 'static {
    /// The target stays valid until [`SurfaceDelegate::destroyed`] returns. Called again after
    /// each `destroyed` when the Host recreates its target, for example on Android resume.
    fn created(&mut self, target: NativeSurface, size: SurfaceSize);
    fn resized(&mut self, size: SurfaceSize);
    fn frame(&mut self, frame: FrameInfo);
    /// Everything that references the target must be released before this returns.
    fn destroyed(&mut self);
}

/// A render target that a [`SurfaceView`] element shows.
///
/// Frames are delivered after creation, after each resize, after [`Surface::request_frame`], and
/// on every display refresh while [`Surface::set_continuous`] is on.
#[derive(Clone)]
pub struct Surface {
    inner: Rc<Inner>,
}

struct Inner {
    id: u64,
    // Hosts call in from display callbacks outside Whisker's runtime, where signals cannot be reached.
    context: Option<WeakRuntimeContext>,
    delegate: RefCell<Box<dyn SurfaceDelegate>>,
    prop: RwSignal<String>,
    wakes: Cell<u64>,
    continuous: Cell<bool>,
    pending: Cell<bool>,
    attached: Cell<bool>,
    ticking: Cell<bool>,
}

thread_local! {
    static SURFACES: RefCell<HashMap<u64, Weak<Inner>>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

impl Surface {
    pub fn new(delegate: impl SurfaceDelegate) -> Self {
        let id = NEXT_ID.with(|next| next.replace(next.get() + 1));
        let inner = Rc::new(Inner {
            id,
            context: current_runtime_context(),
            delegate: RefCell::new(Box::new(delegate)),
            prop: whisker::signal(prop_value(id, 0)),
            wakes: Cell::new(0),
            continuous: Cell::new(false),
            pending: Cell::new(false),
            attached: Cell::new(false),
            ticking: Cell::new(false),
        });
        SURFACES.with(|surfaces| surfaces.borrow_mut().insert(id, Rc::downgrade(&inner)));
        Self { inner }
    }

    /// Delivers one more frame.
    pub fn request_frame(&self) {
        self.inner.pending.set(true);
        self.inner.wake();
    }

    /// Delivers a frame on every display refresh while on.
    pub fn set_continuous(&self, continuous: bool) {
        self.inner.continuous.set(continuous);
        if continuous {
            self.inner.wake();
        }
    }

    /// A handle that does not keep the surface alive, for use inside its own delegate.
    pub fn downgrade(&self) -> WeakSurface {
        WeakSurface(Rc::downgrade(&self.inner))
    }
}

/// A [`Surface`] that may have been dropped.
#[derive(Clone)]
pub struct WeakSurface(Weak<Inner>);

impl WeakSurface {
    pub fn upgrade(&self) -> Option<Surface> {
        self.0.upgrade().map(|inner| Surface { inner })
    }
}

impl Inner {
    // The Host stops its display link when a frame returns false, so only an idle Host needs the prop.
    fn wake(&self) {
        if !self.attached.get() || self.ticking.replace(true) {
            return;
        }
        let wakes = self.wakes.get() + 1;
        self.wakes.set(wakes);
        let (prop, value) = (self.prop, prop_value(self.id, wakes));
        self.in_context(|| {
            prop.try_set(value);
        });
    }

    fn in_context(&self, f: impl FnOnce()) {
        let mut f = Some(f);
        if let Some(context) = &self.context {
            context.enter(|| f.take().map(|f| f()));
        }
        if let Some(f) = f {
            f();
        }
    }

    fn with_delegate(&self, call: impl FnOnce(&mut dyn SurfaceDelegate)) {
        self.in_context(|| {
            if let Ok(mut delegate) = self.delegate.try_borrow_mut() {
                call(delegate.as_mut());
            }
        });
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if self.attached.get() {
            self.with_delegate(|delegate| delegate.destroyed());
        }
        let _ = SURFACES.try_with(|surfaces| surfaces.borrow_mut().remove(&self.id));
    }
}

fn prop_value(id: u64, wakes: u64) -> String {
    format!("{id}:{wakes}")
}

/// Shows a [`Surface`] in the element's box.
#[whisker::component]
pub fn surface_view(surface: Surface, style: Style) -> Element {
    SurfaceElement::builder()
        .surface(surface.inner.prop)
        .style(style.clone())
        .build()
}

#[doc(hidden)]
#[whisker::module_element(name = "whisker-surface:Surface", measurement = None)]
pub fn surface_element(surface: Signal<String>, style: Style) {}

#[doc(hidden)]
pub fn __whisker_element_module_definition() -> whisker::ElementModuleDefinition {
    whisker::ElementModuleDefinition::new(
        env!("CARGO_PKG_NAME"),
        [surface_element_schema::element_provider()],
    )
}

/// Entry points for Host implementations.
///
/// A Host schedules a frame after `attach`, after `resize`, and whenever the `surface` prop
/// changes, and keeps scheduling until `frame` returns false.
#[doc(hidden)]
pub mod host {
    use super::*;

    /// The surface id inside a `surface` prop value.
    pub fn parse_id(prop: &str) -> Option<u64> {
        prop.split(':').next()?.parse().ok()
    }

    fn find(id: u64) -> Option<Rc<Inner>> {
        SURFACES.with(|surfaces| surfaces.borrow().get(&id).and_then(Weak::upgrade))
    }

    pub fn attach(id: u64, target: NativeSurface, size: SurfaceSize) {
        let Some(inner) = find(id) else { return };
        if inner.attached.replace(true) {
            inner.with_delegate(|delegate| delegate.destroyed());
        }
        inner.ticking.set(true);
        inner.with_delegate(|delegate| delegate.created(target, size));
    }

    pub fn resize(id: u64, size: SurfaceSize) {
        let Some(inner) = find(id) else { return };
        if !inner.attached.get() {
            return;
        }
        inner.ticking.set(true);
        inner.with_delegate(|delegate| delegate.resized(size));
    }

    /// Returns whether the Host should deliver another frame.
    pub fn frame(id: u64, timestamp: Duration) -> bool {
        let Some(inner) = find(id) else { return false };
        if !inner.attached.get() {
            return false;
        }
        inner.pending.set(false);
        inner.with_delegate(|delegate| delegate.frame(FrameInfo { timestamp }));
        let more = inner.continuous.get() || inner.pending.get();
        inner.ticking.set(more);
        more
    }

    pub fn detach(id: u64) {
        let Some(inner) = find(id) else { return };
        if inner.attached.replace(false) {
            inner.ticking.set(false);
            inner.with_delegate(|delegate| delegate.destroyed());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use whisker::runtime::{RuntimeContext, RuntimeWakeHandle};

    use super::*;

    #[derive(Default)]
    struct Log(Rc<RefCell<Vec<&'static str>>>);

    impl SurfaceDelegate for Log {
        fn created(&mut self, _: NativeSurface, _: SurfaceSize) {
            self.0.borrow_mut().push("created");
        }
        fn resized(&mut self, _: SurfaceSize) {
            self.0.borrow_mut().push("resized");
        }
        fn frame(&mut self, _: FrameInfo) {
            self.0.borrow_mut().push("frame");
        }
        fn destroyed(&mut self) {
            self.0.borrow_mut().push("destroyed");
        }
    }

    const SIZE: SurfaceSize = SurfaceSize {
        width: 4,
        height: 4,
        scale: 1.0,
    };

    fn layer() -> NativeSurface {
        NativeSurface::CoreAnimationLayer(NonNull::dangling())
    }

    fn setup() -> (RuntimeContext, Surface, u64, Rc<RefCell<Vec<&'static str>>>) {
        let context = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        let events = Rc::new(RefCell::new(Vec::new()));
        let surface = context.enter(|| Surface::new(Log(events.clone())));
        let id = surface.inner.id;
        (context, surface, id, events)
    }

    #[test]
    fn frames_stop_after_one_unless_requested_or_continuous() {
        let (context, surface, id, events) = setup();
        host::attach(id, layer(), SIZE);
        assert!(!host::frame(id, Duration::ZERO));
        context.enter(|| surface.request_frame());
        assert!(!host::frame(id, Duration::ZERO));
        surface.set_continuous(true);
        assert!(host::frame(id, Duration::ZERO));
        assert_eq!(*events.borrow(), ["created", "frame", "frame", "frame"]);
    }

    #[test]
    fn waking_an_idle_host_changes_the_prop() {
        let (context, surface, id, _) = setup();
        let prop = surface.inner.prop;
        host::attach(id, layer(), SIZE);
        host::frame(id, Duration::ZERO);
        let before = context.enter(|| prop.get_untracked());
        surface.request_frame();
        let after = context.enter(|| prop.get_untracked());
        assert_ne!(before, after);
        assert_eq!(host::parse_id(&after), Some(id));
    }

    #[test]
    fn detach_and_drop_destroy_once() {
        let (context, surface, id, events) = setup();
        host::attach(id, layer(), SIZE);
        host::detach(id);
        host::detach(id);
        host::attach(id, layer(), SIZE);
        context.enter(|| drop(surface));
        assert!(!host::frame(id, Duration::ZERO));
        assert_eq!(
            *events.borrow(),
            ["created", "destroyed", "created", "destroyed"]
        );
    }
}
