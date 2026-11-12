//! Image fetching, decoding and fitting shared by the Hosts that draw images themselves.

use std::collections::BTreeMap;
use std::io::Read;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
pub use image::RgbaImage;
use image::imageops::{self, FilterType};

use crate::ImageMode;

mod managed;

pub use managed::{configure_managed_cache, load_managed, remove_managed_image};

const MAX_DOWNLOAD: u64 = 64 * 1024 * 1024;

/// Parses a `mode` wire string; unknown modes yield `None`.
pub fn parse_mode(value: &str) -> Option<ImageMode> {
    match value {
        "aspectFill" => Some(ImageMode::AspectFill),
        "aspectFit" => Some(ImageMode::AspectFit),
        "scaleToFill" => Some(ImageMode::ScaleToFill),
        "center" => Some(ImageMode::Center),
        _ => None,
    }
}

/// Loads `src` (http(s), `file://`, base64 `data:image/` or a path) through a shared decode cache.
pub fn load_image(src: &str, headers: &str) -> Result<Arc<RgbaImage>, String> {
    let cache_key = format!("{src}\n{headers}");
    if let Some(image) = image_cache()
        .lock()
        .expect("Image cache lock poisoned")
        .get(&cache_key)
        .cloned()
    {
        return Ok(image);
    }
    let bytes = if src.starts_with("http://") || src.starts_with("https://") {
        fetch(&ureq::agent(), src, headers, MAX_DOWNLOAD)?
    } else if let Some(path) = src.strip_prefix("file://") {
        std::fs::read(path).map_err(|error| error.to_string())?
    } else if src.starts_with("data:image/") {
        decode_data_url(src)?
    } else {
        std::fs::read(src).map_err(|error| error.to_string())?
    };
    let image = image::load_from_memory(&bytes)
        .map(image::DynamicImage::into_rgba8)
        .map(Arc::new)
        .map_err(|error| error.to_string())?;
    image_cache()
        .lock()
        .expect("Image cache lock poisoned")
        .put(cache_key, Arc::clone(&image));
    Ok(image)
}

/// Downloads at most `limit` bytes of `src`, sending the JSON-object `headers`.
fn fetch(agent: &ureq::Agent, src: &str, headers: &str, limit: u64) -> Result<Vec<u8>, String> {
    let parsed: BTreeMap<String, String> = if headers.trim().is_empty() {
        BTreeMap::new()
    } else {
        serde_json::from_str(headers)
            .map_err(|error| format!("invalid image request headers: {error}"))?
    };
    let mut request = agent.get(src);
    for (name, value) in parsed {
        request = request.set(&name, &value);
    }
    let response = request.call().map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn image_cache() -> &'static Mutex<lru::LruCache<String, Arc<RgbaImage>>> {
    static CACHE: OnceLock<Mutex<lru::LruCache<String, Arc<RgbaImage>>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(lru::LruCache::new(
            NonZeroUsize::new(64).expect("cache capacity is non-zero"),
        ))
    })
}

fn decode_data_url(src: &str) -> Result<Vec<u8>, String> {
    let (metadata, payload) = src
        .split_once(',')
        .ok_or_else(|| "image data URL has no payload".to_owned())?;
    if !metadata.ends_with(";base64") {
        return Err("only base64 image data URLs are supported".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|error| error.to_string())
}

/// Resamples `source` into a `width`×`height` canvas the way `mode` lays it out.
pub fn fit_image(source: &RgbaImage, mode: ImageMode, width: u32, height: u32) -> RgbaImage {
    match mode {
        ImageMode::ScaleToFill => imageops::resize(source, width, height, FilterType::Triangle),
        ImageMode::AspectFill => {
            let scale =
                (width as f64 / source.width() as f64).max(height as f64 / source.height() as f64);
            let resized = imageops::resize(
                source,
                (source.width() as f64 * scale).round().max(1.0) as u32,
                (source.height() as f64 * scale).round().max(1.0) as u32,
                FilterType::Triangle,
            );
            let x = resized.width().saturating_sub(width) / 2;
            let y = resized.height().saturating_sub(height) / 2;
            imageops::crop_imm(&resized, x, y, width, height).to_image()
        }
        ImageMode::AspectFit => {
            let scale =
                (width as f64 / source.width() as f64).min(height as f64 / source.height() as f64);
            let resized = imageops::resize(
                source,
                (source.width() as f64 * scale).round().max(1.0) as u32,
                (source.height() as f64 * scale).round().max(1.0) as u32,
                FilterType::Triangle,
            );
            centered(&resized, width, height)
        }
        ImageMode::Center => centered(source, width, height),
    }
}

fn centered(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let mut target = RgbaImage::new(width, height);
    let x = (width as i64 - source.width() as i64) / 2;
    let y = (height as i64 - source.height() as i64) / 2;
    imageops::overlay(&mut target, source, x, y);
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_fit_letterboxes_without_distorting() {
        let source = RgbaImage::from_pixel(4, 2, image::Rgba([255, 0, 0, 255]));
        let fitted = fit_image(&source, ImageMode::AspectFit, 4, 4);
        assert_eq!(fitted.dimensions(), (4, 4));
        assert_eq!(fitted.get_pixel(0, 0).0[3], 0);
        assert_eq!(fitted.get_pixel(0, 1).0, [255, 0, 0, 255]);
    }

    #[test]
    fn file_loader_populates_the_shared_decode_cache() {
        let path = std::env::temp_dir().join(format!("whisker-image-{}.png", std::process::id()));
        image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 3, image::Rgba([1, 2, 3, 255])))
            .save(&path)
            .unwrap();
        let source = path.to_string_lossy();
        let first = load_image(&source, "").unwrap();
        let second = load_image(&source, "").unwrap();
        assert_eq!(first.dimensions(), (2, 3));
        assert!(Arc::ptr_eq(&first, &second));
        std::fs::remove_file(path).ok();
    }
}
