use std::time::Duration;

use glam::{Mat4, Quat, Vec2, Vec3};
use whisker::css::Color;
use whisker_surface::{FrameInfo, SurfaceSize};
use whisker_wgpu::{Gpu, Renderer, wgpu};

use super::pipeline::SHARED;
use super::*;
use crate::core::{
    Fog, Geometry, Light, Material, Node, Projection, Scene, Shadow, StandardMaterial, ToneMapping,
    Transform,
};

#[test]
fn collect_composes_transforms_and_skips_hidden_subtrees() {
    let root = Node::new();
    let group = Node::new();
    group.set_transform(Transform::from_translation(Vec3::X));
    let mesh = Node::new();
    mesh.set_transform(Transform::from_translation(Vec3::Y));
    mesh.set_mesh(Some((
        Geometry::cuboid(Vec3::ONE),
        Material::Unlit {
            color: Color::rgb(0, 0, 0),
        },
    )));
    let hidden = Node::new();
    hidden.set_visible(false);
    let under_hidden = Node::new();
    under_hidden.set_mesh(mesh.mesh());
    root.add_child(&group);
    group.add_child(&mesh);
    root.add_child(&hidden);
    hidden.add_child(&under_hidden);

    let collected = collect(&root);
    assert_eq!(collected.draws.len(), 1);
    assert_eq!(
        collected.draws[0].model.transform_point3(Vec3::ZERO),
        Vec3::new(1.0, 1.0, 0.0)
    );
}

#[test]
fn directional_lights_point_back_toward_the_light() {
    let root = Node::new();
    let light = Node::new();
    light.set_transform(Transform {
        rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        ..Transform::IDENTITY
    });
    light.set_light(Some(Light::Directional {
        color: Color::rgb(255, 255, 255),
        intensity: 2.0,
        shadow: None,
    }));
    root.add_child(&light);
    let collected = collect(&root);
    let light = &collected.directional[0];
    assert!(light.toward.abs_diff_eq(Vec3::Y, 1e-5), "{}", light.toward);
    assert_eq!(light.radiance, [2.0, 2.0, 2.0, 0.0]);
}

/// Runs `test` with a device of its own, dropping every GPU object before the test thread exits.
fn with_gpu(test: impl FnOnce(&Gpu)) {
    // Concurrent devices on WARP, Windows' software adapter, crash the test process.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
        ..Default::default()
    }))
    .unwrap();
    let gpu = Gpu {
        instance,
        adapter,
        device,
        queue,
    };
    test(&gpu);
    SHARED.with_borrow_mut(Option::take);
}

const SIZE: u32 = 64;

/// Renders `scene` offscreen and returns its sRGB pixels.
fn render(gpu: &Gpu, scene: Scene) -> Vec<[u8; 4]> {
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut renderer = SceneRenderer::new(gpu, format, scene);
    renderer.resize(
        gpu,
        SurfaceSize {
            width: SIZE,
            height: SIZE,
            scale: 1.0,
        },
    );
    renderer.render(
        gpu,
        &texture.create_view(&Default::default()),
        FrameInfo {
            timestamp: Duration::ZERO,
        },
    );
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(SIZE * SIZE * 4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        texture.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    bytemuck::cast_slice(&buffer.slice(..).get_mapped_range().unwrap()).to_vec()
}

fn pixel(pixels: &[[u8; 4]], x: u32, y: u32) -> [u8; 4] {
    pixels[(y * SIZE + x) as usize]
}

#[test]
fn draws_an_unlit_mesh_over_the_background() {
    with_gpu(|gpu| {
        let scene = Scene::new();
        scene.set_background(Color::rgb(0, 0, 255));
        let mesh = Node::new();
        mesh.set_mesh(Some((
            Geometry::cuboid(Vec3::splat(1.0)),
            Material::Unlit {
                color: Color::rgb(255, 0, 0),
            },
        )));
        scene.root().add_child(&mesh);
        let pixels = render(gpu, scene);
        assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, 0, 0), [0, 0, 255, 255]);
    });
}

#[test]
fn a_light_facing_the_surface_brightens_it_fully() {
    with_gpu(|gpu| {
        let scene = Scene::new();
        let camera = Node::new();
        camera.set_transform(Transform::looking_at(
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::ZERO,
            Vec3::Z,
        ));
        camera.set_camera(Some(Projection::Perspective {
            fov_y: 0.5,
            near: 0.1,
            far: 10.0,
        }));
        let floor = Node::new();
        floor.set_mesh(Some((
            Geometry::plane(Vec2::splat(4.0)),
            Material::Standard(StandardMaterial {
                roughness: 1.0,
                ..Default::default()
            }),
        )));
        let light = Node::new();
        light.set_transform(Transform {
            rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            ..Transform::IDENTITY
        });
        light.set_light(Some(Light::Directional {
            color: Color::rgb(255, 255, 255),
            intensity: 1.0,
            shadow: None,
        }));
        for node in [&camera, &floor, &light] {
            scene.root().add_child(node);
        }
        let lit = pixel(&render(gpu, scene.clone()), SIZE / 2, SIZE / 2);
        assert!(lit[0] >= 235, "{lit:?}");

        light.remove_from_parent();
        let dark = pixel(&render(gpu, scene), SIZE / 2, SIZE / 2);
        assert_eq!(dark, [0, 0, 0, 255]);
    });
}

#[test]
fn translucent_meshes_blend_over_opaque_ones() {
    with_gpu(|gpu| {
        let scene = Scene::new();
        let back = Node::new();
        back.set_mesh(Some((
            Geometry::cuboid(Vec3::ONE),
            Material::Unlit {
                color: Color::rgb(0, 0, 0),
            },
        )));
        let front = Node::new();
        front.set_transform(Transform::from_translation(Vec3::new(0.0, 0.0, 1.0)));
        front.set_mesh(Some((
            Geometry::cuboid(Vec3::splat(0.5)),
            Material::Unlit {
                color: Color::rgba(255, 255, 255, 0.5),
            },
        )));
        // Listed first so that only sorting puts it after the opaque mesh.
        scene.root().add_child(&front);
        scene.root().add_child(&back);
        let center = pixel(&render(gpu, scene), SIZE / 2, SIZE / 2);
        assert!((180..=195).contains(&center[0]), "{center:?}");
    });
}

#[test]
fn a_mesh_centered_off_screen_still_draws_the_part_in_view() {
    with_gpu(|gpu| {
        let scene = Scene::new();
        scene.set_background(Color::rgb(0, 0, 255));
        let mesh = Node::new();
        mesh.set_transform(Transform::from_translation(Vec3::new(6.0, 0.0, 0.0)));
        mesh.set_mesh(Some((
            Geometry::cuboid(Vec3::new(12.0, 1.0, 1.0)),
            Material::Unlit {
                color: Color::rgb(255, 0, 0),
            },
        )));
        scene.root().add_child(&mesh);
        let pixels = render(gpu, scene);
        assert_eq!(pixel(&pixels, SIZE / 2 + 4, SIZE / 2), [255, 0, 0, 255]);
    });
}

/// A white matte floor seen from straight above, with a unit cube floating at `cube`.
fn shadow_stage(eye: Vec3, fov_y: f32, cube: Vec3) -> (Scene, Node, Node, Mat4) {
    let scene = Scene::new();
    let camera = Node::new();
    let transform = Transform::looking_at(eye, Vec3::new(eye.x, 0.0, eye.z), Vec3::NEG_Z);
    let projection = Projection::Perspective {
        fov_y,
        near: 0.1,
        far: 20.0,
    };
    camera.set_transform(transform);
    camera.set_camera(Some(projection));
    let floor = Node::new();
    floor.set_mesh(Some((
        Geometry::plane(Vec2::splat(12.0)),
        Material::Standard(StandardMaterial {
            roughness: 1.0,
            ..Default::default()
        }),
    )));
    let caster = Node::new();
    caster.set_transform(Transform::from_translation(cube));
    caster.set_mesh(Some((
        Geometry::cuboid(Vec3::ONE),
        Material::Unlit {
            color: Color::rgb(255, 0, 0),
        },
    )));
    for node in [&camera, &floor, &caster] {
        scene.root().add_child(node);
    }
    let view_proj = projection.to_matrix(1.0) * transform.to_matrix().inverse();
    (scene, floor, caster, view_proj)
}

/// The pixel a point on the floor lands on.
fn floor_pixel(pixels: &[[u8; 4]], view_proj: Mat4, x: f32, z: f32) -> [u8; 4] {
    let ndc = view_proj.project_point3(Vec3::new(x, 0.0, z));
    let column = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32;
    let row = ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32;
    pixel(pixels, column.min(SIZE - 1), row.min(SIZE - 1))
}

/// A sun shining down and toward -X at 45°.
fn sun(shadow: Option<Shadow>) -> Node {
    let light = Node::new();
    light.set_transform(Transform {
        rotation: crate::core::rotation_toward(Vec3::new(-1.0, -1.0, 0.0), Vec3::Y),
        ..Transform::IDENTITY
    });
    light.set_light(Some(Light::Directional {
        color: Color::rgb(255, 255, 255),
        intensity: 1.0,
        shadow,
    }));
    light
}

const SUN_SHADOW: Shadow = Shadow {
    map_size: 512,
    cascades: 2,
    distance: 20.0,
    softness: 1.0,
    bias: 1.0,
};

#[test]
fn a_shadowing_sun_darkens_the_floor_behind_a_mesh() {
    with_gpu(|gpu| {
        let (scene, _, _, view_proj) = shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::Y * 1.5);
        scene.root().add_child(&sun(Some(SUN_SHADOW)));
        let pixels = render(gpu, scene);
        let shadowed = floor_pixel(&pixels, view_proj, -1.5, 0.0);
        let lit = floor_pixel(&pixels, view_proj, 1.5, 0.0);
        assert!(shadowed[0] < 20, "shadowed {shadowed:?}");
        assert!(lit[0] > 200, "lit {lit:?}");
    });
}

#[test]
fn a_sun_without_a_shadow_lights_the_floor_behind_a_mesh() {
    with_gpu(|gpu| {
        let (scene, _, _, view_proj) = shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::Y * 1.5);
        scene.root().add_child(&sun(None));
        let pixel = floor_pixel(&render(gpu, scene), view_proj, -1.5, 0.0);
        assert!(pixel[0] > 200, "{pixel:?}");
    });
}

#[test]
fn shadow_flags_opt_meshes_out_of_casting_and_receiving() {
    with_gpu(|gpu| {
        let (scene, floor, caster, view_proj) =
            shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::Y * 1.5);
        scene.root().add_child(&sun(Some(SUN_SHADOW)));
        caster.set_cast_shadow(false);
        let pixel = floor_pixel(&render(gpu, scene.clone()), view_proj, -1.5, 0.0);
        assert!(pixel[0] > 200, "not casting {pixel:?}");

        caster.set_cast_shadow(true);
        floor.set_receive_shadow(false);
        let pixel = floor_pixel(&render(gpu, scene), view_proj, -1.5, 0.0);
        assert!(pixel[0] > 200, "not receiving {pixel:?}");
    });
}

#[test]
fn a_mesh_outside_the_view_still_casts_into_it() {
    with_gpu(|gpu| {
        let (scene, _, _, view_proj) = shadow_stage(Vec3::new(-2.5, 3.0, 0.0), 0.3, Vec3::Y * 2.5);
        scene.root().add_child(&sun(Some(SUN_SHADOW)));
        let pixel = floor_pixel(&render(gpu, scene), view_proj, -2.5, 0.0);
        assert!(pixel[0] < 20, "{pixel:?}");
    });
}

#[test]
fn a_shadowing_point_light_darkens_the_floor_under_a_mesh() {
    with_gpu(|gpu| {
        let (scene, _, _, view_proj) = shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::Y * 1.5);
        let light = Node::new();
        light.set_transform(Transform::from_translation(Vec3::Y * 3.0));
        light.set_light(Some(Light::Point {
            color: Color::rgb(255, 255, 255),
            intensity: 10.0,
            range: 20.0,
            shadow: Some(Shadow {
                map_size: 256,
                ..Shadow::default()
            }),
        }));
        scene.root().add_child(&light);
        let pixels = render(gpu, scene);
        let shadowed = floor_pixel(&pixels, view_proj, 1.2, 0.0);
        let lit = floor_pixel(&pixels, view_proj, 2.5, 0.0);
        assert!(shadowed[0] < 20, "shadowed {shadowed:?}");
        assert!(lit[0] > 150, "lit {lit:?}");
    });
}

fn spot(at: Vec3, outer_angle: f32, shadow: Option<Shadow>) -> Node {
    let light = Node::new();
    light.set_transform(Transform {
        translation: at,
        rotation: crate::core::rotation_toward(Vec3::NEG_Y, Vec3::Y),
        scale: Vec3::ONE,
    });
    light.set_light(Some(Light::Spot {
        color: Color::rgb(255, 255, 255),
        intensity: 9.0,
        range: 20.0,
        inner_angle: outer_angle * 0.8,
        outer_angle,
        shadow,
    }));
    light
}

#[test]
fn a_spot_light_lights_only_inside_its_cone() {
    with_gpu(|gpu| {
        let (scene, _, caster, view_proj) =
            shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::Y * 1.5);
        caster.remove_from_parent();
        scene.root().add_child(&spot(Vec3::Y * 3.0, 0.4, None));
        let pixels = render(gpu, scene);
        let inside = floor_pixel(&pixels, view_proj, 0.5, 0.0);
        let outside = floor_pixel(&pixels, view_proj, 2.5, 0.0);
        assert!(inside[0] > 200, "inside {inside:?}");
        assert_eq!(outside, [0, 0, 0, 255]);
    });
}

#[test]
fn a_shadowing_spot_light_darkens_the_floor_under_a_mesh() {
    with_gpu(|gpu| {
        let (scene, _, caster, view_proj) =
            shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::Y * 1.5);
        caster.set_transform(Transform {
            translation: Vec3::Y * 1.5,
            scale: Vec3::splat(0.4),
            ..Transform::IDENTITY
        });
        scene.root().add_child(&spot(
            Vec3::new(-1.0, 3.0, 0.0),
            0.9,
            Some(Shadow {
                map_size: 256,
                ..Shadow::default()
            }),
        ));
        let pixels = render(gpu, scene);
        // The cube at x 0 casts toward +x, twice as far out as it is from the light.
        let shadowed = floor_pixel(&pixels, view_proj, 1.0, 0.0);
        let lit = floor_pixel(&pixels, view_proj, -1.0, 0.8);
        assert!(shadowed[0] < 20, "shadowed {shadowed:?}");
        assert!(lit[0] > 150, "lit {lit:?}");
    });
}

#[test]
fn a_hemisphere_light_shades_by_facing() {
    with_gpu(|gpu| {
        let (scene, _, caster, view_proj) =
            shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::Y * 1.5);
        caster.remove_from_parent();
        scene.set_hemisphere_light(Color::rgb(255, 0, 0), Color::rgb(0, 0, 255), 1.0);
        let up = floor_pixel(&render(gpu, scene), view_proj, 1.0, 0.0);
        assert!(up[0] > 200 && up[2] < 20, "{up:?}");
    });
}

/// The floor's brightness under a sun of `intensity` straight above, with `tone_mapping`.
fn toned(gpu: &Gpu, tone_mapping: ToneMapping, exposure: f32, intensity: f32) -> u8 {
    let (scene, _, caster, view_proj) = shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::Y * 1.5);
    caster.remove_from_parent();
    let light = Node::new();
    light.set_transform(Transform {
        rotation: crate::core::rotation_toward(Vec3::NEG_Y, Vec3::Z),
        ..Transform::IDENTITY
    });
    light.set_light(Some(Light::Directional {
        color: Color::rgb(255, 255, 255),
        intensity,
        shadow: None,
    }));
    scene.root().add_child(&light);
    scene.set_tone_mapping(tone_mapping, exposure);
    floor_pixel(&render(gpu, scene), view_proj, 1.0, 0.0)[0]
}

#[test]
fn tone_mapping_rolls_off_highlights_instead_of_clipping() {
    with_gpu(|gpu| {
        assert_eq!(toned(gpu, ToneMapping::None, 1.0, 4.0), 255);
        assert_eq!(
            toned(gpu, ToneMapping::None, 0.25, 4.0),
            toned(gpu, ToneMapping::None, 1.0, 1.0)
        );
        // 4 / (1 + 4) is 0.8 linear, 231 encoded.
        let reinhard = toned(gpu, ToneMapping::Reinhard, 1.0, 4.0);
        assert!((229..=233).contains(&reinhard), "{reinhard}");
        for curve in [ToneMapping::Aces, ToneMapping::AgX] {
            let (dim, bright) = (toned(gpu, curve, 1.0, 0.5), toned(gpu, curve, 1.0, 4.0));
            assert!(dim < bright && bright < 255, "{curve:?} {dim} {bright}");
        }
    });
}

#[test]
fn fog_fades_distant_surfaces_into_its_color() {
    with_gpu(|gpu| {
        let scene = Scene::new();
        let mesh = Node::new();
        mesh.set_mesh(Some((
            Geometry::cuboid(Vec3::ONE),
            Material::Unlit {
                color: Color::rgb(255, 0, 0),
            },
        )));
        scene.root().add_child(&mesh);
        let gray = Color::rgb(128, 128, 128);
        scene.set_fog(Some(Fog::Linear {
            color: gray,
            near: 0.0,
            far: 1.0,
        }));
        assert_eq!(
            pixel(&render(gpu, scene.clone()), SIZE / 2, SIZE / 2),
            [128, 128, 128, 255]
        );

        scene.set_fog(Some(Fog::Exponential {
            color: gray,
            density: 0.01,
        }));
        let thin = pixel(&render(gpu, scene), SIZE / 2, SIZE / 2);
        assert!(thin[0] > 240 && thin[1] < 20, "{thin:?}");
    });
}
