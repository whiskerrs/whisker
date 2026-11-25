use super::*;

fn befores(names: &[Option<&str>]) -> Vec<Option<String>> {
    names.iter().map(|name| name.map(str::to_string)).collect()
}

#[test]
fn layers_draw_beneath_the_style_layer_they_name_in_their_own_order() {
    let befores = befores(&[Some("labels"), None, Some("labels"), Some("roads")]);
    assert_eq!(
        layers_before(&befores, "labels").collect::<Vec<_>>(),
        [0, 2]
    );
    assert_eq!(layers_before(&befores, "roads").collect::<Vec<_>>(), [3]);
    assert_eq!(layers_before(&befores, "water").count(), 0);
}

#[test]
fn layers_no_style_layer_placed_draw_last() {
    let steps = [Step::Layer(2), Step::ClearMask, Step::Layer(0)];
    assert_eq!(unplaced_layers(&steps, 4), [1, 3]);
}

#[test]
fn pitched_views_keep_tiles_at_the_zoom_shown_extrusions_start() {
    let layers = crate::style::parse_layers(&serde_json::json!([
        {"id": "fill", "type": "fill", "source": "v", "source-layer": "b", "minzoom": 15},
        {"id": "3d", "type": "fill-extrusion", "source": "v", "source-layer": "b", "minzoom": 14.5},
        {"id": "other", "type": "fill-extrusion", "source": "w", "source-layer": "b", "minzoom": 16},
    ]));
    assert_eq!(extrusion_floor(&layers, "v", 16.0), 14);
    assert_eq!(extrusion_floor(&layers, "v", 13.0), 0);
    assert_eq!(extrusion_floor(&layers, "w", 15.0), 0);
}
