//! The cities people walk in, and the avatars they walk as.

use whisker_map::LatLng;

pub struct City {
    pub name: &'static str,
    pub center: LatLng,
    /// Degrees clockwise from north that the camera faces on arrival.
    pub bearing: f32,
    pub accent: u32,
}

pub const CITIES: [City; 5] = [
    City {
        name: "Tokyo",
        center: LatLng::new(35.6598, 139.7006),
        bearing: -28.0,
        accent: 0xff3d7f,
    },
    City {
        name: "New York",
        center: LatLng::new(40.7577, -73.9857),
        bearing: 29.0,
        accent: 0xffb000,
    },
    City {
        name: "Paris",
        center: LatLng::new(48.8580, 2.3550),
        bearing: -15.0,
        accent: 0x3d7bff,
    },
    City {
        name: "London",
        center: LatLng::new(51.5134, -0.1340),
        bearing: 20.0,
        accent: 0x00c48c,
    },
    City {
        name: "Seoul",
        center: LatLng::new(37.5634, 126.9853),
        bearing: 10.0,
        accent: 0xa35bff,
    },
];

pub struct Look {
    pub model: &'static [u8],
    /// The face icon, bundled from `assets/faces`.
    pub face: &'static str,
    pub color: u32,
}

pub static LOOKS: [Look; 4] = [
    Look {
        model: include_bytes!("../assets/models/mina.glb"),
        face: "mina.png",
        color: 0x2ed3b7,
    },
    Look {
        model: include_bytes!("../assets/models/kai.glb"),
        face: "kai.png",
        color: 0x2f8cff,
    },
    Look {
        model: include_bytes!("../assets/models/sora.glb"),
        face: "sora.png",
        color: 0xa35bff,
    },
    Look {
        model: include_bytes!("../assets/models/taro.glb"),
        face: "taro.png",
        color: 0xff4d5e,
    },
];

pub fn css_color(rgb: u32) -> String {
    format!("#{rgb:06x}")
}
