use super::*;
use serde_json::{Value, json};
fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/web.json")).unwrap()
}
fn project(v: Value) -> WebProjectIr {
    serde_json::from_value(v["project"].clone()).unwrap()
}
#[test]
fn web_output_paths_are_encoded_and_owned() {
    let mut p = project(fixture());
    validate_web(&p).unwrap();
    assert_eq!(
        p.output_url(&ProjectPath::new("写真/a #?%.js").unwrap())
            .unwrap(),
        "/example/%E5%86%99%E7%9C%9F/a%20%23%3F%25.js"
    );
    p.base_path = "//elsewhere/".into();
    assert!(validate_web(&p).is_err());
    let mut p = project(fixture());
    p.resources[0].destination = p.document.clone().unwrap();
    assert!(validate_web(&p).is_err());
    let mut p = project(fixture());
    p.resources[0].destination = ProjectPath::new("pkg").unwrap();
    assert!(
        validate_web(&p)
            .unwrap_err()
            .to_string()
            .contains("overlapping")
    );
}
#[test]
fn worker_defaults_headers_and_directory_coverage_round_trip() {
    let mut v = fixture();
    let w = &mut v["project"];
    w["service_workers"]["main"]["scope"] = Value::Null;
    w["service_workers"]["main"]["update_via_cache"] = json!("none");
    w["service_workers"]["nested"] = json!({"script":"offline/worker.js","kind":"classic"});
    w["resources"]
        .as_array_mut()
        .unwrap()
        .push(json!({"source":"offline","destination":"offline","kind":"directory"}));
    w["response_headers"] = json!([{"id":"isolation","scope":{"kind":"all"},"headers":{"cross-origin-opener-policy":"same-origin"}}]);
    let p = project(v);
    validate_web(&p).unwrap();
    assert_eq!(
        p,
        serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap()
    );
    let mut bad = p.clone();
    bad.service_workers.get_mut("nested").unwrap().scope = Some("/example/".into());
    assert!(
        validate_web(&bad)
            .unwrap_err()
            .to_string()
            .contains("duplicate service worker")
    );
    let mut bad = p.clone();
    bad.service_workers.get_mut("nested").unwrap().script =
        ProjectPath::new("not-distributed.js").unwrap();
    assert!(
        validate_web(&bad)
            .unwrap_err()
            .to_string()
            .contains("not distributed")
    );
    let mut bad = p;
    bad.response_headers[0]
        .headers
        .insert("x-header".into(), "bad\r\nvalue".into());
    assert!(validate_web(&bad).is_err());
}
#[test]
fn html_rejects_ambiguous_or_unserializable_structures() {
    for element in [
        json!({"name":"img","children":[{"kind":"text","value":"bad"}]}),
        json!({"name":"script","children":[{"kind":"element","value":{"name":"span"}}]}),
        json!({"name":"script","children":[{"kind":"text","value":"</scr"},{"kind":"text","value":"ipt>"}]}),
        json!({"name":"meta","attributes":{"name":"a","NAME":"b"}}),
        json!({"name":"title"}),
        json!({"name":"link","attributes":{"rel":"manifest"}}),
    ] {
        let mut v = fixture();
        v["project"]["head"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"invalid","element":element}));
        assert!(validate_web(&project(v)).is_err());
    }
}
#[test]
fn web_named_html_and_native_json_merge_transactionally() {
    let mut a = project(fixture());
    let mut b = a.clone();
    b.head.push(WebHtmlContribution {
        id: "last".into(),
        element: HtmlElement {
            name: "meta".into(),
            attributes: BTreeMap::new(),
            children: vec![],
        },
    });
    b.manifest
        .as_mut()
        .unwrap()
        .properties
        .insert("extension-member".into(), json!({"a":1}));
    a.merge_from(&b).unwrap();
    a.merge_from(&b).unwrap();
    assert_eq!(a.head.len(), 4);
    assert_eq!(a.head.last().unwrap().id, "last");
    let before = a.clone();
    let mut bad = a.clone();
    bad.head.push(WebHtmlContribution {
        id: "temporary".into(),
        element: bad.head[0].element.clone(),
    });
    bad.manifest
        .as_mut()
        .unwrap()
        .properties
        .insert("extension-member".into(), json!({"a":2}));
    assert!(a.merge_from(&bad).is_err());
    assert_eq!(a, before);
}

#[test]
fn empty_web_declarations_accept_an_application_and_additive_plugins() {
    let mut empty = WebProjectIr::default();
    assert!(validate_web(&empty).is_err());
    empty.merge_from(&project(fixture())).unwrap();
    validate_web(&empty).unwrap();
    let additional = WebProjectIr {
        html_attributes: [("dir".into(), Some("ltr".into()))].into(),
        ..Default::default()
    };
    empty.merge_from(&additional).unwrap();
    assert_eq!(empty.title, "Example");
    assert_eq!(empty.html_attributes["dir"], Some("ltr".into()));
}
