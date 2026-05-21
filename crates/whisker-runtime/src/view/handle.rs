//! [`Element`] — opaque, `Copy`, backend-agnostic identifier.
//!
//! IDs are allocated by the renderer's [`create_element`] call and
//! are valid until [`release_element`] (or the renderer being
//! uninstalled). They have no semantic meaning to user code beyond
//! "name this element in subsequent renderer calls"; the renderer
//! is free to use them as indices, hash keys, or whatever fits.
//!
//! [`create_element`]: super::create_element
//! [`release_element`]: super::release_element

/// Backend-agnostic element handle. `Copy` so it threads through
/// reactive closures without lifetime gymnastics.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Element(pub(crate) u32);

impl Element {
    /// The numeric id this handle wraps. Mostly useful for renderers
    /// that store per-element state in side maps.
    pub fn id(self) -> u32 {
        self.0
    }

    /// Construct a handle from a raw id. Use this only when bridging
    /// from a renderer-internal map (e.g. the `MockRenderer` test
    /// fixture); otherwise let [`create_element`] hand them out.
    ///
    /// [`create_element`]: super::create_element
    pub fn from_raw(id: u32) -> Self {
        Self(id)
    }
}
