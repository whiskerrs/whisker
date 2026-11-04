use glam::{Mat4, Vec3, Vec4};
use hecs::World;

use crate::Transform;
use crate::renderer::{DEPTH_FORMAT, HDR_FORMAT};

/// One live particle as the GPU sees it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ParticleInstance {
    pub(crate) center: [f32; 3],
    pub(crate) size: f32,
    pub(crate) color: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
struct Particle {
    position: Vec3,
    velocity: Vec3,
    age: f32,
}

/// Throws off glowing sprites that add light, as a component next to a [`Transform`].
///
/// Good for fire, sparks, and magic. Sprites do not shadow or sort, and add to what is behind them, so
/// they cannot show smoke that darkens. Simulation runs on the CPU, so keep thousands, not millions.
/// The default is a small campfire flame, which shows without a [`Sky`](crate::Sky).
///
/// ```
/// # use kiba::prelude::*;
/// let embers = ParticleEmitter {
///     rate: 20.0,
///     lifetime: 3.0,
///     ..ParticleEmitter::default()
/// };
/// ```
#[derive(Clone, Debug)]
pub struct ParticleEmitter {
    /// Sprites per second. Zero leaves only [`ParticleEmitter::burst`].
    pub rate: f32,
    /// Seconds each lives.
    pub lifetime: f32,
    /// Units per second at birth, along the entity's +Y, spread by `spread`.
    pub speed: f32,
    /// How far from +Y a sprite may start, from 0 for a line to 1 for a hemisphere.
    pub spread: f32,
    /// Acceleration on every sprite, in world space.
    pub gravity: Vec3,
    /// Half the width of a sprite at birth and at death.
    pub size: (f32, f32),
    /// Linear RGB light at birth and at death, which can exceed 1 for bloom. It fades to none at death.
    pub color: (Vec3, Vec3),
    /// The most alive at once, past which births are dropped.
    pub capacity: usize,
    /// The sprites alive now, which Kiba advances; leave it at its default.
    pub sprites: Sprites,
}

/// The live sprites of one [`ParticleEmitter`].
#[derive(Clone, Debug)]
pub struct Sprites {
    particles: Vec<Particle>,
    owed: f32,
    seed: u32,
}

impl Default for Sprites {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            owed: 0.0,
            seed: 0x9E37_79B9,
        }
    }
}

impl Default for ParticleEmitter {
    /// A small upward orange flame.
    fn default() -> Self {
        Self {
            rate: 60.0,
            lifetime: 1.0,
            speed: 1.5,
            spread: 0.25,
            gravity: Vec3::new(0.0, 0.5, 0.0),
            size: (0.12, 0.02),
            color: (Vec3::new(6.0, 2.5, 0.6), Vec3::new(1.5, 0.2, 0.05)),
            capacity: 2000,
            sprites: Sprites::default(),
        }
    }
}

impl ParticleEmitter {
    /// Releases `count` sprites at the next update.
    pub fn burst(&mut self, count: u32) {
        self.sprites.owed += count as f32;
    }

    /// How many are alive.
    pub fn alive(&self) -> usize {
        self.sprites.particles.len()
    }
}

impl Sprites {
    fn next_random(&mut self) -> f32 {
        // Xorshift, because effects need speed and variety rather than quality.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }
}

impl ParticleEmitter {
    fn advance(&mut self, transform: &Transform, delta_seconds: f32) {
        let (gravity, lifetime, spread, speed) =
            (self.gravity, self.lifetime, self.spread, self.speed);
        let sprites = &mut self.sprites;
        sprites.particles.retain_mut(|particle| {
            particle.age += delta_seconds;
            particle.velocity += gravity * delta_seconds;
            particle.position += particle.velocity * delta_seconds;
            particle.age < lifetime
        });
        sprites.owed += self.rate * delta_seconds;
        while sprites.owed >= 1.0 {
            sprites.owed -= 1.0;
            if sprites.particles.len() >= self.capacity {
                continue;
            }
            let angle = sprites.next_random() * std::f32::consts::TAU;
            let outward = sprites.next_random().sqrt() * spread;
            let local = Vec3::new(angle.cos() * outward, 1.0, angle.sin() * outward).normalize();
            sprites.particles.push(Particle {
                position: transform.translation,
                velocity: transform.rotation * local * speed,
                age: 0.0,
            });
        }
    }

    fn instances(&self) -> impl Iterator<Item = ParticleInstance> + '_ {
        self.sprites.particles.iter().map(|particle| {
            let life = (particle.age / self.lifetime).clamp(0.0, 1.0);
            let color = self.color.0.lerp(self.color.1, life);
            ParticleInstance {
                center: particle.position.to_array(),
                size: self.size.0 + (self.size.1 - self.size.0) * life,
                color: Vec4::new(color.x, color.y, color.z, 1.0 - life).to_array(),
            }
        })
    }
}

pub(crate) fn run_particles(world: &mut World, delta_seconds: f32) {
    for (_, (transform, emitter)) in world.query_mut::<(&Transform, &mut ParticleEmitter)>() {
        emitter.advance(transform, delta_seconds);
    }
}

pub(crate) fn extract_particles(world: &World) -> Vec<ParticleInstance> {
    world
        .query::<&ParticleEmitter>()
        .iter()
        .flat_map(|(_, emitter)| emitter.instances().collect::<Vec<_>>())
        .collect()
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ParticleView {
    view_projection: Mat4,
    right: Vec4,
    up: Vec4,
}

/// Draws every sprite of a frame.
pub(crate) struct ParticlePass {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bindings: wgpu::BindGroup,
    instances: Option<wgpu::Buffer>,
}

impl ParticlePass {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("particles.wgsl"));
        let additive = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kiba particles"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: size_of::<ParticleInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3,
                        1 => Float32,
                        2 => Float32x4,
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: additive,
                        alpha: additive,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kiba particles"),
            size: size_of::<ParticleView>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kiba particles"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            uniform,
            bindings,
            instances: None,
        }
    }

    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_projection: Mat4,
        (right, up): (Vec3, Vec3),
        particles: &[ParticleInstance],
    ) {
        if particles.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(particles);
        if self
            .instances
            .as_ref()
            .is_none_or(|buffer| buffer.size() < bytes.len() as u64)
        {
            self.instances = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("kiba particles"),
                size: (bytes.len() as u64).next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let instances = self.instances.as_ref().expect("allocated above");
        queue.write_buffer(instances, 0, bytes);
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&ParticleView {
                view_projection,
                right: right.extend(0.0),
                up: up.extend(0.0),
            }),
        );
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, count: usize) {
        let Some(instances) = self.instances.as_ref().filter(|_| count > 0) else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bindings, &[]);
        pass.set_vertex_buffer(0, instances.slice(..));
        pass.draw(0..6, 0..count as u32);
    }
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    #[test]
    fn sprites_are_born_at_the_rate_and_die_at_their_lifetime() {
        let mut emitter = ParticleEmitter {
            rate: 10.0,
            lifetime: 1.0,
            ..ParticleEmitter::default()
        };
        for _ in 0..50 {
            emitter.advance(&Transform::default(), 0.01);
        }
        assert!((4..=6).contains(&emitter.alive()), "{}", emitter.alive());
        for _ in 0..300 {
            emitter.advance(&Transform::default(), 0.01);
        }
        assert!((9..=11).contains(&emitter.alive()), "{}", emitter.alive());
        emitter.rate = 0.0;
        for _ in 0..120 {
            emitter.advance(&Transform::default(), 0.01);
        }
        assert_eq!(emitter.alive(), 0);
    }

    #[test]
    fn bursts_arrive_at_once_and_capacity_holds() {
        let mut emitter = ParticleEmitter {
            rate: 0.0,
            capacity: 5,
            ..ParticleEmitter::default()
        };
        emitter.burst(100);
        emitter.advance(&Transform::default(), 0.01);
        assert_eq!(emitter.alive(), 5);
    }

    #[test]
    fn sprites_leave_along_the_entity_and_fade_with_age() {
        let turned = Transform {
            rotation: Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2),
            ..Transform::default()
        };
        let mut emitter = ParticleEmitter {
            rate: 0.0,
            spread: 0.0,
            gravity: Vec3::ZERO,
            speed: 1.0,
            ..ParticleEmitter::default()
        };
        emitter.burst(1);
        emitter.advance(&turned, 0.0);
        emitter.advance(&turned, 0.5);
        let sprite = emitter.instances().next().expect("one alive");
        assert!(sprite.center[0] > 0.4, "{:?}", sprite.center);
        assert!((sprite.color[3] - 0.5).abs() < 0.01);
    }
}
