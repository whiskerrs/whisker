//! CSS colors as MapLibre styles write them.

/// An sRGB color with straight alpha, each channel 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct Rgba(pub [f32; 4]);

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba([0.0; 4]);
    pub const BLACK: Rgba = Rgba([0.0, 0.0, 0.0, 1.0]);

    pub fn premultiplied(self, opacity: f32) -> [f32; 4] {
        let [r, g, b, a] = self.0;
        let a = a * opacity;
        [r * a, g * a, b * a, a]
    }

    pub fn lerp(self, other: Rgba, t: f32) -> Rgba {
        let mut out = [0.0; 4];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.0[i] + (other.0[i] - self.0[i]) * t;
        }
        Rgba(out)
    }
}

pub(crate) fn parse(text: &str) -> Option<Rgba> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    if lower == "transparent" {
        return Some(Rgba::TRANSPARENT);
    }
    if let Some(hex) = lower.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some((name, args)) = lower.split_once('(') {
        let args = args.strip_suffix(')')?;
        let parts: Vec<&str> = args
            .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
            .filter(|p| !p.is_empty())
            .collect();
        return match name.trim() {
            "rgb" | "rgba" => parse_rgb(&parts),
            "hsl" | "hsla" => parse_hsl(&parts),
            _ => None,
        };
    }
    whisker_style::named_color_srgb(&lower).map(|[r, g, b]| {
        Rgba([
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            1.0,
        ])
    })
}

fn parse_hex(hex: &str) -> Option<Rgba> {
    let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
    let pair = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 | 4 => (
            digit(0)? * 17,
            digit(1)? * 17,
            digit(2)? * 17,
            if hex.len() == 4 { digit(3)? * 17 } else { 255 },
        ),
        6 | 8 => (
            pair(0)?,
            pair(2)?,
            pair(4)?,
            if hex.len() == 8 { pair(6)? } else { 255 },
        ),
        _ => return None,
    };
    Some(Rgba([
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        f32::from(a) / 255.0,
    ]))
}

fn alpha(part: Option<&&str>) -> Option<f32> {
    match part {
        None => Some(1.0),
        Some(p) => match p.strip_suffix('%') {
            Some(pct) => pct.parse::<f32>().ok().map(|v| v / 100.0),
            None => p.parse::<f32>().ok(),
        },
    }
    .map(|a| a.clamp(0.0, 1.0))
}

fn parse_rgb(parts: &[&str]) -> Option<Rgba> {
    if parts.len() < 3 {
        return None;
    }
    let channel = |p: &str| -> Option<f32> {
        match p.strip_suffix('%') {
            Some(pct) => pct.parse::<f32>().ok().map(|v| v / 100.0),
            None => p.parse::<f32>().ok().map(|v| v / 255.0),
        }
        .map(|v| v.clamp(0.0, 1.0))
    };
    Some(Rgba([
        channel(parts[0])?,
        channel(parts[1])?,
        channel(parts[2])?,
        alpha(parts.get(3))?,
    ]))
}

fn parse_hsl(parts: &[&str]) -> Option<Rgba> {
    if parts.len() < 3 {
        return None;
    }
    let h = parts[0].trim_end_matches("deg").parse::<f32>().ok()?;
    let s = parts[1].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
    let l = parts[2].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
    let [r, g, b] = hsl_to_rgb(h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    Some(Rgba([r, g, b, alpha(parts.get(3))?]))
}

pub(crate) fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.0) / 360.0;
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |t: f32| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_css_colors() {
        assert_eq!(parse("#fff"), Some(Rgba([1.0, 1.0, 1.0, 1.0])));
        assert_eq!(parse("#ff000080").unwrap().0[0], 1.0);
        assert!((parse("#ff000080").unwrap().0[3] - 0.502).abs() < 0.01);
        assert_eq!(
            parse("rgba(255, 0, 0, 0.5)"),
            Some(Rgba([1.0, 0.0, 0.0, 0.5]))
        );
        assert_eq!(parse("rgb(0 0 255)"), Some(Rgba([0.0, 0.0, 1.0, 1.0])));
        let hsl = parse("hsl(120, 100%, 50%)").unwrap();
        assert!((hsl.0[1] - 1.0).abs() < 1e-5 && hsl.0[0].abs() < 1e-5);
        assert_eq!(parse("red"), Some(Rgba([1.0, 0.0, 0.0, 1.0])));
        assert_eq!(parse("hsla(0, 0%, 0%, 0.3)").unwrap().0[3], 0.3);
        assert_eq!(parse("nope"), None);
    }
}
