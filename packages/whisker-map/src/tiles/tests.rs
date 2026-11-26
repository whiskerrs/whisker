use super::*;

fn square() -> mvt::Layer {
    mvt::Layer {
        name: "b".into(),
        extent: 4096,
        features: vec![mvt::Feature {
            id: None,
            kind: mvt::GeometryKind::Polygon,
            properties: HashMap::new(),
            geometry: vec![vec![[0, 0], [100, 0], [100, 100], [0, 100], [0, 0]]],
            progress: Vec::new(),
        }],
    }
}

fn built(z: u8) -> Vec<usize> {
    let layers = crate::style::parse_layers(&serde_json::json!([
        {"id": "fill", "type": "fill", "source": "v", "source-layer": "b", "minzoom": 13.5},
        {"id": "3d", "type": "fill-extrusion", "source": "v", "source-layer": "b", "minzoom": 14},
    ]));
    let id = TileId::new(z, 0, 0);
    match vector_content(id, Arc::new(vec![square()]), &layers, &[0, 1], 14) {
        TileContent::Vector { buckets, .. } => buckets.iter().map(|b| b.layer).collect(),
        _ => unreachable!(),
    }
}

#[test]
fn a_tile_carries_a_layer_from_the_whole_zoom_its_minimum_falls_in() {
    assert_eq!(built(13), vec![0]);
    assert_eq!(built(14), vec![0, 1]);
}

#[test]
fn a_tile_leaves_circles_in_its_buffer_to_the_tile_they_belong_to() {
    let points = mvt::Layer {
        name: "p".into(),
        extent: 4096,
        features: vec![mvt::Feature {
            id: None,
            kind: mvt::GeometryKind::Point,
            properties: HashMap::new(),
            geometry: vec![vec![[-20, 100], [100, 100], [100, 4100]]],
            progress: Vec::new(),
        }],
    };
    let layers = crate::style::parse_layers(&serde_json::json!([
        {"id": "dots", "type": "circle", "source": "v", "source-layer": "p"},
    ]));
    let content = vector_content(
        TileId::new(14, 0, 0),
        Arc::new(vec![points]),
        &layers,
        &[0],
        14,
    );
    let TileContent::Vector { buckets, .. } = content else {
        unreachable!()
    };
    let bucket::Geometry::Circle(mesh) = &buckets[0].geometry else {
        unreachable!()
    };
    assert_eq!(mesh.vertices.len(), 4);
}
