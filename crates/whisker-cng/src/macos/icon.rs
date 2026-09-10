use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use image::imageops::FilterType;
use whisker_config::{AppIconConfig, Config};
use whisker_plugin::{Plugin, PluginConfig};

pub(super) fn source(config: &Config, app_dir: &Path) -> Result<Option<Vec<u8>>> {
    let Some(config) = config.plugins.get(AppIconConfig::NAME) else {
        return Ok(None);
    };
    let config: AppIconConfig =
        serde_json::from_value(config.clone()).context("read AppIcon configuration")?;
    crate::plugins::app_icon::AppIcon.validate(&config)?;
    config
        .source
        .map(|path| {
            let path = app_dir.join(path);
            std::fs::read(&path).with_context(|| format!("read app icon {}", path.display()))
        })
        .transpose()
}

pub(super) fn render(png: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .context("decode macOS app icon PNG")?;
    ensure!(
        image.width() == image.height() && image.width() >= 1024,
        "macOS app icon must be a square PNG of at least 1024×1024 pixels"
    );
    let mut images = Vec::new();
    for size in [16, 32, 128, 256, 512] {
        for scale in [1, 2] {
            let edge = size * scale;
            let resized = image.resize_exact(edge, edge, FilterType::Lanczos3);
            let mut bytes = Cursor::new(Vec::new());
            resized.write_to(&mut bytes, image::ImageFormat::Png)?;
            let suffix = if scale == 2 { "@2x" } else { "" };
            images.push((
                format!("icon_{size}x{size}{suffix}.png"),
                bytes.into_inner(),
            ));
        }
    }
    Ok(images)
}
