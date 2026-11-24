//! Shading for `raster-dem` tiles.

use crate::tiles::Dem;

/// Earth's circumference at the equator in meters.
const CIRCUMFERENCE: f32 = 40_075_016.7;

/// An RGBA image, straight alpha, that darkens slopes facing away from a light in the northwest
/// and lightens those facing it.
pub(crate) fn hillshade_texture(dem: &Dem, z: u8) -> Vec<u8> {
    let size = dem.size as i32;
    let meters_per_sample = CIRCUMFERENCE / (dem.size as f32 * (1u64 << z) as f32);
    // MapLibre exaggerates slopes more at low zooms, where they would otherwise vanish.
    let exaggeration = if z < 15 {
        (15 - z) as f32 * 0.4 + 1.0
    } else {
        1.0
    };
    let azimuth = 315f32.to_radians();
    let altitude = 45f32.to_radians();
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let h = |dx: i32, dy: i32| dem.height(x + dx, y + dy);
            let dzdx = ((h(1, -1) + 2.0 * h(1, 0) + h(1, 1))
                - (h(-1, -1) + 2.0 * h(-1, 0) + h(-1, 1)))
                / (8.0 * meters_per_sample);
            let dzdy = ((h(-1, 1) + 2.0 * h(0, 1) + h(1, 1))
                - (h(-1, -1) + 2.0 * h(0, -1) + h(1, -1)))
                / (8.0 * meters_per_sample);
            let slope = (exaggeration * (dzdx * dzdx + dzdy * dzdy).sqrt()).atan();
            let aspect = dzdy.atan2(-dzdx);
            let shade = altitude.sin() * slope.cos()
                + altitude.cos()
                    * slope.sin()
                    * (azimuth - aspect - std::f32::consts::FRAC_PI_2).cos();
            let flat = altitude.sin();
            let delta = shade - flat;
            if delta >= 0.0 {
                let a = (delta * 2.0).clamp(0.0, 1.0);
                pixels.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
            } else {
                let a = (-delta * 2.0).clamp(0.0, 1.0);
                pixels.extend_from_slice(&[0, 0, 0, (a * 255.0) as u8]);
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_ground_is_transparent() {
        let dem = Dem {
            size: 4,
            heights: vec![100.0; 36],
        };
        let pixels = hillshade_texture(&dem, 10);
        assert_eq!(pixels.len(), 64);
        assert!(pixels.chunks(4).all(|p| p[3] == 0));
    }
}
