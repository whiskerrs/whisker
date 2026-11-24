use std::time::Duration;

use glam::{Mat4, Quat, Vec2, Vec3};
use whisker::css::Color;
use whisker_surface::{FrameInfo, SurfaceSize};
use whisker_wgpu::{Gpu, Renderer, wgpu};

use super::pipeline::SHARED;
use super::*;
use crate::core::{
    AmbientOcclusion, Bloom, CustomMaterial, DepthOfField, Environment, Fog, Geometry, Instances,
    Light, Material, MaterialShader, Node, PostProcessing, Projection, Scene, Shadow,
    StandardMaterial, Texture, ToneMapping, Transform,
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

    let collected = collect(&root, Vec3::ZERO);
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
    let collected = collect(&root, Vec3::ZERO);
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
    render_into(gpu, scene, wgpu::TextureFormat::Rgba8UnormSrgb)
}

/// Renders `scene` into a `format` target, as one first frame.
fn render_into(gpu: &Gpu, scene: Scene, format: wgpu::TextureFormat) -> Vec<[u8; 4]> {
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
    toned_through(
        gpu,
        tone_mapping,
        exposure,
        intensity,
        PostProcessing::default(),
    )
}

fn toned_through(
    gpu: &Gpu,
    tone_mapping: ToneMapping,
    exposure: f32,
    intensity: f32,
    post_processing: PostProcessing,
) -> u8 {
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
    scene.set_post_processing(post_processing);
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

/// An environment colored by the axis each direction leans most toward.
fn axis_environment() -> Environment {
    let (width, height) = (128, 64);
    let pixels = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .map(|(x, y)| {
            let theta = (y as f32 + 0.5) / height as f32 * std::f32::consts::PI;
            let phi = ((x as f32 + 0.5) / width as f32 - 0.5) * 2.0 * std::f32::consts::PI;
            let d = Vec3::new(
                theta.sin() * phi.sin(),
                theta.cos(),
                -theta.sin() * phi.cos(),
            );
            let a = d.abs();
            if a.x >= a.y && a.x >= a.z {
                if d.x > 0.0 { Vec3::X } else { Vec3::Z }
            } else if a.y >= a.z {
                if d.y > 0.0 { Vec3::ONE } else { Vec3::ZERO }
            } else if d.z > 0.0 {
                Vec3::Y
            } else {
                Vec3::new(1.0, 1.0, 0.0)
            }
        })
        .collect();
    Environment::from_radiance(width, height, pixels)
}

fn mirror_from(eye: Vec3, up: Vec3) -> [u8; 4] {
    let scene = Scene::new();
    scene.set_environment(Some(axis_environment()), 1.0);
    let camera = Node::new();
    camera.set_transform(Transform::looking_at(eye, Vec3::ZERO, up));
    camera.set_camera(Some(Projection::Perspective {
        fov_y: 0.3,
        near: 0.1,
        far: 20.0,
    }));
    let mirror = Node::new();
    mirror.set_mesh(Some((
        Geometry::cuboid(Vec3::ONE),
        Material::Standard(StandardMaterial {
            metallic: 1.0,
            roughness: 0.0,
            ..Default::default()
        }),
    )));
    scene.root().add_child(&camera);
    scene.root().add_child(&mirror);
    let mut out = [0; 4];
    with_gpu(|gpu| out = pixel(&render(gpu, scene), SIZE / 2, SIZE / 2));
    out
}

#[test]
fn a_mirror_reflects_the_environment_it_faces() {
    let red = mirror_from(Vec3::X * 5.0, Vec3::Y);
    assert!(red[0] > 200 && red[1] < 40 && red[2] < 40, "+X {red:?}");
    let green = mirror_from(Vec3::Z * 5.0, Vec3::Y);
    assert!(
        green[1] > 200 && green[0] < 40 && green[2] < 40,
        "+Z {green:?}"
    );
    let white = mirror_from(Vec3::Y * 5.0, Vec3::Z);
    assert!(white.iter().take(3).all(|c| *c > 200), "+Y {white:?}");
}

#[test]
fn an_even_environment_lights_a_matte_surface_by_its_radiance() {
    with_gpu(|gpu| {
        let (scene, _, caster, view_proj) =
            shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::Y * 1.5);
        caster.remove_from_parent();
        let environment = Environment::from_radiance(8, 4, vec![Vec3::splat(0.5); 32]);
        scene.set_environment(Some(environment), 1.0);
        let lit = floor_pixel(&render(gpu, scene), view_proj, 1.0, 0.0);
        // 0.5 minus the few percent reflected instead of diffused, about 180 encoded.
        assert!((170..=192).contains(&lit[0]), "{lit:?}");
    });
}

#[test]
fn the_environment_can_stand_in_for_the_background() {
    with_gpu(|gpu| {
        let scene = Scene::new();
        scene.set_background(Color::rgb(0, 0, 255));
        scene.set_environment(Some(axis_environment()), 1.0);
        let pixels = render(gpu, scene.clone());
        assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [0, 0, 255, 255]);

        scene.set_environment_background(true);
        // The default camera looks down -Z, which the environment paints yellow.
        let sky = pixel(&render(gpu, scene), SIZE / 2, SIZE / 2);
        assert!(sky[0] > 200 && sky[1] > 200 && sky[2] < 40, "{sky:?}");
    });
}

/// A red cube `size` wide over a blue background, from the default camera.
fn post_stage(size: f32, post_processing: PostProcessing) -> Scene {
    let scene = Scene::new();
    scene.set_background(Color::rgb(0, 0, 255));
    let mesh = Node::new();
    mesh.set_mesh(Some((
        Geometry::cuboid(Vec3::splat(size)),
        Material::Unlit {
            color: Color::rgb(255, 0, 0),
        },
    )));
    scene.root().add_child(&mesh);
    scene.set_post_processing(post_processing);
    scene
}

/// The first column right of center that shows the background.
fn cube_edge(pixels: &[[u8; 4]]) -> u32 {
    (SIZE / 2..SIZE)
        .find(|&x| pixel(pixels, x, SIZE / 2)[0] < 128)
        .unwrap()
}

#[test]
fn flat_regions_pass_through_the_post_chain_unchanged() {
    with_gpu(|gpu| {
        let pixels = render(
            gpu,
            post_stage(
                1.0,
                PostProcessing {
                    fxaa: true,
                    ..Default::default()
                },
            ),
        );
        assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, 0, 0), [0, 0, 255, 255]);
    });
}

#[test]
fn bloom_spreads_bright_surfaces_past_their_edges() {
    with_gpu(|gpu| {
        let plain = render(gpu, post_stage(0.5, PostProcessing::default()));
        let edge = cube_edge(&plain);
        let bloom = PostProcessing {
            bloom: Some(Bloom {
                threshold: 0.5,
                intensity: 1.0,
                ..Default::default()
            }),
            ..Default::default()
        };
        let glowing = render(gpu, post_stage(0.5, bloom));
        let (before, after) = (
            pixel(&plain, edge + 2, SIZE / 2),
            pixel(&glowing, edge + 2, SIZE / 2),
        );
        assert!(after[0] > before[0] + 20, "{before:?} {after:?}");
    });
}

#[test]
fn depth_of_field_blurs_surfaces_away_from_focus() {
    with_gpu(|gpu| {
        let plain = render(gpu, post_stage(1.0, PostProcessing::default()));
        let edge = cube_edge(&plain);
        let blur = |focus_distance| PostProcessing {
            depth_of_field: Some(DepthOfField {
                focus_distance,
                focus_range: 0.5,
                max_blur: 8.0,
            }),
            ..Default::default()
        };
        let blurred = render(gpu, post_stage(1.0, blur(100.0)));
        let outside = pixel(&blurred, edge + 1, SIZE / 2);
        assert!(outside[0] > 40, "{outside:?}");
        assert_eq!(pixel(&blurred, 0, 0), [0, 0, 255, 255]);
    });
}

#[test]
fn ambient_occlusion_darkens_the_floor_beside_a_mesh() {
    with_gpu(|gpu| {
        let floor_at = |occlusion: Option<AmbientOcclusion>| {
            let (scene, _, _, view_proj) =
                shadow_stage(Vec3::new(2.0, 4.0, 0.0), 1.2, Vec3::Y * 0.5);
            scene.set_ambient_light(Color::rgb(255, 255, 255), 0.5);
            scene.set_post_processing(PostProcessing {
                ambient_occlusion: occlusion,
                ..Default::default()
            });
            let pixels = render(gpu, scene);
            (
                i32::from(floor_pixel(&pixels, view_proj, 0.6, 0.0)[0]),
                i32::from(floor_pixel(&pixels, view_proj, 3.0, 2.0)[0]),
            )
        };
        let (near, far) = floor_at(None);
        let (occluded_near, occluded_far) = floor_at(Some(AmbientOcclusion::default()));
        assert!(occluded_near + 20 < near, "{near} {occluded_near}");
        assert!(occluded_far + 3 >= far, "{far} {occluded_far}");
    });
}

/// A lookup table of `size` slices laid side by side, mapping each color through `map`.
fn lookup_table(size: u32, map: impl Fn([u32; 3]) -> [u32; 3]) -> Texture {
    let level = |v: u32| v * 255 / (size - 1);
    let mut pixels = Vec::new();
    for g in 0..size {
        for b in 0..size {
            for r in 0..size {
                let [r, g, b] = map([level(r), level(g), level(b)]);
                pixels.extend([r as u8, g as u8, b as u8, 255]);
            }
        }
    }
    Texture::from_rgba8(size * size, size, pixels)
}

#[test]
fn color_grading_maps_colors_through_the_lookup_table() {
    with_gpu(|gpu| {
        let graded = |lut| {
            render(
                gpu,
                post_stage(
                    1.0,
                    PostProcessing {
                        color_grading: Some(lut),
                        ..Default::default()
                    },
                ),
            )
        };
        let identity = graded(lookup_table(16, |c| c));
        assert_eq!(pixel(&identity, SIZE / 2, SIZE / 2), [255, 0, 0, 255]);
        assert_eq!(pixel(&identity, 0, 0), [0, 0, 255, 255]);
        let inverted = graded(lookup_table(16, |c| c.map(|v| 255 - v)));
        assert_eq!(pixel(&inverted, SIZE / 2, SIZE / 2), [0, 255, 255, 255]);
    });
}

#[test]
fn the_post_chain_tone_maps_from_the_first_frame() {
    with_gpu(|gpu| {
        let fxaa = PostProcessing {
            fxaa: true,
            ..Default::default()
        };
        let direct = toned(gpu, ToneMapping::Reinhard, 1.0, 4.0);
        let post = toned_through(gpu, ToneMapping::Reinhard, 1.0, 4.0, fxaa);
        assert!(direct.abs_diff(post) <= 2, "{direct} {post}");
    });
}

#[test]
fn the_post_chain_encodes_for_linear_targets() {
    with_gpu(|gpu| {
        for fxaa in [false, true] {
            let scene = post_stage(
                1.0,
                PostProcessing {
                    fxaa,
                    color_grading: Some(lookup_table(16, |c| c)),
                    ..Default::default()
                },
            );
            let pixels = render_into(gpu, scene, wgpu::TextureFormat::Rgba8Unorm);
            assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [255, 0, 0, 255]);
            assert_eq!(pixel(&pixels, 0, 0), [0, 0, 255, 255]);
        }
    });
}

#[test]
fn bloom_picks_up_light_brighter_than_white() {
    with_gpu(|gpu| {
        let glowing = |bloom: Option<Bloom>| {
            let scene = post_stage(
                0.5,
                PostProcessing {
                    bloom,
                    ..Default::default()
                },
            );
            let mesh = Node::new();
            mesh.set_mesh(Some((
                Geometry::cuboid(Vec3::splat(0.5)),
                Material::Standard(StandardMaterial {
                    base_color: Color::rgb(0, 0, 0),
                    emissive: Color::rgb(255, 255, 255),
                    emissive_intensity: 4.0,
                    ..Default::default()
                }),
            )));
            scene.root().add_child(&mesh);
            render(gpu, scene)
        };
        let plain = glowing(None);
        let edge = cube_edge(&plain);
        let bloomed = glowing(Some(Bloom {
            intensity: 1.0,
            ..Default::default()
        }));
        let (before, after) = (
            pixel(&plain, edge + 2, SIZE / 2),
            pixel(&bloomed, edge + 2, SIZE / 2),
        );
        assert!(after[0] > before[0] + 20, "{before:?} {after:?}");
    });
}

/// A node drawing a red cube half a unit wide at each of `at`, relative to the node at `origin`.
fn instanced_cubes(origin: Vec3, at: &[Vec3]) -> Node {
    let node = Node::new();
    node.set_transform(Transform::from_translation(origin));
    node.set_mesh(Some((
        Geometry::cuboid(Vec3::splat(0.5)),
        Material::Unlit {
            color: Color::rgb(255, 0, 0),
        },
    )));
    node.set_instances(Some(Instances::new(
        at.iter().map(|&p| Mat4::from_translation(p)).collect(),
    )));
    node
}

#[test]
fn instances_draw_one_copy_per_transform() {
    with_gpu(|gpu| {
        let scene = Scene::new();
        scene.set_background(Color::rgb(0, 0, 255));
        let left = Vec3::new(-1.5, 0.0, 0.0);
        scene
            .root()
            .add_child(&instanced_cubes(Vec3::ZERO, &[left, -left]));
        let pixels = render(gpu, scene);
        // The default camera, 5 units back with a 60° field of view, puts x = ±1.5 at ±0.52.
        assert_eq!(pixel(&pixels, 15, SIZE / 2), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, 48, SIZE / 2), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [0, 0, 255, 255]);
    });
}

#[test]
fn instances_in_view_draw_when_their_node_is_out_of_it() {
    with_gpu(|gpu| {
        let scene = Scene::new();
        let away = Vec3::new(50.0, 0.0, 0.0);
        scene
            .root()
            .add_child(&instanced_cubes(away, &[Vec3::ZERO, -away]));
        let pixels = render(gpu, scene);
        assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [255, 0, 0, 255]);
    });
}

#[test]
fn instance_rotations_turn_normals_with_them() {
    with_gpu(|gpu| {
        let scene = Scene::new();
        let plane = Node::new();
        plane.set_mesh(Some((
            Geometry::plane(Vec2::splat(4.0)),
            Material::Standard(StandardMaterial {
                roughness: 1.0,
                ..Default::default()
            }),
        )));
        // Stood up to face the camera, and stretched, which a plain model matrix would mis-shade.
        plane.set_instances(Some(Instances::new(vec![
            Mat4::from_scale(Vec3::new(1.0, 1.0, 3.0))
                * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
        ])));
        let light = Node::new();
        light.set_light(Some(Light::Directional {
            color: Color::rgb(255, 255, 255),
            intensity: 1.0,
            shadow: None,
        }));
        scene.root().add_child(&plane);
        scene.root().add_child(&light);
        let lit = pixel(&render(gpu, scene), SIZE / 2, SIZE / 2);
        assert!(lit[0] >= 235, "{lit:?}");
    });
}

#[test]
fn instances_cast_shadows() {
    with_gpu(|gpu| {
        let (scene, _, caster, view_proj) = shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::ZERO);
        caster.set_instances(Some(Instances::new(vec![Mat4::from_translation(
            Vec3::Y * 1.5,
        )])));
        scene.root().add_child(&sun(Some(SUN_SHADOW)));
        let pixels = render(gpu, scene);
        let shadowed = floor_pixel(&pixels, view_proj, -1.5, 0.0);
        assert!(shadowed[0] < 20, "shadowed {shadowed:?}");
    });
}

#[test]
fn raycasts_report_the_instance_they_hit() {
    let scene = Scene::new();
    let right = Vec3::new(1.5, 0.0, 0.0);
    let cubes = instanced_cubes(Vec3::ZERO, &[-right, right]);
    scene.root().add_child(&cubes);
    let hits = scene.raycast(crate::Ray::new(Vec3::new(1.5, 0.0, 5.0), Vec3::NEG_Z));
    assert_eq!(hits.len(), 2, "{hits:?}");
    assert!(
        hits.iter()
            .all(|hit| hit.node == cubes && hit.instance == Some(1))
    );
    assert!((hits[0].point.x - 1.5).abs() < 1e-4);
}

#[test]
fn visible_distance_hides_nodes_by_how_far_the_camera_is() {
    with_gpu(|gpu| {
        let scene = post_stage(1.0, PostProcessing::default());
        let group = Node::new();
        let mesh = scene.root().children()[0].clone();
        mesh.remove_from_parent();
        group.add_child(&mesh);
        scene.root().add_child(&group);
        let ray = crate::Ray::new(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z);

        // The default camera is 5 units from the origin.
        group.set_visible_distance(Some(0.0..4.0));
        assert_eq!(
            pixel(&render(gpu, scene.clone()), SIZE / 2, SIZE / 2),
            [0, 0, 255, 255]
        );
        assert!(scene.raycast(ray).is_empty());

        group.set_visible_distance(Some(4.0..6.0));
        assert_eq!(
            pixel(&render(gpu, scene.clone()), SIZE / 2, SIZE / 2),
            [255, 0, 0, 255]
        );
        assert!(!scene.raycast(ray).is_empty());
    });
}

#[test]
fn more_point_lights_than_a_uniform_holds_all_light() {
    with_gpu(|gpu| {
        let (scene, _, caster, view_proj) = shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::ZERO);
        caster.remove_from_parent();
        let spots: Vec<(f32, f32)> = (0..8)
            .flat_map(|i| (0..5).map(move |j| (i as f32 - 3.5, j as f32 - 2.0)))
            .collect();
        for &(x, z) in &spots {
            let light = Node::new();
            light.set_transform(Transform::from_translation(Vec3::new(x, 0.3, z)));
            light.set_light(Some(Light::Point {
                color: Color::rgb(255, 255, 255),
                intensity: 1.0,
                range: 0.4,
                shadow: None,
            }));
            scene.root().add_child(&light);
        }
        let pixels = render(gpu, scene);
        for &(x, z) in &spots {
            let lit = floor_pixel(&pixels, view_proj, x, z);
            assert!(lit[0] > 100, "{x} {z} {lit:?}");
        }
        let between = floor_pixel(&pixels, view_proj, 0.0, 0.5);
        assert!(between[0] < 20, "{between:?}");
    });
}

#[test]
fn lights_out_of_view_leave_the_shadowing_ones_their_maps() {
    with_gpu(|gpu| {
        let (scene, _, _, view_proj) = shadow_stage(Vec3::new(0.0, 6.0, 0.0), 1.2, Vec3::Y * 1.5);
        for x in [-100.0, 100.0] {
            let far = Node::new();
            far.set_transform(Transform::from_translation(Vec3::new(x, 3.0, 0.0)));
            far.set_light(Some(Light::Point {
                color: Color::rgb(255, 255, 255),
                intensity: 10.0,
                range: 5.0,
                shadow: Some(Shadow::default()),
            }));
            scene.root().add_child(&far);
        }
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

/// A plane facing up, seen from straight above, lit by a sun at `elevation` radians over the
/// horizon toward +X.
fn material_stage(material: StandardMaterial, elevation: f32) -> (Scene, Mat4) {
    let (scene, floor, caster, view_proj) = shadow_stage(Vec3::new(0.0, 3.0, 0.0), 1.2, Vec3::ZERO);
    caster.remove_from_parent();
    floor.set_mesh(Some((
        Geometry::plane(Vec2::splat(12.0)),
        Material::Standard(material),
    )));
    let light = Node::new();
    light.set_transform(Transform {
        rotation: crate::core::rotation_toward(
            Vec3::new(-elevation.cos(), -elevation.sin(), 0.0),
            Vec3::Z,
        ),
        ..Transform::IDENTITY
    });
    light.set_light(Some(Light::Directional {
        color: Color::rgb(255, 255, 255),
        intensity: 1.0,
        shadow: None,
    }));
    scene.root().add_child(&light);
    (scene, view_proj)
}

const BLACK: Color = Color::rgb(0, 0, 0);

#[test]
fn a_clearcoat_adds_a_sharp_highlight_over_a_rough_surface() {
    with_gpu(|gpu| {
        let center = |clearcoat| {
            let (scene, _) = material_stage(
                StandardMaterial {
                    base_color: BLACK,
                    roughness: 1.0,
                    clearcoat,
                    clearcoat_roughness: 0.1,
                    ..Default::default()
                },
                std::f32::consts::FRAC_PI_2,
            );
            pixel(&render(gpu, scene), SIZE / 2, SIZE / 2)[0]
        };
        let (plain, coated) = (center(0.0), center(1.0));
        assert!(coated > plain + 100, "{plain} {coated}");
    });
}

#[test]
fn a_sheen_glows_under_grazing_light() {
    with_gpu(|gpu| {
        let center = |sheen_color| {
            let (scene, _) = material_stage(
                StandardMaterial {
                    base_color: BLACK,
                    roughness: 1.0,
                    sheen_color,
                    sheen_roughness: 0.5,
                    ..Default::default()
                },
                0.3,
            );
            pixel(&render(gpu, scene), SIZE / 2, SIZE / 2)[0]
        };
        let (plain, sheen) = (center(BLACK), center(Color::rgb(255, 255, 255)));
        assert!(sheen > plain + 30, "{plain} {sheen}");
    });
}

/// How many pixels of the row and of the column through the center are lit.
fn highlight_extent(pixels: &[[u8; 4]]) -> (usize, usize) {
    let lit = |p: [u8; 4]| p[0] > 60;
    let row = (0..SIZE)
        .filter(|&x| lit(pixel(pixels, x, SIZE / 2)))
        .count();
    let column = (0..SIZE)
        .filter(|&y| lit(pixel(pixels, SIZE / 2, y)))
        .count();
    (row, column)
}

#[test]
fn anisotropy_stretches_highlights_along_one_direction() {
    with_gpu(|gpu| {
        let extent = |anisotropy| {
            let (scene, _) = material_stage(
                StandardMaterial {
                    base_color: BLACK,
                    roughness: 0.4,
                    anisotropy,
                    ..Default::default()
                },
                std::f32::consts::FRAC_PI_2,
            );
            highlight_extent(&render(gpu, scene))
        };
        let (row, column) = extent(0.0);
        assert!(row.abs_diff(column) <= 2, "{row} {column}");
        let (along, across) = extent(0.9);
        let (rotated_along, rotated_across) = extent(-0.9);
        assert!(along.abs_diff(across) > 8, "{along} {across}");
        assert!(
            (along > across) != (rotated_along > rotated_across),
            "{along} {across} {rotated_along} {rotated_across}"
        );
    });
}

/// A red cube behind a pane of `material` facing the default camera, over a blue background.
fn behind_glass(material: StandardMaterial) -> Scene {
    let scene = post_stage(1.0, PostProcessing::default());
    scene.root().children()[0].set_transform(Transform::from_translation(Vec3::NEG_Z * 2.0));
    let pane = Node::new();
    pane.set_transform(Transform {
        translation: Vec3::Z,
        rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        ..Transform::IDENTITY
    });
    pane.set_mesh(Some((
        Geometry::plane(Vec2::splat(6.0)),
        Material::Standard(material),
    )));
    scene.root().add_child(&pane);
    scene
}

#[test]
fn transmissive_surfaces_show_the_scene_behind_them() {
    with_gpu(|gpu| {
        let opaque = render(gpu, behind_glass(StandardMaterial::default()));
        assert_eq!(pixel(&opaque, SIZE / 2, SIZE / 2), [0, 0, 0, 255]);

        let glass = StandardMaterial {
            transmission: 1.0,
            roughness: 0.0,
            ior: 1.0,
            ..Default::default()
        };
        let pixels = render(gpu, behind_glass(glass));
        let through = pixel(&pixels, SIZE / 2, SIZE / 2);
        assert!(through[0] > 200 && through[2] < 40, "{through:?}");
        let background = pixel(&pixels, 2, 2);
        assert!(background[2] > 200 && background[0] < 40, "{background:?}");
    });
}

#[test]
fn transmission_tints_by_the_base_color() {
    with_gpu(|gpu| {
        let glass = StandardMaterial {
            base_color: Color::rgb(0, 255, 0),
            transmission: 1.0,
            roughness: 0.0,
            ior: 1.0,
            ..Default::default()
        };
        let pixels = render(gpu, behind_glass(glass));
        let background = pixel(&pixels, 2, 2);
        assert!(background[2] < 40, "{background:?}");
    });
}

fn custom_scene(material: CustomMaterial) -> Scene {
    let scene = Scene::new();
    let mesh = Node::new();
    mesh.set_mesh(Some((
        Geometry::cuboid(Vec3::ONE),
        Material::Custom(material),
    )));
    scene.root().add_child(&mesh);
    scene
}

#[test]
fn a_custom_material_computes_its_surface_in_wgsl() {
    with_gpu(|gpu| {
        let shader = MaterialShader::new(
            "fn surface(input: SurfaceInput) -> Surface {
                var s = standard_surface(input);
                s.base_color = vec4(0.0, 0.0, 0.0, 1.0);
                s.emissive = draw.custom[0].rgb + textureSample(texture0, material_sampler, input.uv).rgb;
                return s;
            }",
        )
        .unwrap();
        let mut material = CustomMaterial::new(shader.clone());
        material.params[0] = glam::Vec4::new(1.0, 0.0, 0.0, 0.0);
        material.textures = vec![Texture::from_rgba8(1, 1, vec![0, 0, 255, 255])];
        let pixels = render(gpu, custom_scene(material));
        assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [255, 0, 255, 255]);

        let mut other = CustomMaterial::new(shader);
        other.params[0] = glam::Vec4::new(0.0, 1.0, 0.0, 0.0);
        other.textures = vec![Texture::from_rgba8(1, 1, vec![0, 0, 0, 255])];
        let pixels = render(gpu, custom_scene(other));
        assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [0, 255, 0, 255]);
    });
}

#[test]
fn invalid_material_shaders_are_rejected_with_the_reason() {
    let missing = MaterialShader::new("fn not_surface() {}").unwrap_err();
    assert!(missing.to_string().contains("surface"), "{missing}");
    let typo = MaterialShader::new(
        "fn surface(input: SurfaceInput) -> Surface { return standard_surface(input) }",
    )
    .unwrap_err();
    assert!(!typo.to_string().is_empty());
}

#[test]
fn transmission_sees_through_under_post_processing() {
    with_gpu(|gpu| {
        let scene = behind_glass(StandardMaterial {
            transmission: 1.0,
            roughness: 0.0,
            ior: 1.0,
            ..Default::default()
        });
        scene.set_post_processing(PostProcessing {
            fxaa: true,
            ..Default::default()
        });
        let through = pixel(&render(gpu, scene), SIZE / 2, SIZE / 2);
        assert!(through[0] > 200 && through[2] < 40, "{through:?}");
    });
}
