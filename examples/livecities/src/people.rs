//! Simulated people walking a city's real streets.

use whisker_map::LatLng;

const METERS_PER_DEGREE: f64 = 111_320.0;
/// Road ends closer than this, in meters, join, which stitches together lines cut at tile edges.
const SNAP: f64 = 2.0;
/// How far from the city's center, in meters, people walk.
pub const RADIUS: f64 = 900.0;
/// How long one wave of the avatar's clip lasts.
const WAVE_SECONDS: f64 = 2.6;

const NAMES: &[&str] = &[
    "Aiko", "Ben", "Chloe", "Diego", "Emi", "Finn", "Gina", "Hiro", "Ines", "Jae", "Kofi", "Lena",
    "Mateo", "Nora", "Omar", "Pia", "Quinn", "Rin", "Sami", "Tess", "Uma", "Vik", "Wren", "Yuki",
    "Zoe", "Arlo", "Bea", "Cyrus", "Dara", "Eli", "Fumi", "Gus", "Hana", "Ivo", "Juno", "Kai",
    "Lou", "Mika", "Nico", "Ola",
];

/// Meters east and north of `anchor`.
pub fn to_meters(anchor: LatLng, place: LatLng) -> [f64; 2] {
    let east = (place.lng - anchor.lng) * METERS_PER_DEGREE * anchor.lat.to_radians().cos();
    let north = (place.lat - anchor.lat) * METERS_PER_DEGREE;
    [east, north]
}

pub fn to_latlng(anchor: LatLng, meters: [f64; 2]) -> LatLng {
    LatLng::new(
        anchor.lat + meters[1] / METERS_PER_DEGREE,
        anchor.lng + meters[0] / (METERS_PER_DEGREE * anchor.lat.to_radians().cos()),
    )
}

/// The walkable streets as a graph, in meters around the city's center.
#[derive(Default)]
pub struct Roads {
    pub points: Vec<[f64; 2]>,
    /// Per point: the neighbouring points and the street's name.
    pub links: Vec<Vec<(usize, u32)>>,
    pub names: Vec<String>,
}

impl Roads {
    /// Joins the named street geometries, GeoJSON in longitude and latitude, near `center`.
    pub fn from_streets<'a>(
        center: LatLng,
        streets: impl IntoIterator<Item = (&'a str, &'a serde_json::Value)>,
    ) -> Roads {
        let mut roads = Roads::default();
        let mut index = std::collections::HashMap::new();
        let mut node = |roads: &mut Roads, p: [f64; 2]| -> usize {
            let key = ((p[0] / SNAP).round() as i64, (p[1] / SNAP).round() as i64);
            *index.entry(key).or_insert_with(|| {
                roads.points.push(p);
                roads.links.push(Vec::new());
                roads.points.len() - 1
            })
        };
        for (name, geometry) in streets {
            let name_index = match roads.names.iter().position(|n| n == name) {
                Some(i) => i as u32,
                None => {
                    roads.names.push(name.to_string());
                    roads.names.len() as u32 - 1
                }
            };
            for line in lines(geometry) {
                let mut previous: Option<usize> = None;
                for place in line {
                    let p = to_meters(center, place);
                    if p[0].hypot(p[1]) > RADIUS {
                        previous = None;
                        continue;
                    }
                    let current = node(&mut roads, p);
                    if let Some(previous) = previous
                        && previous != current
                    {
                        roads.link(previous, current, name_index);
                        roads.link(current, previous, name_index);
                    }
                    previous = Some(current);
                }
            }
        }
        roads
    }

    /// A street between `from` and `to`, named by whichever line gave it a name.
    fn link(&mut self, from: usize, to: usize, name: u32) {
        let named = !self.names[name as usize].is_empty();
        match self.links[from].iter_mut().find(|(t, _)| *t == to) {
            Some(link) if named => link.1 = name,
            Some(_) => {}
            None => self.links[from].push((to, name)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.links.iter().all(Vec::is_empty)
    }
}

fn lines(geometry: &serde_json::Value) -> Vec<Vec<LatLng>> {
    let line = |coords: &serde_json::Value| -> Vec<LatLng> {
        coords
            .as_array()
            .map(|points| {
                points
                    .iter()
                    .filter_map(|p| Some(LatLng::new(p.get(1)?.as_f64()?, p.get(0)?.as_f64()?)))
                    .collect()
            })
            .unwrap_or_default()
    };
    let coords = &geometry["coordinates"];
    match geometry["type"].as_str() {
        Some("LineString") => vec![line(coords)],
        Some("MultiLineString") => coords
            .as_array()
            .map(|lines| lines.iter().map(line).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mood {
    Walking,
    Waving(f64),
    Resting(f64),
}

#[derive(Clone, Debug)]
pub struct Walker {
    pub id: usize,
    pub name: &'static str,
    /// Which avatar model and color the walker uses.
    pub look: usize,
    from: usize,
    to: usize,
    /// Meters along the street from `from` to `to`.
    along: f64,
    speed: f64,
    pub walked: f64,
    pub mood: Mood,
}

/// A small deterministic random source, so every run starts the same way.
pub struct Dice(u64);

impl Dice {
    pub fn new(seed: u64) -> Dice {
        Dice(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    pub fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn below(&mut self, n: usize) -> usize {
        ((self.next() * n as f64) as usize).min(n.saturating_sub(1))
    }
}

/// Everyone walking one city.
pub struct Crowd {
    pub center: LatLng,
    pub roads: Roads,
    pub walkers: Vec<Walker>,
    dice: Dice,
}

impl Crowd {
    pub fn new(center: LatLng, roads: Roads, count: usize, looks: usize, seed: u64) -> Crowd {
        let mut dice = Dice::new(seed);
        let starts: Vec<usize> = (0..roads.points.len())
            .filter(|&i| !roads.links[i].is_empty())
            .collect();
        let mut walkers = Vec::new();
        if !starts.is_empty() {
            for id in 0..count {
                let from = starts[dice.below(starts.len())];
                let to = roads.links[from][dice.below(roads.links[from].len())].0;
                walkers.push(Walker {
                    id,
                    name: NAMES[dice.below(NAMES.len())],
                    look: id % looks.max(1),
                    from,
                    to,
                    along: 0.0,
                    speed: 3.5 + dice.next() * 2.0,
                    walked: dice.next() * 3000.0,
                    mood: Mood::Walking,
                });
            }
        }
        Crowd {
            center,
            roads,
            walkers,
            dice,
        }
    }

    /// Advances everyone by `dt` seconds.
    pub fn step(&mut self, dt: f64) {
        let Crowd {
            roads,
            walkers,
            dice,
            ..
        } = self;
        for walker in walkers.iter_mut() {
            match &mut walker.mood {
                Mood::Waving(left) | Mood::Resting(left) => {
                    *left -= dt;
                    if *left <= 0.0 {
                        walker.mood = Mood::Walking;
                    }
                    continue;
                }
                Mood::Walking => {}
            }
            let mut travel = walker.speed * dt;
            walker.walked += travel;
            loop {
                let length = distance(roads.points[walker.from], roads.points[walker.to]);
                if walker.along + travel < length {
                    walker.along += travel;
                    break;
                }
                travel -= length - walker.along;
                let next = &roads.links[walker.to];
                let choices: Vec<usize> = next
                    .iter()
                    .map(|(to, _)| *to)
                    .filter(|to| *to != walker.from)
                    .collect();
                let back = walker.from;
                walker.from = walker.to;
                walker.to = if choices.is_empty() {
                    back
                } else {
                    choices[dice.below(choices.len())]
                };
                walker.along = 0.0;
                let roll = dice.next();
                if roll < 0.03 {
                    walker.mood = Mood::Resting(1.5 + dice.next() * 2.5);
                    break;
                }
                if roll < 0.05 {
                    walker.mood = Mood::Waving(WAVE_SECONDS);
                    break;
                }
            }
        }
    }

    /// Meters east and north of the center, and the heading in radians clockwise from north.
    pub fn place(&self, walker: &Walker) -> ([f64; 2], f64) {
        let a = self.roads.points[walker.from];
        let b = self.roads.points[walker.to];
        let length = distance(a, b).max(1e-6);
        let k = (walker.along / length).clamp(0.0, 1.0);
        let p = [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k];
        (p, (b[0] - a[0]).atan2(b[1] - a[1]))
    }

    pub fn street(&self, walker: &Walker) -> &str {
        self.roads.links[walker.from]
            .iter()
            .find(|(to, _)| *to == walker.to)
            .map_or("", |(_, name)| self.roads.names[*name as usize].as_str())
    }

    pub fn wave(&mut self, id: usize) {
        if let Some(walker) = self.walkers.get_mut(id) {
            walker.mood = Mood::Waving(WAVE_SECONDS);
        }
    }

    /// Everyone as GeoJSON points.
    pub fn geojson(&self, colors: &[&str]) -> serde_json::Value {
        let features: Vec<serde_json::Value> = self
            .walkers
            .iter()
            .map(|w| {
                let place = to_latlng(self.center, self.place(w).0);
                serde_json::json!({
                    "type": "Feature",
                    "id": w.id,
                    "properties": {
                        "walker": w.id,
                        "color": colors[w.look % colors.len()],
                    },
                    "geometry": {"type": "Point", "coordinates": [place.lng, place.lat]},
                })
            })
            .collect();
        serde_json::json!({"type": "FeatureCollection", "features": features})
    }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

#[cfg(test)]
mod tests;
