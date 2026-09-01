//! Static application-background validation and platform formatting.

use anyhow::{Result, bail};
use whisker_config::Config;

pub(crate) const DEFAULT_BACKGROUND: &str = "#FFFFFF";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AppBackground {
    hex: String,
    rgb: [u8; 3],
}

impl AppBackground {
    pub(crate) fn resolve(config: &Config) -> Result<Self> {
        Self::parse(config.background.as_deref().unwrap_or(DEFAULT_BACKGROUND))
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        let bytes = value.as_bytes();
        if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit) {
            bail!("whisker.rs: app.background(\"…\") must use #RRGGBB, got `{value}`");
        }
        let component = |range: std::ops::Range<usize>| {
            u8::from_str_radix(&value[range], 16).expect("validated hexadecimal component")
        };
        Ok(Self {
            hex: value.to_ascii_uppercase(),
            rgb: [component(1..3), component(3..5), component(5..7)],
        })
    }

    pub(crate) fn hex(&self) -> &str {
        &self.hex
    }

    pub(crate) fn rgb(&self) -> [u8; 3] {
        self.rgb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_hex_and_defaults_to_white() {
        assert_eq!(AppBackground::parse("#10a0f8").unwrap().hex(), "#10A0F8");
        assert_eq!(
            AppBackground::resolve(&Config::default()).unwrap().hex(),
            DEFAULT_BACKGROUND,
        );
    }

    #[test]
    fn rejects_values_that_are_not_opaque_rgb() {
        for value in ["white", "#FFF", "#101018FF", "101018", "#GG0011"] {
            assert!(AppBackground::parse(value).is_err(), "accepted {value}");
        }
    }
}
