use std::sync::{Arc, Weak};

#[derive(Debug)]
pub(crate) struct TextureData {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

/// An image used by a [`Material`](crate::Material).
///
/// Clones share one GPU copy, which is released after the last clone is dropped.
#[derive(Clone, Debug)]
pub struct Texture(Arc<TextureData>);

impl PartialEq for Texture {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Texture {
    /// Creates a texture from rows of 8-bit RGBA pixels, starting at the top row.
    ///
    /// # Panics
    ///
    /// Panics if a side is zero, or `pixels` does not hold exactly `width * height` pixels.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        assert!(width > 0 && height > 0, "texture sides must not be zero");
        assert_eq!(
            pixels.len(),
            width as usize * height as usize * 4,
            "texture pixels must match its size"
        );
        Self(Arc::new(TextureData {
            width,
            height,
            pixels,
        }))
    }

    pub(crate) fn data(&self) -> &TextureData {
        &self.0
    }

    pub(crate) fn downgrade(&self) -> Weak<TextureData> {
        Arc::downgrade(&self.0)
    }

    /// Identifies the shared data. A [`Weak`] from [`Self::downgrade`] keeps the address unique.
    pub(crate) fn key(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}

pub(crate) struct MipLevel {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

/// Halves `data` down to one pixel. sRGB color averages in linear light, or dark and bright detail
/// would darken with distance.
pub(crate) fn mip_chain(data: &TextureData, srgb: bool) -> Vec<MipLevel> {
    let to_linear: Vec<f32> = (0..=255u32)
        .map(|value| {
            let value = value as f32 / 255.0;
            if !srgb {
                value
            } else if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        })
        .collect();
    let encode = |linear: f32| -> u8 {
        let value = if !srgb {
            linear
        } else if linear <= 0.0031308 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        (value * 255.0).round() as u8
    };

    let mut levels = vec![MipLevel {
        width: data.width,
        height: data.height,
        pixels: data.pixels.clone(),
    }];
    while let Some(last) = levels.last().filter(|l| l.width > 1 || l.height > 1) {
        let (width, height) = ((last.width / 2).max(1), (last.height / 2).max(1));
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0.0f32; 4];
                for (sx, sy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let px = (x * 2 + sx).min(last.width - 1) as usize;
                    let py = (y * 2 + sy).min(last.height - 1) as usize;
                    let at = (py * last.width as usize + px) * 4;
                    for channel in 0..3 {
                        sum[channel] += to_linear[last.pixels[at + channel] as usize];
                    }
                    sum[3] += f32::from(last.pixels[at + 3]) / 255.0;
                }
                pixels.extend([
                    encode(sum[0] / 4.0),
                    encode(sum[1] / 4.0),
                    encode(sum[2] / 4.0),
                    (sum[3] / 4.0 * 255.0).round() as u8,
                ]);
            }
        }
        levels.push(MipLevel {
            width,
            height,
            pixels,
        });
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chain_halves_each_side_down_to_one_pixel() {
        let texture = Texture::from_rgba8(5, 2, vec![0; 40]);
        let sizes: Vec<_> = mip_chain(texture.data(), false)
            .iter()
            .map(|level| (level.width, level.height, level.pixels.len()))
            .collect();
        assert_eq!(sizes, [(5, 2, 40), (2, 1, 8), (1, 1, 4)]);
    }

    #[test]
    fn srgb_color_averages_in_linear_light() {
        let pixels = [0, 0, 0, 255, 255, 255, 255, 255].to_vec();
        let texture = Texture::from_rgba8(2, 1, pixels);

        assert_eq!(mip_chain(texture.data(), true)[1].pixels[0], 188);
        assert_eq!(mip_chain(texture.data(), false)[1].pixels[0], 128);
    }

    #[test]
    #[should_panic(expected = "match its size")]
    fn pixels_of_the_wrong_length_are_rejected() {
        Texture::from_rgba8(2, 2, vec![0; 4]);
    }
}
