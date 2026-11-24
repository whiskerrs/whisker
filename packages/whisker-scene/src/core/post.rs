use super::Texture;

/// Effects applied to the rendered image, each off until set.
///
/// With any of them on, the scene renders to an intermediate image first, so tone mapping applies
/// to the background too.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PostProcessing {
    pub bloom: Option<Bloom>,
    pub ambient_occlusion: Option<AmbientOcclusion>,
    pub depth_of_field: Option<DepthOfField>,
    /// A color lookup table applied after tone mapping: `size` slices of `size`×`size` laid side
    /// by side, red across each slice, green down, blue from slice to slice.
    pub color_grading: Option<Texture>,
    /// Smooths edges that multisampling misses, such as those inside alpha-masked textures.
    pub fxaa: bool,
}

impl PostProcessing {
    pub(crate) fn active(&self) -> bool {
        self.bloom.is_some()
            || self.ambient_occlusion.is_some()
            || self.depth_of_field.is_some()
            || self.color_grading.is_some()
            || self.fxaa
    }
}

/// A glow spreading from the parts of the image brighter than `threshold`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bloom {
    pub intensity: f32,
    /// The brightness, before tone mapping, where glow begins.
    pub threshold: f32,
    /// How far the glow spreads, `0.0..=1.0`.
    pub radius: f32,
}

impl Default for Bloom {
    fn default() -> Self {
        Bloom {
            intensity: 0.3,
            threshold: 1.0,
            radius: 0.7,
        }
    }
}

/// Darkening of creases and corners, estimated from the depth of what is on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmbientOcclusion {
    /// How far, in scene units, nearby surfaces occlude.
    pub radius: f32,
    pub intensity: f32,
}

impl Default for AmbientOcclusion {
    fn default() -> Self {
        AmbientOcclusion {
            radius: 0.5,
            intensity: 1.0,
        }
    }
}

/// Blur of what lies outside `focus_range` around `focus_distance` from the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthOfField {
    pub focus_distance: f32,
    pub focus_range: f32,
    /// The widest blur, in pixels across.
    pub max_blur: f32,
}

impl Default for DepthOfField {
    fn default() -> Self {
        DepthOfField {
            focus_distance: 10.0,
            focus_range: 5.0,
            max_blur: 8.0,
        }
    }
}
