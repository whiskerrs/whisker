use std::sync::{Arc, OnceLock};

use glam::{UVec3, Vec3};

use crate::model::{read_file, report_failure};
use crate::{LoadState, renderer};

const MAGIC: &[u8; 4] = b"KIBA";
const VERSION: u32 = 1;
const PROBE_SECTION: &[u8; 4] = b"PROB";
const HEADER_LENGTH: usize = 4 + 4 + 4 + 4 + 12 + 12 + 4;

/// How much of the sky each point of a box can see, in a grid cooked ahead of time.
#[derive(Debug, PartialEq)]
pub(crate) struct ProbeData {
    pub(crate) dimensions: UVec3,
    /// The middle of the first probe.
    pub(crate) origin: Vec3,
    pub(crate) spacing: f32,
    /// For each probe, X then Y then Z: the mean as alpha, and the direction of most sky as RGB.
    pub(crate) visibility: Vec<[u8; 4]>,
}

impl ProbeData {
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let payload = HEADER_LENGTH - 16 + self.visibility.len() * 4;
        let mut bytes = Vec::with_capacity(16 + payload);
        bytes.extend(MAGIC);
        bytes.extend(VERSION.to_le_bytes());
        bytes.extend(PROBE_SECTION);
        bytes.extend((payload as u32).to_le_bytes());
        for value in self.dimensions.to_array() {
            bytes.extend(value.to_le_bytes());
        }
        for value in self.origin.to_array() {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(self.spacing.to_le_bytes());
        bytes.extend(self.visibility.iter().flatten());
        bytes
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let body = bytes
            .get(..HEADER_LENGTH)
            .ok_or("the file is too short to be a cooked pack")?;
        if &body[..4] != MAGIC {
            return Err("the file is not a cooked pack".into());
        }
        let word = |at: usize| [body[at], body[at + 1], body[at + 2], body[at + 3]];
        let version = u32::from_le_bytes(word(4));
        if version != VERSION {
            return Err(format!(
                "pack version {version}, and this engine reads {VERSION}"
            ));
        }
        if &body[8..12] != PROBE_SECTION {
            return Err("the pack holds no light probes".into());
        }
        let unsigned = |at| u32::from_le_bytes(word(at));
        let float = |at| f32::from_le_bytes(word(at));
        let dimensions = UVec3::new(unsigned(16), unsigned(20), unsigned(24));
        let count = dimensions.as_u64vec3().element_product() as usize;
        let data = &bytes[HEADER_LENGTH..];
        if count == 0 || data.len() != count * 4 {
            return Err("the pack's probe count does not match its data".into());
        }
        Ok(Self {
            dimensions,
            origin: Vec3::new(float(28), float(32), float(36)),
            spacing: float(40),
            visibility: data
                .chunks_exact(4)
                .map(|texel| [texel[0], texel[1], texel[2], texel[3]])
                .collect(),
        })
    }
}

/// Darkens sky light where geometry blocks it, spawned alone as `(ProbeVolume::load(path),)`.
///
/// Cook it with `kiba-cook`, from models that do not move. It stores what can see the sky rather than
/// its color, so the [`Sky`](crate::Sky) can change without cooking again. Surfaces outside the box
/// see the whole sky, and only one volume is used if the world has several.
#[derive(Clone, Debug)]
pub struct ProbeVolume(Arc<OnceLock<Result<ProbeData, String>>>);

impl ProbeVolume {
    /// Starts loading a cooked file and returns at once.
    ///
    /// `location` is a URL on the Web and a path elsewhere, as `asset!` gives. Check
    /// [`ProbeVolume::load_state`] to learn about progress and failures, which are also logged.
    pub fn load(location: impl Into<String>) -> Self {
        let state = Arc::new(OnceLock::new());
        let finish = {
            let state = state.clone();
            move |result: Result<Vec<u8>, String>| {
                let parsed = result.and_then(|bytes| ProbeData::from_bytes(&bytes));
                report_failure(parsed.as_ref().err());
                let _ = state.set(parsed);
            }
        };
        read_file(location.into(), finish);
        Self(state)
    }

    /// Reads a cooked file already in memory.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        Ok(Self::from_data(ProbeData::from_bytes(bytes)?))
    }

    /// Reports how far loading has come.
    pub fn load_state(&self) -> LoadState {
        match self.0.get() {
            None => LoadState::Loading,
            Some(Ok(_)) => LoadState::Ready,
            Some(Err(reason)) => LoadState::Failed(reason.clone()),
        }
    }

    pub(crate) fn from_data(data: ProbeData) -> Self {
        Self(Arc::new(OnceLock::from(Ok(data))))
    }

    pub(crate) fn data(&self) -> Option<&ProbeData> {
        self.0.get()?.as_ref().ok()
    }

    pub(crate) fn key(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}

/// The box a volume covers: its minimum corner and its size, reaching half a spacing past the probes.
pub(crate) fn bounds(data: &ProbeData) -> (Vec3, Vec3) {
    (
        data.origin - Vec3::splat(data.spacing / 2.0),
        data.dimensions.as_vec3() * data.spacing,
    )
}

/// Makes the GPU copy of a volume.
pub(crate) fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &ProbeData,
) -> wgpu::TextureView {
    renderer::volume_view(device, queue, data.dimensions, &data.visibility)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume() -> ProbeData {
        ProbeData {
            dimensions: UVec3::new(2, 1, 1),
            origin: Vec3::new(1.0, 2.0, 3.0),
            spacing: 0.5,
            visibility: vec![[1, 2, 3, 4], [5, 6, 7, 8]],
        }
    }

    #[test]
    fn a_volume_survives_a_round_trip_through_its_file_format() {
        assert_eq!(
            ProbeData::from_bytes(&volume().to_bytes()).unwrap(),
            volume()
        );
    }

    #[test]
    fn broken_files_are_errors_rather_than_panics() {
        let good = volume().to_bytes();
        assert!(ProbeData::from_bytes(b"KIBA").is_err());
        assert!(ProbeData::from_bytes(&good[..good.len() - 1]).is_err());
        let mut wrong_magic = good.clone();
        wrong_magic[0] = b'X';
        assert!(ProbeData::from_bytes(&wrong_magic).is_err());
        let mut wrong_version = good;
        wrong_version[4] = 99;
        assert!(
            ProbeData::from_bytes(&wrong_version)
                .unwrap_err()
                .contains("version")
        );
    }

    #[test]
    fn the_box_reaches_half_a_spacing_past_the_probes() {
        let (min, size) = bounds(&volume());
        assert_eq!(min, Vec3::new(0.75, 1.75, 2.75));
        assert_eq!(size, Vec3::new(1.0, 0.5, 0.5));
    }
}
