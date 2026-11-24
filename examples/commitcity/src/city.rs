//! Where each day and landmark stands in the town, and the models they are drawn with.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};
use std::rc::Rc;

use whisker_scene::math::{Mat4, Quat, Vec3};
use whisker_scene::{Geometry, Material, Model, Node};

use crate::data::{Building, Year};

/// Width of one lot.
pub const LOT: f32 = 1.3;
/// Width of the main street, which runs between Tuesday and Wednesday.
pub const STREET: f32 = 1.9;
pub const STREET_Z: f32 = -0.5 * LOT;
/// The back edge of the north row, where the landmark quarter begins.
pub const NORTH_EDGE: f32 = STREET_Z - STREET / 2.0 - 3.0 * LOT;

pub fn length(weeks: usize) -> f32 {
    weeks as f32 * LOT
}

pub fn week_x(week: usize, weeks: usize) -> f32 {
    (week as f32 - (weeks as f32 - 1.0) / 2.0) * LOT
}

/// Lots face the main street, so the row a weekday sits in decides its side.
pub fn placement(year: &Year, index: usize) -> (Vec3, Quat) {
    let (week, weekday) = year.cell(index);
    let north = weekday < 3;
    let edge = STREET / 2.0 + LOT / 2.0;
    let z = if north {
        STREET_Z - edge - (2 - weekday) as f32 * LOT
    } else {
        STREET_Z + edge + (weekday - 3) as f32 * LOT
    };
    let rotation = if north {
        Quat::IDENTITY
    } else {
        Quat::from_rotation_y(PI)
    };
    let jitter = (crate::data::variant(year.days[index].date) % 7) as f32 * 0.012;
    (
        Vec3::new(week_x(week, year.weeks()), 0.0, z),
        rotation * Quat::from_rotation_y(jitter - 0.036),
    )
}

pub fn scale(building: Building) -> f32 {
    match building {
        Building::Sakura | Building::Pine => 1.15,
        Building::Lantern => 0.32,
        Building::Inn | Building::MerchantHouse => 1.0,
        _ => 0.92,
    }
}

/// A ward gate across the street where each month begins.
pub fn gate_rotation() -> Quat {
    Quat::from_rotation_y(FRAC_PI_2)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Landmark {
    /// The busiest day's castle keep.
    Castle,
    /// The busiest week's temple.
    Temple,
    /// The longest streak's shrine.
    Shrine,
}

/// The landmarks a year earns, each placed behind the north row near the days it stands for.
pub fn landmarks(year: &Year) -> Vec<(Landmark, Vec3)> {
    let weeks = year.weeks();
    let x_of = |index: usize| week_x(year.cell(index).0, weeks);
    let mut wanted: Vec<(Landmark, f32, f32)> = Vec::new();
    if let Some(day) = year.busiest_day {
        wanted.push((Landmark::Castle, x_of(day), 9.0));
    }
    if let Some(week) = year.busiest_week {
        wanted.push((Landmark::Temple, x_of(week.start), 8.0));
    }
    if let Some(streak) = year.longest_streak {
        wanted.push((Landmark::Shrine, x_of(streak.start + streak.days / 2), 6.0));
    }
    let half = length(weeks) / 2.0;
    wanted.sort_by(|a, b| a.1.total_cmp(&b.1));
    for i in 1..wanted.len() {
        let gap = (wanted[i - 1].2 + wanted[i].2) / 2.0 + 2.0;
        wanted[i].1 = wanted[i].1.max(wanted[i - 1].1 + gap);
    }
    if let Some(last) = wanted.last()
        && last.1 + last.2 / 2.0 > half
    {
        let shift = last.1 + last.2 / 2.0 - half;
        for entry in &mut wanted {
            entry.1 -= shift;
        }
    }
    wanted
        .into_iter()
        .map(|(landmark, x, size)| {
            let depth = match landmark {
                Landmark::Castle => 4.5,
                _ => 2.0,
            };
            (landmark, Vec3::new(x, 0.0, NORTH_EDGE - depth - size / 2.0))
        })
        .collect()
}

/// A model's first mesh and the transform that places it in the model.
#[derive(Clone)]
pub struct Part {
    pub geometry: Geometry,
    pub material: Material,
    pub local: Mat4,
}

fn first_part(node: &Node, parent: Mat4) -> Option<Part> {
    let world = parent * node.transform().to_matrix();
    if let Some((geometry, material)) = node.mesh() {
        return Some(Part {
            geometry,
            material,
            local: world,
        });
    }
    node.children()
        .iter()
        .find_map(|child| first_part(child, world))
}

fn part(bytes: &[u8]) -> Part {
    let model = Model::from_slice(bytes).expect("bundled model");
    first_part(&model.instantiate(), Mat4::IDENTITY).expect("model has a mesh")
}

pub fn street_parts() -> HashMap<Building, Part> {
    HashMap::from([
        (
            Building::Sakura,
            part(include_bytes!("../assets/models/sakura.glb")),
        ),
        (
            Building::Pine,
            part(include_bytes!("../assets/models/matsu.glb")),
        ),
        (
            Building::Lantern,
            part(include_bytes!("../assets/models/ishidoro.glb")),
        ),
        (
            Building::Machiya,
            part(include_bytes!("../assets/models/machiya.glb")),
        ),
        (
            Building::Farmhouse,
            part(include_bytes!("../assets/models/kominka.glb")),
        ),
        (
            Building::Teahouse,
            part(include_bytes!("../assets/models/chaya.glb")),
        ),
        (
            Building::MerchantHouse,
            part(include_bytes!("../assets/models/shoka.glb")),
        ),
        (
            Building::Storehouse,
            part(include_bytes!("../assets/models/kura.glb")),
        ),
        (
            Building::Inn,
            part(include_bytes!("../assets/models/ryokan.glb")),
        ),
    ])
}

pub struct Scenery {
    pub gate: Part,
    pub cedar: Part,
    pub willow: Part,
    pub bridge: Part,
    pub castle: Model,
    pub temple: Model,
    pub pagoda: Model,
    pub bell: Model,
    pub shrine: Model,
    pub torii: Model,
    pub lantern: Model,
}

/// The scenery models, parsed once per thread.
pub fn scenery() -> Rc<Scenery> {
    thread_local! {
        static SCENERY: Rc<Scenery> = Rc::new(load_scenery());
    }
    SCENERY.with(Rc::clone)
}

fn load_scenery() -> Scenery {
    let model = |bytes: &[u8]| Model::from_slice(bytes).expect("bundled model");
    Scenery {
        gate: part(include_bytes!("../assets/models/kido.glb")),
        cedar: part(include_bytes!("../assets/models/sugi.glb")),
        willow: part(include_bytes!("../assets/models/yanagi.glb")),
        bridge: part(include_bytes!("../assets/models/hashi.glb")),
        castle: model(include_bytes!("../assets/models/tenshu.glb")),
        temple: model(include_bytes!("../assets/models/hondo.glb")),
        pagoda: model(include_bytes!("../assets/models/goju.glb")),
        bell: model(include_bytes!("../assets/models/shoro.glb")),
        shrine: model(include_bytes!("../assets/models/jinja.glb")),
        torii: model(include_bytes!("../assets/models/torii.glb")),
        lantern: model(include_bytes!("../assets/models/ishidoro.glb")),
    }
}
