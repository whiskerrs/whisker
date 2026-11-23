use std::os::fd::OwnedFd;
use std::rc::Rc;

/// One plane of a [`DmabufFrame`].
#[derive(Debug)]
pub struct DmabufPlane {
    pub fd: OwnedFd,
    pub offset: u32,
    pub stride: u32,
}

/// A finished frame in GPU memory. The renderer must have finished writing it.
pub struct DmabufFrame {
    pub width: u32,
    pub height: u32,
    /// DRM fourcc code, such as `AR24` for premultiplied BGRA.
    pub fourcc: u32,
    pub modifier: u64,
    pub planes: Vec<DmabufPlane>,
    /// Called, possibly on another thread, once the Host no longer reads the buffer.
    pub release: Option<Box<dyn FnOnce() + Send>>,
}

/// Shows frames that the renderer finished, on Hosts that cannot hand out a window.
#[derive(Clone)]
pub struct DmabufSink(Rc<dyn DmabufPresenter>);

impl std::fmt::Debug for DmabufSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DmabufSink")
    }
}

impl DmabufSink {
    #[doc(hidden)]
    pub fn new(presenter: impl DmabufPresenter) -> Self {
        Self(Rc::new(presenter))
    }

    /// Shows a frame in GPU memory. Returns false when the Host cannot import it, so the caller can
    /// fall back to [`DmabufSink::present_pixels`].
    pub fn present(&self, frame: DmabufFrame) -> bool {
        self.0.present(frame)
    }

    /// DRM format modifiers the Host can import for `fourcc`.
    pub fn modifiers(&self, fourcc: u32) -> Vec<u64> {
        self.0.modifiers(fourcc)
    }

    /// Shows premultiplied RGBA8 pixels copied from the GPU.
    pub fn present_pixels(&self, width: u32, height: u32, stride: u32, pixels: &[u8]) {
        self.0.present_pixels(width, height, stride, pixels);
    }
}

#[doc(hidden)]
pub trait DmabufPresenter: 'static {
    fn present(&self, frame: DmabufFrame) -> bool;
    fn modifiers(&self, fourcc: u32) -> Vec<u64>;
    fn present_pixels(&self, width: u32, height: u32, stride: u32, pixels: &[u8]);
}
