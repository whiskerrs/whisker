use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default, Serialize, Deserialize)]
struct Config;
impl PluginConfig for Config {
    const NAME: &'static str = "counted";
}
struct Counted(Arc<AtomicUsize>);
impl ProjectPlugin for Counted {
    type Config = Config;
    fn contribute(&self, _: &ProjectContext, _: &Config) -> Result<ProjectUpdate> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ProjectUpdate::Keep)
    }
}
fn context() -> ProjectContext {
    ProjectContext {
        project: serde_json::from_str(include_str!("../fixtures/web.json")).unwrap(),
        app_crate_dir: None,
    }
}

#[test]
fn describe_checks_compatibility_without_executing_plugin() {
    let count = Arc::new(AtomicUsize::new(0));
    let mut output = Vec::new();
    serve(
        Counted(count.clone()),
        encode(&Request::Describe).unwrap().as_slice(),
        &mut output,
    )
    .unwrap();
    assert!(
        matches!(decode::<Response>(&output).unwrap(), Response::Describe {descriptor} if descriptor.name == "counted")
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn missing_or_mismatched_versions_are_rejected_before_payload_decoding() {
    for protocol in [
        serde_json::json!({"version": 0, "ir_schema": 1}),
        serde_json::json!({"version": 1, "ir_schema": 999}),
    ] {
        let bytes = serde_json::to_vec(
            &serde_json::json!({"protocol":protocol,"payload":"invalid payload"}),
        )
        .unwrap();
        assert!(format!("{:#}", decode::<Request>(&bytes).unwrap_err()).contains("unsupported"));
    }
    for bytes in [
        br#"{"payload":{"request":"describe"}}"#.as_slice(),
        br#"{"protocol":{"version":1},"payload":{"request":"describe"}}"#.as_slice(),
    ] {
        assert!(decode::<Request>(bytes).is_err());
    }
}

#[test]
fn unknown_envelope_context_and_project_fields_are_rejected() {
    let request = Request::Contribute {
        descriptor: Descriptor::of(&Counted(Arc::default())),
        config: serde_json::Value::Null,
        context: Box::new(context()),
    };
    for pointer in ["", "/payload/context", "/payload/context/project/project"] {
        let mut value: serde_json::Value =
            serde_json::from_slice(&encode(&request).unwrap()).unwrap();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("future_field".into(), true.into());
        assert!(
            format!(
                "{:#}",
                decode::<Request>(&serde_json::to_vec(&value).unwrap()).unwrap_err()
            )
            .contains("unknown field")
        );
    }
}

#[test]
fn changed_descriptor_and_unsupported_request_never_execute_contribution() {
    let count = Arc::new(AtomicUsize::new(0));
    let plugin = Counted(count.clone());
    let mut descriptor = Descriptor::of(&plugin);
    descriptor.after.push("unexpected".into());
    let request = Request::Contribute {
        descriptor,
        config: serde_json::Value::Null,
        context: Box::new(context()),
    };
    let mut output = Vec::new();
    assert!(serve(plugin, encode(&request).unwrap().as_slice(), &mut output).is_err());
    assert!(output.is_empty());
    let mut request: serde_json::Value =
        serde_json::from_slice(&encode(&request).unwrap()).unwrap();
    request["protocol"]["ir_schema"] = 42.into();
    assert!(
        serve(
            Counted(count.clone()),
            serde_json::to_vec(&request).unwrap().as_slice(),
            &mut output
        )
        .is_err()
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
}
