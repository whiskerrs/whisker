use super::*;
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn repository_suite_is_well_formed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/host-conformance");
    let (manifest, all) = load_all(&root).unwrap();
    let (_, desktop) = load_required(&root, Host::Desktop).unwrap();
    assert_eq!(all.len(), manifest.cases.len());
    assert_eq!(
        desktop.len(),
        manifest
            .cases
            .iter()
            .filter(|case| case.requires(Host::Desktop))
            .count()
    );
    assert!(desktop.iter().any(|case| case.scenario.reference.is_some()));
    assert!(desktop.iter().any(|case| case.scenario.upstream.is_none()));
}

#[test]
fn capability_target_is_complete_and_disjoint() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/host-conformance");
    let source = std::fs::read_to_string(root.join("capabilities.json")).unwrap();
    let document: serde_json::Value = serde_json::from_str(&source).unwrap();
    assert_eq!(document["schema"], 2);

    let target = &document["target"];
    let properties = document["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|capability| capability["properties"].as_array().into_iter().flatten())
        .map(|property| property.as_str().unwrap())
        .collect::<Vec<_>>();
    let unique = properties.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(
        properties.len(),
        target["property_count"].as_u64().unwrap() as usize
    );
    assert_eq!(unique.len(), properties.len());

    let excluded = document["excluded_registered_properties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["property"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        excluded.len(),
        target["excluded_registered_property_count"]
            .as_u64()
            .unwrap() as usize
    );
    assert!(unique.is_disjoint(&excluded));

    let pending = target["pending_registry_properties"].as_array().unwrap();
    assert_eq!(
        unique.len() - pending.len(),
        target["target_registered_property_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        target["target_registered_property_count"].as_u64().unwrap()
            + target["excluded_registered_property_count"]
                .as_u64()
                .unwrap(),
        target["registered_property_count"].as_u64().unwrap()
    );
    assert_eq!(
        target["property_count"].as_u64().unwrap()
            + target["non_property_features"].as_array().unwrap().len() as u64,
        target["feature_count"].as_u64().unwrap()
    );
}
