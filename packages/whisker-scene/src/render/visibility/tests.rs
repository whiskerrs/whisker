use glam::{Mat4, Vec3};
use whisker::css::Color;

use super::*;
use crate::core::{Geometry, Material};

fn camera() -> (Mat4, Frustum) {
    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
    let projection = Mat4::perspective_rh(60f32.to_radians(), 1.0, 0.1, 20.0);
    (view, Frustum::new(projection * view))
}

#[test]
fn frustum_keeps_spheres_that_touch_the_view_and_rejects_the_rest() {
    let (_, frustum) = camera();
    assert!(frustum.intersects_sphere(Vec3::ZERO, 0.5));
    assert!(
        !frustum.intersects_sphere(Vec3::new(0.0, 0.0, 8.0), 0.5),
        "behind the camera"
    );
    assert!(
        !frustum.intersects_sphere(Vec3::new(0.0, 0.0, -30.0), 0.5),
        "past the far plane"
    );
    assert!(
        !frustum.intersects_sphere(Vec3::new(10.0, 0.0, 0.0), 0.5),
        "off to the side"
    );
    assert!(
        frustum.intersects_sphere(Vec3::new(10.0, 0.0, 0.0), 8.0),
        "straddling an edge"
    );
}

fn draw(at: Vec3, color: Color) -> Draw {
    Draw {
        model: Mat4::from_translation(at),
        geometry: Geometry::cuboid(Vec3::ONE),
        material: Material::Unlit { color },
        deform: None,
    }
}

#[test]
fn draw_order_drops_unseen_draws_and_puts_translucent_ones_last_farthest_first() {
    let (view, frustum) = camera();
    let glass = Color::rgba(255, 255, 255, 0.5);
    let draws = [
        draw(Vec3::new(0.0, 0.0, 1.0), glass),
        draw(Vec3::new(40.0, 0.0, 0.0), Color::rgb(0, 0, 0)),
        draw(Vec3::new(0.0, 0.0, -3.0), glass),
        draw(Vec3::ZERO, Color::rgb(0, 0, 0)),
    ];
    let order: Vec<(usize, bool)> = draw_order(&draws, view, &frustum)
        .iter()
        .map(|ordered| (ordered.index, ordered.blended))
        .collect();
    assert_eq!(order, [(3, false), (2, true), (0, true)]);
}

#[test]
fn scaling_a_draw_grows_its_bounds() {
    let (view, frustum) = camera();
    let mut wide = draw(Vec3::new(12.0, 0.0, 0.0), Color::rgb(0, 0, 0));
    assert!(draw_order(std::slice::from_ref(&wide), view, &frustum).is_empty());
    wide.model *= Mat4::from_scale(Vec3::new(20.0, 1.0, 1.0));
    assert_eq!(draw_order(&[wide], view, &frustum).len(), 1);
}
