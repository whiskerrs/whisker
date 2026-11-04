use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// WGSL that moves a [`Material`](crate::Material)'s vertices and colors its surface, named by
/// [`Material::shader`](crate::Material::shader).
///
/// The source may define either or both of these functions, and Kiba lights the result as it lights
/// any material, so shadows, lights, sky, fog, and exposure still apply:
///
/// ```wgsl
/// // In the entity's space, before its Transform. Positions and normals both may change.
/// struct Geometry { position: vec3<f32>, normal: vec3<f32>, uv: vec2<f32>, color: vec3<f32> }
/// fn vertex(geometry: Geometry) -> Geometry
///
/// // `surface` arrives filled from the material and its textures; return what the pixel should be.
/// struct Surface {
///     base_color: vec3<f32>,  // linear RGB, lit by the sun, lights, and sky
///     opacity: f32,           // used as the material's AlphaMode says
///     metallic: f32,
///     roughness: f32,
///     emissive: vec3<f32>,    // added after lighting, in the scale of Sun::color
///     normal: vec3<f32>,      // world space, unit length, toward the viewer
/// }
/// struct Pixel {
///     world_position: vec3<f32>,
///     entity_position: vec3<f32>,  // where on the moved mesh, in the entity's space
///     uv: vec2<f32>,
///     screen_uv: vec2<f32>,        // (0, 0) at the top left of the view, (1, 1) at the bottom right
///     to_camera: vec3<f32>,        // unit length
/// }
/// fn surface(surface: Surface, pixel: Pixel) -> Surface
/// ```
///
/// Both may call:
///
/// | Function | Returns |
/// |---|---|
/// | `time() -> f32` | Seconds since the game started |
/// | `parameter(index: u32) -> vec4<f32>` | [`Material::parameters`](crate::Material::parameters)`[index]`, for `index` below 8 |
/// | `to_sun() -> vec3<f32>`, `sun_color() -> vec3<f32>` | The [`Sun`](crate::Sun)'s unit direction toward it and its color |
/// | `sky_color(direction: vec3<f32>) -> vec3<f32>` | The light arriving from `direction`, from the [`Sky`](crate::Sky) or the sun's ambient |
/// | `to_entity(world: vec3<f32>) -> vec3<f32>` | A world position in the entity's space, as [`Transform`](crate::Transform) places it |
///
/// A surface whose material uses [`AlphaMode::Blend`](crate::AlphaMode::Blend) may also see the
/// opaque scene and sky behind it, which the other modes see as black and infinitely far:
///
/// | Function | Returns |
/// |---|---|
/// | `scene_color(screen_uv: vec2<f32>) -> vec3<f32>` | The color behind, in the scale of `emissive`, so refraction is `emissive = scene_color(bent_uv)` with an opacity of 1 and a black base color |
/// | `scene_position(screen_uv: vec2<f32>) -> vec3<f32>` | The world position behind, so `to_entity(scene_position(pixel.screen_uv))` projects a decal drawn on a double-sided cube |
///
/// Blended surfaces do not see one another this way. Kiba's own functions share the module, so a name
/// that clashes with one fails to compile. A shader that fails to compile is logged with the compiler's
/// message, and its entities draw as plain materials until the source changes.
///
/// ```
/// # use kiba::prelude::*;
/// let wobble = Shader::new(
///     "fn vertex(geometry: Geometry) -> Geometry {
///          var moved = geometry;
///          moved.position += geometry.normal * sin(time() * 4.0 + geometry.position.y * 8.0) * parameter(0u).x;
///          return moved;
///      }",
/// )
/// .bounds_scale(1.2);
/// let mut jelly = Material {
///     shader: Some(wobble),
///     ..Material::default()
/// };
/// jelly.parameters[0].x = 0.05;
/// ```
#[derive(Clone, Debug)]
pub struct Shader(Arc<ShaderData>);

#[derive(Debug)]
pub(crate) struct ShaderData {
    pub(crate) source: String,
    key: u64,
    bounds_scale: f32,
}

impl PartialEq for Shader {
    fn eq(&self, other: &Self) -> bool {
        self.0.key == other.0.key && self.0.bounds_scale == other.0.bounds_scale
    }
}

impl Shader {
    /// Creates a shader from WGSL that defines `vertex`, `surface`, or both.
    pub fn new(source: impl Into<String>) -> Self {
        let source = source.into();
        let mut hasher = rustc_hash::FxHasher::default();
        source.hash(&mut hasher);
        // Zero stands for Kiba's own shader.
        let key = hasher.finish().max(1);
        Self(Arc::new(ShaderData {
            source,
            key,
            bounds_scale: 1.0,
        }))
    }

    /// Widens the sphere that culls entities by `scale`, for a `vertex` that moves past the mesh's
    /// bounds; an entity is otherwise hidden while only its moved part is in view.
    pub fn bounds_scale(self, scale: f32) -> Self {
        Self(Arc::new(ShaderData {
            source: self.0.source.clone(),
            key: self.0.key,
            bounds_scale: scale,
        }))
    }

    pub(crate) fn data(&self) -> &ShaderData {
        &self.0
    }

    /// The same for every shader built from the same source, so a hot reload reuses compiled pipelines.
    pub(crate) fn key(&self) -> u64 {
        self.0.key
    }

    pub(crate) fn culling_scale(&self) -> f32 {
        self.0.bounds_scale
    }
}
