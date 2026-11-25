//! Layers: renderers that draw inside a pass another renderer owns, sharing its depth.

use std::cell::{Cell, RefCell, RefMut};
use std::collections::HashSet;
use std::rc::Rc;

use whisker_surface::{FrameInfo, WeakSurface};

use crate::{Gpu, wgpu};

/// What a host draws a layer into, for one frame.
#[derive(Clone, Copy, Debug)]
pub struct LayerTarget {
    /// The layer's local space to the camera's view space: rotation, translation and one uniform
    /// scale. The local space's units and axes are the host's to document.
    pub view: [[f32; 4]; 4],
    /// View space to clip space.
    pub projection: [[f32; 4]; 4],
    /// The frame's time, the same one the host's own animations use.
    pub frame: FrameInfo,
    pub color_format: wgpu::TextureFormat,
    pub depth_format: wgpu::TextureFormat,
    pub samples: u32,
    /// Physical pixels.
    pub size: (u32, u32),
    /// Physical pixels per logical one.
    pub scale: f32,
}

/// A renderer that draws inside a host's render pass.
///
/// Local space to clip space has a positive determinant, so front faces wind counter-clockwise.
pub trait Layer: 'static {
    /// Called once, before the first frame, with the host that draws it.
    fn attach(&mut self, _host: LayerHost) {}
    /// Before the host's pass begins: offscreen passes such as shadow maps.
    fn prepare(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, target: &LayerTarget);
    /// Inside the host's pass, depth-tested against what the host drew so far.
    fn draw(&mut self, gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>, target: &LayerTarget);
    /// A tap at `point` in logical pixels, before the host's own handling; returns whether the
    /// layer took it.
    fn tap(&mut self, _point: [f32; 2], _target: &LayerTarget) -> bool {
        false
    }
}

/// A shared layer, cheap to clone into components.
#[derive(Clone)]
pub struct LayerHandle(Rc<RefCell<dyn Layer>>);

impl LayerHandle {
    pub fn new(layer: impl Layer) -> LayerHandle {
        LayerHandle(Rc::new(RefCell::new(layer)))
    }

    /// Panics if the layer is already borrowed, as when a layer's own call reaches its handle.
    pub fn borrow_mut(&self) -> RefMut<'_, dyn Layer> {
        self.0.borrow_mut()
    }
}

impl PartialEq for LayerHandle {
    fn eq(&self, other: &LayerHandle) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for LayerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LayerHandle")
            .field(&Rc::as_ptr(&self.0).cast::<()>())
            .finish()
    }
}

/// Asks a host's surface for frames on behalf of one layer.
///
/// The surface draws continuously while any of its `LayerHost`s asks, so a host that also wants
/// continuous frames asks through a [`LayerHost::separate`] of its own rather than the surface.
#[derive(Clone)]
pub struct LayerHost {
    surface: Rc<Shared>,
    vote: Rc<Vote>,
}

struct Shared {
    surface: WeakSurface,
    continuous: RefCell<HashSet<u64>>,
    next: Cell<u64>,
}

struct Vote {
    id: u64,
    surface: Rc<Shared>,
}

impl LayerHost {
    pub fn new(surface: WeakSurface) -> LayerHost {
        let surface = Rc::new(Shared {
            surface,
            continuous: RefCell::new(HashSet::new()),
            next: Cell::new(0),
        });
        LayerHost::voting(surface)
    }

    /// Another handle on the same surface, whose continuous request counts on its own.
    pub fn separate(&self) -> LayerHost {
        LayerHost::voting(self.surface.clone())
    }

    fn voting(surface: Rc<Shared>) -> LayerHost {
        let id = surface.next.replace(surface.next.get() + 1);
        LayerHost {
            vote: Rc::new(Vote {
                id,
                surface: surface.clone(),
            }),
            surface,
        }
    }

    pub fn request_frame(&self) {
        if let Some(surface) = self.surface.surface.upgrade() {
            surface.request_frame();
        }
    }

    pub fn set_continuous(&self, continuous: bool) {
        let mut votes = self.surface.continuous.borrow_mut();
        let was = !votes.is_empty();
        if continuous {
            votes.insert(self.vote.id);
        } else {
            votes.remove(&self.vote.id);
        }
        let now = !votes.is_empty();
        drop(votes);
        if was != now {
            self.surface.apply(now);
        }
    }
}

impl Shared {
    fn apply(&self, continuous: bool) {
        if let Some(surface) = self.surface.upgrade() {
            surface.set_continuous(continuous);
        }
    }
}

impl Drop for Vote {
    fn drop(&mut self) {
        let Ok(mut votes) = self.surface.continuous.try_borrow_mut() else {
            return;
        };
        if votes.remove(&self.id) && votes.is_empty() {
            drop(votes);
            self.surface.apply(false);
        }
    }
}

#[cfg(test)]
mod tests;
