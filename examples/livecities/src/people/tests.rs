use super::*;

const CENTER: LatLng = LatLng::new(35.0, 139.0);

fn street(name: &'static str, coords: &[[f64; 2]]) -> (&'static str, serde_json::Value) {
    (
        name,
        serde_json::json!({"type": "LineString", "coordinates": coords}),
    )
}

fn roads(streets: &[(&'static str, serde_json::Value)]) -> Roads {
    Roads::from_streets(CENTER, streets.iter().map(|(n, g)| (*n, g)))
}

#[test]
fn lines_cut_at_a_tile_edge_join_into_one_street() {
    let roads = roads(&[
        street("A", &[[139.0, 35.0], [139.001, 35.0]]),
        street("A", &[[139.001, 35.000_001], [139.002, 35.0]]),
    ]);
    assert_eq!(roads.points.len(), 3);
    assert_eq!(roads.links[1].len(), 2);
}

#[test]
fn walkers_stay_on_the_streets_and_keep_moving() {
    let roads = roads(&[
        street("A", &[[139.0, 35.0], [139.002, 35.0]]),
        street("B", &[[139.002, 35.0], [139.002, 35.002]]),
    ]);
    let corner = to_meters(CENTER, LatLng::new(35.0, 139.002))[0];
    let mut crowd = Crowd::new(CENTER, roads, 20, 4, 1);
    let before: Vec<f64> = crowd.walkers.iter().map(|w| w.walked).collect();
    for _ in 0..600 {
        crowd.step(1.0 / 60.0);
    }
    for (walker, before) in crowd.walkers.iter().zip(before) {
        let (p, _) = crowd.place(walker);
        let on_a = p[1].abs() < 1e-6 && p[0] >= -1e-6;
        let on_b = (p[0] - corner).abs() < 1e-6;
        assert!(on_a || on_b, "{p:?} is off the streets");
        assert!(walker.walked > before || walker.mood != Mood::Walking);
        assert!(["A", "B"].contains(&crowd.street(walker)));
    }
}
