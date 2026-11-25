//! The town's 3D content: the street of days, its landmarks and the countryside.

use std::f32::consts::{FRAC_PI_2, PI};

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_scene::math::{Mat4, Quat, Vec2, Vec3, Vec4};
use whisker_scene::{
    CustomMaterial, Geometry, Instances, Material, MaterialShader, Mesh, ModelView, SceneHit,
    StandardMaterial,
};

use crate::city::{self, Landmark, Part};
use crate::data::{Building, Year};
use crate::land;

/// Seconds between the first and last week rising.
pub const RISE_SPREAD: f32 = 2.4;
pub const RISE_TIME: f32 = 0.7;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Selection {
    Day(usize),
    Landmark(Landmark),
}

/// The clock the rising animation reads, advanced once per frame while it runs.
#[derive(Clone, Copy)]
pub struct Rise {
    pub now: RwSignal<f32>,
    pub born: RwSignal<f32>,
    pub running: RwSignal<bool>,
}

impl Rise {
    pub fn new() -> Rise {
        Rise {
            now: signal(0.0),
            born: signal(f32::NAN),
            running: signal(true),
        }
    }

    /// Replays the rise from the clock's next tick.
    pub fn restart(self) {
        self.born.set(f32::NAN);
        self.running.set(true);
    }

    pub fn tick(self, now: f32) {
        if self.born.get_untracked().is_nan() {
            self.born.set(now);
        }
        self.now.set(now);
        if self.running.get_untracked()
            && now - self.born.get_untracked() > RISE_SPREAD + RISE_TIME + 0.2
        {
            self.running.set(false);
        }
    }

    /// How far the building at `fraction` along the street has risen, overshooting before it settles.
    fn grown(self, fraction: f32) -> f32 {
        if !self.running.get() {
            return 1.0;
        }
        if self.born.get().is_nan() {
            return 0.0;
        }
        let t = ((self.now.get() - self.born.get() - fraction * RISE_SPREAD) / RISE_TIME)
            .clamp(0.0, 1.0);
        let (c, u) = (1.6, t - 1.0);
        1.0 + (c + 1.0) * u * u * u + c * u * u
    }
}

pub fn matte(color: Color, roughness: f32) -> Material {
    Material::Standard(StandardMaterial {
        base_color: color,
        roughness,
        ..Default::default()
    })
}

/// A rough material tinted `hex` (sRGB) and grained with the tiling detail texture every `1 / scale` units.
fn textured(hex: u32, scale: f32) -> Material {
    let linear = |shift: u32| {
        let c = ((hex >> shift) & 0xff) as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let shader = MaterialShader::new(GROUND_TINT).expect("tint shader");
    let mut material = CustomMaterial::new(shader);
    material.params[0] = Vec4::new(linear(16), linear(8), linear(0), scale);
    material.textures = vec![land::detail_map()];
    Material::Custom(material)
}

const GROUND_TINT: &str = "
fn surface(input: SurfaceInput) -> Surface {
    var s = standard_surface(input);
    let grain = textureSample(texture0, material_sampler, input.world.xz * draw.custom[0].w).r;
    let coarse = textureSample(texture0, material_sampler, input.world.xz * draw.custom[0].w * 0.13).r;
    s.base_color = vec4(draw.custom[0].rgb * (0.55 + 0.6 * grain) * (0.8 + 0.4 * coarse), 1.0);
    s.roughness = 0.97;
    s.metallic = 0.0;
    return s;
}
";

/// One kind of building along the street, drawn as instances of its model.
#[component]
fn street_kind(
    kind: Building,
    part: Part,
    year: RwSignal<Year>,
    rise: Rise,
    selected: RwSignal<Option<Selection>>,
) -> Element {
    let members = computed(move || {
        year.with(|year| {
            (0..year.days.len())
                .filter(|&i| year.days[i].building == kind)
                .collect::<Vec<_>>()
        })
    });
    let local = part.local;
    let instances = computed(move || {
        let members = members.get();
        if members.is_empty() {
            return None;
        }
        let picked = selected.get();
        year.with(|year| {
            let base = city::scale(kind) * city::LOT;
            let transforms = members
                .iter()
                .map(|&i| {
                    let (position, rotation) = city::placement(year, i);
                    let fraction = year.cell(i).0 as f32 / year.weeks() as f32;
                    let grown = rise.grown(fraction).max(0.001);
                    let lift = if picked == Some(Selection::Day(i)) {
                        1.15
                    } else {
                        1.0
                    };
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(base, base * grown * lift, base),
                        rotation,
                        position,
                    ) * local
                })
                .collect();
            Some(Instances::new(transforms))
        })
    });
    render! {
        Mesh(
            geometry: part.geometry.clone(),
            material: part.material.clone(),
            instances: instances,
            visible: computed(move || !members.get().is_empty()),
            on_tap: move |hit: SceneHit| {
                if let Some(i) = hit.instance.and_then(|n| members.get_untracked().get(n as usize).copied()) {
                    selected.set(Some(Selection::Day(i)));
                }
            },
        )
    }
}

#[component]
pub fn street(year: RwSignal<Year>, rise: Rise, selected: RwSignal<Option<Selection>>) -> Element {
    let parts = city::street_parts();
    let kinds: Vec<(Building, Part)> = parts.into_iter().collect();
    let kinds = StoredValue::new(kinds);
    let length = computed(move || city::length(year.with(Year::weeks)));
    let street = computed(move || Geometry::plane(Vec2::new(length.get() + 6.0, city::STREET)));
    let rows = computed(move || {
        Geometry::plane(Vec2::new(
            length.get() + 2.0,
            city::STREET + 7.0 * city::LOT + 0.6,
        ))
    });
    let gates = computed(move || {
        year.with(|year| {
            let weeks = year.weeks();
            let mut transforms = Vec::new();
            for (i, day) in year.days.iter().enumerate() {
                if day.date.day == 1 && i > 0 {
                    let x = city::week_x(year.cell(i).0, weeks) - city::LOT / 2.0;
                    transforms.push(Mat4::from_scale_rotation_translation(
                        Vec3::splat(city::STREET * 1.25),
                        city::gate_rotation(),
                        Vec3::new(x, 0.0, city::STREET_Z),
                    ));
                }
            }
            transforms
        })
    });
    let gate = city::scenery().gate.clone();
    let gate_local = gate.local;
    render! {
        Fragment {
            Mesh(
                geometry: rows,
                material: textured(0x857660, 0.9),
                position: Vec3::new(0.0, -0.005, city::STREET_Z + 0.5 * city::LOT - 0.3 + 0.15),
                receive_shadow: true,
                cast_shadow: false,
            )
            Mesh(
                geometry: street,
                material: textured(0x9a8a70, 0.7),
                position: Vec3::new(0.0, 0.002, city::STREET_Z),
                cast_shadow: false,
            )
            Mesh(
                geometry: gate.geometry.clone(),
                material: gate.material.clone(),
                instances: computed(move || Some(Instances::new(gates.get().into_iter().map(|t| t * gate_local).collect()))),
            )
            ForEach(
                each: move || (0..kinds.with(Vec::len)).collect::<Vec<_>>(),
                key: |i: &usize| *i,
                children: move |i: ReadSignal<usize>| {
                    let (kind, part) = kinds.with(|k| k[i.get_untracked()].clone());
                    render! { StreetKind(kind: kind, part: part, year: year, rise: rise, selected: selected) }
                },
            )
        }
    }
}

fn wall(from: Vec2, to: Vec2) -> (Vec3, Quat, Vec3) {
    let mid = (from + to) / 2.0;
    let along = to - from;
    (
        Vec3::new(mid.x, 0.35, mid.y),
        Quat::from_rotation_y(-along.y.atan2(along.x)),
        Vec3::new(along.length(), 0.7, 0.18),
    )
}

/// A plastered wall with a tiled cap around a `size` square centred on the origin, open toward +z.
fn enclosure(size: f32) -> Vec<(Vec3, Quat, Vec3)> {
    let h = size / 2.0;
    let gap = 1.6;
    vec![
        wall(Vec2::new(-h, -h), Vec2::new(h, -h)),
        wall(Vec2::new(-h, -h), Vec2::new(-h, h)),
        wall(Vec2::new(h, -h), Vec2::new(h, h)),
        wall(Vec2::new(-h, h), Vec2::new(-gap / 2.0, h)),
        wall(Vec2::new(gap / 2.0, h), Vec2::new(h, h)),
    ]
}

/// Stone walls sloping in from a `bottom` square to a `top` square, `height` tall.
fn stone_base(bottom: f32, top: f32, height: f32) -> Geometry {
    let (b, t) = (bottom / 2.0, top / 2.0);
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let (mut positions, mut normals, mut uvs, mut indices) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for side in 0..4 {
        let (a, c) = (corners[side], corners[(side + 1) % 4]);
        let quad = [
            Vec3::new(a.0 * b, 0.0, a.1 * b),
            Vec3::new(c.0 * b, 0.0, c.1 * b),
            Vec3::new(c.0 * t, height, c.1 * t),
            Vec3::new(a.0 * t, height, a.1 * t),
        ];
        let normal = (quad[1] - quad[0]).cross(quad[3] - quad[0]).normalize() * -1.0;
        let first = positions.len() as u32;
        for (i, p) in quad.iter().enumerate() {
            positions.push(*p);
            normals.push(normal);
            uvs.push(Vec2::new(
                [0.0, 1.0, 1.0, 0.0][i] * bottom,
                [0.0, 0.0, 1.0, 1.0][i] * height,
            ));
        }
        indices.extend([first, first + 2, first + 1, first, first + 3, first + 2]);
    }
    let first = positions.len() as u32;
    for (x, z) in corners {
        positions.push(Vec3::new(x * t, height, z * t));
        normals.push(Vec3::Y);
        uvs.push(Vec2::new(x, z));
    }
    indices.extend([first, first + 2, first + 1, first, first + 3, first + 2]);
    Geometry::from_vertices(positions, normals, Some(uvs), indices)
}

#[component]
fn landmark_view(landmark: Landmark, at: Vec3, selected: RwSignal<Option<Selection>>) -> Element {
    let scenery = city::scenery();
    let pick = move |_: SceneHit| selected.set(Some(Selection::Landmark(landmark)));
    let plaster = matte(Color::hex(0xe9e2d0), 0.9);
    let tile = matte(Color::hex(0x3d4146), 0.6);
    let gravel = textured(0x9f998a, 1.4);
    let walls = |size: f32| {
        enclosure(size)
            .into_iter()
            .flat_map(|(position, rotation, scale)| {
                let cap = (
                    position + Vec3::Y * 0.4,
                    rotation,
                    Vec3::new(scale.x + 0.1, 0.1, 0.34),
                );
                [((position, rotation, scale), false), (cap, true)]
            })
            .collect::<Vec<_>>()
    };
    match landmark {
        Landmark::Castle => {
            let water = land::water_material([0.07, 0.17, 0.2], 0.06, 0.08);
            let moat: Vec<Mat4> = [
                (Vec3::new(0.0, 0.0, -6.0), Vec3::new(13.6, 1.0, 1.6)),
                (Vec3::new(0.0, 0.0, 6.0), Vec3::new(13.6, 1.0, 1.6)),
                (Vec3::new(-6.0, 0.0, 0.0), Vec3::new(1.6, 1.0, 10.4)),
                (Vec3::new(6.0, 0.0, 0.0), Vec3::new(1.6, 1.0, 10.4)),
            ]
            .into_iter()
            .map(|(p, s)| {
                Mat4::from_scale_rotation_translation(s, Quat::IDENTITY, at + p + Vec3::Y * -0.12)
            })
            .collect();
            render! {
                Fragment {
                    Mesh(geometry: stone_base(9.0, 7.4, 1.6), material: textured(0x6a665e, 1.1), position: at, on_tap: pick)
                    Mesh(geometry: Geometry::plane(Vec2::ONE), material: water, instances: Some(Instances::new(moat)), cast_shadow: false)
                    ModelView(model: scenery.castle.clone(), position: at + Vec3::Y * 1.6, scale: Vec3::splat(6.2), on_tap: pick)
                }
            }
        }
        Landmark::Temple => {
            let pieces = std::rc::Rc::new(walls(9.0));
            let count = pieces.len();
            render! {
                Fragment {
                    Mesh(geometry: Geometry::plane(Vec2::splat(9.0)), material: gravel, position: at + Vec3::Y * 0.004, cast_shadow: false)
                    ModelView(model: scenery.temple.clone(), position: at + Vec3::new(0.4, 0.0, -1.2), scale: Vec3::splat(4.6), on_tap: pick)
                    ModelView(model: scenery.pagoda.clone(), position: at + Vec3::new(-3.0, 0.0, -2.4), scale: Vec3::splat(2.2), on_tap: pick)
                    ModelView(model: scenery.bell.clone(), position: at + Vec3::new(3.0, 0.0, 2.6), rotation: Quat::from_rotation_y(-FRAC_PI_2), scale: Vec3::splat(1.5), on_tap: pick)
                    ForEach(
                        each: move || (0..count).collect::<Vec<_>>(),
                        key: |i: &usize| *i,
                        children: move |i: ReadSignal<usize>| {
                            let ((position, rotation, scale), cap) = pieces[i.get_untracked()];
                            render! {
                                Mesh(
                                    geometry: Geometry::cuboid(Vec3::ONE),
                                    material: if cap { tile.clone() } else { plaster.clone() },
                                    position: at + position,
                                    rotation: rotation,
                                    scale: scale,
                                )
                            }
                        },
                    )
                }
            }
        }
        Landmark::Shrine => {
            render! {
                Fragment {
                    Mesh(geometry: Geometry::plane(Vec2::new(1.4, 6.5)), material: gravel, position: at + Vec3::new(0.0, 0.004, 1.8), cast_shadow: false)
                    ModelView(model: scenery.shrine.clone(), position: at + Vec3::new(0.0, 0.0, -1.0), scale: Vec3::splat(3.0), on_tap: pick)
                    ModelView(model: scenery.torii.clone(), position: at + Vec3::new(0.0, 0.0, 4.6), scale: Vec3::splat(2.6), on_tap: pick)
                    ModelView(model: scenery.lantern.clone(), position: at + Vec3::new(-1.1, 0.0, 1.6), scale: Vec3::splat(0.5))
                    ModelView(model: scenery.lantern.clone(), position: at + Vec3::new(1.1, 0.0, 1.6), rotation: Quat::from_rotation_y(PI), scale: Vec3::splat(0.5))
                }
            }
        }
    }
}

#[component]
pub fn landmarks(year: RwSignal<Year>, selected: RwSignal<Option<Selection>>) -> Element {
    let placed = computed(move || year.with(city::landmarks));
    render! {
        ForEach(
            each: move || placed.get(),
            key: |(landmark, at): &(Landmark, Vec3)| (*landmark as u8, (at.x * 10.0) as i32),
            children: move |entry: ReadSignal<(Landmark, Vec3)>| {
                let (landmark, at) = entry.get_untracked();
                render! { LandmarkView(landmark: landmark, at: at, selected: selected) }
            },
        )
    }
}

fn instanced(part: &Part, transforms: Vec<Mat4>) -> Option<Instances> {
    Some(Instances::new(
        transforms.into_iter().map(|t| t * part.local).collect(),
    ))
}

#[component]
pub fn countryside() -> Element {
    let scenery = city::scenery();
    let ([young, ripe], bunds) = land::paddies();
    let (snow, rock) = land::peak();
    let (river, river_at) = land::river();
    let (near_cedars, far_cedars) = land::cedars();
    let cedars = instanced(&scenery.cedar, near_cedars);
    let willows = instanced(&scenery.willow, land::willows());
    let bridges = instanced(&scenery.bridge, land::bridges());
    render! {
        Fragment {
            Mesh(geometry: land::terrain(), material: land::ground_material(), cast_shadow: false)
            Mesh(geometry: river, material: land::water_material([0.09, 0.2, 0.24], 0.08, 0.06), position: river_at, cast_shadow: false)
            Mesh(geometry: Geometry::plane(Vec2::ONE), material: textured(0x6f8f3c, 2.2), instances: Some(Instances::new(young)), cast_shadow: false)
            Mesh(geometry: Geometry::plane(Vec2::ONE), material: textured(0xa59a4a, 2.2), instances: Some(Instances::new(ripe)), cast_shadow: false)
            Mesh(geometry: Geometry::cuboid(Vec3::ONE), material: textured(0x7a6a4c, 1.0), instances: Some(Instances::new(bunds)), cast_shadow: false)
            Mesh(geometry: scenery.cedar.geometry.clone(), material: scenery.cedar.material.clone(), instances: cedars)
            Mesh(geometry: land::distant_cedar(), material: matte(Color::hex(0x2c4a2e), 0.9), instances: Some(Instances::new(far_cedars)), cast_shadow: false)
            Mesh(geometry: scenery.willow.geometry.clone(), material: scenery.willow.material.clone(), instances: willows)
            Mesh(geometry: scenery.bridge.geometry.clone(), material: scenery.bridge.material.clone(), instances: bridges)
            Mesh(geometry: rock, material: matte(Color::hex(0x6b7486), 0.95), cast_shadow: false, receive_shadow: false)
            Mesh(geometry: snow, material: matte(Color::hex(0xf2f4f7), 0.8), cast_shadow: false, receive_shadow: false)
        }
    }
}
