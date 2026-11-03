use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use glam::{IVec2, Vec3};
use hecs::{Entity, World};

use crate::model::{read_file, report_failure};
use crate::pack::{MODEL_SECTION, PROBE_SECTION, Reader, WORLD_SECTION, read_model, read_section};
use crate::probes::{ProbeData, ProbeVolume};
use crate::{Camera, Model, Transform};

const MOST_LOADS_AT_ONCE: usize = 4;

/// How far a [`WorldStream`] reaches and how much it may hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreamSettings {
    /// Cells nearer the camera than this, center to camera, show their full detail.
    pub full_distance: f32,
    /// Cells nearer than this show at least a simplified stand-in. Farther ones show nothing.
    pub far_distance: f32,
    /// How many bytes of cooked cells to keep. The nearest cells come first when this runs short.
    pub budget_bytes: u64,
}

impl Default for StreamSettings {
    /// Half a kilometer of detail and two of stand-ins, within the lowest tier's 300 MB of textures.
    fn default() -> Self {
        Self {
            full_distance: 512.0,
            far_distance: 2048.0,
            budget_bytes: 300 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Detail {
    Full,
    Far,
}

#[derive(Debug)]
struct CellData {
    model: Model,
    probes: Option<ProbeVolume>,
    bytes: u64,
}

type Pending<T> = Arc<OnceLock<Result<T, String>>>;

#[derive(Debug)]
struct Cell {
    data: Pending<CellData>,
    entities: Vec<Entity>,
    last_wanted: u64,
}

#[derive(Debug)]
struct Manifest {
    cell_size: f32,
    cells: HashSet<IVec2>,
    base: String,
}

/// Loads a cooked world around the camera, spawned alone as `(WorldStream::load(path, settings),)`.
///
/// Cook the world with `kiba-cook world`, and keep its directory together. Point at its
/// `index.kiba`. Cells appear as entities with a [`Model`], and go again once they are out of reach.
/// Only one stream is used if the world has several.
#[derive(Debug)]
pub struct WorldStream {
    settings: StreamSettings,
    manifest: Pending<Manifest>,
    cells: HashMap<(IVec2, Detail), Cell>,
    tick: u64,
}

fn parse_manifest(bytes: &[u8], location: &str) -> Result<Manifest, String> {
    let mut input = Reader::new(read_section(bytes, WORLD_SECTION)?);
    let cell_size = input.f32()?;
    let positions: Vec<[i32; 2]> = input.slice()?;
    if cell_size <= 0.0 {
        return Err("the world's cell size must be greater than zero".into());
    }
    let base = location.rfind('/').map_or("", |at| &location[..=at]);
    Ok(Manifest {
        cell_size,
        cells: positions.into_iter().map(IVec2::from).collect(),
        base: base.to_owned(),
    })
}

fn parse_cell(bytes: &[u8]) -> Result<CellData, String> {
    let data = read_model(read_section(bytes, MODEL_SECTION)?)?;
    let probes = read_section(bytes, PROBE_SECTION)
        .ok()
        .map(ProbeData::from_section)
        .transpose()?
        .map(ProbeVolume::from_data);
    Ok(CellData {
        model: Model::from_data(data),
        probes,
        bytes: bytes.len() as u64,
    })
}

impl WorldStream {
    /// Starts loading the world whose `index.kiba` is at `location`, and returns at once.
    ///
    /// `location` is a URL on the Web and a path elsewhere, as `asset!` gives.
    pub fn load(location: impl Into<String>, settings: StreamSettings) -> Self {
        let location = location.into();
        let manifest = Arc::new(OnceLock::new());
        let finish = {
            let manifest = manifest.clone();
            let location = location.clone();
            move |result: Result<Vec<u8>, String>| {
                let parsed = result.and_then(|bytes| parse_manifest(&bytes, &location));
                report_failure(parsed.as_ref().err());
                let _ = manifest.set(parsed);
            }
        };
        read_file(location, finish);
        Self {
            settings,
            manifest,
            cells: HashMap::new(),
            tick: 0,
        }
    }

    /// How many bytes of cooked cells are in memory now.
    pub fn loaded_bytes(&self) -> u64 {
        self.cells.values().map(Cell::bytes).sum()
    }

    /// Why the world could not start, if it could not. Failed cells only log and stay empty.
    pub fn failure(&self) -> Option<&str> {
        match self.manifest.get() {
            Some(Err(reason)) => Some(reason),
            _ => None,
        }
    }

    fn start_load(manifest: &Manifest, cell: IVec2, detail: Detail) -> Cell {
        let suffix = match detail {
            Detail::Full => "",
            Detail::Far => ".far",
        };
        let location = format!("{}cell_{}_{}{suffix}.kiba", manifest.base, cell.x, cell.y);
        let data = Arc::new(OnceLock::new());
        let finish = {
            let data = data.clone();
            move |result: Result<Vec<u8>, String>| {
                let parsed = result.and_then(|bytes| parse_cell(&bytes));
                report_failure(parsed.as_ref().err());
                let _ = data.set(parsed);
            }
        };
        read_file(location, finish);
        Cell {
            data,
            entities: Vec::new(),
            last_wanted: 0,
        }
    }

    /// Decides what to hold this frame, returning entities to despawn and cells to show.
    fn plan(&mut self, camera: Vec3) -> (Vec<Entity>, Vec<(IVec2, Detail)>) {
        self.tick += 1;
        let Some(Ok(manifest)) = self.manifest.get() else {
            return Default::default();
        };
        let settings = self.settings;
        let size = manifest.cell_size;
        let mut wanted: Vec<(f32, IVec2, Detail)> = manifest
            .cells
            .iter()
            .filter_map(|&cell| {
                let center = (cell.as_vec2() + 0.5) * size;
                let distance = center.distance(glam::Vec2::new(camera.x, camera.z));
                let detail = if distance < settings.full_distance {
                    Detail::Full
                } else if distance < settings.far_distance {
                    Detail::Far
                } else {
                    return None;
                };
                Some((distance, cell, detail))
            })
            .collect();
        wanted.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));

        let mut loading = self
            .cells
            .values()
            .filter(|c| c.data.get().is_none())
            .count();
        let mut spent = 0;
        let mut within_budget = HashSet::new();
        for &(_, cell, detail) in &wanted {
            let bytes = self.cells.get(&(cell, detail)).map_or(0, Cell::bytes);
            if spent + bytes > settings.budget_bytes {
                break;
            }
            spent += bytes;
            within_budget.insert((cell, detail));
            if !self.cells.contains_key(&(cell, detail)) && loading < MOST_LOADS_AT_ONCE {
                self.cells
                    .insert((cell, detail), Self::start_load(manifest, cell, detail));
                loading += 1;
            }
        }

        let ready = |cells: &HashMap<(IVec2, Detail), Cell>, key| {
            cells
                .get(&key)
                .is_some_and(|c: &Cell| matches!(c.data.get(), Some(Ok(_))))
        };
        let mut shown = HashSet::new();
        for &(_, cell, detail) in &wanted {
            if !within_budget.contains(&(cell, detail)) {
                continue;
            }
            if ready(&self.cells, (cell, detail)) {
                shown.insert((cell, detail));
            } else if ready(&self.cells, (cell, Detail::Far)) {
                // A stand-in fills the gap until the detail arrives, even if it is out of budget.
                shown.insert((cell, Detail::Far));
            } else if ready(&self.cells, (cell, Detail::Full)) {
                shown.insert((cell, Detail::Full));
            }
        }

        let tick = self.tick;
        let mut despawn = Vec::new();
        for (key, cell) in &mut self.cells {
            if within_budget.contains(key) || shown.contains(key) {
                cell.last_wanted = tick;
            }
            if !shown.contains(key) {
                despawn.append(&mut cell.entities);
            }
        }
        // Cells stay cached past their reach, so turning back does not fetch them again.
        let mut cached = self.loaded_bytes();
        if cached > settings.budget_bytes {
            let mut idle: Vec<_> = self
                .cells
                .iter()
                .filter(|(_, cell)| cell.last_wanted != tick && cell.data.get().is_some())
                .map(|(key, cell)| (cell.last_wanted, *key))
                .collect();
            idle.sort_unstable_by_key(|(last_wanted, _)| *last_wanted);
            for (_, key) in idle {
                if cached <= settings.budget_bytes {
                    break;
                }
                let bytes = self.cells.remove(&key).map_or(0, |cell| cell.bytes());
                cached -= bytes;
            }
        }
        let show = shown
            .into_iter()
            .filter(|key| self.cells.get(key).is_some_and(|c| c.entities.is_empty()))
            .collect();
        (despawn, show)
    }
}

impl Cell {
    fn bytes(&self) -> u64 {
        match self.data.get() {
            Some(Ok(data)) => data.bytes,
            _ => 0,
        }
    }
}

pub(crate) fn run_streams(world: &mut World) {
    let camera = world
        .query_mut::<(&Transform, &Camera)>()
        .into_iter()
        .next()
        .map(|(_, (transform, _))| transform.translation);
    let Some(camera) = camera else {
        return;
    };
    let Some((id, (despawn, show))) = world
        .query_mut::<&mut WorldStream>()
        .into_iter()
        .next()
        .map(|(id, stream)| (id, stream.plan(camera)))
    else {
        return;
    };
    for entity in despawn {
        let _ = world.despawn(entity);
    }
    for key in show {
        let parts = {
            let stream = world.get::<&WorldStream>(id).expect("found above");
            let Some(Ok(data)) = stream.cells[&key].data.get() else {
                continue;
            };
            (data.model.clone(), data.probes.clone())
        };
        let mut entities = vec![world.spawn((Transform::default(), parts.0))];
        if let Some(probes) = parts.1 {
            entities.push(world.spawn((probes,)));
        }
        let mut stream = world.get::<&mut WorldStream>(id).expect("found above");
        if let Some(cell) = stream.cells.get_mut(&key) {
            cell.entities = entities;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelData;
    use crate::pack::{Writer, write_model, write_pack};

    fn ready_cell(bytes: u64) -> Cell {
        Cell {
            data: Arc::new(OnceLock::from(Ok(CellData {
                model: Model::from_data(ModelData {
                    parts: Vec::new(),
                    rig: None,
                }),
                probes: None,
                bytes,
            }))),
            entities: Vec::new(),
            last_wanted: 0,
        }
    }

    /// A stream with an in-memory manifest of cells along X, which loads cells instantly.
    fn stream(settings: StreamSettings) -> WorldStream {
        WorldStream {
            settings,
            manifest: Arc::new(OnceLock::from(Ok(Manifest {
                cell_size: 100.0,
                cells: (0..10).map(|x| IVec2::new(x, 0)).collect(),
                base: "/nonexistent/".into(),
            }))),
            cells: HashMap::new(),
            tick: 0,
        }
    }

    fn fill(stream: &mut WorldStream, bytes: u64) {
        // Real loads of the manifest's nonexistent files may have failed already.
        for cell in stream.cells.values_mut() {
            if !matches!(cell.data.get(), Some(Ok(_))) {
                *cell = ready_cell(bytes);
            }
        }
    }

    fn settle(stream: &mut WorldStream, camera: Vec3, bytes: u64) -> Vec<(IVec2, Detail)> {
        let mut shown = Vec::new();
        for _ in 0..20 {
            let (_, show) = stream.plan(camera);
            shown.extend(show);
            fill(stream, bytes);
        }
        shown
    }

    #[test]
    fn near_cells_get_detail_and_farther_ones_a_stand_in() {
        let mut stream = stream(StreamSettings {
            full_distance: 120.0,
            far_distance: 300.0,
            budget_bytes: u64::MAX,
        });
        let shown = settle(&mut stream, Vec3::new(50.0, 0.0, 50.0), 1);
        assert!(shown.contains(&(IVec2::new(0, 0), Detail::Full)));
        assert!(shown.contains(&(IVec2::new(2, 0), Detail::Far)));
        assert!(!shown.iter().any(|(cell, _)| cell.x > 3));
    }

    #[test]
    fn the_budget_keeps_the_nearest_cells_and_never_overflows_for_long() {
        let mut stream = stream(StreamSettings {
            full_distance: 2000.0,
            far_distance: 2000.0,
            budget_bytes: 350,
        });
        for x in 0..10 {
            let camera = Vec3::new(50.0 + 100.0 * x as f32, 0.0, 50.0);
            settle(&mut stream, camera, 100);
            assert!(stream.loaded_bytes() <= 500, "{}", stream.loaded_bytes());
        }
        let nearest = (IVec2::new(9, 0), Detail::Full);
        assert!(stream.cells.contains_key(&nearest));
        assert!(!stream.cells.contains_key(&(IVec2::new(0, 0), Detail::Full)));
    }

    #[test]
    fn a_stand_in_stays_until_the_detail_is_ready() {
        let mut stream = stream(StreamSettings {
            full_distance: 120.0,
            far_distance: 300.0,
            budget_bytes: u64::MAX,
        });
        let cell = IVec2::new(1, 0);
        settle(&mut stream, Vec3::new(-50.0, 0.0, 50.0), 1);
        assert!(stream.cells.contains_key(&(cell, Detail::Far)));

        stream.cells.remove(&(IVec2::new(0, 0), Detail::Full));
        let near = Vec3::new(150.0, 0.0, 50.0);
        let (despawn, _) = stream.plan(near);
        assert!(despawn.is_empty());
        assert!(stream.cells.contains_key(&(cell, Detail::Full)));
        fill(&mut stream, 1);
        let (_, show) = stream.plan(near);
        assert!(show.contains(&(cell, Detail::Full)));
    }

    #[test]
    fn a_cooked_cell_and_manifest_parse_back() {
        let model = ModelData {
            parts: Vec::new(),
            rig: None,
        };
        let pack = write_pack(&[(MODEL_SECTION, write_model(&model))]);
        let cell = parse_cell(&pack).expect("valid");
        assert_eq!(cell.bytes, pack.len() as u64);
        assert!(cell.probes.is_none());

        let mut out = Writer::default();
        out.f32(256.0);
        out.slice(&[[1i32, -2]]);
        let pack = write_pack(&[(WORLD_SECTION, out.0)]);
        let manifest = parse_manifest(&pack, "https://example.com/w/index.kiba").expect("valid");
        assert_eq!(manifest.base, "https://example.com/w/");
        assert!(manifest.cells.contains(&IVec2::new(1, -2)));
    }
}
