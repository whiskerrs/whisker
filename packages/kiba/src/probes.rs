use std::sync::{Arc, OnceLock};

use glam::{UVec3, Vec3};

use crate::model::{read_file, report_failure};
use crate::pack::{PROBE_SECTION, Reader, read_section};
#[cfg(any(test, feature = "cook"))]
use crate::pack::{Writer, write_pack};
use crate::{LoadState, renderer};

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
    #[cfg(any(test, feature = "cook"))]
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        write_pack(&[(PROBE_SECTION, self.to_section())])
    }

    #[cfg(any(test, feature = "cook"))]
    pub(crate) fn to_section(&self) -> Vec<u8> {
        let mut out = Writer::default();
        for value in self.dimensions.to_array() {
            out.u32(value);
        }
        out.vec3(self.origin);
        out.f32(self.spacing);
        out.slice(&self.visibility);
        out.0
    }

    pub(crate) fn from_section(section: &[u8]) -> Result<Self, String> {
        let mut input = Reader::new(section);
        let dimensions = UVec3::new(input.u32()?, input.u32()?, input.u32()?);
        let origin = input.vec3()?;
        let spacing = input.f32()?;
        let visibility: Vec<[u8; 4]> = input.slice()?;
        let count = dimensions.as_u64vec3().element_product();
        if count == 0 || visibility.len() as u64 != count {
            return Err("the pack's probe count does not match its data".into());
        }
        Ok(Self {
            dimensions,
            origin,
            spacing,
            visibility,
        })
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        Self::from_section(read_section(bytes, PROBE_SECTION)?)
    }
}

/// Darkens sky light where geometry blocks it, spawned alone as `(ProbeVolume::load(path),)`.
///
/// Cook it with `kiba-cook`, from models that do not move. It stores what can see the sky rather than
/// its color, so the [`Sky`](crate::Sky) can change without cooking again. Surfaces outside the box
/// see the whole sky. A frame uses the first volume holding the camera, so volumes apply only near it.
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

    /// Whether the volume has loaded and its box holds `point`.
    pub(crate) fn contains(&self, point: Vec3) -> bool {
        self.data().is_some_and(|data| {
            let (min, size) = bounds(data);
            point.cmpge(min).all() && point.cmple(min + size).all()
        })
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
