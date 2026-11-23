use std::time::Duration;

use glam::{Quat, Vec2, Vec3};
use whisker::css::Color;
use whisker_surface::{FrameInfo, SurfaceSize};
use whisker_wgpu::{Gpu, Renderer, wgpu};

use super::*;
use crate::core::{
    Geometry, Light, Material, Node, Projection, Scene, StandardMaterial, Transform,
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
        collected.draws[0].0.transform_point3(Vec3::ZERO),
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
    }));
    root.add_child(&light);
    let collected = collect(&root);
    let (toward, radiance) = collected.directional[0];
    assert!(toward.abs_diff_eq(Vec3::Y, 1e-5), "{toward}");
    assert_eq!(radiance, [2.0, 2.0, 2.0, 0.0]);
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
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
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
