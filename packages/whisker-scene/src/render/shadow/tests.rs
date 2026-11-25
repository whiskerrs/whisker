use glam::{Vec3, Vec4Swizzles};

use super::*;
use crate::core::{Projection, Transform};

fn eye_at(position: Vec3) -> Eye {
    Eye {
        world: Transform::looking_at(position, position + Vec3::new(0.0, -0.5, -1.0), Vec3::Y)
            .to_matrix(),
        projection: Projection::Perspective {
            fov_y: 1.0,
            near: 0.1,
            far: 100.0,
        }
        .to_matrix(1.5),
    }
}

const TOWARD: Vec3 = Vec3::new(0.3, 0.9, 0.3);

#[test]
fn splits_grow_toward_the_reach_and_end_on_it() {
    let splits = splits(0.1, 50.0, 3);
    assert!(splits[0] < splits[1] && splits[1] < splits[2], "{splits:?}");
    assert!((splits[2] - 50.0).abs() < 1e-3, "{splits:?}");
    assert_eq!(super::splits(0.1, 50.0, 1)[0], 50.0);
}

#[test]
fn a_cascade_holds_its_whole_slice() {
    let eye = eye_at(Vec3::new(3.0, 5.0, 2.0));
    let (matrix, _) = cascade_matrix(eye, 2.0, 10.0, TOWARD.normalize(), 1024, &[]);
    for corner in slice_corners(eye, 2.0, 10.0) {
        let clip = matrix * eye.world.transform_point3(corner).extend(1.0);
        let clip = clip.xyz() / clip.w;
        assert!(clip.x.abs() <= 1.0 && clip.y.abs() <= 1.0, "{clip}");
        assert!((0.0..=1.0).contains(&clip.z), "{clip}");
    }
}

#[test]
fn a_cascade_moves_by_whole_texels() {
    let toward = TOWARD.normalize();
    let size = 1024;
    let texel_offset = |eye: Eye| {
        let (matrix, _) = cascade_matrix(eye, 2.0, 10.0, toward, size, &[]);
        let x = matrix.project_point3(Vec3::ZERO).x * size as f32 / 2.0;
        x - x.round()
    };
    let a = texel_offset(eye_at(Vec3::new(3.0, 5.0, 2.0)));
    let b = texel_offset(eye_at(Vec3::new(3.013, 5.0, 2.007)));
    assert!((a - b).abs() < 1e-2, "{a} {b}");
}

#[test]
fn a_cascade_reaches_back_to_casters_between_it_and_the_light() {
    let eye = eye_at(Vec3::new(0.0, 5.0, 0.0));
    let toward = TOWARD.normalize();
    let slice_center = eye.world.transform_point3(Vec3::new(0.0, 0.0, -6.0));
    let caster = slice_center + toward * 40.0;
    let (matrix, _) = cascade_matrix(eye, 2.0, 10.0, toward, 1024, &[(caster, 1.0)]);
    let depth = matrix.project_point3(caster).z;
    assert!((0.0..=1.0).contains(&depth), "{depth}");
}

#[test]
fn point_faces_each_see_their_own_axis() {
    let faces = face_matrices(Vec3::ZERO, 10.0);
    for (matrix, (direction, _)) in faces.iter().zip(FACES) {
        let clip = matrix.project_point3(direction * 5.0);
        assert!(clip.x.abs() < 1e-4 && clip.y.abs() < 1e-4, "{clip}");
        assert!((0.0..1.0).contains(&clip.z), "{clip}");
    }
}

#[test]
fn shadows_reach_past_the_far_plane_only_when_not_held_within_it() {
    use crate::core::{Light, Node, Shadow};
    use whisker::css::Color;
    let root = Node::new();
    let sun = Node::new();
    sun.set_light(Some(Light::Directional {
        color: Color::rgb(255, 255, 255),
        intensity: 1.0,
        shadow: Some(Shadow {
            distance: 500.0,
            cascades: 2,
            ..Default::default()
        }),
    }));
    root.add_child(&sun);
    let collected = crate::render::collect(&root, Vec3::ZERO);
    let reach = |within_far| {
        let plan = plan(&collected, eye_at(Vec3::ZERO), within_far, 4096);
        let cascades = plan.directional[0].as_ref().unwrap();
        cascades.splits[cascades.count - 1]
    };
    assert!((reach(true) - 100.0).abs() < 0.1, "{}", reach(true));
    assert!((reach(false) - 500.0).abs() < 0.1, "{}", reach(false));
}
