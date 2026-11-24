use glam::{Quat, Vec2, Vec3};
use whisker::css::Color;

use super::*;
use crate::core::{Material, Projection, Transform};

fn unlit() -> Material {
    Material::Unlit {
        color: Color::rgb(255, 255, 255),
    }
}

fn mesh_at(geometry: Geometry, position: Vec3) -> Node {
    let node = Node::new();
    node.set_mesh(Some((geometry, unlit())));
    node.set_transform(Transform::from_translation(position));
    node
}

#[test]
fn rays_from_the_view_pass_through_the_projected_point() {
    let scene = Scene::new();
    let camera = Node::new();
    camera.set_transform(Transform::looking_at(
        Vec3::new(1.0, 2.0, 6.0),
        Vec3::ZERO,
        Vec3::Y,
    ));
    let projection = Projection::Perspective {
        fov_y: 50f32.to_radians(),
        near: 0.1,
        far: 100.0,
    };
    camera.set_camera(Some(projection));
    scene.root().add_child(&camera);
    let size = Vec2::new(400.0, 300.0);
    let target = Vec3::new(0.7, -0.4, 0.5);
    let clip = projection.to_matrix(size.x / size.y)
        * camera.transform().to_matrix().inverse()
        * target.extend(1.0);
    let ndc = clip.truncate() / clip.w;
    let point = Vec2::new((ndc.x + 1.0) / 2.0 * size.x, (1.0 - ndc.y) / 2.0 * size.y);
    let ray = scene.ray_from_view(point, size).unwrap();
    let along = (target - ray.origin).dot(ray.direction);
    assert!(ray.at(along).abs_diff_eq(target, 1e-4), "{ray:?}");
    assert!(scene.ray_from_view(point, Vec2::ZERO).is_none());
}

#[test]
fn view_points_round_trip_with_rays_from_the_view() {
    let projections = [
        Projection::Perspective {
            fov_y: 50f32.to_radians(),
            near: 0.1,
            far: 20.0,
        },
        Projection::Orthographic {
            height: 4.0,
            near: 0.1,
            far: 20.0,
        },
    ];
    for projection in projections {
        let scene = Scene::new();
        let camera = Node::new();
        camera.set_transform(Transform::looking_at(
            Vec3::new(1.0, 2.0, 6.0),
            Vec3::ZERO,
            Vec3::Y,
        ));
        camera.set_camera(Some(projection));
        scene.root().add_child(&camera);
        let size = Vec2::new(400.0, 300.0);

        let target = Vec3::new(0.7, -0.4, 0.5);
        let point = scene.view_point(target, size).unwrap();
        let ray = scene.ray_from_view(point, size).unwrap();
        let along = (target - ray.origin).dot(ray.direction);
        assert!(ray.at(along).abs_diff_eq(target, 1e-4), "{projection:?}");

        let point = Vec2::new(90.0, 220.0);
        let ray = scene.ray_from_view(point, size).unwrap();
        let back = scene.view_point(ray.at(3.0), size).unwrap();
        assert!(back.abs_diff_eq(point, 1e-2), "{projection:?} {back}");

        let behind = Vec3::new(1.0, 2.0, 6.0) + Vec3::new(1.0, 2.0, 6.0).normalize();
        assert_eq!(scene.view_point(behind, size), None, "{projection:?}");
        assert_eq!(scene.view_point(Vec3::new(-3.0, -6.0, -18.0), size), None);
        assert_eq!(scene.view_point(target, Vec2::new(0.0, 300.0)), None);
    }
}

#[test]
fn orthographic_rays_run_parallel_to_the_view_direction() {
    let scene = Scene::new();
    let camera = Node::new();
    let transform = Transform::looking_at(Vec3::new(1.0, 2.0, 6.0), Vec3::ZERO, Vec3::Y);
    camera.set_transform(transform);
    camera.set_camera(Some(Projection::Orthographic {
        height: 4.0,
        near: 0.1,
        far: 100.0,
    }));
    scene.root().add_child(&camera);
    let size = Vec2::new(400.0, 300.0);
    let forward = transform.rotation * Vec3::NEG_Z;
    let up = transform.rotation * Vec3::Y;
    let center = scene.ray_from_view(size / 2.0, size).unwrap();
    let top = scene.ray_from_view(Vec2::new(200.0, 0.0), size).unwrap();
    assert!(center.direction.abs_diff_eq(forward, 1e-5), "{center:?}");
    assert!(top.direction.abs_diff_eq(forward, 1e-5), "{top:?}");
    assert!(
        (top.origin - center.origin).abs_diff_eq(up * 2.0, 1e-4),
        "{top:?}"
    );
    let along = (Vec3::ZERO - center.origin).dot(center.direction);
    assert!(center.at(along).abs_diff_eq(Vec3::ZERO, 1e-4));
    let target = mesh_at(Geometry::sphere(0.3, 16), up * 1.8);
    scene.root().add_child(&target);
    let hit = scene
        .raycast(top)
        .into_iter()
        .next()
        .expect("hits the sphere");
    assert_eq!(hit.node, target);
}

#[test]
fn hits_come_nearest_first_and_skip_hidden_meshes() {
    let scene = Scene::new();
    let far = mesh_at(Geometry::cuboid(Vec3::ONE), Vec3::new(0.0, 0.0, -5.0));
    let near = mesh_at(Geometry::sphere(0.5, 24), Vec3::new(0.0, 0.0, -2.0));
    let hidden = mesh_at(Geometry::cuboid(Vec3::ONE), Vec3::new(0.0, 0.0, -1.0));
    hidden.set_visible(false);
    for node in [&far, &near, &hidden] {
        scene.root().add_child(node);
    }
    let hits = scene.raycast(Ray::new(Vec3::ZERO, Vec3::NEG_Z));
    let order: Vec<&Node> = hits.iter().map(|hit| &hit.node).collect();
    // Each closed shape is entered and left.
    assert_eq!(order, [&near, &near, &far, &far]);
    assert!(
        (hits[0].distance - 1.5).abs() < 1e-2,
        "{}",
        hits[0].distance
    );
    assert!(hits[0].normal.dot(Vec3::Z) > 0.95, "{}", hits[0].normal);
    assert!((hits[2].distance - 4.5).abs() < 1e-5);
    assert!(hits[2].point.abs_diff_eq(Vec3::new(0.0, 0.0, -4.5), 1e-5));
}

#[test]
fn rays_follow_parent_rotation_and_scale() {
    let scene = Scene::new();
    let group = Node::new();
    group.set_transform(Transform {
        translation: Vec3::new(3.0, 0.0, 0.0),
        rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        scale: Vec3::splat(2.0),
    });
    let plane = mesh_at(Geometry::plane(Vec2::splat(1.0)), Vec3::ZERO);
    group.add_child(&plane);
    scene.root().add_child(&group);
    let hits = scene.raycast(Ray::new(Vec3::new(3.9, 5.0, 0.9), Vec3::NEG_Y));
    assert_eq!(hits.len(), 1);
    assert!((hits[0].distance - 5.0).abs() < 1e-5);
    let miss = scene.raycast(Ray::new(Vec3::new(4.1, 5.0, 0.0), Vec3::NEG_Y));
    assert!(miss.is_empty());
}

#[test]
fn the_bvh_finds_what_testing_every_triangle_finds() {
    let geometry = Geometry::sphere(1.0, 64);
    let data = geometry.data();
    let bvh = Bvh::build(&geometry);
    for i in 0..200 {
        let angle = i as f32 * 0.37;
        let origin = Vec3::new(
            angle.cos() * 3.0,
            (i as f32 * 0.11).sin() * 1.5,
            angle.sin() * 3.0,
        );
        let direction = (Vec3::new(0.1, -0.2, 0.05) * (i % 7) as f32 - origin).normalize();
        let ray = (origin, direction);
        let mut expected: Vec<usize> = data
            .indices
            .chunks_exact(3)
            .enumerate()
            .filter(|(_, corners)| hit_triangle(&ray, &data.vertices, corners).is_some())
            .map(|(triangle, _)| triangle)
            .collect();
        let mut found = Vec::new();
        bvh.each_hit(&ray, &data.vertices, &data.indices, |triangle, _| {
            found.push(triangle)
        });
        expected.sort();
        found.sort();
        assert_eq!(found, expected, "ray {i}");
    }
}
