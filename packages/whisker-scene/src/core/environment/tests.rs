use glam::{Vec2, Vec3};

use super::*;

fn even(radiance: Vec3) -> Environment {
    Environment::from_radiance(32, 16, vec![radiance; 32 * 16])
}

#[test]
fn an_even_environment_irradiates_and_reflects_its_radiance_everywhere() {
    let radiance = Vec3::new(0.2, 0.5, 1.5);
    let environment = even(radiance);
    let baked = environment.data().baked();
    for d in [Vec3::X, Vec3::NEG_Y, Vec3::new(0.3, 0.8, -0.5).normalize()] {
        let e = evaluate_irradiance(&baked.irradiance, d);
        assert!(e.abs_diff_eq(radiance, 0.02), "{d} {e}");
    }
    for level in &baked.levels {
        for texel in level {
            assert!(texel.abs_diff_eq(radiance, 1e-3), "{texel}");
        }
    }
}

#[test]
fn levels_halve_down_to_the_roughest() {
    let baked_levels = even(Vec3::ONE).data().baked().levels.len();
    assert_eq!(baked_levels, CUBE_LEVELS as usize);
    let sizes: Vec<usize> = even(Vec3::ONE)
        .data()
        .baked()
        .levels
        .iter()
        .map(Vec::len)
        .collect();
    assert_eq!(sizes[0], (CUBE_SIZE * CUBE_SIZE * 6) as usize);
    assert_eq!(sizes[1], sizes[0] / 4);
}

#[test]
fn the_panorama_centers_on_minus_z_with_up_on_top() {
    assert!(equirect_uv(Vec3::NEG_Z).abs_diff_eq(Vec2::new(0.5, 0.5), 1e-6));
    assert!((equirect_uv(Vec3::Y).y).abs() < 1e-6);
    assert!(equirect_uv(Vec3::X).abs_diff_eq(Vec2::new(0.75, 0.5), 1e-6));
}

#[test]
fn cube_faces_center_on_their_axes() {
    let axes = [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
    ];
    for (face, axis) in axes.into_iter().enumerate() {
        let center = cube_direction(face, 1, 1, 3);
        assert!(center.abs_diff_eq(axis, 1e-6), "{face} {center}");
    }
}

#[test]
fn a_bright_sky_lights_up_facing_surfaces_and_rough_levels_blur_it() {
    let gradient = Environment::gradient(
        Color::rgb(255, 255, 255),
        Color::rgb(128, 128, 128),
        Color::rgb(0, 0, 0),
    );
    let baked = gradient.data().baked();
    let up = evaluate_irradiance(&baked.irradiance, Vec3::Y);
    let down = evaluate_irradiance(&baked.irradiance, Vec3::NEG_Y);
    assert!(up.x > 0.5 && down.x < 0.1, "{up} {down}");
    let spread = |level: &[Vec3]| {
        let (min, max) = level.iter().fold((f32::MAX, f32::MIN), |(lo, hi), t| {
            (lo.min(t.x), hi.max(t.x))
        });
        max - min
    };
    assert!(spread(&baked.levels[5]) < spread(&baked.levels[0]));
}

#[test]
fn decodes_radiance_hdr() {
    let mut bytes = Vec::new();
    let pixels = vec![image::Rgb([4.0f32, 0.5, 0.25]); 4 * 2];
    image::codecs::hdr::HdrEncoder::new(&mut bytes)
        .encode(&pixels, 4, 2)
        .unwrap();
    let environment = Environment::decode(&bytes).unwrap();
    let texel = environment.data().pixels[0];
    assert!(
        texel.abs_diff_eq(Vec3::new(4.0, 0.5, 0.25), 0.05),
        "{texel}"
    );
}
