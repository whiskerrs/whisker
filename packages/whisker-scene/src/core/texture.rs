use std::cell::OnceCell;
use std::rc::Rc;

/// An sRGB image that materials sample, uploaded to the GPU once and shared.
#[derive(Clone)]
pub struct Texture {
    data: Rc<TextureData>,
}

pub(crate) struct TextureData {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
    pub(crate) gpu: OnceCell<crate::render::GpuTexture>,
}

/// Why [`Texture::decode`] failed.
#[derive(Debug)]
pub struct TextureError(image::ImageError);

impl std::fmt::Display for TextureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the image could not be decoded: {}", self.0)
    }
}

impl std::error::Error for TextureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl PartialEq for Texture {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.data, &other.data)
    }
}

impl std::fmt::Debug for Texture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Texture")
            .field("width", &self.data.width)
            .field("height", &self.data.height)
            .finish()
    }
}

impl Texture {
    /// # Panics
    ///
    /// If `pixels` is not `width * height * 4` bytes, or either side is zero.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Texture {
        assert!(
            width > 0 && height > 0,
            "a texture needs at least one pixel"
        );
        assert_eq!(
            pixels.len(),
            width as usize * height as usize * 4,
            "pixels must be width * height RGBA8 values"
        );
        Texture {
            data: Rc::new(TextureData {
                width,
                height,
                pixels,
                gpu: OnceCell::new(),
            }),
        }
    }

    /// Decodes a PNG or JPEG file.
    pub fn decode(bytes: &[u8]) -> Result<Texture, TextureError> {
        let image = image::load_from_memory(bytes)
            .map_err(TextureError)?
            .into_rgba8();
        let (width, height) = image.dimensions();
        Ok(Texture::from_rgba8(width, height, image.into_raw()))
    }

    pub fn width(&self) -> u32 {
        self.data.width
    }

    pub fn height(&self) -> u32 {
        self.data.height
    }

    pub(crate) fn data(&self) -> &TextureData {
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_png() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 3, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let texture = Texture::decode(&png).unwrap();
        assert_eq!((texture.width(), texture.height()), (2, 3));
        assert_eq!(&texture.data().pixels[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Texture::decode(b"not an image").is_err());
    }
}
